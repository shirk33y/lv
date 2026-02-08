#!/bin/bash
# Dev build + run (Linux), separate target dir
set -eo pipefail
cd "$(dirname "$0")/.."
CARGO_TARGET_DIR=target-linux-intel cargo run -- "$@"
