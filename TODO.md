# lv TODO

## Indexing / metadata
- [ ] Populate `duration_ms`, `bitrate`, `codecs` from ffprobe during thumbnail job
- [ ] Add `gop_frames INTEGER` column to `meta` table (keyframe interval)
- [ ] Index GOP size from ffprobe: `ffprobe -select_streams v:0 -skip_frame nokey -show_entries frame=pts_time`
- [ ] Populate `exif_json` from EXIF data (kamadak-exif crate or exiftool)
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
- [ ] Extend existing video prefetch into TikTok-style prewarm so next/previous mpv media starts playback instantly without spinner
- [ ] Investigate dedicated prewarm mpv handle or demux/cache strategy for adjacent videos

## Video playback
- [ ] Keep libmpv as source of truth for seek, duration, pause, loop, and animated WebP playback
- [ ] Add richer playback controls that mirror normal mpv status: seekbar, position, duration, paused/playing, volume
- [ ] Test rapid navigation across video/video, video/image, image/video, missing files, and animated WebP
