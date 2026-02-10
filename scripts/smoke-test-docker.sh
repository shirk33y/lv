#!/bin/bash
# Build & smoke-test all targets via multi-stage Dockerfiles.
#
# Usage:
#   scripts/smoke-test-docker.sh              # all targets
#   scripts/smoke-test-docker.sh linux-x86_64 # single target
#   scripts/smoke-test-docker.sh --build-only # build without smoke test
#
# Each Dockerfile has 3 stages: build → smoke → out
#   --target=smoke  builds + runs CLI smoke tests
#   --target=out    extracts artifacts to dist/
set -eo pipefail
cd "$(dirname "$0")/.."

BUILD_ONLY=false
TARGETS=()

for arg in "$@"; do
  case "$arg" in
    --build-only) BUILD_ONLY=true ;;
    *) TARGETS+=("$arg") ;;
  esac
done

ALL_TARGETS=("linux-x86_64" "linux-aarch64" "windows-x86_64")
if [ ${#TARGETS[@]} -eq 0 ]; then
  TARGETS=("${ALL_TARGETS[@]}")
fi

PASS=0
FAIL=0

for target in "${TARGETS[@]}"; do
  DOCKERFILE="docker/Dockerfile.${target}"
  if [ ! -f "$DOCKERFILE" ]; then
    echo "FATAL: $DOCKERFILE not found" >&2
    exit 1
  fi

  # ── Build + smoke test ──────────────────────────────────────────────
  if [ "$BUILD_ONLY" = true ]; then
    STAGE="out"
    echo "─── $target (build only) ───"
  else
    # arm64 smoke stage needs QEMU binfmt (build stage cross-compiles natively)
    if [[ "$target" == *aarch64* ]] && ! grep -q aarch64 /proc/sys/fs/binfmt_misc/qemu-aarch64 2>/dev/null; then
      echo "─── $target (build only — QEMU binfmt not registered, skipping smoke) ───"
      echo "  To enable smoke tests: docker run --privileged --rm tonistiigi/binfmt --install arm64"
      STAGE="out"
    else
      STAGE="smoke"
      echo "─── $target (build + smoke) ───"
    fi
  fi

  if docker build -f "$DOCKERFILE" --target="$STAGE" . 2>&1 | sed "s/^/  [$target] /"; then
    echo "  ✅ $target $STAGE passed"
    PASS=$((PASS + 1))

    # Extract artifacts after successful build
    echo "  → Extracting artifacts to dist/"
    docker build -f "$DOCKERFILE" --target=out -o dist . 2>&1 | tail -1 | sed "s/^/  [$target] /"
  else
    echo "  ❌ $target $STAGE FAILED"
    FAIL=$((FAIL + 1))
  fi

  echo ""
done

# ── Summary ───────────────────────────────────────────────────────────
TOTAL=$((PASS + FAIL))
echo "=== Results: $PASS passed, $FAIL failed ($TOTAL targets) ==="
if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
