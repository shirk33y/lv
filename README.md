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

If Cargo reports missing `stable-x86_64-unknown-linux-gnu`, install the toolchain first, then add targets:

```sh
rustup toolchain install stable
rustup default stable
rustup target add x86_64-unknown-linux-gnu
```

For release builds, prefer Make targets over ad-hoc Cargo commands. `make configure` writes `config.local.mk` once for native C library prefixes; `make check`, `make test`, and `make flatpak-release` reuse that config.

Generated build caches such as `.flatpak-builder/`, `build/`, `dist/`, and `target/` are excluded from Cargo package fingerprinting. Do not remove those excludes from `Cargo.toml`; otherwise Cargo can scan unreadable Flatpak cache loops and fail before tests compile.

On Bazzite, run one-time configure, then use `make` targets:

```sh
brew install sdl2 mpv xorgproto libx11 libxext libxrandr libxfixes

make configure
make check
make test
```

Use `LV_NATIVE_PREFIXES=/custom/prefix1:/custom/prefix2 make configure` if the libraries live outside Homebrew.

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
| `scripts/build-flatpak.sh [--install]` | build Flatpak bundle via container runtime |
| `scripts/clean.sh` | remove build artifacts |
| `scripts/build-appimage.sh [arch] [binary]` | build AppImage → `build/appimage/` |
| `scripts/build-deb.sh [arch] [binary]` | build .deb package → `build/deb/` |

```sh
scripts/dev-linux.sh track ~/Photos
scripts/docker-build.sh linux-intel   # or: all
```

## Build Formats

### Flatpak

Build and smoke-test Flatpak through Make. This is the same path used by GitHub Actions releases:

```sh
make flatpak-release                            # x86_64 bundle + CLI/video smoke tests
make flatpak-release FLATPAK_ARCH=aarch64       # ARM64 bundle on native ARM64 Linux
make flatpak-release CONTAINER_RUNTIME=docker   # use Docker instead of Podman
```

Lower-level script entry points:

```sh
./scripts/build-flatpak.sh --build-only         # build bundle
./scripts/build-flatpak.sh --install            # build + install for current user
./scripts/build-flatpak.sh --no-cache           # clean build (longer)
./scripts/build-flatpak.sh --rebuild-image      # rebuild env image (forces runtime re-download)
```

**Requirements**: podman or Docker on Linux. Debian, Ubuntu, Fedora, and Bazzite hosts all use the same containerized build path. First build takes ~30-40min while runtimes download; later builds use cached runtimes (~5-10min).

Flatpak release builds are native per CPU architecture. Build `x86_64` on x86_64 Linux and `aarch64` on ARM64 Linux. Cross-arch Flatpak builds through QEMU are blocked by default because Flatpak uses bubblewrap namespaces, which are unreliable under container emulation. Set `LV_ALLOW_FLATPAK_EMULATION=1` only for experimental debugging.

Flatpak builds run through rootless podman with `--userns=keep-id`, so generated files stay owned by the current user. The build script also disables the OSTree repo percentage free-space guard for the local build repo, which avoids false failures on nearly full filesystems. Build output and Flatpak caches are ignored by Git; do not commit `build/lv.flatpak`, `cargo-sources.json`, or `.flatpak-builder/`.

`--install` uses `flatpak install --user` and creates a user PATH wrapper.

Smoke tests install the bundle in a privileged smoke container, verify `libmpv.so` is bundled and dynamically resolved, scan an MP4 fixture, launch the app under Xvfb, and compare captured frames to confirm playback advances.

**Manifest**: `extra/flatpak/io.github.shirk33y.lv.json` — runtime 24.08, SDK extensions (ffmpeg-full, rust-stable), app id `io.github.shirk33y.lv`

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
