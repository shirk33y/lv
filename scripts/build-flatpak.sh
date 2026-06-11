#!/bin/bash
# Build lv as a Flatpak using podman
# Usage: ./scripts/build-flatpak.sh [--install] [--build-only] [--no-cache] [--rebuild-image]
#
# Requires podman. Builds entirely in containers:
#   1. podman build  — creates build-env image with flatpak runtimes (cached by default)
#   2. podman run --userns=keep-id — generates cargo-sources.json from Cargo.lock
#   3. podman run --userns=keep-id --privileged — runs flatpak-builder, produces build/lv.flatpak
#
# Output: build/lv.flatpak (gitignored)
# Cache: build/flatpak-home/ and ~/.cache/flatpak/ (both user-owned)

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

APP_ID="com.shirk33y.lv"
ENV_IMAGE="lv-flatpak-env"
TMPDIR="${TMPDIR:-$HOME/.buildah-tmp}"
BUILD_DIR="${BUILD_DIR:-build}"
FLATPAK_CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/flatpak"
FLATPAK_HOME="$BUILD_DIR/flatpak-home"
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"
PODMAN_KEEP_ID=(
    --userns=keep-id
    --user "$HOST_UID:$HOST_GID"
    --group-add keep-groups
)
PODMAN_COMMON=(
    --security-opt=label=disable
    -e "HOME=/tmp/flatpak-home"
    -e "XDG_CACHE_HOME=/tmp/flatpak-cache"
    -e "XDG_RUNTIME_DIR=/tmp/flatpak-runtime"
)
CONTAINER_ENTRYPOINT=(
    sh -lc 'mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR" && exec "$@"' sh
)

mkdir -p "$TMPDIR" "$BUILD_DIR" "$FLATPAK_CACHE" "$FLATPAK_HOME"

fix_output_ownership() {
    local path
    for path in "$BUILD_DIR" cargo-sources.json extra/flatpak/cargo-sources.json; do
        [ -e "$path" ] || continue
        podman unshare chown -R 0:0 "$path" 2>/dev/null || \
            chown -R "$HOST_UID:$HOST_GID" "$path" 2>/dev/null || true
    done
}
trap fix_output_ownership EXIT INT TERM

# Check if image exists (unless --rebuild-image)
if [ "$REBUILD_IMAGE" = false ] && podman image exists "$ENV_IMAGE" 2>/dev/null; then
    echo "==> [1/4] Using cached build-env image..."
else
    echo "==> [1/4] Building build-env image from freedesktopsdk/flatpak + cargo generator..."
    TMPDIR="$TMPDIR" podman build \
        -f extra/flatpak/Dockerfile.flatpak \
        -t "$ENV_IMAGE" .
fi

echo "==> [2/4] Generating cargo-sources.json..."
TMPDIR="$TMPDIR" podman run --rm \
    "${PODMAN_KEEP_ID[@]}" \
    "${PODMAN_COMMON[@]}" \
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

TMPDIR="$TMPDIR" podman run --rm \
    "${PODMAN_KEEP_ID[@]}" \
    "${PODMAN_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    sh -lc 'ostree --repo="$1" init --mode=archive-z2 2>/dev/null || true; ostree --repo="$1" config set core.min-free-space-percent 0' sh /src/"$BUILD_DIR"/repo

TMPDIR="$TMPDIR" podman run --rm \
    --privileged \
    "${PODMAN_KEEP_ID[@]}" \
    "${PODMAN_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    -v "$FLATPAK_CACHE:/tmp/flatpak-cache/flatpak:rw" \
    -w /src \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    flatpak-builder \
      --disable-rofiles-fuse \
      --force-clean \
      --repo=/src/"$BUILD_DIR"/repo \
      /src/"$BUILD_DIR"/flatpak-build \
      "$BUILD_DIR/$APP_ID.json"

echo "==> [4/4] Creating bundle $BUILD_DIR/lv.flatpak..."
TMPDIR="$TMPDIR" podman run --rm \
    "${PODMAN_KEEP_ID[@]}" \
    "${PODMAN_COMMON[@]}" \
    -v "$(pwd):/src:rw" \
    -v "$(pwd)/$FLATPAK_HOME:/tmp/flatpak-home:rw" \
    "$ENV_IMAGE" \
    "${CONTAINER_ENTRYPOINT[@]}" \
    flatpak build-bundle /src/"$BUILD_DIR"/repo /src/"$BUILD_DIR"/lv.flatpak "$APP_ID"

echo ""
echo "==> Done! Bundle: $(pwd)/$BUILD_DIR/lv.flatpak"

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
    flatpak install --user -y --bundle "$BUILD_DIR/lv.flatpak"
    echo "==> Installed! Run with: lv [files]"
fi
