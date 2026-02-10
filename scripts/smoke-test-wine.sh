#!/bin/bash
# CLI smoke test for Windows .exe under Wine.
# Expects lv.exe + DLLs in the same directory, and test fixtures available.
# Usage: scripts/smoke-test-wine.sh <dir-with-lv.exe> <fixtures-dir>
set -eo pipefail

BIN_DIR="${1:?Usage: $0 <dir-with-lv.exe> <fixtures-dir>}"
FIXTURES="${2:?Usage: $0 <dir-with-lv.exe> <fixtures-dir>}"
BINARY="$BIN_DIR/lv.exe"
TMPDIR_SMOKE="$(mktemp -d /tmp/lv-smoke-wine.XXXXXX)"
DB_PATH="$TMPDIR_SMOKE/lv-smoke.db"

trap 'rm -rf "$TMPDIR_SMOKE"' EXIT

if [ ! -f "$BINARY" ]; then
  echo "FATAL: lv.exe not found: $BINARY" >&2
  exit 1
fi

run_lv() {
  WINEDEBUG=-all wine "$BINARY" "$@" 2>&1
}

PASS=0
FAIL=0
ok()   { PASS=$((PASS + 1)); echo "  ✅ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ❌ $1"; }

echo "=== lv CLI smoke tests (Wine) ==="
echo "Binary:   $BINARY"
echo "DB:       $DB_PATH"
echo "Fixtures: $FIXTURES"
echo ""

export LV_DB_PATH="$DB_PATH"

# ── 1. --help works ──────────────────────────────────────────────────
echo "--- 1. --help ---"
if run_lv --help >/dev/null 2>&1; then
  ok "--help exits 0"
else
  fail "--help failed (exit $?)"
fi

# ── 2. help output contains expected text ────────────────────────────
echo "--- 2. help content ---"
HELP=$(run_lv --help 2>&1)
if echo "$HELP" | grep -qi "track\|scan\|watch"; then
  ok "help mentions subcommands"
else
  fail "help output missing expected subcommands"
fi

# ── 3. track a directory ─────────────────────────────────────────────
echo "--- 3. track ---"
if run_lv track "$FIXTURES" 2>&1; then
  ok "track $FIXTURES"
else
  fail "track failed (exit $?)"
fi

# ── 4. status shows tracked dir ─────────────────────────────────────
echo "--- 4. status ---"
STATUS=$(run_lv status 2>&1)
echo "$STATUS" | head -10 | sed 's/^/  /'
if echo "$STATUS" | grep -qi "track\|dir\|file"; then
  ok "status output looks valid"
else
  fail "status output unexpected"
fi

# ── 5. scan discovers files ──────────────────────────────────────────
echo "--- 5. scan ---"
if run_lv scan 2>&1; then
  ok "scan completed"
else
  fail "scan failed (exit $?)"
fi

# ── 6. untrack ────────────────────────────────────────────────────────
echo "--- 6. untrack ---"
if run_lv untrack "$FIXTURES" 2>&1; then
  ok "untrack $FIXTURES"
else
  fail "untrack failed (exit $?)"
fi

# ── 7. DB file was created ───────────────────────────────────────────
echo "--- 7. DB file ---"
if [ -f "$DB_PATH" ]; then
  DB_SIZE=$(stat -c%s "$DB_PATH" 2>/dev/null || echo 0)
  ok "DB exists ($DB_SIZE bytes)"
else
  fail "DB file not created at $DB_PATH"
fi

# ── Summary ───────────────────────────────────────────────────────────
echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ "$FAIL" -gt 0 ]; then
  exit 1
fi
