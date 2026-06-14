#!/bin/bash
# Build lv as a Flatpak using podman or docker
# Usage: ./scripts/build-flatpak.sh [--install] [--build-only] [--no-cache] [--rebuild-image]
#
# Requires podman or docker. Builds entirely in containers:
#   1. container build — creates build-env image with flatpak runtimes
#   2. container run — generates cargo-sources.json from Cargo.lock
#   3. container run --privileged — runs flatpak-builder, produces bundle
#
# Output: ${FLATPAK_BUNDLE:-build/lv.flatpak} (gitignored)
# Cache: build/flatpak-home/, build/flatpak-state/, and ~/.cache/flatpak/

set -euo pipefail

INSTALL=false
NO_CACHE=false
REBUILD_IMAGE=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install) INSTALL=true; shift ;;
    --build-only) INSTALL=false; shift ;;
    --no-cache) NO_CACHE=true; shift ;;
    --rebuild-image) REBUILD_IMAGE=true; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

cd "$(dirname "$0")/.."

APP_ID="io.github.shirk33y.lv"
CONTAINER_RUNTIME="${CONTAINER_RUNTIME:-podman}"
CONTAINER_PLATFORM="${CONTAINER_PLATFORM:-}"
PLATFORM_SUFFIX="${CONTAINER_PLATFORM//\//-}"
ENV_IMAGE="${ENV_IMAGE:-lv-flatpak-env${PLATFORM_SUFFIX:+-$PLATFORM_SUFFIX}}"
TMPDIR="${TMPDIR:-$HOME/.buildah-tmp}"
BUILD_DIR="${BUILD_DIR:-build}"
FLATPAK_BUNDLE="${FLATPAK_BUNDLE:-$BUILD_DIR/lv.flatpak}"
FLATPAK_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/flatpak"
FLATPAK_HOME="$BUILD_DIR/flatpak-home"
FLATPAK_STATE_DIR="${FLATPAK_STATE_DIR:-$BUILD_DIR/flatpak-state}"
FLATPAK_FORCE_CLEAN="${FLATPAK_FORCE_CLEAN:-1}"
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"
CONTAINER_PLATFORM_ARGS=()
if [ -n "$CONTAINER_PLATFORM" ]; then
    CONTAINER_PLATFORM_ARGS=(--platform "$CONTAINER_PLATFORM")
fi
HOST_ARCH="$(uname -m)"
TARGET_ARCH="$HOST_ARCH"
case "$CONTAINER_PLATFORM" in
    linux/amd64) TARGET_ARCH="x86_64" ;;
    linux/arm64) TARGET_ARCH="aarch64" ;;
esac
if [ "$TARGET_ARCH" != "$HOST_ARCH" ] && [ "${LV_ALLOW_FLATPAK_EMULATION:-0}" != "1" ]; then
    cat >&2 <<EOF
Refusing cross-arch Flatpak build: host=$HOST_ARCH target=$TARGET_ARCH.
Flatpak uses bubblewrap namespaces, which are unreliable under Docker/Podman + QEMU.
Use native $TARGET_ARCH Linux, or set LV_ALLOW_FLATPAK_EMULATION=1 to force experimental emulation.
EOF
    exit 1
fi
CONTAINER_USER_ARGS=(--user "$HOST_UID:$HOST_GID")
if [ "$CONTAINER_RUNTIME" = "podman" ]; then
    CONTAINER_USER_ARGS=(--userns=keep-id --user "$HOST_UID:$HOST_GID" --group-add keep-groups)
fi
CONTAINER_COMMON=(
    -e "HOME=/tmp/flatpak-home"
    -e "XDG_CACHE_HOME=/tmp/flatpak-cache"
    -e "XDG_RUNTIME_DIR=/tmp/flatpak-runtime"
)
if [ "$CONTAINER_RUNTIME" = "podman" ]; then
    CONTAINER_COMMON=(--security-opt=label=disable "${CONTAINER_COMMON[@]}")
fi
CONTAINER_ENTRYPOINT=(
    sh -lc 'mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR" && exec "$@"' sh
)

mkdir -p "$TMPDIR" "$BUILD_DIR" "$FLATPAK_CACHE" "$FLATPAK_HOME" "$FLATPAK_STATE_DIR"

if ! command -v "$CONTAINER_RUNTIME" >/dev/null 2>&1; then
    echo "Missing container runtime: $CONTAINER_RUNTIME" >&2
    exit 1
fi

image_exists() {
    if [ "$CONTAINER_RUNTIME" = "podman" ]; then
        podman image exists "$ENV_IMAGE" 2>/dev/null
    else
        "$CONTAINER_RUNTIME" image inspect "$ENV_IMAGE" >/dev/null 2>&1
    fi
}

fix_output_ownership() {
    local path
    for path in "$BUILD_DIR" cargo-sources.json extra/flatpak/cargo-sources.json; do
        [ -e "$path" ] || continue
        if [ "$CONTAINER_RUNTIME" = "podman" ]; then
            podman unshare chown -R 0:0 "$path" 2>/dev/null || true
            continue
        elif image_exists; then
            "$CONTAINER_RUNTIME" run --rm \
                "${CONTAINER_PLATFORM_ARGS[@]}" \
                -v "$(pwd):/src:rw" \
                -w /src \
                "$ENV_IMAGE" \
                chown -R "$HOST_UID:$HOST_GID" "$path" 2>/dev/null || true
        fi
        chown -R "$HOST_UID:$HOST_GID" "$path" 2>/dev/null || true
    done
}
trap fix_output_ownership EXIT INT TERM

# Check if image exists (unless --rebuild-image)
if [ "$REBUILD_IMAGE" = false ] && image_exists; then
    echo "==> [1/4] Using cached build-env image..."
else
    echo "==> [1/4] Building build-env image with $CONTAINER_RUNTIME..."
    BUILD_ARGS=("${CONTAINER_PLATFORM_ARGS[@]}")
    if [ "$NO_CACHE" = true ]; then
        BUILD_ARGS+=(--no-cache)
    fi
    TMPDIR="$TMPDIR" "$CONTAINER_RUNTIME" build \
        "${BUILD_ARGS[@]}" \
        -f extra/flatpak/Dockerfile.flatpak \
        -t "$ENV_IMAGE" .
fi

echo "==> [2/4] Generating cargo-sources.json..."
TMPDIR="$TMPDIR" "$CONTAINER_RUNTIME" run --rm \
    "${CONTAINER_PLATFORM_ARGS[@]}" \
    "${CONTAINER_USER_ARGS[@]}" \
    "${CONTAINER_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    python3 /usr/local/bin/flatpak-cargo-generator.py /src/Cargo.lock -o /src/cargo-sources.json
# Copy to manifest directory (flatpak-builder resolves relative paths from manifest location)
cp -f cargo-sources.json extra/flatpak/cargo-sources.json

# Merge cargo sources into manifest: prepend all cargo crates to lv module's sources
# Remove cargo-sources.json file entry (no longer needed after merge)
# Adjust paths since merged manifest is in build/ not extra/flatpak/ (remove one ../ level)
jq --slurpfile cargo_sources cargo-sources.json \
    '.modules[] |= if .name == "lv" then .sources = ($cargo_sources[0] + (.sources | map(select(.path != "cargo-sources.json")))) else . end' \
    extra/flatpak/"$APP_ID.json" | \
    jq '.modules[] |= if .name == "lv" then .sources |= map(if .path and (.path | startswith("../")) then .path |= .[3:] else . end) else . end' \
    > "$BUILD_DIR/$APP_ID.json"

echo "==> [3/4] Running flatpak-builder (using $FLATPAK_CACHE for runtime cache)..."

TMPDIR="$TMPDIR" "$CONTAINER_RUNTIME" run --rm \
    "${CONTAINER_PLATFORM_ARGS[@]}" \
    "${CONTAINER_USER_ARGS[@]}" \
    "${CONTAINER_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    sh -lc 'ostree --repo="$1" init --mode=archive-z2 2>/dev/null || true; ostree --repo="$1" config set core.min-free-space-percent 0' sh /src/"$BUILD_DIR"/repo

FORCE_CLEAN_ARGS=()
if [ "$FLATPAK_FORCE_CLEAN" != "0" ]; then
    FORCE_CLEAN_ARGS+=(--force-clean)
else
    rm -rf "$BUILD_DIR/flatpak-build"
fi

TMPDIR="$TMPDIR" "$CONTAINER_RUNTIME" run --rm \
    --privileged \
    "${CONTAINER_PLATFORM_ARGS[@]}" \
    "${CONTAINER_USER_ARGS[@]}" \
    "${CONTAINER_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    -v "$FLATPAK_CACHE:/tmp/flatpak-cache/flatpak:rw" \
    -w /src \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    flatpak-builder \
      --disable-rofiles-fuse \
      "${FORCE_CLEAN_ARGS[@]}" \
      --state-dir=/src/"$FLATPAK_STATE_DIR" \
      --repo=/src/"$BUILD_DIR"/repo \
      /src/"$BUILD_DIR"/flatpak-build \
      "$BUILD_DIR/$APP_ID.json"

echo "==> [4/4] Creating bundle $FLATPAK_BUNDLE..."
TMPDIR="$TMPDIR" "$CONTAINER_RUNTIME" run --rm \
    "${CONTAINER_PLATFORM_ARGS[@]}" \
    "${CONTAINER_USER_ARGS[@]}" \
    "${CONTAINER_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    flatpak build-bundle /src/"$BUILD_DIR"/repo /src/"$FLATPAK_BUNDLE" "$APP_ID"

echo ""
echo "==> Done! Bundle: $(pwd)/$FLATPAK_BUNDLE"

# Create wrapper script for PATH
echo "==> Creating CLI wrapper..."
WRAPPER_CONTENT="#!/bin/bash
exec flatpak run $APP_ID \"\$@\""

# Try user-level wrapper (no sudo needed)
mkdir -p ~/.local/bin
if echo "$WRAPPER_CONTENT" > ~/.local/bin/lv && chmod +x ~/.local/bin/lv; then
    echo "✅ User wrapper: ~/.local/bin/lv (no sudo needed)"
fi

if [ "$INSTALL" = true ]; then
    echo "==> Installing (user)..."
    flatpak install --user -y --bundle "$FLATPAK_BUNDLE"
    echo "==> Installed! Run with: lv [files]"
fi
