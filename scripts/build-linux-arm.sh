#!/bin/bash
# Release build for aarch64 Linux (cross-compile)
set -eo pipefail
cd "$(dirname "$0")/.."
PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu \
PKG_CONFIG_PATH=/usr/lib/aarch64-linux-gnu/pkgconfig \
cargo build --release --target aarch64-unknown-linux-gnu
aarch64-linux-gnu-strip target/aarch64-unknown-linux-gnu/release/lv-imgui
