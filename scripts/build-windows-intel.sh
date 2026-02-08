#!/bin/bash
# Release build + NSIS installer for Windows (cross-compile via cargo-xwin)
set -eo pipefail
cd "$(dirname "$0")/.."

LV_VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"

cargo xwin build --release --target x86_64-pc-windows-msvc
mkdir -p build-installer
cp target/x86_64-pc-windows-msvc/release/lv-imgui.exe build-installer/
cp pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll pkg/installer.nsi build-installer/
cd build-installer && makensis -DLV_VERSION="$LV_VERSION" installer.nsi
echo "==> build-installer/lv-setup-${LV_VERSION}.exe"
