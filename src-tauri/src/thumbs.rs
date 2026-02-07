use anyhow::{Context, Result};
use image::GenericImageView;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;
#[cfg(unix)]
use wait_timeout::ChildExt;

use crate::data::Db;
use crate::debug::dbg_log;

const THUMB_MAX_SIZE: u32 = 256;
const SHADOW_W: u32 = 6;
const SHADOW_H: u32 = 4;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
];

static FFMPEG_INIT: Once = Once::new();

/// Ensure ffmpeg is available — download via ffmpeg-sidecar if not on system PATH.
pub fn ensure_ffmpeg() {
    FFMPEG_INIT.call_once(|| {
        if which("ffmpeg") {
            dbg_log!("ffmpeg: using system binary");
            return;
        }
        dbg_log!("ffmpeg: not on PATH, downloading via sidecar...");
        match ffmpeg_sidecar::download::auto_download() {
            Ok(_) => dbg_log!("ffmpeg: sidecar download complete"),
            Err(e) => eprintln!("warning: ffmpeg download failed: {}", e),
        }
    });
}

/// Resolve ffmpeg binary path — prefer system, fall back to sidecar.
fn ffmpeg_bin() -> PathBuf {
    if which("ffmpeg") {
        return PathBuf::from("ffmpeg");
    }
    if let Ok(dir) = ffmpeg_sidecar::paths::sidecar_dir() {
        let bin = if cfg!(windows) {
            dir.join("ffmpeg.exe")
        } else {
            dir.join("ffmpeg")
        };
        if bin.exists() {
            return bin;
        }
    }
    PathBuf::from("ffmpeg")
}

/// Resolve ffprobe binary path — prefer system, fall back to sidecar.
fn ffprobe_bin() -> PathBuf {
    if which("ffprobe") {
        return PathBuf::from("ffprobe");
    }
    if let Ok(dir) = ffmpeg_sidecar::paths::sidecar_dir() {
        let bin = if cfg!(windows) {
            dir.join("ffprobe.exe")
        } else {
            dir.join("ffprobe")
        };
        if bin.exists() {
            return bin;
        }
    }
    PathBuf::from("ffprobe")
}

fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a thumbnail for a given meta_id.
/// Images: fast decode + thumbnail() (Triangle filter).
/// Videos: ffmpeg keyframe extraction (5 I-frames, picks middle one).
pub fn generate_for_meta(db: &Db, meta_id: i64) -> Result<()> {
    let path = db
        .file_path_for_meta(meta_id)
        .context("no file found for meta")?;

    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let is_video = VIDEO_EXTENSIONS.contains(&ext.as_str());

    let (webp_buf, orig_w, orig_h) = if is_video {
        generate_video_thumb(&path)?
    } else {
        generate_image_thumb(&path)?
    };

    let fmt = detect_format(&ext);
    db.meta_set_dimensions(meta_id, orig_w, orig_h, fmt);
    db.thumb_save(meta_id, "default", &webp_buf);

    // Generate tiny 6x4 shadow from the main thumbnail
    if let Ok(shadow_buf) = generate_shadow(&webp_buf) {
        db.thumb_save(meta_id, "shadow", &shadow_buf);
    }

    Ok(())
}

/// Generate a tiny 6x4 blurred WebP from the main thumbnail buffer.
fn generate_shadow(thumb_webp: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(thumb_webp).context("decode thumb for shadow")?;
    let tiny = img.resize_exact(SHADOW_W, SHADOW_H, image::imageops::FilterType::Triangle);
    // Apply a simple box blur by resizing down then up, but 6x4 is already so small
    // the browser CSS blur handles the rest.
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    tiny.write_to(&mut cursor, image::ImageFormat::WebP)?;
    Ok(buf)
}

/// Fast image thumbnail: uses thumbnail() which does a single-pass box filter
/// (much faster than Lanczos3 for preview use).
fn generate_image_thumb(path: &str) -> Result<(Vec<u8>, u32, u32)> {
    let img = image::open(path).context("decode failed")?;
    let (w, h) = img.dimensions();

    // thumbnail() uses a fast approximation — ~3-5x faster than resize(Lanczos3)
    let thumb = img.thumbnail(THUMB_MAX_SIZE, THUMB_MAX_SIZE);

    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    thumb.write_to(&mut cursor, image::ImageFormat::WebP)?;

    Ok((buf, w, h))
}

/// Max seconds to wait for ffprobe/ffmpeg before killing.
const FF_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a command with a timeout. Kills the process if it exceeds the limit.
fn run_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output> {
    use std::io::Read;

    #[cfg(not(unix))]
    let wait_result: Result<Option<std::process::ExitStatus>, std::io::Error> =
        child.wait().map(Some);
    #[cfg(unix)]
    let wait_result = child.wait_timeout(timeout);

    match wait_result {
        Ok(Some(status)) => {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut out) = child.stdout.take() {
                out.read_to_end(&mut stdout).ok();
            }
            if let Some(mut err) = child.stderr.take() {
                err.read_to_end(&mut stderr).ok();
            }
            Ok(std::process::Output {
                status,
                stdout,
                stderr,
            })
        }
        Ok(None) => {
            // Timed out — kill it
            child.kill().ok();
            child.wait().ok();
            anyhow::bail!("timed out after {}s", timeout.as_secs());
        }
        Err(e) => anyhow::bail!("wait failed: {}", e),
    }
}

/// Video thumbnail via ffmpeg: seek to ~30%, extract single keyframe,
/// scale to 256px width, output WebP. No full file decode.
fn generate_video_thumb(path: &str) -> Result<(Vec<u8>, u32, u32)> {
    ensure_ffmpeg();

    // Get video dimensions + duration via ffprobe (with timeout)
    let probe_child = Command::new(ffprobe_bin())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,duration",
            "-of",
            "csv=p=0",
            path,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("ffprobe failed to start")?;

    let probe = run_with_timeout(probe_child, FF_TIMEOUT)?;

    let probe_str = String::from_utf8_lossy(&probe.stdout);
    let parts: Vec<&str> = probe_str.trim().split(',').collect();
    let orig_w: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1920);
    let orig_h: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1080);
    let duration: f64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(60.0);

    // Seek to ~30% (avoids intros/black screens)
    let seek_to = (duration * 0.3).max(1.0);
    dbg_log!(
        "video thumb: {}x{} dur={:.0}s seek={:.1}s",
        orig_w,
        orig_h,
        duration,
        seek_to
    );

    // Extract single keyframe, scale, output WebP to stdout (with timeout)
    let ff_child = Command::new(ffmpeg_bin())
        .args([
            "-ss",
            &format!("{:.1}", seek_to),
            "-skip_frame",
            "nokey",
            "-i",
            path,
            "-vframes",
            "1",
            "-vf",
            &format!("scale={}:-2", THUMB_MAX_SIZE),
            "-c:v",
            "libwebp",
            "-quality",
            "50",
            "-f",
            "webp",
            "-y",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("ffmpeg failed to start")?;

    let output = run_with_timeout(ff_child, FF_TIMEOUT)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "ffmpeg error: {}",
            stderr.lines().last().unwrap_or("unknown")
        );
    }

    if output.stdout.is_empty() {
        anyhow::bail!("ffmpeg produced empty output");
    }

    Ok((output.stdout, orig_w, orig_h))
}

fn detect_format(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" => "bmp",
        "tiff" | "tif" => "tiff",
        "avif" => "avif",
        "heic" | "heif" => "heic",
        "svg" => "svg",
        "mp4" | "m4v" => "mp4",
        "mkv" => "mkv",
        "avi" => "avi",
        "mov" => "mov",
        "webm" => "webm",
        "flv" => "flv",
        "wmv" => "wmv",
        "3gp" => "3gp",
        _ => "unknown",
    }
}
