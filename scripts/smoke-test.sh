#!/bin/bash
# Smoke test: launch lv with test fixtures, take screenshots, quit.
#
# Requirements: xdotool, import (ImageMagick), a running X/Wayland session.
# Usage: scripts/smoke-test.sh [--update-reference] [--binary PATH]
#
# Screenshots are saved to test/screenshots/actual/.
# With --update-reference, they are also copied to test/screenshots/reference/.
# A pixel-diff summary is printed at the end (requires ImageMagick compare).

set -eo pipefail
cd "$(dirname "$0")/.."

UPDATE_REF=false
CUSTOM_BINARY=""
USE_WINE=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --update-reference) UPDATE_REF=true; shift ;;
        --binary) CUSTOM_BINARY="$2"; shift 2 ;;
        --wine) USE_WINE=true; shift ;;
        *) shift ;;
    esac
done

# ── Paths ────────────────────────────────────────────────────────────────
FIXTURES="$(pwd)/test/fixtures"
ACTUAL="$(pwd)/test/screenshots/actual"
REFERENCE="$(pwd)/test/screenshots/reference"
TMPDIR_SMOKE="$(mktemp -d /tmp/lv-smoke.XXXXXX)"
DB_PATH="$TMPDIR_SMOKE/lv-smoke.db"
BINARY="${CUSTOM_BINARY:-$(pwd)/target-linux-intel/debug/lv-imgui}"

# Wine wrapper: prepend wine to all binary invocations
if $USE_WINE; then
    run_bin() { wine "$BINARY" "$@"; }
else
    run_bin() { "$BINARY" "$@"; }
fi

trap 'rm -rf "$TMPDIR_SMOKE"' EXIT

# ── Preflight ────────────────────────────────────────────────────────────
for cmd in xdotool import; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "FATAL: $cmd not found. Install it first." >&2
        exit 1
    fi
done

if [[ ! -f "$BINARY" ]]; then
    if [[ -n "$CUSTOM_BINARY" ]]; then
        echo "FATAL: binary not found: $BINARY" >&2
        exit 1
    fi
    echo "Building lv-imgui (debug)..."
    CARGO_TARGET_DIR=target-linux-intel cargo build 2>&1 | tail -3
fi

# ── Generate test fixtures if missing ────────────────────────────────────
if [[ ! -f "$FIXTURES/red_800x600.png" ]]; then
    echo "Generating test fixtures..."
    python3 "$FIXTURES/generate.py"
fi

mkdir -p "$ACTUAL"

# ── Helper: screenshot the lv window ─────────────────────────────────────
screenshot() {
    local name="$1"
    local wid="$2"
    import -window "$wid" "$ACTUAL/$name"
    echo "  📸 $name"
}

# ── Helper: wait for window to appear ────────────────────────────────────
wait_for_window() {
    local attempts=0
    while true; do
        # SDL2 window starts as "lv", then updates to "[1/N] file — dir — lv ..."
        # Search by exact name first (initial), then by pattern (after title update)
        WID=$(xdotool search --name '^lv$' 2>/dev/null | head -1 || true)
        [[ -z "$WID" ]] && WID=$(xdotool search --name ' — lv ' 2>/dev/null | head -1 || true)
        if [[ -n "$WID" ]]; then
            echo "  Window found: $WID"
            return 0
        fi
        attempts=$((attempts + 1))
        if [[ $attempts -ge 50 ]]; then
            echo "FATAL: lv window did not appear within 5s" >&2
            return 1
        fi
        sleep 0.1
    done
}

# ── Run ──────────────────────────────────────────────────────────────────
echo "=== lv smoke test ==="
echo "DB:       $DB_PATH"
echo "Fixtures: $FIXTURES"
echo ""

# Track fixtures and launch GUI
export LV_DB_PATH="$DB_PATH"
run_bin track "$FIXTURES" 2>/dev/null
echo "Tracked $(ls "$FIXTURES"/*.png | wc -l) test images"

# Launch app in background pointing at fixtures dir
run_bin "$FIXTURES" 2>"$TMPDIR_SMOKE/stderr.log" &
APP_PID=$!
echo "Launched lv (PID $APP_PID)"

# Wait for window
if ! wait_for_window; then
    kill "$APP_PID" 2>/dev/null || true
    cat "$TMPDIR_SMOKE/stderr.log"
    exit 1
fi

sleep 0.5  # let first frame render

# ── Screenshot 1: initial state ──────────────────────────────────────────
echo ""
echo "--- Screenshot 1: initial state ---"
T0=$(date +%s%N)
screenshot "01_initial.png" "$WID"
T1=$(date +%s%N)
echo "  Capture time: $(( (T1 - T0) / 1000000 ))ms"

# ── Navigate: press j (next image) ───────────────────────────────────────
echo ""
echo "--- Navigate: j (next) ---"
xdotool key --window "$WID" j
sleep 0.5

# ── Screenshot 2: after navigation ──────────────────────────────────────
echo "--- Screenshot 2: after j ---"
screenshot "02_after_nav.png" "$WID"

# ── Wait 5 seconds (let app settle, detect freezes) ─────────────────────
echo ""
echo "--- Waiting 5s... ---"
sleep 5

# ── Screenshot 3: after 5s idle ─────────────────────────────────────────
echo "--- Screenshot 3: after 5s idle ---"
screenshot "03_after_5s.png" "$WID"

# ── Navigate: press j twice more ────────────────────────────────────────
echo ""
echo "--- Navigate: j j (skip 2) ---"
xdotool key --window "$WID" j
sleep 0.3
xdotool key --window "$WID" j
sleep 0.5

# ── Screenshot 4: different image ───────────────────────────────────────
echo "--- Screenshot 4: after 2x j ---"
screenshot "04_navigated.png" "$WID"

# ── Quit ─────────────────────────────────────────────────────────────────
echo ""
echo "--- Sending q to quit ---"
T_QUIT0=$(date +%s%N)
xdotool key --window "$WID" q

# Wait for process to exit (measure close time)
QUIT_TIMEOUT=10
QUIT_ELAPSED=0
while kill -0 "$APP_PID" 2>/dev/null; do
    sleep 0.1
    QUIT_ELAPSED=$((QUIT_ELAPSED + 1))
    if [[ $QUIT_ELAPSED -ge $((QUIT_TIMEOUT * 10)) ]]; then
        echo "WARNING: app did not exit within ${QUIT_TIMEOUT}s, killing" >&2
        kill -9 "$APP_PID" 2>/dev/null || true
        break
    fi
done
T_QUIT1=$(date +%s%N)
QUIT_MS=$(( (T_QUIT1 - T_QUIT0) / 1000000 ))
echo "  Quit time: ${QUIT_MS}ms"

if [[ $QUIT_MS -gt 2000 ]]; then
    echo "⚠️  SLOW QUIT: ${QUIT_MS}ms (>2s)" >&2
fi

# ── Stderr log ───────────────────────────────────────────────────────────
echo ""
echo "--- App stderr (last 20 lines) ---"
tail -20 "$TMPDIR_SMOKE/stderr.log" | sed 's/^/  /'

# ── Update reference screenshots ─────────────────────────────────────────
if $UPDATE_REF; then
    echo ""
    echo "--- Updating reference screenshots ---"
    mkdir -p "$REFERENCE"
    cp "$ACTUAL"/*.png "$REFERENCE/"
    echo "  Copied $(ls "$ACTUAL"/*.png | wc -l) screenshots to reference/"
fi

# ── Visual diff ──────────────────────────────────────────────────────────
echo ""
echo "--- Visual diff (actual vs reference) ---"
HAS_DIFF=false
if [[ -d "$REFERENCE" ]] && ls "$REFERENCE"/*.png &>/dev/null; then
    for ref_img in "$REFERENCE"/*.png; do
        name=$(basename "$ref_img")
        actual_img="$ACTUAL/$name"
        if [[ ! -f "$actual_img" ]]; then
            echo "  MISSING: $name (no actual screenshot)"
            HAS_DIFF=true
            continue
        fi
        if command -v compare &>/dev/null; then
            # ImageMagick compare: output pixel difference metric
            DIFF=$(compare -metric AE "$ref_img" "$actual_img" /dev/null 2>&1 || true)
            if [[ "$DIFF" == "0" ]]; then
                echo "  ✅ $name — identical"
            else
                echo "  ❌ $name — $DIFF pixels differ"
                # Generate diff image
                compare "$ref_img" "$actual_img" "$ACTUAL/diff_${name}" 2>/dev/null || true
                HAS_DIFF=true
            fi
        else
            # Fallback: byte comparison
            if cmp -s "$ref_img" "$actual_img"; then
                echo "  ✅ $name — identical (byte-level)"
            else
                echo "  ❌ $name — differs (byte-level)"
                HAS_DIFF=true
            fi
        fi
    done
else
    echo "  No reference screenshots yet. Run with --update-reference first."
fi

echo ""
if $HAS_DIFF; then
    echo "RESULT: ❌ Visual differences detected"
    echo "  Check test/screenshots/actual/ for current screenshots"
    echo "  Check test/screenshots/actual/diff_* for diff images"
    exit 1
else
    echo "RESULT: ✅ All screenshots match (or no reference yet)"
fi
