//! Directory scanner: discover media files and insert into DB.

use std::path::Path;
use walkdir::WalkDir;

use crate::db::Db;

use crate::clean_path;

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

    // Prune: get the canonical dir (what discover stores in the DB) and check
    // every file under it for existence on disk.
    let canon_dir = root
        .canonicalize()
        .map(|p| clean_path(&p.to_string_lossy()))
        .unwrap_or_else(|_| clean_path(&root.to_string_lossy()));

    let db_files = db.files_by_dir(&canon_dir);
    let mut pruned = 0usize;
    for f in &db_files {
        if !Path::new(&f.path).exists() {
            db.remove_file_by_id(f.id);
            eprintln!("rescan: pruned {}", f.path);
            pruned += 1;
        }
    }

    if updated > 0 || pruned > 0 {
        eprintln!(
            "rescan: {} — {} added/updated, {} pruned",
            canon_dir, updated, pruned
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

    // ── Regression: paths stored in DB must never have \\?\ prefix ──────

    #[test]
    fn discover_no_win_prefix_in_db_paths() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();

        discover(&db, dir.path());

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        let files = db.files_by_dir(&dir_str);
        assert_eq!(files.len(), 1);
        // No file path or dir should contain the Windows extended-length prefix
        for f in &files {
            assert!(
                !f.path.starts_with(r"\\?\"),
                "path has \\\\?\\ prefix: {}",
                f.path
            );
            assert!(
                !f.dir.starts_with(r"\\?\"),
                "dir has \\\\?\\ prefix: {}",
                f.dir
            );
        }
    }

    #[test]
    fn rescan_no_win_prefix_in_db_paths() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.png"), b"img").unwrap();

        let (updated, _pruned) = rescan(&db, dir.path());
        assert_eq!(updated, 1);

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        for f in &db.files_by_dir(&dir_str) {
            assert!(
                !f.path.starts_with(r"\\?\"),
                "path has \\\\?\\ prefix: {}",
                f.path
            );
            assert!(
                !f.dir.starts_with(r"\\?\"),
                "dir has \\\\?\\ prefix: {}",
                f.dir
            );
        }
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
        let (_updated, pruned) = rescan(&db, dir.path());
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

    #[test]
    fn rescan_idempotent() {
        // Running rescan twice with no disk changes should be a no-op
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();
        std::fs::write(dir.path().join("b.png"), b"img2").unwrap();

        let (u1, p1) = rescan(&db, dir.path());
        assert_eq!(u1, 2);
        assert_eq!(p1, 0);

        // Second rescan: nothing changed on disk
        let (u2, p2) = rescan(&db, dir.path());
        assert_eq!(u2, 0, "no new files to add");
        assert_eq!(p2, 0, "no files to prune");

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        assert_eq!(db.files_by_dir(&dir_str).len(), 2);
    }

    #[test]
    fn rescan_non_media_files_ignored() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"text").unwrap();
        std::fs::write(dir.path().join("data.json"), b"{}").unwrap();
        std::fs::write(dir.path().join("script.py"), b"print()").unwrap();
        std::fs::write(dir.path().join("photo.jpg"), b"img").unwrap();

        let (updated, pruned) = rescan(&db, dir.path());
        assert_eq!(updated, 1, "only photo.jpg should be added");
        assert_eq!(pruned, 0);
    }

    #[test]
    fn rescan_with_subdirectories() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("root.jpg"), b"img").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/nested.png"), b"img2").unwrap();

        let (updated, _pruned) = rescan(&db, dir.path());
        assert!(updated >= 2, "should find files in subdirectories");
    }

    #[test]
    fn rescan_prune_all_files_leaves_empty_list() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();
        std::fs::write(dir.path().join("b.png"), b"img2").unwrap();

        rescan(&db, dir.path());
        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        assert_eq!(db.files_by_dir(&dir_str).len(), 2);

        // Delete all files
        std::fs::remove_file(dir.path().join("a.jpg")).unwrap();
        std::fs::remove_file(dir.path().join("b.png")).unwrap();

        let (_u, pruned) = rescan(&db, dir.path());
        assert_eq!(pruned, 2);
        assert!(db.files_by_dir(&dir_str).is_empty());
    }

    #[test]
    fn rescan_files_by_dir_matches_disk_exactly() {
        // Regression: after rescan, files_by_dir should return exactly
        // the files that exist on disk — no stale entries, no missing entries.
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"a").unwrap();
        std::fs::write(dir.path().join("b.png"), b"b").unwrap();
        std::fs::write(dir.path().join("c.mp4"), b"c").unwrap();

        rescan(&db, dir.path());

        // Offline: delete b, add d, keep a and c
        std::fs::remove_file(dir.path().join("b.png")).unwrap();
        std::fs::write(dir.path().join("d.gif"), b"d").unwrap();

        rescan(&db, dir.path());

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        let files = db.files_by_dir(&dir_str);
        let names: Vec<&str> = files.iter().map(|f| f.filename.as_str()).collect();

        // Every file in the list should exist on disk
        for f in &files {
            assert!(
                std::path::Path::new(&f.path).exists(),
                "stale entry in DB: {}",
                f.path
            );
        }

        // Every media file on disk should be in the list
        assert!(names.contains(&"a.jpg"));
        assert!(names.contains(&"c.mp4"));
        assert!(names.contains(&"d.gif"));
        assert!(!names.contains(&"b.png"), "deleted file still in DB");
    }

    // ── Scanner edge cases ────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn discover_circular_symlink_no_infinite_loop() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("real.jpg"), b"img").unwrap();
        // Create circular symlink: sub -> ..
        std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();

        // discover uses WalkDir with follow_links(true) which has cycle detection
        // This should complete without hanging
        let count = discover(&db, dir.path());
        assert!(count >= 1, "should find at least real.jpg");
    }

    #[cfg(unix)]
    #[test]
    fn discover_permission_denied_skips_file() {
        use std::os::unix::fs::PermissionsExt;

        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readable.jpg"), b"img").unwrap();
        std::fs::write(dir.path().join("secret.jpg"), b"img").unwrap();

        // Make secret.jpg unreadable
        let perms = std::fs::Permissions::from_mode(0o000);
        std::fs::set_permissions(dir.path().join("secret.jpg"), perms).unwrap();

        // discover should not panic, should find at least readable.jpg
        let count = discover(&db, dir.path());
        assert!(count >= 1);

        // Cleanup: restore permissions so tempdir can be deleted
        let perms = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(dir.path().join("secret.jpg"), perms).unwrap();
    }

    #[test]
    fn discover_zero_byte_media_files() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        // Zero-byte file with media extension
        std::fs::write(dir.path().join("empty.jpg"), b"").unwrap();
        std::fs::write(dir.path().join("normal.png"), b"img data").unwrap();

        let count = discover(&db, dir.path());
        // Both should be added (we don't reject zero-byte files at scan time)
        assert_eq!(count, 2);

        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        let files = db.files_by_dir(&dir_str);
        assert_eq!(files.len(), 2);

        // Zero-byte file should have size 0
        let empty = files.iter().find(|f| f.filename == "empty.jpg").unwrap();
        let (_, size, _) = db.file_lookup(&empty.path).unwrap();
        assert_eq!(size, Some(0));
    }

    #[test]
    fn discover_file_disappears_during_scan() {
        // TOCTOU: file exists when readdir runs but gone by canonicalize
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("stable.jpg"), b"img").unwrap();
        // Create and immediately delete — WalkDir may or may not see it
        std::fs::write(dir.path().join("ghost.jpg"), b"img").unwrap();
        std::fs::remove_file(dir.path().join("ghost.jpg")).unwrap();

        // Should not panic regardless of timing
        let count = discover(&db, dir.path());
        assert!(count >= 1, "should find at least stable.jpg");
    }

    #[cfg(unix)]
    #[test]
    fn discover_symlink_target_deleted() {
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.jpg");
        std::fs::write(&target, b"img").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("link.jpg")).unwrap();

        // Both real and link should be found (they canonicalize to same path)
        discover(&db, dir.path());

        // Now delete the target
        std::fs::remove_file(&target).unwrap();

        // Rescan should prune the dead entries
        let (_updated, pruned) = rescan(&db, dir.path());
        assert!(pruned >= 1, "dead symlink target should be pruned");
    }

    #[test]
    fn rescan_nonexistent_dir_no_panic() {
        let db = Db::open_memory();
        db.ensure_schema();

        let (updated, pruned) = rescan(&db, std::path::Path::new("/nonexistent/dir/xyz"));
        assert_eq!(updated, 0);
        assert_eq!(pruned, 0);
    }

    #[test]
    fn discover_does_not_reinsert_deleted_files() {
        // Regression: discover() should skip files that don't exist on disk
        // (canonicalize fails → continue). This ensures rescan's prune
        // isn't undone by a subsequent discover call.
        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"img").unwrap();

        discover(&db, dir.path());
        let dir_str = clean_path(&dir.path().canonicalize().unwrap().to_string_lossy());
        assert_eq!(db.files_by_dir(&dir_str).len(), 1);

        // Delete from disk, then prune from DB
        std::fs::remove_file(dir.path().join("a.jpg")).unwrap();
        let files = db.files_by_dir(&dir_str);
        db.remove_file_by_id(files[0].id);
        assert!(db.files_by_dir(&dir_str).is_empty());

        // discover again — should NOT re-add the deleted file
        let added = discover(&db, dir.path());
        assert_eq!(added, 0);
        assert!(db.files_by_dir(&dir_str).is_empty());
    }
}
