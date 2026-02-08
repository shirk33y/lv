//! Directory scanner: discover media files and insert into DB.

use std::path::Path;
use walkdir::WalkDir;

use crate::db::Db;

/// Strip Windows extended-length path prefix (`\\?\`) if present.
fn clean_path(s: &str) -> String {
    s.strip_prefix(r"\\?\").unwrap_or(s).to_string()
}

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

        let dir = clean_path(&abs.parent().unwrap_or(Path::new("")).to_string_lossy());
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

        let path_str = clean_path(&abs.to_string_lossy());
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

/// Full rescan of a watched directory: discover new/updated files, prune deleted ones.
/// Returns (added_or_updated, pruned).
pub fn rescan(db: &Db, root: &Path) -> (usize, usize) {
    let updated = discover(db, root);

    // Prune: check every file in DB under this dir and remove if gone from disk.
    let dir_str = clean_path(&root.to_string_lossy());
    // Also try canonicalized form (Windows canonicalize adds \\?\ which clean_path strips)
    let canon_dir = root
        .canonicalize()
        .map(|p| clean_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| dir_str.clone());

    let db_paths = db.file_paths_under(&canon_dir);
    let mut pruned = 0usize;
    for (id, path) in &db_paths {
        if !Path::new(path).exists() {
            db.remove_file_by_id(*id);
            eprintln!("rescan: pruned {}", path);
            pruned += 1;
        }
    }
    // If canon_dir differs from dir_str, also check files stored under the raw dir
    if canon_dir != dir_str {
        for (id, path) in &db.file_paths_under(&dir_str) {
            if !Path::new(path).exists() {
                db.remove_file_by_id(*id);
                eprintln!("rescan: pruned {}", path);
                pruned += 1;
            }
        }
    }

    if updated > 0 || pruned > 0 {
        eprintln!(
            "rescan: {} — {} added/updated, {} pruned",
            dir_str, updated, pruned
        );
    }

    (updated, pruned)
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

#[allow(dead_code)]
pub fn is_media_ext(ext: &str) -> bool {
    MEDIA_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_leap ─────────────────────────────────────────────────────────

    #[test]
    fn leap_years() {
        assert!(is_leap(2000)); // divisible by 400
        assert!(is_leap(2024)); // divisible by 4, not 100
        assert!(is_leap(1600));
        assert!(is_leap(2400));
    }

    #[test]
    fn non_leap_years() {
        assert!(!is_leap(1900)); // divisible by 100 but not 400
        assert!(!is_leap(2100));
        assert!(!is_leap(2023));
        assert!(!is_leap(2025));
        assert!(!is_leap(1));
    }

    // ── iso_lite ────────────────────────────────────────────────────────

    #[test]
    fn iso_lite_epoch_zero() {
        assert_eq!(iso_lite(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn iso_lite_known_dates() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(iso_lite(1704067200), "2024-01-01T00:00:00Z");
        // 2000-01-01 00:00:00 UTC = 946684800
        assert_eq!(iso_lite(946684800), "2000-01-01T00:00:00Z");
    }

    #[test]
    fn iso_lite_with_time() {
        // 1970-01-01 12:30:45 = 45045
        assert_eq!(iso_lite(45045), "1970-01-01T12:30:45Z");
    }

    #[test]
    fn iso_lite_leap_day() {
        // 2024-02-29 00:00:00 UTC = 1709164800
        assert_eq!(iso_lite(1709164800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn iso_lite_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        assert_eq!(iso_lite(1704067199), "2023-12-31T23:59:59Z");
    }

    // ── media extension filtering ───────────────────────────────────────

    #[test]
    fn media_ext_images() {
        for ext in &[
            "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif", "ico",
        ] {
            assert!(is_media_ext(ext), "{} should be media", ext);
        }
    }

    #[test]
    fn media_ext_videos() {
        for ext in &[
            "mp4", "avi", "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
        ] {
            assert!(is_media_ext(ext), "{} should be media", ext);
        }
    }

    #[test]
    fn media_ext_case_insensitive() {
        assert!(is_media_ext("JPG"));
        assert!(is_media_ext("Png"));
        assert!(is_media_ext("MKV"));
        assert!(is_media_ext("WebM"));
    }

    #[test]
    fn non_media_ext_rejected() {
        for ext in &[
            "txt", "pdf", "doc", "rs", "html", "css", "json", "xml", "zip", "exe", "sh", "py",
            "svg", "avif",
        ] {
            assert!(!is_media_ext(ext), "{} should NOT be media", ext);
        }
    }

    #[test]
    fn empty_ext_rejected() {
        assert!(!is_media_ext(""));
    }

    // ── rescan ───────────────────────────────────────────────────────────

    #[test]
    fn rescan_adds_new_files() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();

        let (updated, pruned) = rescan(&db, dir.path());
        assert_eq!(updated, 1, "should add new file");
        assert_eq!(pruned, 0);

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        let files = db.files_by_dir(&dir_str);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "a.jpg");
    }

    #[test]
    fn rescan_prunes_deleted_files() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();
        std::fs::write(dir.path().join("b.png"), b"img").unwrap();

        // Initial scan
        rescan(&db, dir.path());
        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        assert_eq!(db.files_by_dir(&dir_str).len(), 2);

        // Delete one file from disk
        std::fs::remove_file(dir.path().join("a.jpg")).unwrap();

        // Rescan should prune it
        let (updated, pruned) = rescan(&db, dir.path());
        assert_eq!(pruned, 1, "should prune deleted file");
        let files = db.files_by_dir(&dir_str);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "b.png");
    }

    #[test]
    fn rescan_updates_changed_files() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"small").unwrap();

        rescan(&db, dir.path());
        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        let files = db.files_by_dir(&dir_str);
        let old_size = db.file_lookup(&files[0].path).unwrap().1;

        // Modify the file (make it bigger)
        std::fs::write(dir.path().join("a.jpg"), b"much larger content here!!!").unwrap();

        let (updated, pruned) = rescan(&db, dir.path());
        assert!(updated >= 1, "should detect changed file");
        assert_eq!(pruned, 0);

        let new_size = db.file_lookup(&files[0].path).unwrap().1;
        assert_ne!(old_size, new_size, "size should be updated");
    }

    #[test]
    fn rescan_full_sync() {
        // Simulate what happens between app sessions:
        // some files added, some deleted, some changed
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keep.jpg"), b"keep").unwrap();
        std::fs::write(dir.path().join("delete_me.png"), b"gone").unwrap();
        std::fs::write(dir.path().join("change_me.gif"), b"old").unwrap();

        rescan(&db, dir.path());
        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        assert_eq!(db.files_by_dir(&dir_str).len(), 3);

        // Simulate offline changes
        std::fs::remove_file(dir.path().join("delete_me.png")).unwrap();
        std::fs::write(
            dir.path().join("change_me.gif"),
            b"new content that is longer",
        )
        .unwrap();
        std::fs::write(dir.path().join("new_file.mp4"), b"video").unwrap();

        let (updated, pruned) = rescan(&db, dir.path());
        assert!(updated >= 2, "should add new + update changed");
        assert_eq!(pruned, 1, "should prune deleted");

        let files = db.files_by_dir(&dir_str);
        assert_eq!(files.len(), 3, "keep + changed + new");
        let names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();
        assert!(names.contains(&"keep.jpg"));
        assert!(names.contains(&"change_me.gif"));
        assert!(names.contains(&"new_file.mp4"));
        assert!(!names.contains(&"delete_me.png"));
    }

    #[test]
    fn rescan_empty_dir_no_panic() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        let (updated, pruned) = rescan(&db, dir.path());
        assert_eq!(updated, 0);
        assert_eq!(pruned, 0);
    }
}
