#!/bin/bash
# Release build + NSIS installer for Windows (cross-compile via cargo-xwin)
set -eo pipefail
cd "$(dirname "$0")/.."

LV_VERSION="${LV_VERSION:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')-$(date -u +%Y%m%dT%H%M%S)-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)}"

# Fetch libmpv-2.dll if not present
bash scripts/fetch-mpvlib.sh

cargo xwin build --release --target x86_64-pc-windows-msvc
mkdir -p build-installer
cp target/x86_64-pc-windows-msvc/release/lv.exe build-installer/
cp pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll pkg/win64/lv.ico pkg/installer.nsi build-installer/
cd build-installer && makensis -DLV_VERSION="$LV_VERSION" installer.nsi
echo "==> build-installer/lv-setup-${LV_VERSION}.exe"
