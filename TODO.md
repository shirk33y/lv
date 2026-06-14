# lv TODO

## Done (v0.7)
- [x] `meta_tags` junction table replacing JSON column for tags
- [x] Batch migration from JSON to `meta_tags` (backup first, progress via statics)
- [x] `LIKE '%"tag"%'` → `EXISTS (SELECT 1 FROM meta_tags ...)` across all queries
- [x] Keybinding: `y`=like, `shift+y`=unlike, `2`-`9`=add c2-c9, `shift+2-9`=remove c2-c9
- [x] Full-width bottom seekbar (idle 3px, hover 24px, opacity art)
- [x] Window resize handles (6px edges+corners) via hit-test
- [x] `make dev`/`check`/`test` targets, `scripts/native-env.sh`, `scripts/configure-native.sh`
- [x] `RESIZING_WINDOW.md`: Wayland cursor-shape-v1 limitation documented

## Phase 2: Property system + new CLI
- [ ] `dir_properties` table (key-value, ZFS-style)
  ```sql
  CREATE TABLE dir_properties (
      dir_id INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
      key TEXT NOT NULL,
      value TEXT NOT NULL DEFAULT '',
      PRIMARY KEY (dir_id, key)
  );
  ```
- [ ] Migrate `watched` flag → `watch_mode` property (notify/auto)
- [ ] Migrate `recursive` flag → `recursive` property
- [ ] New flat CLI:
  ```
  lv add PATH           add dir, scan, auto-watch
  lv remove PATH        remove from library
  lv sync [PATH]        one-shot scan + jobs
  lv status             library stats
  lv get PATH [prop]    get dir property(ies)
  lv set PATH K=V [K=V] set dir properties
  ```
- [ ] Deprecation aliases: `track`→`add`, `untrack`→`remove`, `watch`→`set`, etc.
- [ ] Remove old `track`/`untrack`/`watch`/`unwatch`/`scan`/`worker` commands

## Phase 3: lv find
- [ ] `lv find [OPTIONS] [pattern]` with fd-style filtering
- [ ] Glob match by default, `--regex` opt-in
- [ ] Size specs: `+10M`, `-500K`
- [ ] Duration specs: `+30s`, `-5m`
- [ ] Resolution presets: `thumb|vga|sd|hd|4k|8k|photo`
- [ ] Resolution specs: `+1920`, `-1080`
- [ ] Tag filter: `--tag like`, `--tag c3`
- [ ] `--sort name|size|duration|resolution|random`
- [ ] `--count`, `--list`, `--print0`
- [ ] Same flags usable as `lv` pre-filter in GUI

## Phase 4: Daemon
- [ ] `lv sync -b` = headless daemon
- [ ] IPC via shared SQLite + `PRAGMA data_version` polling
- [ ] Daemon loop: poll DB every 100ms, adjust watchers, process jobs
- [ ] `commands` table for GUI→daemon signalling
- [ ] Systemd user service
- [ ] `r` key triggers sync request via commands table (once daemon exists)

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
