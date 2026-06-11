#!/bin/bash
# Remove build artifacts
set -eo pipefail
cd "$(dirname "$0")/.."

rm -rf dist/ build/ build-installer/ target-linux-intel/ target-linux-arm/ target-windows-intel/
rm -rf .flatpak-builder/ cargo-sources.json extra/flatpak/cargo-sources.json src/cargo-sources.json build.log

WIN_TARGET_PARENT="/mnt/c/Users/$USER/AppData/Local/lv-dev"
rm -rf "$WIN_TARGET_PARENT/target-windows-intel/" 2>/dev/null || true
