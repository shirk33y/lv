#!/bin/bash
# Docker entrypoint for lv multi-target builds.
# Usage: docker run --rm -v $PWD:/src lv-builder <target> [targets...]
#
# Targets:
#   linux-x86_64    — Linux x86_64 binary + AppImage
#   linux-aarch64   — Linux aarch64 binary + AppImage (cross-compiled)
#   windows-x86_64  — Windows x86_64 binary + NSIS installer
#   all             — all of the above
set -euo pipefail

OUTDIR="/src/dist"
mkdir -p "$OUTDIR"

build_linux_x86_64() {
    echo "══════ linux-x86_64 ══════"
    cargo build --release --target x86_64-unknown-linux-gnu

    cp target/x86_64-unknown-linux-gnu/release/lv-imgui "$OUTDIR/lv-imgui-linux-x86_64"
    echo "  → $OUTDIR/lv-imgui-linux-x86_64"

    # AppImage
    echo "  → building AppImage..."
    ARCH=x86_64 TARGET_DIR=target/x86_64-unknown-linux-gnu/release \
        _build_appimage x86_64
}

build_linux_aarch64() {
    echo "══════ linux-aarch64 ══════"
    PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
    PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig \
    cargo build --release --target aarch64-unknown-linux-gnu

    cp target/aarch64-unknown-linux-gnu/release/lv-imgui "$OUTDIR/lv-imgui-linux-aarch64"
    echo "  → $OUTDIR/lv-imgui-linux-aarch64"

    # AppImage
    echo "  → building AppImage..."
    ARCH=aarch64 TARGET_DIR=target/aarch64-unknown-linux-gnu/release \
        _build_appimage aarch64
}

build_windows_x86_64() {
    echo "══════ windows-x86_64 ══════"
    cargo xwin build --release --target x86_64-pc-windows-msvc

    cp target/x86_64-pc-windows-msvc/release/lv-imgui.exe "$OUTDIR/lv-imgui-windows-x86_64.exe"
    echo "  → $OUTDIR/lv-imgui-windows-x86_64.exe"

    # NSIS installer
    echo "  → building installer..."
    local tmp
    tmp=$(mktemp -d)
    cp target/x86_64-pc-windows-msvc/release/lv-imgui.exe "$tmp/"
    cp pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll pkg/installer.nsi "$tmp/"
    (cd "$tmp" && makensis installer.nsi)
    cp "$tmp/lv-setup.exe" "$OUTDIR/lv-setup-windows-x86_64.exe"
    rm -rf "$tmp"
    echo "  → $OUTDIR/lv-setup-windows-x86_64.exe"
}

_build_appimage() {
    local arch="$1"
    local target_dir="${TARGET_DIR:-target/release}"
    local appdir
    appdir=$(mktemp -d)

    mkdir -p "$appdir/usr/bin" "$appdir/usr/lib" \
             "$appdir/usr/share/applications" \
             "$appdir/usr/share/icons/hicolor/scalable/apps"

    cp "$target_dir/lv-imgui" "$appdir/usr/bin/lv-imgui"
    chmod +x "$appdir/usr/bin/lv-imgui"

    cp pkg/lv.desktop "$appdir/usr/share/applications/"
    cp pkg/lv.svg "$appdir/usr/share/icons/hicolor/scalable/apps/"
    ln -sf usr/share/applications/lv.desktop "$appdir/lv.desktop"
    ln -sf usr/share/icons/hicolor/scalable/apps/lv.svg "$appdir/lv.svg"

    # Bundle shared libs
    local sysroot=""
    if [ "$arch" = "aarch64" ]; then
        sysroot="/usr/aarch64-linux-gnu"
    fi

    for lib in libSDL2 libmpv; do
        local so=""
        if [ -n "$sysroot" ]; then
            so=$(find "$sysroot/lib" /usr/lib/aarch64-linux-gnu -name "${lib}*.so*" -not -name "*.a" 2>/dev/null | head -1)
        else
            so=$(ldconfig -p | grep "$lib" | head -1 | awk '{print $NF}')
        fi
        if [ -n "$so" ]; then
            cp "$so" "$appdir/usr/lib/"
            echo "    bundled: $so"
        else
            echo "    WARNING: $lib not found"
        fi
    done

    # AppRun wrapper
    cat > "$appdir/AppRun" << 'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$HERE/usr/lib:${LD_LIBRARY_PATH:-}"
exec "$HERE/usr/bin/lv-imgui" "$@"
APPRUN
    chmod +x "$appdir/AppRun"

    # Download appimagetool for the HOST arch (always x86_64 in this container)
    local tool="/tmp/appimagetool"
    if [ ! -f "$tool" ]; then
        curl -fsSL "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" -o "$tool"
        chmod +x "$tool"
    fi

    # Build (--appimage-extract-and-run avoids FUSE requirement in Docker)
    ARCH="$arch" "$tool" --appimage-extract-and-run "$appdir" "$OUTDIR/lv-${arch}.AppImage"
    rm -rf "$appdir"
    echo "  → $OUTDIR/lv-${arch}.AppImage"
}

# ── Main ──────────────────────────────────────────────────────────────
targets=("${@:-all}")

for target in "${targets[@]}"; do
    case "$target" in
        linux-x86_64|linux-x64)   build_linux_x86_64 ;;
        linux-aarch64|linux-arm64) build_linux_aarch64 ;;
        windows-x86_64|windows|win) build_windows_x86_64 ;;
        all)
            build_linux_x86_64
            build_linux_aarch64
            build_windows_x86_64
            ;;
        *)
            echo "Unknown target: $target"
            echo "Valid: linux-x86_64, linux-aarch64, windows-x86_64, all"
            exit 1
            ;;
    esac
done

echo ""
echo "══════ Build complete ══════"
ls -lh "$OUTDIR"/lv-* 2>/dev/null || true
