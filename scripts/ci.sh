#!/bin/bash
# CI: test + clippy + fmt in parallel
set -eo pipefail
cd "$(dirname "$0")/.."
printf '%s\0' 'cargo test' 'cargo clippy -- -D warnings' 'cargo fmt -- --check' | bash scripts/multi.sh
