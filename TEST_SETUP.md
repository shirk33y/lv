# Testing Setup for `lv`

## Overview

This project includes three levels of testing:
1. **Unit Tests** — Rust `#[test]` via `cargo test`
2. **CLI Smoke Tests** — End-to-end CLI functionality
3. **Flatpak Smoke Tests** — Flatpak distribution integrity

## Running Locally

### Unit Tests
```bash
cargo test
```

### CLI Smoke Tests
Build the app, then run:
```bash
cargo build --release
LV_FIXTURES=test/fixtures bash scripts/smoke-test-cli.sh
```

### Flatpak Smoke Tests
1. Build flatpak locally:
   ```bash
   distrobox-host-exec ./scripts/build-flatpak.sh
   ```

2. Run smoke tests:
   ```bash
   bash scripts/smoke-test-flatpak.sh
   ```

3. (Optional) Run advanced video tests:
   ```bash
   bash scripts/smoke-test-flatpak-video.sh
   ```

### Containerized Smoke Tests
Run flatpak tests in Docker:
```bash
docker build -f docker/Dockerfile.flatpak-smoke -t lv-flatpak-smoke .
docker run --rm lv-flatpak-smoke
```

## CI/CD Integration

### On Every Push/PR (`ci.yml`)
- ✅ Cargo test
- ✅ CLI smoke tests (binary built in-repo)
- ✅ Clippy linting
- ✅ Format checking

### On Release (`build.yml`)
- ✅ Build Linux x86_64 (.deb, .AppImage)
- ✅ Build Linux ARM64
- ✅ Build Windows
- ⚠️ Flatpak tests (optional, if `build/lv.flatpak` exists)

**Note:** Full flatpak build requires local nested Docker/Podman setup. It's not performed in GHA.
Build locally and commit the `.flatpak` bundle to test in the release pipeline.

## Test Fixtures

### Image Files
- `test/fixtures/red_800x600.png` — 800×600 red image
- `test/fixtures/blue_800x600.png` — 800×600 blue image
- `test/fixtures/green_800x600.png` — 800×600 green image
- `test/fixtures/white_400x300.png` — 400×300 white image
- `test/fixtures/dark_1920x1080.png` — 1920×1080 dark image

### Video File
- `test/fixtures/sample_video.mp4` — 2-second red test video (H.264, AAC, 23KB)

## Test Scripts

### `scripts/smoke-test-cli.sh`
CLI functionality tests:
- Binary availability (`--help`)
- Help output validation
- Track/untrack directories
- Status reporting
- File scanning and hashing
- Watch/unwatch functionality
- Database creation

### `scripts/smoke-test-flatpak.sh`
Flatpak integration tests:
- Bundle installation
- CLI in sandbox
- Image processing
- Video processing
- Directory scanning
- Sandbox environment validation

### `scripts/smoke-test-flatpak-video.sh`
Advanced video tests:
- FFmpeg extension availability
- libmpv in sandbox
- Video metadata extraction
- Codec support verification
- Runtime environment checks

## Adding New Tests

1. **CLI tests**: Update `smoke-test-cli.sh`
2. **Image/video tests**: Update `smoke-test-flatpak.sh` or create new test
3. **Container tests**: Update `docker/Dockerfile.flatpak-smoke`

## Debugging Failed Tests

### Image Issues
```bash
# Check if lv can open image
LV_DB_PATH=/tmp/test.db lv scan test/fixtures/red_800x600.png
```

### Video Issues
```bash
# Check video codec support
flatpak run com.shirk33y.lv scan test/fixtures/sample_video.mp4 -v

# Check FFmpeg availability
flatpak list --app | grep ffmpeg
```

### Database Issues
```bash
# Reset test database
rm -f /tmp/lv-smoke.db
LV_DB_PATH=/tmp/lv-smoke.db bash scripts/smoke-test-cli.sh
```
