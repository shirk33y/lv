#!/bin/bash
# Build lv as a Flatpak using podman
# Usage: ./scripts/build-flatpak.sh [--install] [--build-only]
#
# Requires podman. Builds entirely in containers:
#   1. podman build  — creates build-env image with flatpak runtimes (cached)
#   2. podman run    — generates cargo-sources.json from Cargo.lock
#   3. podman run --privileged — runs flatpak-builder, produces build/lv.flatpak
#
# Output: build/lv.flatpak (gitignored)

set -euo pipefail

INSTALL=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install) INSTALL=true; shift ;;
    --build-only) INSTALL=false; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

cd "$(dirname "$0")/.."

APP_ID="com.shirk33y.lv"
ENV_IMAGE="lv-flatpak-env"
TMPDIR="${TMPDIR:-$HOME/.buildah-tmp}"
BUILD_DIR="${BUILD_DIR:-build}"
mkdir -p "$TMPDIR" "$BUILD_DIR"

echo "==> [1/4] Building flatpak build-env image..."
TMPDIR="$TMPDIR" podman build \
    --cap-add=SYS_ADMIN \
    --security-opt=seccomp=unconfined \
    -f dist/flatpak/Dockerfile.flatpak \
    -t "$ENV_IMAGE" .

echo "==> [2/4] Generating cargo-sources.json..."
TMPDIR="$TMPDIR" podman run --rm \
    --security-opt=label=disable \
    -v "$(pwd):/src" \
    "$ENV_IMAGE" \
    python3 /usr/local/bin/flatpak-cargo-generator.py /src/Cargo.lock -o /src/cargo-sources.json

echo "==> [3/4] Running flatpak-builder..."
TMPDIR="$TMPDIR" podman run --rm \
    --privileged \
    --security-opt=label=disable \
    -v "$(pwd):/src" \
    -w /src \
    "$ENV_IMAGE" \
    flatpak-builder \
      --disable-rofiles-fuse \
      --force-clean \
      --repo=/src/"$BUILD_DIR"/repo \
      /src/"$BUILD_DIR"/flatpak-build \
      dist/flatpak/"$APP_ID.json"

echo "==> [4/4] Creating bundle $BUILD_DIR/lv.flatpak..."
TMPDIR="$TMPDIR" podman run --rm \
    --security-opt=label=disable \
    -v "$(pwd):/src" \
    "$ENV_IMAGE" \
    flatpak build-bundle /src/"$BUILD_DIR"/repo /src/"$BUILD_DIR"/lv.flatpak "$APP_ID"

echo ""
echo "==> Done! Bundle: $(pwd)/$BUILD_DIR/lv.flatpak"

if [ "$INSTALL" = true ]; then
    echo "==> Installing (system-wide)..."
    flatpak install -y --bundle "$BUILD_DIR/lv.flatpak"
    echo "==> Installed! Run with: flatpak run $APP_ID"
    echo ""
    echo "To make 'lv' available in PATH, create a wrapper:"
    echo "  sudo tee /usr/local/bin/lv > /dev/null <<'EOF'"
    echo "  #!/bin/bash"
    echo "  exec flatpak run $APP_ID \"\$@\""
    echo "  EOF"
    echo "  sudo chmod +x /usr/local/bin/lv"
fi
