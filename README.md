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
```

**Requirements**: podman, ~30-40min build time (FFmpeg prebuilt SDK, SDL2, libmpv, Rust offline crates)

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
