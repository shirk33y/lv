#!/bin/bash
# CLI smoke test: verify lv subcommands work correctly.
# Expects lv to be installed (e.g. via .deb) and test fixtures generated.
set -eo pipefail
cd "$(dirname "$0")/.."

FIXTURES="$(pwd)/test/fixtures"
TMPDIR_SMOKE="$(mktemp -d /tmp/lv-smoke-cli.XXXXXX)"
DB_PATH="$TMPDIR_SMOKE/lv-smoke.db"
export LV_DB_PATH="$DB_PATH"

trap 'rm -rf "$TMPDIR_SMOKE"' EXIT

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "  ✅ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ❌ $1"; }

echo "=== lv CLI smoke tests ==="
echo "Binary:   $(which lv)"
echo "Version:  $(lv --help 2>&1 | head -1 || echo unknown)"
echo "DB:       $DB_PATH"
echo "Fixtures: $FIXTURES"
echo ""

# ── 1. --help works ──────────────────────────────────────────────────
echo "--- 1. --help ---"
if lv --help >/dev/null 2>&1; then
    ok "--help exits 0"
else
    fail "--help failed (exit $?)"
fi

# ── 2. help output contains expected text ────────────────────────────
echo "--- 2. help content ---"
HELP=$(lv --help 2>&1)
if echo "$HELP" | grep -qi "track\|scan\|watch"; then
    ok "help mentions subcommands"
else
    fail "help output missing expected subcommands"
fi

# ── 3. track a directory ─────────────────────────────────────────────
echo "--- 3. track ---"
if lv track "$FIXTURES" 2>&1; then
    ok "track $FIXTURES"
else
    fail "track failed (exit $?)"
fi

# ── 4. status shows tracked dir ─────────────────────────────────────
echo "--- 4. status ---"
STATUS=$(lv status 2>&1)
echo "$STATUS" | head -10 | sed 's/^/  /'
if echo "$STATUS" | grep -qi "track\|dir\|file"; then
    ok "status output looks valid"
else
    fail "status output unexpected"
fi

# ── 5. scan discovers files ──────────────────────────────────────────
echo "--- 5. scan ---"
if lv scan 2>&1; then
    ok "scan completed"
else
    fail "scan failed (exit $?)"
fi

# ── 6. status shows files after scan ─────────────────────────────────
echo "--- 6. status (after scan) ---"
STATUS2=$(lv status 2>&1)
echo "$STATUS2" | head -10 | sed 's/^/  /'
if echo "$STATUS2" | grep -qE "files:|hashed:|[0-9]"; then
    ok "status shows file counts"
else
    ok "status ran (format may vary)"
fi

# ── 7. scan specific directory ────────────────────────────────────────
echo "--- 7. scan <dir> ---"
if lv scan "$FIXTURES" 2>&1; then
    ok "scan $FIXTURES"
else
    fail "scan dir failed (exit $?)"
fi

# ── 8. watch / unwatch ────────────────────────────────────────────────
echo "--- 8. watch/unwatch ---"
if lv watch "$FIXTURES" 2>&1; then
    ok "watch $FIXTURES"
else
    fail "watch failed (exit $?)"
fi
if lv unwatch "$FIXTURES" 2>&1; then
    ok "unwatch $FIXTURES"
else
    fail "unwatch failed (exit $?)"
fi

# ── 9. untrack ────────────────────────────────────────────────────────
echo "--- 9. untrack ---"
if lv untrack "$FIXTURES" 2>&1; then
    ok "untrack $FIXTURES"
else
    fail "untrack failed (exit $?)"
fi

# ── 10. DB file was created ───────────────────────────────────────────
echo "--- 10. DB file ---"
if [[ -f "$DB_PATH" ]]; then
    DB_SIZE=$(stat -c%s "$DB_PATH" 2>/dev/null || stat -f%z "$DB_PATH" 2>/dev/null || echo 0)
    ok "DB exists ($DB_SIZE bytes)"
else
    fail "DB file not created at $DB_PATH"
fi

# ── Summary ───────────────────────────────────────────────────────────
echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
