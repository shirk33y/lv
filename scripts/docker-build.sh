#!/bin/bash
# Build lv for all targets using Docker.
#
# Usage:
#   bash scripts/docker-build.sh                    # build all targets
#   bash scripts/docker-build.sh linux-x86_64       # single target
#   bash scripts/docker-build.sh linux-arm64 win    # multiple targets
#
# Outputs go to ./dist/
#
# First run builds the Docker image (~5min), subsequent runs reuse it.
# Add --rebuild to force image rebuild.
set -euo pipefail

cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"

IMAGE="lv-builder"
REBUILD=false

# Parse flags
args=()
for arg in "$@"; do
    if [ "$arg" = "--rebuild" ]; then
        REBUILD=true
    else
        args+=("$arg")
    fi
done

# Build image if needed
if [ "$REBUILD" = true ] || ! docker image inspect "$IMAGE" &>/dev/null; then
    echo "══════ Building Docker image ══════"
    docker build -t "$IMAGE" docker/
fi

# Run build
echo "══════ Starting build ══════"
docker run --rm \
    -v "$PWD:/src" \
    -v "lv-cargo-cache:/root/.cargo/registry" \
    -v "lv-cargo-git:/root/.cargo/git" \
    -v "lv-target-cache:/src/target" \
    "$IMAGE" "${args[@]:-all}"
