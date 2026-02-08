#!/bin/bash
# Build an AppImage from the release binary.
# Usage: ./pkg/appimage.sh [arch]
# Expects: target/release/lv-imgui to exist (cargo build --release)
set -euo pipefail

ARCH="${1:-$(uname -m)}"
LV_VERSION="${LV_VERSION:-$(git -C "$(dirname "$0")/.." describe --always --dirty 2>/dev/null || echo dev)}"
APP="lv"
APPDIR="AppDir"

cd "$(dirname "$0")/.."

echo "==> Preparing AppDir for $ARCH"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps"

# Binary
cp target/release/lv-imgui "$APPDIR/usr/bin/lv-imgui"
chmod +x "$APPDIR/usr/bin/lv-imgui"

# Desktop + icon
cp pkg/lv.desktop "$APPDIR/usr/share/applications/lv.desktop"
cp pkg/lv.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/lv.svg"

# Root-level symlinks required by AppImage spec
ln -sf usr/share/applications/lv.desktop "$APPDIR/lv.desktop"
ln -sf usr/share/icons/hicolor/scalable/apps/lv.svg "$APPDIR/lv.svg"
ln -sf usr/bin/lv-imgui "$APPDIR/AppRun"

# Bundle shared libraries (SDL2, mpv, and their deps)
echo "==> Bundling shared libraries"
for lib in libSDL2 libmpv; do
  SO=$(ldconfig -p | grep "$lib" | head -1 | awk '{print $NF}')
  if [ -n "$SO" ]; then
    cp "$SO" "$APPDIR/usr/lib/"
    echo "  $SO"
  else
    echo "  WARNING: $lib not found, skipping"
  fi
done

# Copy transitive deps of bundled libs (skip glibc/ld/pthread)
for lib in "$APPDIR"/usr/lib/*.so*; do
  ldd "$lib" 2>/dev/null | grep "=> /" | awk '{print $3}' | while read dep; do
    base=$(basename "$dep")
    # Skip system libs that AppImage shouldn't bundle
    case "$base" in
      libc.so*|libm.so*|libdl.so*|librt.so*|libpthread.so*|ld-linux*|libstdc++*|libgcc_s*) continue ;;
    esac
    [ ! -f "$APPDIR/usr/lib/$base" ] && cp "$dep" "$APPDIR/usr/lib/" && echo "  $dep"
  done
done

# Create AppRun wrapper that sets LD_LIBRARY_PATH
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib:${LD_LIBRARY_PATH:-}"
exec "$HERE/usr/bin/lv-imgui" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

# Download appimagetool if not present
TOOL="appimagetool-${ARCH}.AppImage"
if [ ! -f "$TOOL" ]; then
  echo "==> Downloading appimagetool"
  curl -fsSL "https://github.com/AppImage/appimagetool/releases/download/continuous/$TOOL" -o "$TOOL"
  chmod +x "$TOOL"
fi

# Build AppImage
echo "==> Building AppImage"
ARCH="$ARCH" ./"$TOOL" "$APPDIR" "lv-${LV_VERSION}-${ARCH}.AppImage"

echo "==> Done: lv-${LV_VERSION}-${ARCH}.AppImage"
rm -rf "$APPDIR"
