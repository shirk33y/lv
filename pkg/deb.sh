#!/bin/bash
# Build a .deb package from the release binary.
# Usage: ./pkg/deb.sh [arch] [binary]
#   arch    — amd64 | arm64 (default: dpkg --print-architecture)
#   binary  — path to lv binary (default: target/release/lv)
# Set LV_VERSION to override the version tag.
set -euo pipefail

ARCH="${1:-$(dpkg --print-architecture 2>/dev/null || echo amd64)}"
BINARY="${2:-target/release/lv}"
LV_VERSION="${LV_VERSION:-$(git -C "$(dirname "$0")/.." describe --always --dirty 2>/dev/null || echo dev)}"
OUTPUT="lv_${LV_VERSION}_${ARCH}.deb"

cd "$(dirname "$0")/.."

if [ ! -f "$BINARY" ]; then
  echo "ERROR: binary not found: $BINARY" >&2
  exit 1
fi

echo "==> Building $OUTPUT"

STAGING=$(mktemp -d)
trap "rm -rf $STAGING" EXIT

# Resolve multiarch lib dir
case "$ARCH" in
  amd64)  LIBDIR="/usr/lib/x86_64-linux-gnu" ;;
  arm64)  LIBDIR="/usr/lib/aarch64-linux-gnu" ;;
  *)      echo "ERROR: unsupported arch: $ARCH" >&2; exit 1 ;;
esac

# Binary
install -Dm755 "$BINARY" "$STAGING/usr/bin/lv"

# Bundled shared libraries (RPATH in binary points to ../lib/lv/)
PRIVLIB="$STAGING/usr/lib/lv"
mkdir -p "$PRIVLIB"
for lib in libmpv.so.2 libSDL2-2.0.so.0; do
  src="$LIBDIR/$lib"
  if [ -L "$src" ]; then
    cp -L "$src" "$PRIVLIB/$lib"
  elif [ -f "$src" ]; then
    cp "$src" "$PRIVLIB/$lib"
  else
    echo "WARNING: $src not found, skipping" >&2
  fi
done

# Desktop file
install -Dm644 pkg/lv.desktop "$STAGING/usr/share/applications/lv.desktop"

# Icon
install -Dm644 pkg/lv-256.png "$STAGING/usr/share/icons/hicolor/256x256/apps/lv.png"

# Control file
mkdir -p "$STAGING/DEBIAN"
cat > "$STAGING/DEBIAN/control" << EOF
Package: lv
Version: ${LV_VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Depends: libc6, libgcc-s1
Maintainer: Mateusz Shirkey <shirk3y@gmail.com>
Description: Fast keyboard-driven media viewer
 Single Rust binary, SQLite library database, GPU-rendered UI.
 Supports images and video via mpv/SDL2.
 Bundles libmpv and libSDL2 — no extra dependencies needed.
EOF

dpkg-deb --build --root-owner-group "$STAGING" "$OUTPUT"
echo "==> Done: $OUTPUT"
