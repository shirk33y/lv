//! Status bar and imgui overlay helpers.
//!
//! Uses Dear ImGui for rendering. Font is DejaVu Sans Mono bundled in binary
//! with Latin Extended glyph ranges for full UTF-8 filename support (Polish, etc.).

use imgui::{Condition, FontConfig, FontGlyphRanges, FontSource, WindowFlags};

/// DejaVu Sans Mono bundled in the binary — no system font dependency.
pub const BUNDLED_FONT: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");

/// Glyph ranges: Basic Latin + Latin Supplement + Latin Extended-A + punctuation + symbols.
const GLYPH_RANGES: &[u32] = &[
    0x0020, 0x00FF, // Basic Latin + Latin Supplement
    0x0100, 0x017F, // Latin Extended-A (Polish Ł Ś ć ź etc.)
    0x0180, 0x024F, // Latin Extended-B
    0x2000, 0x206F, // General Punctuation (—, …)
    0x2190, 0x21FF, // Arrows
    0x2600, 0x26FF, // Misc Symbols (♥)
    0x2764, 0x2764, // ❤
    0,
];

/// Load the bundled font into an imgui context with proper glyph ranges.
pub fn add_font(imgui: &mut imgui::Context) {
    imgui.fonts().add_font(&[FontSource::TtfData {
        data: BUNDLED_FONT,
        size_pixels: 15.0,
        config: Some(FontConfig {
            glyph_ranges: FontGlyphRanges::from_slice(GLYPH_RANGES),
            oversample_h: 2,
            oversample_v: 1,
            pixel_snap_h: true,
            ..Default::default()
        }),
    }]);
}

/// Apply a dark, semi-transparent theme suitable for a media viewer overlay.
pub fn apply_theme(imgui: &mut imgui::Context) {
    let style = imgui.style_mut();
    style.window_rounding = 0.0;
    style.window_border_size = 0.0;
    style.window_padding = [8.0, 4.0];
    style.frame_padding = [4.0, 2.0];
    style.item_spacing = [8.0, 4.0];

    style.colors[imgui::sys::ImGuiCol_WindowBg as usize] = [0.0, 0.0, 0.0, 0.78];
    style.colors[imgui::sys::ImGuiCol_Text as usize] = [0.9, 0.9, 0.9, 1.0];
}

const STATUS_FLAGS: WindowFlags = WindowFlags::NO_TITLE_BAR
    .union(WindowFlags::NO_RESIZE)
    .union(WindowFlags::NO_MOVE)
    .union(WindowFlags::NO_SCROLLBAR)
    .union(WindowFlags::NO_SCROLL_WITH_MOUSE)
    .union(WindowFlags::NO_COLLAPSE)
    .union(WindowFlags::NO_SAVED_SETTINGS)
    .union(WindowFlags::NO_FOCUS_ON_APPEARING)
    .union(WindowFlags::NO_NAV)
    .union(WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS);

const DIM: [f32; 4] = [0.50, 0.50, 0.50, 1.0];
const BRIGHT: [f32; 4] = [0.92, 0.92, 0.92, 1.0];
const ACCENT: [f32; 4] = [1.0, 0.40, 0.40, 1.0];

/// Status bar info passed from main loop.
pub struct StatusInfo<'a> {
    pub index: usize,
    pub total: usize,
    pub path: &'a str,
    pub liked: bool,
    pub is_video: bool,
    pub paused: bool,
    pub video_pos: f64,
    pub video_duration: f64,
    pub volume: i64,
    pub turbo: bool,
}

/// Truncate a string with middle ellipsis to fit within `max_w` pixels.
fn middle_ellipsis(ui: &imgui::Ui, s: &str, max_w: f32) -> String {
    let full_w = ui.calc_text_size(s)[0];
    if full_w <= max_w || s.len() < 8 {
        return s.to_string();
    }
    let ellipsis = "…";
    let ell_w = ui.calc_text_size(ellipsis)[0];
    let budget = max_w - ell_w;
    if budget <= 0.0 {
        return ellipsis.to_string();
    }
    // Binary search: keep N chars from start + M chars from end
    let chars: Vec<char> = s.chars().collect();
    let half = budget / 2.0;
    let mut left_end = 0;
    let mut right_start = chars.len();
    // Find left portion
    for i in 1..chars.len() {
        let sub: String = chars[..i].iter().collect();
        if ui.calc_text_size(&sub)[0] > half {
            break;
        }
        left_end = i;
    }
    // Find right portion
    for i in (0..chars.len()).rev() {
        let sub: String = chars[i..].iter().collect();
        if ui.calc_text_size(&sub)[0] > half {
            break;
        }
        right_start = i;
    }
    if left_end == 0 && right_start == chars.len() {
        return ellipsis.to_string();
    }
    let left: String = chars[..left_end].iter().collect();
    let right: String = chars[right_start..].iter().collect();
    format!("{}{}{}", left, ellipsis, right)
}

/// Draw the status bar at the bottom of the screen.
/// Layout: [left: dirname/filename ♥] [right: [1/45] > 1:30/5:00 | Vol: 100%]
pub fn draw_status_bar(ui: &imgui::Ui, info: &StatusInfo, display_w: f32, display_h: f32) {
    let bar_height = 24.0;
    let pad = 8.0;

    if let Some(_win) = ui
        .window("##statusbar")
        .position([0.0, display_h - bar_height], Condition::Always)
        .size([display_w, bar_height], Condition::Always)
        .bg_alpha(0.78)
        .flags(STATUS_FLAGS)
        .begin()
    {
        // Save initial Y so all elements stay on the same line
        let y = ui.cursor_pos()[1];

        // Build right side: [T] [index/total] + video info
        let turbo_prefix = if info.turbo { "[T] " } else { "" };
        let index_text = format!("{}[{}/{}]", turbo_prefix, info.index, info.total);
        let right_text = if info.is_video {
            let icon = if info.paused { "||" } else { ">" };
            format!(
                "{} {}/{}  Vol: {}%  {}",
                icon,
                fmt_time(info.video_pos),
                fmt_time(info.video_duration),
                info.volume,
                index_text,
            )
        } else {
            index_text.clone()
        };
        let right_w = ui.calc_text_size(&right_text)[0];
        let right_x = display_w - pad - right_w;

        // Available width for left path + heart
        let heart_w = if info.liked {
            ui.calc_text_size(" ♥")[0]
        } else {
            0.0
        };
        let left_max = (right_x - pad * 2.0 - heart_w).max(50.0);

        // Split path into dir + basename
        let (dir_part, base_part) = match info.path.rfind('/') {
            Some(pos) => (&info.path[..=pos], &info.path[pos + 1..]),
            None => ("", info.path),
        };

        // Measure full path width
        let dir_w = if dir_part.is_empty() {
            0.0
        } else {
            ui.calc_text_size(dir_part)[0]
        };
        let base_w = ui.calc_text_size(base_part)[0];
        let path_w = dir_w + base_w;

        // Draw left: path with middle ellipsis on basename if needed
        ui.set_cursor_pos([pad, y]);
        if path_w <= left_max {
            if !dir_part.is_empty() {
                ui.text_colored(DIM, dir_part);
                ui.same_line_with_spacing(0.0, 0.0);
            }
            ui.text_colored(BRIGHT, base_part);
        } else if dir_w > 0.0 && dir_w < left_max * 0.6 {
            ui.text_colored(DIM, dir_part);
            ui.same_line_with_spacing(0.0, 0.0);
            let trunc = middle_ellipsis(ui, base_part, left_max - dir_w);
            ui.text_colored(BRIGHT, trunc);
        } else {
            let trunc = middle_ellipsis(ui, info.path, left_max);
            ui.text_colored(BRIGHT, trunc);
        }

        // Heart after filename
        if info.liked {
            ui.same_line_with_spacing(0.0, 0.0);
            ui.text_colored(ACCENT, " ♥");
        }

        // Draw right: video info + [index/total] (index always rightmost)
        ui.set_cursor_pos([right_x, y]);
        if info.is_video {
            let icon = if info.paused { "||" } else { ">" };
            let progress = format!(
                "{} {}/{}",
                icon,
                fmt_time(info.video_pos),
                fmt_time(info.video_duration),
            );
            ui.text_colored(BRIGHT, &progress);
            ui.same_line();
            ui.text_colored(DIM, format!("Vol: {}%", info.volume));
            ui.same_line();
            ui.text_colored(DIM, &index_text);
        } else {
            ui.text_colored(DIM, &right_text);
        }
    }
}

/// Draw a circular spinner in the center of the screen (shown while video loads).
pub fn draw_spinner(ui: &imgui::Ui, display_w: f32, display_h: f32, time_secs: f32) {
    let draw_list = ui.get_foreground_draw_list();
    let cx = display_w / 2.0;
    let cy = display_h / 2.0;
    let radius = 24.0;
    let thickness = 3.0;
    let segments = 24;
    let arc_frac = 0.75; // 270° arc
    let speed = 4.0;
    let base_angle = time_secs * speed;

    for i in 0..segments {
        let t0 = i as f32 / segments as f32;
        let t1 = (i + 1) as f32 / segments as f32;
        if t1 > arc_frac {
            break;
        }
        let a0 = base_angle + t0 * std::f32::consts::TAU * arc_frac;
        let a1 = base_angle + t1 * std::f32::consts::TAU * arc_frac;
        let alpha = (t0 / arc_frac * 0.85 + 0.15).min(1.0);
        let a_byte = (alpha * 200.0) as u8;
        let color = imgui::ImColor32::from_rgba(220, 220, 220, a_byte);
        draw_list
            .add_line(
                [cx + radius * a0.cos(), cy + radius * a0.sin()],
                [cx + radius * a1.cos(), cy + radius * a1.sin()],
                color,
            )
            .thickness(thickness)
            .build();
    }
}

// ── Info sidebar ─────────────────────────────────────────────────────────

const INFO_FLAGS: WindowFlags = WindowFlags::NO_TITLE_BAR
    .union(WindowFlags::NO_RESIZE)
    .union(WindowFlags::NO_MOVE)
    .union(WindowFlags::NO_COLLAPSE)
    .union(WindowFlags::NO_SAVED_SETTINGS)
    .union(WindowFlags::NO_FOCUS_ON_APPEARING)
    .union(WindowFlags::NO_NAV);

const LABEL_COL: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
const VALUE_COL: [f32; 4] = [0.92, 0.92, 0.92, 1.0];
const HEADER_COL: [f32; 4] = [0.70, 0.80, 1.0, 1.0];

/// Draw the right info sidebar. Returns the panel width (for viewport offset).
/// `scroll_req`: if Some, set scroll to that value.
pub fn draw_info_panel(
    ui: &imgui::Ui,
    meta: &crate::db::FileMeta,
    display_w: f32,
    display_h: f32,
    scroll_req: Option<f32>,
) -> f32 {
    let panel_w = 320.0_f32.min(display_w * 0.35);
    let bar_h = 24.0;
    let stats_h = 130.0;

    if let Some(_win) = ui
        .window("##infopanel")
        .position([display_w - panel_w, 0.0], Condition::Always)
        .size([panel_w, display_h - bar_h - stats_h], Condition::Always)
        .bg_alpha(0.88)
        .flags(INFO_FLAGS)
        .begin()
    {
        if let Some(sy) = scroll_req {
            ui.set_scroll_y(sy);
        }
        ui.text_colored(HEADER_COL, "Info");
        ui.separator();
        ui.spacing();

        let label_w = 90.0;

        let mut rows: Vec<(&str, String)> = Vec::new();
        rows.push(("Filename", meta.filename.clone()));
        rows.push(("Directory", meta.dir.clone()));
        if let Some(size) = meta.size {
            rows.push(("Size", format_size(size)));
        }
        if let Some(ref ts) = meta.modified_at {
            rows.push(("Modified", ts.clone()));
        }
        if let Some(ref fmt) = meta.format {
            rows.push(("Format", fmt.clone()));
        }
        if let (Some(w), Some(h)) = (meta.width, meta.height) {
            rows.push(("Dimensions", format!("{} × {}", w, h)));
        }
        if let Some(dur) = meta.duration_ms {
            rows.push(("Duration", format_duration(dur)));
        }
        if let Some(br) = meta.bitrate {
            rows.push(("Bitrate", format!("{} kbps", br / 1000)));
        }
        if let Some(ref c) = meta.codecs {
            rows.push(("Codecs", c.clone()));
        }
        if !meta.tags.is_empty() {
            rows.push(("Tags", meta.tags.join(", ")));
        }

        for (label, value) in &rows {
            ui.text_colored(LABEL_COL, label);
            ui.same_line_with_pos(label_w);
            // Wrap long values
            let avail = panel_w - label_w - 16.0;
            if ui.calc_text_size(value)[0] > avail && value.len() > 40 {
                // Show wrapped
                ui.text_colored(VALUE_COL, &value[..40.min(value.len())]);
                let rest = &value[40.min(value.len())..];
                if !rest.is_empty() {
                    ui.set_cursor_pos([label_w, ui.cursor_pos()[1]]);
                    ui.text_colored(VALUE_COL, rest);
                }
            } else {
                ui.text_colored(VALUE_COL, value);
            }
        }

        // SHA-512 at bottom (long, special handling)
        if let Some(ref hash) = meta.hash_sha512 {
            ui.spacing();
            ui.separator();
            ui.spacing();
            ui.text_colored(LABEL_COL, "SHA-512");
            // Show hash in two lines of 32 chars
            let h = hash.as_str();
            if h.len() > 32 {
                ui.text_colored(DIM, &h[..32]);
                ui.text_colored(DIM, &h[32..64.min(h.len())]);
                if h.len() > 64 {
                    ui.text_colored(DIM, &h[64..96.min(h.len())]);
                    if h.len() > 96 {
                        ui.text_colored(DIM, &h[96..]);
                    }
                }
            } else {
                ui.text_colored(DIM, h);
            }
        }

        // AI metadata
        if let Some(ref info) = meta.pnginfo {
            ui.spacing();
            ui.separator();
            ui.spacing();
            ui.text_colored(HEADER_COL, "AI");
            ui.text_wrapped(info);
        }

        // Path at very bottom
        ui.spacing();
        ui.separator();
        ui.spacing();
        ui.text_colored(LABEL_COL, "Path");
        ui.text_wrapped(&meta.path);
    }

    panel_w
}

fn format_size(bytes: i64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_duration(ms: i64) -> String {
    let s = ms / 1000;
    let m = s / 60;
    let h = m / 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m % 60, s % 60)
    } else {
        format!("{}:{:02}", m, s % 60)
    }
}

/// Draw job/collection stats at bottom of the info sidebar.
pub fn draw_stats_section(
    ui: &imgui::Ui,
    stats: &crate::jobs::JobStats,
    db: &crate::db::Db,
    display_w: f32,
    display_h: f32,
) {
    use std::sync::atomic::Ordering;

    let panel_w = 320.0_f32.min(display_w * 0.35);
    let bar_h = 24.0;
    let panel_h = 130.0;

    let stats_flags = WindowFlags::NO_TITLE_BAR
        .union(WindowFlags::NO_RESIZE)
        .union(WindowFlags::NO_MOVE)
        .union(WindowFlags::NO_COLLAPSE)
        .union(WindowFlags::NO_SAVED_SETTINGS)
        .union(WindowFlags::NO_FOCUS_ON_APPEARING)
        .union(WindowFlags::NO_NAV);

    if let Some(_win) = ui
        .window("##stats")
        .position(
            [display_w - panel_w, display_h - bar_h - panel_h],
            Condition::Always,
        )
        .size([panel_w, panel_h], Condition::Always)
        .bg_alpha(0.88)
        .flags(stats_flags)
        .begin()
    {
        let cs = db.collection_stats();
        let done = stats.done.load(Ordering::Relaxed);
        let failed = stats.failed.load(Ordering::Relaxed);
        let active = stats.active.load(Ordering::Relaxed);
        let turbo = stats.turbo.load(Ordering::Relaxed);
        let rpm = stats.jobs_per_min.load(Ordering::Relaxed);
        let rpm_str = if rpm >= 10 {
            format!("{}", rpm / 10)
        } else {
            format!("{}.{}", rpm / 10, rpm % 10)
        };

        // Collection
        ui.text_colored(HEADER_COL, "Collection");
        ui.separator();
        let pct_hash = if cs.total_files > 0 {
            cs.hashed * 100 / cs.total_files
        } else {
            0
        };
        let pct_exif = if cs.total_files > 0 {
            cs.with_exif * 100 / cs.total_files
        } else {
            0
        };
        ui.text_colored(
            DIM,
            format!("{} files  {} dirs", cs.total_files, cs.total_dirs),
        );
        ui.text_colored(
            DIM,
            format!("# {}/{}  {}%", cs.hashed, cs.total_files, pct_hash),
        );
        ui.text_colored(
            DIM,
            format!("E {}/{}  {}%", cs.with_exif, cs.total_files, pct_exif),
        );

        ui.spacing();

        // Jobs
        let mode = if turbo { "Turbo" } else { "Lazy" };
        ui.text_colored(HEADER_COL, format!("Jobs [{}]", mode));
        ui.separator();
        ui.text_colored(
            DIM,
            format!(
                "{}/min  ok:{}  err:{}  run:{}",
                rpm_str, done, failed, active
            ),
        );
        if cs.failed > 0 {
            ui.text_colored([1.0, 0.4, 0.4, 1.0], format!("fails: {}", cs.failed));
        }
        let last_err = stats.last_error();
        if !last_err.is_empty() {
            ui.text_colored([0.7, 0.4, 0.4, 1.0], &last_err[..last_err.len().min(40)]);
        }
    }
}

/// Format seconds as H:MM:SS or M:SS.
pub fn fmt_time(secs: f64) -> String {
    if secs < 0.0 || !secs.is_finite() {
        return "--:--".to_string();
    }
    let total = secs as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_time_zero() {
        assert_eq!(fmt_time(0.0), "0:00");
    }

    #[test]
    fn fmt_time_seconds_only() {
        assert_eq!(fmt_time(5.0), "0:05");
        assert_eq!(fmt_time(59.0), "0:59");
    }

    #[test]
    fn fmt_time_minutes() {
        assert_eq!(fmt_time(60.0), "1:00");
        assert_eq!(fmt_time(90.0), "1:30");
        assert_eq!(fmt_time(754.0), "12:34");
    }

    #[test]
    fn fmt_time_hours() {
        assert_eq!(fmt_time(3600.0), "1:00:00");
        assert_eq!(fmt_time(3661.0), "1:01:01");
        assert_eq!(fmt_time(7384.0), "2:03:04");
    }

    #[test]
    fn fmt_time_fractional() {
        assert_eq!(fmt_time(90.7), "1:30");
    }

    #[test]
    fn fmt_time_negative() {
        assert_eq!(fmt_time(-1.0), "--:--");
        assert_eq!(fmt_time(-0.1), "--:--");
    }

    #[test]
    fn fmt_time_nan_inf() {
        assert_eq!(fmt_time(f64::NAN), "--:--");
        assert_eq!(fmt_time(f64::INFINITY), "--:--");
        assert_eq!(fmt_time(f64::NEG_INFINITY), "--:--");
    }

    #[test]
    fn glyph_ranges_terminated() {
        // GLYPH_RANGES must end with 0 for imgui
        assert_eq!(*GLYPH_RANGES.last().unwrap(), 0);
    }

    #[test]
    fn glyph_ranges_pairs() {
        // Every range before the terminator should be a start,end pair
        let ranges = &GLYPH_RANGES[..GLYPH_RANGES.len() - 1];
        assert_eq!(ranges.len() % 2, 0, "glyph ranges must be pairs");
        for chunk in ranges.chunks(2) {
            assert!(
                chunk[0] <= chunk[1],
                "range start {} > end {}",
                chunk[0],
                chunk[1]
            );
        }
    }

    #[test]
    fn bundled_font_not_empty() {
        assert!(BUNDLED_FONT.len() > 1000, "font should be at least 1KB");
    }
}
