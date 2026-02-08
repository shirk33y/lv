# src-imgui — mpv + Dear ImGui frontend for lv

Native media viewer frontend replacing the Tauri/Preact webview.

## Architecture

```
winit (window + events)
  └─ glow (OpenGL context)
       ├─ libmpv render API → texture (video/image playback)
       └─ imgui-rs overlay (sidebar, thumbnails, status bar)
```

## Dependencies

- **lv-core** — shared backend (SQLite, scanner, worker, thumbs)
- **libmpv2** — Rust bindings to libmpv (render-to-texture)
- **imgui + imgui-glow-renderer** — Dear ImGui with OpenGL backend
- **winit + glutin** — window management + GL context
- **glow** — OpenGL bindings

## What stays the same

All backend logic lives in `src-core/` (to be extracted from `src-tauri/`):
`data.rs`, `db.rs`, `scanner.rs`, `worker.rs`, `thumbs.rs`, `cli.rs`, `debug.rs`

## What this replaces

- `src/` (Preact frontend) — sidebar, viewer, keybinds, status bar
- `src-tauri/src/ipc.rs` — Tauri invoke handlers (direct Rust calls instead)
- `src-tauri/src/protocol.rs` — thumb:// and lv-file:// URI schemes (GL textures instead)

## Planned structure

```
src-imgui/
├── Cargo.toml
└── src/
    ├── main.rs          # winit event loop, GL context, imgui init
    ├── mpv.rs           # libmpv render API → GL texture
    ├── thumb_cache.rs   # SQLite WebP blobs → GL textures, LRU
    ├── keys.rs          # keybind dispatch
    └── ui/
        ├── mod.rs
        ├── sidebar.rs   # thumbnail grid
        ├── viewer.rs    # main media display
        └── status.rs    # status bar, log overlay
```

## Runtime requirements

- `libmpv.so.2` (Linux), `libmpv-2.dll` (Windows), or `libmpv.dylib` (macOS)
- System install or bundled alongside the binary
