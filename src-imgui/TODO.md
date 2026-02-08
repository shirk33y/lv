# src-imgui TODO

## Indexing / metadata
- [ ] Populate `duration_ms`, `bitrate`, `codecs` from ffprobe during thumbnail job
- [ ] Add `gop_frames INTEGER` column to `meta` table (keyframe interval)
- [ ] Index GOP size from ffprobe: `ffprobe -select_streams v:0 -skip_frame nokey -show_entries frame=pts_time`
- [ ] Populate `exif_json` from EXIF data (kamadak-exif crate or exiftool)
- [ ] Populate `pnginfo` from PNG tEXt chunks (Stable Diffusion metadata)

## UI
- [ ] Replace window-title status bar with imgui overlay
- [ ] Thumbnail sidebar (imgui Image() with GL textures from SQLite)
- [ ] Info overlay (i key)
- [ ] Log overlay (x key)
- [ ] Help overlay (? key)

## Architecture
- [ ] Extract src-core from src-tauri (shared backend library)
- [ ] Wire src-imgui to depend on src-core instead of its own db.rs
- [ ] Cargo workspace at repo root

## Performance
- [ ] Store decoded RGBA blobs in SQLite for instant second-view
- [ ] Use libjpeg-turbo directly for JPEG (bypass image crate overhead)
- [ ] Cache first video frame as texture for instant re-display
- [ ] Preload strategy: prioritize direction of travel

## Packaging
- [ ] AppImage bundle (lv-imgui + libmpv.so + ffmpeg)
- [ ] Windows .exe bundle (lv-imgui.exe + mpv-2.dll)
