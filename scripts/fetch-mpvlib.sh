#!/bin/bash
# Download libmpv-2.dll for Windows x64 packaging.
# Source: shinchiro mpv-winbuild-cmake builds on SourceForge.
# Usage: bash scripts/fetch-mpvlib.sh [output_dir]
set -euo pipefail

OUT="${1:-pkg/win64}"
DLL="$OUT/libmpv-2.dll"

if [ -f "$DLL" ]; then
  echo "libmpv-2.dll already exists, skipping download"
  exit 0
fi

MPV_VERSION="20260201-git-40d2947"
URL="https://sourceforge.net/projects/mpv-player-windows/files/libmpv/mpv-dev-x86_64-${MPV_VERSION}.7z/download"

echo "==> Downloading mpv-dev-x86_64-${MPV_VERSION}.7z"
TMP=$(mktemp -d)
trap "rm -rf $TMP" EXIT

curl -fsSL -o "$TMP/mpv-dev.7z" -L "$URL"
7z e "$TMP/mpv-dev.7z" libmpv-2.dll -o"$TMP/extract" -y > /dev/null

mkdir -p "$OUT"
mv "$TMP/extract/libmpv-2.dll" "$DLL"
echo "==> Saved: $DLL ($(du -h "$DLL" | cut -f1))"
