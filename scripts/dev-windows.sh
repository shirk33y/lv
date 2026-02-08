#!/bin/bash
# Dev build + run (Windows/WSL), separate target dir
set -eo pipefail
cd "$(dirname "$0")/.."

WIN_TARGET_PARENT="/mnt/c/Users/$USER/AppData/Local/lv-dev"

if grep -qi microsoft /proc/version 2>/dev/null; then
    echo ":: building via Windows cargo.exe …"
    mkdir -p "$WIN_TARGET_PARENT"
    WIN_TD=$(wslpath -w "$WIN_TARGET_PARENT/target-windows-intel")
    cargo.exe build --target-dir "$WIN_TD"
    echo ":: copying DLLs …"
    cp -u pkg/win64/SDL2.dll pkg/win64/libmpv-2.dll \
        "$WIN_TARGET_PARENT/target-windows-intel/debug/"
    echo ":: launching …"
    "$WIN_TARGET_PARENT/target-windows-intel/debug/lv-imgui.exe" "$@"
else
    echo ":: building + running for Windows (native) …"
    cargo run --target-dir target-windows-intel -- "$@"
fi
