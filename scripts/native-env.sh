#!/bin/bash
# Run a command with native-linker env for local libs.
# Usage:
#   scripts/native-env.sh cargo test
#   LV_NATIVE_PREFIXES=/opt/foo:/opt/bar scripts/native-env.sh cargo test
set -euo pipefail

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <command> [args...]" >&2
    exit 2
fi

prefixes=()
if [ -n "${LV_NATIVE_PREFIXES:-}" ]; then
    IFS=':' read -r -a prefixes <<< "${LV_NATIVE_PREFIXES}"
elif command -v brew >/dev/null 2>&1; then
    prefixes+=("$(brew --prefix)")
fi

if [ "${#prefixes[@]}" -eq 0 ]; then
    exec "$@"
fi

pkg_config_paths=()
lib_paths=()
for prefix in "${prefixes[@]}"; do
    pkg_config_paths+=(
        "$prefix/opt/sdl2/lib/pkgconfig"
        "$prefix/opt/mpv/lib/pkgconfig"
        "$prefix/opt/xorgproto/share/pkgconfig"
        "$prefix/opt/libx11/lib/pkgconfig"
        "$prefix/opt/libxext/lib/pkgconfig"
        "$prefix/opt/libxrandr/lib/pkgconfig"
        "$prefix/opt/libxfixes/lib/pkgconfig"
    )
    lib_paths+=(
        "$prefix/opt/sdl2/lib"
        "$prefix/opt/mpv/lib"
    )
done

pkg_config_path=$(IFS=:; printf '%s' "${pkg_config_paths[*]}")
lib_path=$(IFS=:; printf '%s' "${lib_paths[*]}")

export PKG_CONFIG_PATH="${pkg_config_path}${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
export LIBRARY_PATH="${lib_path}${LIBRARY_PATH:+:${LIBRARY_PATH}}"
export LD_LIBRARY_PATH="${lib_path}${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"

exec "$@"
