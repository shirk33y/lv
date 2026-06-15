#!/bin/bash
# Build lv as a Flatpak natively (no Docker/Podman)
# Requires: flatpak, flatpak-builder, python3, jq
set -euo pipefail

APP_ID="io.github.shirk33y.lv"
BUILD_DIR="${BUILD_DIR:-build}"
FLATPAK_BUNDLE="${FLATPAK_BUNDLE:-$BUILD_DIR/lv.flatpak}"
FLATPAK_STATE_DIR="${FLATPAK_STATE_DIR:-$BUILD_DIR/flatpak-state}"
FLATPAK_FORCE_CLEAN="${FLATPAK_FORCE_CLEAN:-1}"

cd "$(dirname "$0")/.."

mkdir -p "$BUILD_DIR" "$FLATPAK_STATE_DIR"

echo "==> [1/3] Generating cargo-sources.json..."
python3 extra/flatpak/flatpak-cargo-generator.py Cargo.lock -o cargo-sources.json

echo "==> Merging cargo sources into manifest..."
jq --slurpfile cargo_sources cargo-sources.json \
    '.modules[] |= if .name == "lv" then .sources = ($cargo_sources[0] + (.sources | map(select(.path != "cargo-sources.json")))) else . end' \
    extra/flatpak/"$APP_ID.json" | \
    jq '.modules[] |= if .name == "lv" then .sources |= map(if .path and (.path | startswith("../")) then .path |= .[3:] else . end) else . end' \
    > "$BUILD_DIR/$APP_ID.json"

rm -f cargo-sources.json

echo "==> [2/3] Running flatpak-builder..."
ostree --repo="$BUILD_DIR/repo" init --mode=archive-z2 2>/dev/null || true
ostree --repo="$BUILD_DIR/repo" config set core.min-free-space-percent 0

FORCE_CLEAN_ARGS=()
if [ "$FLATPAK_FORCE_CLEAN" != "0" ]; then
    FORCE_CLEAN_ARGS+=(--force-clean)
fi

flatpak-builder \
    --disable-rofiles-fuse \
    "${FORCE_CLEAN_ARGS[@]}" \
    --state-dir="$FLATPAK_STATE_DIR" \
    --repo="$BUILD_DIR/repo" \
    "$BUILD_DIR/flatpak-build" \
    "$BUILD_DIR/$APP_ID.json"

echo "==> [3/3] Creating bundle $FLATPAK_BUNDLE..."
flatpak build-bundle "$BUILD_DIR/repo" "$FLATPAK_BUNDLE" "$APP_ID"

echo "Done! Bundle: $(pwd)/$FLATPAK_BUNDLE"
