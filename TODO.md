# lv TODO

## ✓ Phase 1: meta_tags junction table
- [x] Schema migration + backward compat
- [x] All 11 query sites using EXISTS subquery
- [x] Keybinding: `y`=like, `shift+y`=unlike, `2`-`9`=add/remove c2-c9

## ✓ Phase 2: Property columns + new CLI
- [x] 6 columns on directories table (watch_mode, poll_interval, max_depth, include_ext, label)
- [x] dir_properties KV table removed
- [x] watcher, CLI commands updated
- [x] make configure, pre-commit hook
- [x] Fix duplicate clap aliases

## ✓ Phase 3: lv find
- [x] Glob→SQL LIKE filtering, --count, --print0
- [x] Size/duration/resolution/tag filters
- [x] --sort name|size|duration|resolution|random
- [x] 55 unit tests

## ✓ Phase 4: Daemon
- [x] `lv sync -b` headless daemon
- [x] IPC via PRAGMA data_version polling (500ms)
- [x] commands table (scan/shutdown)
- [x] Dynamic watcher sync on DB changes
- [x] `r` key writes scan command to commands table
- [ ] Systemd user service (scripts/lv-daemon.service)

## Indexing / metadata
- [ ] Populate `duration_ms`, `bitrate`, `codecs` from ffprobe during thumbnail job
- [ ] Add `gop_frames INTEGER` column to `meta` table (keyframe interval)
- [ ] Index GOP size from ffprobe
- [ ] Populate `exif_json` from EXIF data
- [ ] Populate `pnginfo` from PNG tEXt chunks (Stable Diffusion metadata)

## UI
- [ ] Thumbnail sidebar (imgui Image() with GL textures from SQLite)
- [ ] Info overlay (i key)
- [ ] Log overlay (x key)
- [ ] Help overlay (? key)

## Performance
- [ ] Store decoded RGBA blobs in SQLite for instant second-view
- [ ] Use libjpeg-turbo directly for JPEG (bypass image crate overhead)
- [ ] Cache first video frame as texture for instant re-display
- [ ] Preload strategy: prioritize direction of travel
- [ ] Extend video prefetch into prewarm so next/prev mpv media starts instantly
- [ ] Investigate dedicated prewarm mpv handle or demux/cache strategy
