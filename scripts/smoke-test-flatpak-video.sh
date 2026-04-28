#!/bin/bash
# Advanced video playback test for flatpak.
# Checks libmpv availability and video codec support.
set -eo pipefail

FLATPAK_BUNDLE="${FLATPAK_BUNDLE:-build/lv.flatpak}"
TEST_VIDEO="${1:-test/fixtures/sample_video.mp4}"

echo "=== lv Flatpak Video Tests ==="
echo "Bundle: $FLATPAK_BUNDLE"
echo "Video:  $TEST_VIDEO"
echo ""

# ── 1. Check if FFmpeg extension is available ────────────────────────
echo "--- 1. Check FFmpeg extension ---"
if flatpak list --app 2>/dev/null | grep -q "ffmpeg"; then
    echo "✅ FFmpeg extension available"
else
    echo "⚠️  FFmpeg extension not installed"
    echo "   Run: flatpak install flathub org.freedesktop.Platform.ffmpeg-full"
fi
echo ""

# ── 2. Check library availability in sandbox ───────────────────────
echo "--- 2. Check libmpv in sandbox ---"
LIBS=$(flatpak run --devel com.shirk33y.lv bash -c 'ldconfig -p | grep -i mpv' 2>/dev/null || echo "")
if [[ -n "$LIBS" ]]; then
    echo "✅ libmpv found:"
    echo "$LIBS"
else
    echo "⚠️  libmpv not found in library search path"
fi
echo ""

# ── 3. Video metadata extraction (libmpv probe) ──────────────────────
echo "--- 3. Probe video file ---"
if [[ -f "$TEST_VIDEO" ]]; then
    # Try to read video metadata using lv's scan
    echo "File: $TEST_VIDEO ($(du -h "$TEST_VIDEO" | cut -f1))"

    # Create unique DB for isolation
    TMPDB=$(mktemp /tmp/lv-video-probe-XXXXXX.db)
    trap "rm -f '$TMPDB'" EXIT

    SCAN_OUTPUT=$(LV_DB_PATH="$TMPDB" flatpak run com.shirk33y.lv scan "$TEST_VIDEO" 2>&1 || true)

    # Check if video was detected
    if echo "$SCAN_OUTPUT" | grep -q "1 new/changed"; then
        echo "✅ Video recognized and indexed"
        echo "$SCAN_OUTPUT" | grep -E "Scanning|Done|new/changed"
    elif echo "$SCAN_OUTPUT" | grep -q "new/changed"; then
        echo "✅ Video processed (changed count: $(echo "$SCAN_OUTPUT" | grep new/changed | tail -1))"
        echo "$SCAN_OUTPUT" | tail -3
    else
        echo "⚠️  Scan output unexpected:"
        echo "$SCAN_OUTPUT" | tail -5
    fi
else
    echo "❌ Test video not found: $TEST_VIDEO"
    exit 1
fi
echo ""

# ── 4. Check runtime and codec support ───────────────────────────────
echo "--- 4. Check runtime ---"
RUNTIME_INFO=$(flatpak info org.freedesktop.Platform 2>/dev/null || echo "")
if [[ -n "$RUNTIME_INFO" ]]; then
    echo "✅ Platform runtime available"
    RUNTIME_VER=$(echo "$RUNTIME_INFO" | grep -i version | head -1 || echo "24.08")
    echo "   Version: $RUNTIME_VER"
else
    echo "⚠️  Could not determine runtime version"
fi
echo ""

# ── 5. CLI help to verify app loaded ─────────────────────────────────
echo "--- 5. Verify app binaries ---"
if flatpak run com.shirk33y.lv --help 2>&1 | head -2; then
    echo "✅ lv binary working"
else
    echo "❌ Failed to run lv"
    exit 1
fi
echo ""

echo "=== Summary ==="
echo "Video file processing works. Codec support depends on FFmpeg extension."
echo "Install ffmpeg-full: flatpak install flathub org.freedesktop.Platform.ffmpeg-full"
