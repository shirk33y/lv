# lv

[![CI](https://github.com/shirk33y/lv/actions/workflows/ci.yml/badge.svg)](https://github.com/shirk33y/lv/actions/workflows/ci.yml)
[![Release](https://github.com/shirk33y/lv/actions/workflows/release.yml/badge.svg)](https://github.com/shirk33y/lv/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/shirk33y/lv?include_prereleases)](https://github.com/shirk33y/lv/releases/latest)

Fast keyboard-driven media viewer. Single Rust binary, SQLite library database, GPU-rendered UI.

![screenshot](screenshot01.jpg)

## Features

- **Image + video** playback via libmpv render API
- **Dear ImGui** overlay — file info, AI metadata, library stats
- **Keyboard-first** — j/k navigate, h/l switch dirs, y like, u random, n newest
- **Background workers** — SHA-512 hashing, EXIF extraction, AI prompt & settings parsing
- **File watcher** — live directory monitoring with notify
- **Drag & drop** — drop files or folders to browse instantly
- **CLI** — `track`, `untrack`, `watch`, `unwatch`, `scan`, `worker`

## Architecture

```
SDL2 (window + events)
  └─ OpenGL (glow)
       ├─ libmpv render API → texture
       ├─ image crate decode → GL texture (LRU preload cache)
       └─ imgui-rs overlay (status bar, metadata sidebar)
```

## Structure

```
src/
├── main.rs       # SDL2 event loop, GL context, imgui, keybinds
├── db.rs         # SQLite: files, meta, history, directories, jobs
├── scanner.rs    # recursive media discovery + rescan/prune
├── watcher.rs    # notify-based filesystem watcher
├── jobs.rs       # background worker pipeline (hash, exif, ai)
├── aimeta.rs     # AI metadata extraction (pnginfo, ComfyUI)
├── preload.rs    # LRU image preload cache
├── quad.rs       # fullscreen quad rendering
├── statusbar.rs  # imgui status bar + metadata panel
└── cli.rs        # CLI subcommands
```

## Build & run

Rust toolchain:

```sh
rustup default stable
rustup target add x86_64-unknown-linux-gnu
```

If Cargo reports missing `stable-x86_64-unknown-linux-gnu`, run:

```sh
rustup toolchain install stable
```

Generated build caches such as `.flatpak-builder/`, `build/`, `dist/`, and `target/` are excluded from Cargo package fingerprinting. Do not remove those excludes from `Cargo.toml`; otherwise Cargo can scan unreadable Flatpak cache loops and fail before tests compile.

On Bazzite, use Homebrew libraries for native checks/tests:

```sh
brew install sdl2 mpv xorgproto libx11 libxext libxrandr libxfixes

BREW_PREFIX="$(brew --prefix)"
export PKG_CONFIG_PATH="$BREW_PREFIX/opt/sdl2/lib/pkgconfig:$BREW_PREFIX/opt/mpv/lib/pkgconfig:$BREW_PREFIX/opt/xorgproto/share/pkgconfig:$BREW_PREFIX/opt/libx11/lib/pkgconfig:$BREW_PREFIX/opt/libxext/lib/pkgconfig:$BREW_PREFIX/opt/libxrandr/lib/pkgconfig:$BREW_PREFIX/opt/libxfixes/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
export LIBRARY_PATH="$BREW_PREFIX/opt/sdl2/lib:$BREW_PREFIX/opt/mpv/lib:${LIBRARY_PATH:-}"
export LD_LIBRARY_PATH="$BREW_PREFIX/opt/sdl2/lib:$BREW_PREFIX/opt/mpv/lib:${LD_LIBRARY_PATH:-}"

cargo check
cargo test
```

```sh
cargo run --release           # GUI
cargo run -- track ~/Photos   # add directory
cargo run -- scan             # rescan all tracked dirs
cargo run -- worker           # headless hash/exif/ai worker
scripts/ci.sh                 # test + clippy + fmt
```

## Scripts

| Script | Description |
|--------|-------------|
| `scripts/ci.sh` | test + clippy + fmt (parallel) |
| `scripts/dev-linux.sh [args]` | debug build + run (Linux) |
| `scripts/dev-windows.sh [args]` | debug build + run (Windows/WSL) |
| `scripts/build-linux-intel.sh` | release build for x86_64 Linux |
| `scripts/build-linux-arm.sh` | release build for aarch64 Linux |
| `scripts/build-windows-intel.sh` | release build + NSIS installer for Windows |
| `scripts/docker-build.sh [target]` | dockerized cross-builds → `dist/` |
| `scripts/build-flatpak.sh [--install]` | build Flatpak bundle via podman → `build/flatpak/` |
| `scripts/clean.sh` | remove build artifacts |
| `scripts/build-appimage.sh [arch] [binary]` | build AppImage → `build/appimage/` |
| `scripts/build-deb.sh [arch] [binary]` | build .deb package → `build/deb/` |

```sh
scripts/dev-linux.sh track ~/Photos
scripts/docker-build.sh linux-intel   # or: all
```

## Build Formats

### Flatpak

Build Flatpak bundle via podman (containerized, no local deps needed):

```sh
./scripts/build-flatpak.sh --build-only         # build bundle to build/flatpak/lv.flatpak
./scripts/build-flatpak.sh --install            # build + install system-wide
./scripts/build-flatpak.sh --no-cache           # clean build (longer)
./scripts/build-flatpak.sh --rebuild-image      # rebuild env image (forces runtime re-download)
```

**Requirements**: podman, ~30-40min first build (downloads runtimes). Subsequent builds use cached runtimes (~5-10min). Cache stored in `~/.cache/flatpak/`

**System-wide command**: After install, create PATH wrapper:
```bash
sudo tee /usr/local/bin/lv > /dev/null <<'EOF'
#!/bin/bash
exec flatpak run com.shirk33y.lv "$@"
EOF
sudo chmod +x /usr/local/bin/lv
```

**Manifest**: `extra/flatpak/com.shirk33y.lv.json` — runtime 24.08, SDK extensions (ffmpeg-full, rust-stable)

### AppImage & .deb

```sh
scripts/build-appimage.sh [x86_64|aarch64]    # → build/appimage/lv-*.AppImage
scripts/build-deb.sh [amd64|arm64]             # → build/deb/lv-*.deb
```

All build artifacts consolidated to `build/` directory.

## Dependencies (Debian/Ubuntu)
```sh
sudo apt install build-essential ca-certificates curl pkg-config \
    libsdl2-dev libmpv-dev \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu libc6-dev-arm64-cross \
    llvm clang nsis
```
