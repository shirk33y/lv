# lv

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
make ci                       # test + clippy + fmt
```

## Runtime requirements

- **libmpv** — `libmpv.so.2` (Linux), `libmpv-2.dll` (Windows)
- **SDL2** — `libSDL2.so` (Linux), `SDL2.dll` (Windows)
