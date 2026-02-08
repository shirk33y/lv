#!/bin/bash
# Dockerized cross-builds → dist/
# Usage: scripts/docker-build.sh <target>
#   targets: linux-intel, linux-arm, windows-intel, all
set -eo pipefail
cd "$(dirname "$0")/.."

case "${1:-all}" in
    linux-intel)
        docker build -f docker/Dockerfile.linux-x86_64 -o dist . ;;
    linux-arm)
        docker build -f docker/Dockerfile.linux-aarch64 -o dist . ;;
    windows-intel)
        docker build -f docker/Dockerfile.windows-x86_64 -o dist . ;;
    all)
        docker build -f docker/Dockerfile.linux-x86_64 -o dist .
        docker build -f docker/Dockerfile.linux-aarch64 -o dist .
        docker build -f docker/Dockerfile.windows-x86_64 -o dist . ;;
    *)
        echo "Usage: $0 {linux-intel|linux-arm|windows-intel|all}" >&2
        exit 1 ;;
esac
