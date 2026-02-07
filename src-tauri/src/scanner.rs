use std::path::Path;
use walkdir::WalkDir;

use crate::data::Db;

const MEDIA_EXTENSIONS: &[&str] = &[
    // images
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif", "ico", "psd", "raw",
    "cr2", "nef", "arw", "dng", // video
    "mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
];

pub fn discover(db: &Db, root: &Path) -> usize {
    use crate::debug::dbg_log;
    dbg_log!("scan root: {}", root.display());
    let mut count = 0;
    let mut skipped = 0usize;

    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !MEDIA_EXTENSIONS.contains(&ext.as_str()) {
            skipped += 1;
            continue;
        }

        let abs = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };
        // Strip Windows extended-length prefix (\\?\)
        #[cfg(windows)]
        let abs = {
            let s = abs.to_string_lossy();
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                std::path::PathBuf::from(stripped)
            } else {
                abs
            }
        };

        let dir = abs
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string();
        let filename = abs
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let meta = entry.metadata().ok();
        let size = meta.as_ref().map(|m| m.len() as i64);
        let modified_at = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| chrono_lite(d.as_secs()))
        });

        let path_str = abs.to_string_lossy().to_string();
        let mtime_ref = modified_at.as_deref();

        // Check if file already exists in DB
        if let Some((file_id, db_size, db_mtime)) = db.file_lookup(&path_str) {
            let changed = db_size != size || db_mtime.as_deref() != mtime_ref;
            if changed {
                dbg_log!(
                    "~ {} (id={}, size/mtime changed, re-queuing)",
                    filename,
                    file_id
                );
                db.file_mark_changed(file_id, size, mtime_ref);
                db.jobs_enqueue_hash(file_id);
                count += 1;
            }
            continue;
        }

        // New file — insert and enqueue hash job
        if let Some(file_id) = db.file_insert(&path_str, &dir, &filename, size, mtime_ref) {
            db.jobs_enqueue_hash(file_id);
            dbg_log!("+ {} (id={}, job queued)", filename, file_id);
            count += 1;
        }
    }

    dbg_log!(
        "scan done: {} new/changed, {} skipped (non-media)",
        count,
        skipped
    );
    count
}

fn chrono_lite(epoch_secs: u64) -> String {
    // Simple ISO8601 without pulling in chrono crate
    let s = epoch_secs;
    let days = s / 86400;
    let time = s % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let sec = time % 60;

    // Approximate date from epoch days (good enough for display)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let months = [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1;
    for &ml in &months {
        if remaining < ml {
            break;
        }
        remaining -= ml;
        mo += 1;
    }
    let d = remaining + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, sec)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
