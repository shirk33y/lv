#!/bin/bash
# Release build for x86_64 Linux
set -eo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target x86_64-unknown-linux-gnu
strip target/x86_64-unknown-linux-gnu/release/lv-imgui
