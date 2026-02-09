#!/bin/bash
# Release build + AppImage for x86_64 Linux
set -eo pipefail
cd "$(dirname "$0")/.."

export LV_VERSION="${LV_VERSION:-$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')-$(date -u +%Y%m%dT%H%M%S)-$(git rev-parse --short HEAD 2>/dev/null || echo unknown)}"

cargo build --release --target x86_64-unknown-linux-gnu
strip target/x86_64-unknown-linux-gnu/release/lv
./pkg/appimage.sh x86_64 target/x86_64-unknown-linux-gnu/release/lv
./pkg/deb.sh amd64 target/x86_64-unknown-linux-gnu/release/lv
