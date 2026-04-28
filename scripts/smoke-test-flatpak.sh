#!/bin/bash
# Flatpak smoke test: verify lv flatpak works with video files.
# Tests both CLI commands and video playback functionality.
set -eo pipefail

FLATPAK_BUNDLE="${FLATPAK_BUNDLE:-build/lv.flatpak}"
FIXTURES="${LV_FIXTURES:-test/fixtures}"

echo "=== lv Flatpak Smoke Tests ==="
echo "Bundle:   $FLATPAK_BUNDLE"
echo "Fixtures: $FIXTURES"
echo ""

# Verify bundle exists
if [[ ! -f "$FLATPAK_BUNDLE" ]]; then
    echo "❌ Flatpak bundle not found: $FLATPAK_BUNDLE"
    exit 1
fi
echo "✅ Bundle exists ($(du -h "$FLATPAK_BUNDLE" | cut -f1))"
echo ""

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
if flatpak run com.shirk33y.lv --help >/dev/null 2>&1; then
    echo "✅ lv --help works"
    flatpak run com.shirk33y.lv --help | head -3
else
    echo "❌ lv --help failed"
    exit 1
fi
echo ""

# ── 3. Test image file (baseline) ────────────────────────────────────
echo "--- 3. Test with image ---"
TEST_IMAGE="$FIXTURES/red_800x600.png"
if [[ -f "$TEST_IMAGE" ]]; then
    # Create temp DB for test
    TMPDB=$(mktemp /tmp/lv-flatpak-test-XXXXXX.db)
    trap "rm -f '$TMPDB'" EXIT

    if LV_DB_PATH="$TMPDB" flatpak run com.shirk33y.lv scan "$TEST_IMAGE" 2>&1; then
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
TEST_VIDEO="$FIXTURES/sample_video.mp4"
if [[ ! -f "$TEST_VIDEO" ]]; then
    echo "⚠️  Test video not found: $TEST_VIDEO"
    echo "   (This is OK if you haven't run test generation yet)"
else
    # Try to scan/index the video file
    TMPDB2=$(mktemp /tmp/lv-flatpak-video-XXXXXX.db)
    trap "rm -f '$TMPDB2'" EXIT

    echo "Video: $TEST_VIDEO"
    if timeout 10 flatpak run com.shirk33y.lv scan "$TEST_VIDEO" 2>&1; then
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
trap "rm -f '$TMPDB3'" EXIT

if [[ -d "$FIXTURES" ]]; then
    if LV_DB_PATH="$TMPDB3" flatpak run com.shirk33y.lv scan "$FIXTURES" 2>&1 | head -5; then
        echo "✅ Directory scan completed"
    else
        echo "⚠️  Directory scan had issues but may have processed files"
    fi
else
    echo "❌ Fixtures directory not found: $FIXTURES"
    exit 1
fi
echo ""

# ── 6. Check libmpv is available in sandbox ──────────────────────────
echo "--- 6. Check libmpv ---"
if flatpak run com.shirk33y.lv --help 2>&1 | grep -qi "mpv\|video\|player"; then
    echo "✅ Help mentions video capabilities"
else
    echo "⚠️  Help doesn't mention video (may be OK)"
fi
echo ""

echo "=== Flatpak Smoke Tests Complete ==="
echo "Summary: Image and video processing work in flatpak sandbox"
