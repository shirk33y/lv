#!/bin/bash
# Build an AppImage from the release binary.
# Usage: ./pkg/appimage.sh [arch] [binary]
#   arch    — x86_64 (default: uname -m)
#   binary  — path to lv binary (default: target/release/lv)
# Set LV_VERSION to override the version tag.
set -euo pipefail

ARCH="${1:-$(uname -m)}"
BINARY="${2:-target/release/lv}"
LV_VERSION="${LV_VERSION:-$(git -C "$(dirname "$0")/.." describe --always --dirty 2>/dev/null || echo dev)}"
APPDIR="AppDir"
OUTPUT="lv-${LV_VERSION}-${ARCH}.AppImage"

cd "$(dirname "$0")/.."

if [ ! -f "$BINARY" ]; then
  echo "ERROR: binary not found: $BINARY" >&2
  exit 1
fi

echo "==> Preparing AppDir for $ARCH (binary: $BINARY)"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Binary
cp "$BINARY" "$APPDIR/usr/bin/lv"
chmod +x "$APPDIR/usr/bin/lv"

# Desktop + icon
cp pkg/lv.desktop "$APPDIR/usr/share/applications/lv.desktop"
cp pkg/lv-256.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/lv.png"

# Root-level symlinks required by AppImage spec
ln -sf usr/share/applications/lv.desktop "$APPDIR/lv.desktop"
ln -sf usr/share/icons/hicolor/256x256/apps/lv.png "$APPDIR/lv.png"

# ── Bundle shared libraries ──────────────────────────────────────────
echo "==> Bundling shared libraries"

skip_lib() {
  case "$1" in
    libc.so*|libm.so*|libdl.so*|librt.so*|libpthread.so*|ld-linux*) return 0 ;;
    libstdc++*|libgcc_s*) return 0 ;;
  esac
  return 1
}

# Collect deps into a temp file, then process iteratively (avoids subshell issues)
DEPLIST=$(mktemp)
trap "rm -f $DEPLIST" EXIT

if [ -n "${LIB_SEARCH_PATH:-}" ]; then
  # Cross-compile mode: use readelf to find NEEDED libs
  echo "  (cross mode, searching: $LIB_SEARCH_PATH)"
  find_lib() {
    local name="$1"
    IFS=: read -ra dirs <<< "$LIB_SEARCH_PATH"
    for d in "${dirs[@]}"; do
      local found
      found=$(find "$d" -name "${name}" -type f -o -name "${name}" -type l 2>/dev/null | head -1)
      if [ -n "$found" ]; then
        readlink -f "$found"
        return
      fi
    done
  }

  # Seed with binary's NEEDED
  { readelf -d "$APPDIR/usr/bin/lv" 2>/dev/null | grep NEEDED | sed 's/.*\[//;s/\]//' || true; } > "$DEPLIST"

  while [ -s "$DEPLIST" ]; do
    NEXT=$(mktemp)
    while read -r needed; do
      base=$(basename "$needed")
      skip_lib "$base" && continue
      [ -f "$APPDIR/usr/lib/$base" ] && continue
      dep=$(find_lib "$needed")
      if [ -z "$dep" ]; then
        echo "  WARNING: $needed not found"
        continue
      fi
      cp "$dep" "$APPDIR/usr/lib/$base"
      echo "  $base"
      # Queue this lib's deps
      { readelf -d "$dep" 2>/dev/null | grep NEEDED | sed 's/.*\[//;s/\]//' || true; } >> "$NEXT"
    done < "$DEPLIST"
    mv "$NEXT" "$DEPLIST"
  done
else
  # Native mode: use ldd — collect all deps in one pass
  { ldd "$APPDIR/usr/bin/lv" 2>/dev/null | grep "=> /" | awk '{print $3}' || true; } > "$DEPLIST"

  while [ -s "$DEPLIST" ]; do
    NEXT=$(mktemp)
    while read -r dep; do
      base=$(basename "$dep")
      skip_lib "$base" && continue
      [ -f "$APPDIR/usr/lib/$base" ] && continue
      cp "$dep" "$APPDIR/usr/lib/$base"
      echo "  $base"
      # Queue transitive deps
      { ldd "$dep" 2>/dev/null | grep "=> /" | awk '{print $3}' || true; } >> "$NEXT"
    done < "$DEPLIST"
    mv "$NEXT" "$DEPLIST"
  done

  # Second pass: run ldd on every bundled lib to catch transitive deps
  # that the first pass missed (e.g. libblas loaded by libavfilter)
  echo "==> Verifying bundled libraries"
  PREV_COUNT=0
  CUR_COUNT=$(ls "$APPDIR/usr/lib/" | wc -l)
  while [ "$CUR_COUNT" -ne "$PREV_COUNT" ]; do
    PREV_COUNT=$CUR_COUNT
    > "$DEPLIST"
    for lib in "$APPDIR/usr/lib/"*.so*; do
      ldd "$lib" 2>/dev/null | grep "=> /" | awk '{print $3}' >> "$DEPLIST" || true
    done
    sort -u "$DEPLIST" | while read -r dep; do
      base=$(basename "$dep")
      skip_lib "$base" && continue
      [ -f "$APPDIR/usr/lib/$base" ] && continue
      cp "$dep" "$APPDIR/usr/lib/$base"
      echo "  $base (from second pass)"
    done
    CUR_COUNT=$(ls "$APPDIR/usr/lib/" | wc -l)
  done
fi

rm -f "$DEPLIST"

# ── AppRun wrapper (sets LD_LIBRARY_PATH) ─────────────────────────────
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib:${LD_LIBRARY_PATH:-}"
exec "$HERE/usr/bin/lv" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

# ── Download appimagetool (host arch, not target) ────────────────────
HOST_ARCH="$(uname -m)"
TOOL="appimagetool-${HOST_ARCH}.AppImage"
if [ ! -f "$TOOL" ]; then
  echo "==> Downloading appimagetool ($HOST_ARCH)"
  curl -fsSL "https://github.com/AppImage/appimagetool/releases/download/continuous/$TOOL" -o "$TOOL"
  chmod +x "$TOOL"
fi

# ── Build AppImage ────────────────────────────────────────────────────
# Use --appimage-extract-and-run so it works inside Docker (no FUSE)
echo "==> Building $OUTPUT"
ARCH="$ARCH" ./"$TOOL" --appimage-extract-and-run "$APPDIR" "$OUTPUT"

echo "==> Done: $OUTPUT"
rm -rf "$APPDIR"
