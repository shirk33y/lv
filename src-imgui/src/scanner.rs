//! Directory scanner: discover media files and insert into DB.

use std::path::Path;
use walkdir::WalkDir;

use crate::db::Db;

const MEDIA_EXTENSIONS: &[&str] = &[
    // images
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif", "ico",
    // video
    "mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
];

pub fn discover(db: &Db, root: &Path) -> usize {
    let mut count = 0usize;

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
            continue;
        }

        let abs = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
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

        let fmeta = entry.metadata().ok();
        let size = fmeta.as_ref().map(|m| m.len() as i64);
        let modified_at = fmeta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| iso_lite(d.as_secs()))
            });

        let path_str = abs.to_string_lossy().to_string();
        let mtime_ref = modified_at.as_deref();

        if let Some((file_id, db_size, db_mtime)) = db.file_lookup(&path_str) {
            let changed = db_size != size || db_mtime.as_deref() != mtime_ref;
            if changed {
                db.file_update_meta(file_id, size, mtime_ref);
                count += 1;
            }
            continue;
        }

        if db
            .file_insert(&path_str, &dir, &filename, size, mtime_ref)
            .is_some()
        {
            count += 1;
        }
    }

    count
}

fn iso_lite(epoch_secs: u64) -> String {
    let s = epoch_secs;
    let days = s / 86400;
    let time = s % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let sec = time % 60;

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
