# lv — stupid media tracker

Keyboard-driven media library. CLI + GUI. Tauri 2 + Preact.

## Usage

```
lv [PATH]*          # open GUI on path(s)
lv add PATH         # add directory to library
lv -s [PATH]        # scan watched dirs
lv -s-a             # full re-scan all
lv -w PATH          # watch directory
lv -u PATH          # unwatch directory
lv worker           # headless hash + thumbnail jobs
```

## Keys

| Key | Action              |
|-----|---------------------|
| j/k | next / prev file   |
| h/l | prev / next dir    |
| u   | random file        |
| n   | newest file        |
| y   | toggle favorite    |
| m   | random favorite    |
| b   | latest favorite    |
| f   | fullscreen         |
| i   | file info          |
| ?   | help               |

## Stack

- **Backend**: Rust — SQLite (WAL), job queue, SHA-512 dedup, WebP thumbnails
- **Frontend**: Preact + Vite — virtualized sidebar, image preloader, custom protocols
- **Storage**: single `lv.db` — files, metadata, thumbnails, history, jobs
- **Worker**: prioritizes current view context, background hash + thumb generation

## Dev

```
npm install
npm run check       # typecheck + test + build + clippy + cargo test
cargo tauri dev     # run app
```
