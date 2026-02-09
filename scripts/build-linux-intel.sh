#!/bin/bash
# Release build + AppImage for x86_64 Linux
set -eo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target x86_64-unknown-linux-gnu
strip target/x86_64-unknown-linux-gnu/release/lv
./pkg/appimage.sh x86_64 target/x86_64-unknown-linux-gnu/release/lv
