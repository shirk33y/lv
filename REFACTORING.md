# Refactoring: Decompose monolithic `src/`

**Goal:** Extract cohesive modules from `statusbar.rs` and `main.rs` into single-responsibility files.

## Part 1: Split `statusbar.rs` (1212 lines → 4 single-purpose modules)

Current: `statusbar.rs` holds 4 unrelated widgets in one file.
Target:

```
src/
├── assets.rs        font setup, apply_theme, BUNDLED_FONT, GLYPH_RANGES
├── titlebar.rs      WindowAction, BAR_HEIGHT, BUTTON_ZONE_W, StatusInfo,
│                    draw_status_bar, draw_empty_status_bar, seekbar helpers
├── infobar.rs       draw_info_panel, draw_stats_section, format helpers
└── overlay.rs       draw_spinner, draw_error_overlay, draw_empty_overlay,
                     empty_library_hint
```

### Steps

1. Create `assets.rs` — move `add_font`, `apply_theme`, `BUNDLED_FONT`, `GLYPH_RANGES`
2. Create `titlebar.rs` — move `WindowAction`, `BAR_HEIGHT`, `BUTTON_ZONE_W`, `StatusInfo`, `seek_fraction_at`, `seekbar_window_height`, `seekbar_expanded`, `seekbar_label_x`, `video_progress_fraction`, `volume_label`, `draw_empty_status_bar`, `draw_status_bar`, plus 5 WindowAction/constant tests from `main.rs`
3. Create `infobar.rs` — move `draw_info_panel`, `draw_stats_section`, `format_size`, `format_duration`, `collection_name`, `fmt_time`
4. Create `overlay.rs` — move `draw_spinner`, `draw_error_overlay`, `empty_library_hint`, `draw_empty_overlay`
5. Delete `statusbar.rs`
6. Update `main.rs` imports: `statusbar::` → `titlebar::`, `infobar::`, `overlay::`, `assets::`
7. Update any other module that imports from `statusbar`

### Test migration

- WindowAction/bar-height tests → inline `mod tests` in `titlebar.rs`
- Format helper tests → inline in `infobar.rs`
- All statusbar-specific unit tests move with their code
- Integration tests (e.g. main's `empty_files_update_title_is_noop`) stay in `main.rs`

## Part 2: Extract `media.rs` from `main.rs`

Prerequisite: `is_media_extension` and `clean_path` are referenced from watcher/scanner/cli via `crate::`.

```
src/
├── media.rs    IMAGE_EXTS, MPV_IMAGE_EXTS, VIDEO_EXTS,
│               is_media_extension, clean_path, ext_of,
│               is_image, is_video, is_mpv_media, is_media,
│               should_preload_image
```

Moves ~30 tests with it. Updates `crate::is_media_extension` → `crate::media::` and `crate::clean_path` → same in 3 files.

## Part 3: Extract `mpv.rs` from `main.rs`

```
src/
├── mpv.rs      MpvPlaybackState, MpvRenderShared, spawn_mpv_render_thread,
│               apply_mpv_property_update, mpv_loadfile_args, mpv_seek_args,
│               mpv_seek_absolute_args, mpv_loop_file_value, OBS_* constants,
│               VideoPrefetcher, prefetch_file, video_prefetch_paths,
│               schedule_video_prefetch
```

Moves ~8 tests with it.

## Part 4: Cleanup

- Move `src/flatpak-cargo-generator.py` → `scripts/`
- Delete `src/Cargo.lock` (duplicate of root)

## Remaining in `main.rs` after extraction

```
fn main(), event loop, Cli/Commands, handle_drop,
switch_dir, jump_to, next_cursor_after_load_failure,
update_title, set_resize_cursor, print_report, TimingEntry
+ ~130 integration tests
```

~1450 lines non-test + ~3450 lines tests.
