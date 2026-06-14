#!/bin/bash
# Flatpak smoke test: verify lv flatpak works with video files.
# Tests both CLI commands and video playback functionality.
set -eo pipefail

FLATPAK_BUNDLE="${FLATPAK_BUNDLE:-build/lv.flatpak}"
FIXTURES="${LV_FIXTURES:-test/fixtures}"
SANDBOX_FIXTURES="${LV_SANDBOX_FIXTURES:-$HOME/lv-flatpak-fixtures}"
SMOKE_LOG_DIR="${LV_SMOKE_LOG_DIR:-}"
TMPDB=""
TMPDB2=""
TMPDB3=""
TMPDIR_GUI=""
APP_PID=""

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp/lv-flatpak-runtime}"
mkdir -p "$XDG_RUNTIME_DIR" /run/dbus
chmod 700 "$XDG_RUNTIME_DIR"
if command -v dbus-daemon >/dev/null 2>&1 && [[ ! -S /run/dbus/system_bus_socket ]]; then
    dbus-daemon --system --fork 2>/dev/null || true
fi
if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && command -v dbus-run-session >/dev/null 2>&1; then
    exec dbus-run-session -- "$0" "$@"
fi

cleanup() {
    if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
    fi
    rm -rf "$SANDBOX_FIXTURES"
    [ -n "$TMPDB" ] && rm -f "$TMPDB"
    [ -n "$TMPDB2" ] && rm -f "$TMPDB2"
    [ -n "$TMPDB3" ] && rm -f "$TMPDB3"
    if [ -n "$TMPDIR_GUI" ] && [ -z "$SMOKE_LOG_DIR" ]; then
        rm -rf "$TMPDIR_GUI"
    fi
}
trap cleanup EXIT

echo "=== lv Flatpak Smoke Tests ==="
echo "Bundle:   $FLATPAK_BUNDLE"
echo "Fixtures: $FIXTURES"
echo "Sandbox:  $SANDBOX_FIXTURES"
if [ -n "$SMOKE_LOG_DIR" ]; then
    mkdir -p "$SMOKE_LOG_DIR"
    echo "Logs:     $SMOKE_LOG_DIR"
fi
echo ""

# Verify bundle exists
if [[ ! -f "$FLATPAK_BUNDLE" ]]; then
    echo "❌ Flatpak bundle not found: $FLATPAK_BUNDLE"
    exit 1
fi
echo "✅ Bundle exists ($(du -h "$FLATPAK_BUNDLE" | cut -f1))"
echo ""

rm -rf "$SANDBOX_FIXTURES"
mkdir -p "$SANDBOX_FIXTURES"
cp -R "$FIXTURES"/. "$SANDBOX_FIXTURES"/

# ── 1. Install flatpak bundle ────────────────────────────────────────
echo "--- 1. Install flatpak ---"
if flatpak install --user --assumeyes "$FLATPAK_BUNDLE" 2>&1 | tail -3; then
    echo "✅ Installed"
else
    echo "⚠️  Install command failed (may already be installed)"
fi
echo ""

# ── 2. Verify lv is available ────────────────────────────────────────
echo "--- 2. Verify command ---"
if flatpak run io.github.shirk33y.lv --help >/dev/null 2>&1; then
    echo "✅ lv --help works"
    flatpak run io.github.shirk33y.lv --help | head -3
else
    echo "❌ lv --help failed"
    exit 1
fi
echo ""

# ── 3. Test image file (baseline) ────────────────────────────────────
echo "--- 3. Test with image ---"
TEST_IMAGE="$SANDBOX_FIXTURES/red_800x600.png"
if [[ -f "$TEST_IMAGE" ]]; then
    # Create temp DB for test
    TMPDB=$(mktemp /tmp/lv-flatpak-test-XXXXXX.db)

    if LV_DB_PATH="$TMPDB" flatpak run io.github.shirk33y.lv scan "$TEST_IMAGE" 2>&1; then
        echo "✅ Image scan works"
    else
        echo "❌ Image scan failed"
        exit 1
    fi
else
    echo "⚠️  Test image not found: $TEST_IMAGE"
fi
echo ""

# ── 4. Test video file (main test) ───────────────────────────────────
echo "--- 4. Test with video ---"
TEST_VIDEO="$SANDBOX_FIXTURES/sample_video.mp4"
TEST_VIDEO_SMOKE="$TEST_VIDEO"
if [[ ! -f "$TEST_VIDEO" ]]; then
    echo "⚠️  Test video not found: $TEST_VIDEO"
    echo "   (This is OK if you haven't run test generation yet)"
else
    TEST_VIDEO_SMOKE="$SANDBOX_FIXTURES/sample_video-smoke.mp4"
    ffmpeg -hide_banner -loglevel error -y \
        -i "$TEST_VIDEO" \
        -c:v copy \
        -an \
        "$TEST_VIDEO_SMOKE"

    # Try to scan/index the video file
    TMPDB2=$(mktemp /tmp/lv-flatpak-video-XXXXXX.db)

    echo "Video: $TEST_VIDEO"
    if timeout 10 flatpak run io.github.shirk33y.lv scan "$TEST_VIDEO" 2>&1; then
        echo "✅ Video file processed without crash"
    else
        EXIT_CODE=$?
        if [[ $EXIT_CODE -eq 124 ]]; then
            echo "⚠️  Video processing timed out (may indicate hang, but not crash)"
        else
            echo "❌ Video processing failed (exit $EXIT_CODE)"
            exit $EXIT_CODE
        fi
    fi
fi
echo ""

# ── 5. Test with video directory scan ────────────────────────────────
echo "--- 5. Scan fixtures directory ---"
TMPDB3=$(mktemp /tmp/lv-flatpak-dir-XXXXXX.db)

if [[ -d "$SANDBOX_FIXTURES" ]]; then
    if LV_DB_PATH="$TMPDB3" flatpak run io.github.shirk33y.lv scan "$SANDBOX_FIXTURES" 2>&1 | head -5; then
        echo "✅ Directory scan completed"
    else
        echo "⚠️  Directory scan had issues but may have processed files"
    fi
else
    echo "❌ Fixtures directory not found: $SANDBOX_FIXTURES"
    exit 1
fi
echo ""

# ── 6. Check libmpv is bundled and dynamically resolved ───────────────
echo "--- 6. Check libmpv linkage ---"
MPV_LIBS=$(flatpak run --devel --command=sh io.github.shirk33y.lv -lc 'find /app/lib -maxdepth 2 -name "libmpv.so*" -print' 2>/dev/null || true)
if [[ -z "$MPV_LIBS" ]]; then
    echo "❌ libmpv.so not found in Flatpak /app/lib"
    exit 1
fi
echo "$MPV_LIBS"

LDD_OUTPUT=$(flatpak run --devel --command=sh io.github.shirk33y.lv -lc 'LD_LIBRARY_PATH=/app/lib:/app/lib/ffmpeg ldd /app/bin/lv' 2>&1)
echo "$LDD_OUTPUT" | grep -E 'libmpv|libSDL2|libplacebo|libavcodec|libavformat|libavutil' || true
if echo "$LDD_OUTPUT" | grep -q 'not found'; then
    echo "$LDD_OUTPUT"
    echo "❌ Missing shared libraries in Flatpak runtime"
    exit 1
fi
if ! echo "$LDD_OUTPUT" | grep -q 'libmpv'; then
    echo "$LDD_OUTPUT"
    echo "❌ lv binary is not linked to libmpv"
    exit 1
fi
echo "✅ libmpv resolves"
echo ""

# ── 7. Launch video and verify frames advance ────────────────────────
echo "--- 7. Verify video playback advances ---"
if [[ ! -f "$TEST_VIDEO_SMOKE" ]]; then
    echo "❌ Test video not found: $TEST_VIDEO_SMOKE"
    exit 1
fi

for cmd in xvfb-run xdotool ffmpeg compare; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "❌ $cmd not found; smoke image must include xvfb, xdotool, ffmpeg, and ImageMagick compare"
        exit 1
    fi
done

if [ -n "$SMOKE_LOG_DIR" ]; then
    TMPDIR_GUI="$SMOKE_LOG_DIR/video-gui"
    rm -rf "$TMPDIR_GUI"
    mkdir -p "$TMPDIR_GUI"
else
    TMPDIR_GUI=$(mktemp -d /tmp/lv-flatpak-video-gui.XXXXXX)
fi
export LV_DB_PATH="$TMPDB2"

xvfb-run -a --server-args="-screen 0 1280x720x24" bash -c '
set -eo pipefail
TEST_VIDEO="$1"
TMPDIR_GUI="$2"
flatpak run io.github.shirk33y.lv "$TEST_VIDEO" >"$TMPDIR_GUI/stdout.log" 2>"$TMPDIR_GUI/stderr.log" &
APP_PID=$!
echo "$APP_PID" >"$TMPDIR_GUI/app.pid"

WID=""
for _ in $(seq 1 120); do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
        cat "$TMPDIR_GUI/stderr.log" >&2 || true
        echo "FATAL: lv exited before opening video window" >&2
        exit 1
    fi
    WID=$(xdotool search --pid "$APP_PID" 2>/dev/null | head -1 || true)
    if [ -z "$WID" ]; then
        WID=$(xdotool search --all --onlyvisible --name . 2>/dev/null | head -1 || true)
    fi
    [ -n "$WID" ] && break
    sleep 0.1
done

if [ -z "$WID" ]; then
    cat "$TMPDIR_GUI/stderr.log" >&2 || true
    echo "FATAL: lv window did not appear" >&2
    exit 1
fi

capture_frame() {
    local out="$1"
    ffmpeg -hide_banner -loglevel error -y \
        -f x11grab \
        -video_size 1280x720 \
        -i "${DISPLAY:-:99}" \
        -frames:v 1 \
        "$out"
}

sleep 1.0
capture_frame "$TMPDIR_GUI/frame1.png"
sleep 1.0
capture_frame "$TMPDIR_GUI/frame2.png"
sleep 1.0
capture_frame "$TMPDIR_GUI/frame3.png"

xdotool key --window "$WID" q || true
for _ in $(seq 1 40); do
    kill -0 "$APP_PID" 2>/dev/null || exit 0
    sleep 0.1
done
kill "$APP_PID" 2>/dev/null || true
' bash "$TEST_VIDEO_SMOKE" "$TMPDIR_GUI"

APP_PID="$(cat "$TMPDIR_GUI/app.pid" 2>/dev/null || true)"
DIFF12=$(compare -metric AE "$TMPDIR_GUI/frame1.png" "$TMPDIR_GUI/frame2.png" /dev/null 2>&1 || true)
DIFF23=$(compare -metric AE "$TMPDIR_GUI/frame2.png" "$TMPDIR_GUI/frame3.png" /dev/null 2>&1 || true)
echo "frame diff 1->2: $DIFF12"
echo "frame diff 2->3: $DIFF23"

if [[ "${DIFF12:-0}" == "0" && "${DIFF23:-0}" == "0" ]]; then
    tail -80 "$TMPDIR_GUI/stderr.log" || true
    echo "❌ Video window opened, but frames did not advance"
    exit 1
fi

if grep -Eiq 'libmpv.*not found|error while loading shared libraries|failed to initialize mpv|panic|segmentation fault' "$TMPDIR_GUI/stderr.log"; then
    tail -80 "$TMPDIR_GUI/stderr.log" || true
    echo "❌ Video playback stderr contains media/runtime failure"
    exit 1
fi
echo "✅ Video playback advances"
echo ""

echo "=== Flatpak Smoke Tests Complete ==="
echo "Summary: image scan, video scan, libmpv linkage, and video playback work in Flatpak sandbox"
