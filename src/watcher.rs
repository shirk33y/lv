//! Filesystem watcher: monitors watched directories for changes and syncs DB.
//!
//! Spawns a background thread that uses `notify` to watch directories marked
//! as `watched=1` in the DB. File create/modify/remove events are processed
//! and sent to the main thread via a channel so it can refresh the file list.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::db::Db;

/// Extract the parent directory from a path string, handling both `/` and `\` separators.
/// Needed because `std::path::Path::parent()` doesn't understand `\` on Linux.
fn str_parent(p: &str) -> &str {
    let last_fwd = p.rfind('/');
    let last_back = p.rfind('\\');
    let last_sep = match (last_fwd, last_back) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    match last_sep {
        Some(i) => &p[..i],
        None => "",
    }
}

/// Check if a path string has a media extension, handling both `/` and `\` separators.
/// Unlike `is_media()` which uses `Path::extension()` (platform-dependent), this works
/// cross-platform by parsing the extension from the string directly.
fn has_media_ext(p: &str) -> bool {
    let last_sep = p
        .rfind('/')
        .or_else(|| p.rfind('\\'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let filename = &p[last_sep..];
    if let Some(dot) = filename.rfind('.') {
        let ext = &filename[dot + 1..];
        MEDIA_EXTENSIONS.contains(&ext.to_lowercase().as_str())
    } else {
        false
    }
}

/// Events sent from the watcher thread to the main loop.
#[derive(Debug)]
pub enum FsEvent {
    /// A file was created or modified — the dir it belongs to may need refreshing.
    Changed(String),
    /// A file was removed.
    Removed(String),
}

/// Commands sent from the main thread to the watcher thread.
pub enum WatchCmd {
    /// Watch a directory (non-recursively) for changes.
    Watch(String),
    /// Stop watching a directory.
    Unwatch(String),
}

/// Handle to the running watcher. Drop to stop.
pub struct FsWatcher {
    quit: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    cmd_tx: mpsc::Sender<WatchCmd>,
}

impl FsWatcher {
    /// Start the filesystem watcher. Returns the handle and a receiver for events.
    pub fn start(db: Db) -> (Self, mpsc::Receiver<FsEvent>) {
        let (tx, rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let quit = Arc::new(AtomicBool::new(false));
        let quit2 = quit.clone();

        let thread = std::thread::Builder::new()
            .name("fs-watcher".into())
            .spawn(move || {
                run_watcher(db, tx, quit2, cmd_rx);
            })
            .expect("failed to spawn fs-watcher thread");

        (
            FsWatcher {
                quit,
                thread: Some(thread),
                cmd_tx,
            },
            rx,
        )
    }

    /// Dynamically watch a directory (non-recursive).
    pub fn watch_dir(&self, dir: &str) {
        self.cmd_tx.send(WatchCmd::Watch(dir.to_string())).ok();
    }

    /// Dynamically unwatch a directory.
    pub fn unwatch_dir(&self, dir: &str) {
        self.cmd_tx.send(WatchCmd::Unwatch(dir.to_string())).ok();
    }

    pub fn stop(&mut self) {
        self.quit.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.join().ok();
        }
    }
}

impl Drop for FsWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

const MEDIA_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif", "ico", "mp4", "avi",
    "mov", "mkv", "webm", "flv", "wmv", "m4v", "3gp",
];

fn run_watcher(
    db: Db,
    tx: mpsc::Sender<FsEvent>,
    quit: Arc<AtomicBool>,
    cmd_rx: mpsc::Receiver<WatchCmd>,
) {
    // Channel for notify events
    let (ntx, nrx) = mpsc::channel();

    let mut watcher: RecommendedWatcher = match notify::recommended_watcher(move |res| {
        if let Ok(event) = res {
            ntx.send(event).ok();
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("watcher: failed to create: {}", e);
            return;
        }
    };

    // Watch all dirs marked as watched in DB, with nested dedup
    let watched = db.watched_dirs();
    if !watched.is_empty() {
        let effective = dedup_nested(&watched);
        for (dir, recursive) in &effective {
            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            match watcher.watch(Path::new(dir), mode) {
                Ok(()) => eprintln!("watcher: watching {} (recursive={})", dir, recursive),
                Err(e) => eprintln!("watcher: failed to watch {}: {}", dir, e),
            }
        }
        if effective.len() < watched.len() {
            eprintln!(
                "watcher: deduped {} → {} watches (nested dirs skipped)",
                watched.len(),
                effective.len()
            );
        }
    }

    // Process events + commands until quit
    while !quit.load(Ordering::Relaxed) {
        // Process any pending commands (watch/unwatch)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WatchCmd::Watch(dir) => {
                    match watcher.watch(Path::new(&dir), RecursiveMode::NonRecursive) {
                        Ok(()) => eprintln!("watcher: +watch {}", dir),
                        Err(e) => eprintln!("watcher: failed to watch {}: {}", dir, e),
                    }
                }
                WatchCmd::Unwatch(dir) => match watcher.unwatch(Path::new(&dir)) {
                    Ok(()) => eprintln!("watcher: -watch {}", dir),
                    Err(e) => eprintln!("watcher: failed to unwatch {}: {}", dir, e),
                },
            }
        }

        // Process notify events
        match nrx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                handle_event(&db, &tx, event);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    eprintln!("watcher: stopped");
}

pub(crate) fn handle_event(db: &Db, tx: &mpsc::Sender<FsEvent>, event: notify::Event) {
    let is_remove = matches!(event.kind, EventKind::Remove(_));

    for path in &event.paths {
        // Normalize: strip Windows \\?\ prefix early so all paths match the DB.
        // Use string-level helpers (str_parent, has_media_ext) because
        // std::path::Path on Linux doesn't parse Windows `\` as separators.
        let raw_str = path.to_string_lossy().to_string();
        let path_str = crate::clean_path(&raw_str);

        // Skip directories (we only care about files)
        if path.is_dir() {
            continue;
        }
        // For removes the file no longer exists on disk, so is_file() is false.
        // Use string-level extension check for all filtering.
        if !is_remove && path.is_file() && !has_media_ext(&path_str) {
            continue;
        }
        if is_remove && !has_media_ext(&path_str) {
            continue;
        }

        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                // Insert or update the file in DB
                let abs = match path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => path.clone(),
                };
                let abs_str = crate::clean_path(&abs.to_string_lossy());

                if !has_media_ext(&abs_str) {
                    continue;
                }

                let dir = str_parent(&abs_str).to_string();
                let filename = abs
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                let meta = std::fs::metadata(&abs).ok();
                let size = meta.as_ref().map(|m| m.len() as i64);
                let mtime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| format!("{}", d.as_secs()))
                });
                let mtime_ref = mtime.as_deref();

                if let Some((file_id, db_size, db_mtime)) = db.file_lookup(&abs_str) {
                    let changed = db_size != size || db_mtime.as_deref() != mtime_ref;
                    if changed {
                        db.file_update_meta(file_id, size, mtime_ref);
                        eprintln!("watcher: updated {}", filename);
                    }
                } else {
                    db.file_insert(&abs_str, &dir, &filename, size, mtime_ref);
                    eprintln!("watcher: added {}", filename);
                }

                tx.send(FsEvent::Changed(dir)).ok();
            }
            EventKind::Remove(_) => {
                // File no longer exists so we can't canonicalize.
                // path_str is already clean_path'd above.
                if db.file_lookup(&path_str).is_some() {
                    db.remove_file_by_path(&path_str);
                    let dir = str_parent(&path_str).to_string();
                    eprintln!("watcher: removed {}", path_str);
                    tx.send(FsEvent::Removed(dir)).ok();
                }
            }
            _ => {}
        }
    }
}

/// Deduplicate nested watched directories: if `/a` is recursive, skip `/a/b`.
/// Non-recursive dirs are never ancestors (they don't cover children).
fn dedup_nested(dirs: &[(String, bool)]) -> Vec<(String, bool)> {
    // Collect recursive dirs first (they can subsume children)
    let recursive: Vec<&str> = dirs
        .iter()
        .filter(|(_, r)| *r)
        .map(|(p, _)| p.as_str())
        .collect();

    dirs.iter()
        .filter(|(path, _)| {
            // Keep this dir unless a *different* recursive ancestor covers it
            !recursive.iter().any(|ancestor| {
                *ancestor != path.as_str()
                    && (path.starts_with(&format!("{}/", ancestor)) || *ancestor == path.as_str())
            })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_media_ext_recognizes_images() {
        assert!(has_media_ext("/a/photo.jpg"));
        assert!(has_media_ext("/a/photo.PNG"));
        assert!(has_media_ext("/a/photo.webp"));
    }

    #[test]
    fn has_media_ext_recognizes_videos() {
        assert!(has_media_ext("/a/clip.mp4"));
        assert!(has_media_ext("/a/clip.MKV"));
    }

    #[test]
    fn has_media_ext_rejects_non_media() {
        assert!(!has_media_ext("/a/readme.txt"));
        assert!(!has_media_ext("/a/script.rs"));
        assert!(!has_media_ext("/a/.gitignore"));
    }

    #[test]
    fn has_media_ext_no_extension() {
        assert!(!has_media_ext("/a/noext"));
        assert!(!has_media_ext("/a/"));
    }

    #[test]
    fn has_media_ext_windows_backslash() {
        assert!(has_media_ext(r"C:\Users\test\photo.jpg"));
        assert!(has_media_ext(r"C:\Users\test\clip.mp4"));
        assert!(!has_media_ext(r"C:\Users\test\readme.txt"));
    }

    #[test]
    fn has_media_ext_win_prefixed() {
        assert!(has_media_ext(r"\\?\C:\Users\test\photo.jpg"));
        assert!(!has_media_ext(r"\\?\C:\Users\test\readme.txt"));
    }

    // ── str_parent ──────────────────────────────────────────────────────

    #[test]
    fn str_parent_unix() {
        assert_eq!(str_parent("/a/b/c.jpg"), "/a/b");
        assert_eq!(str_parent("/photo.jpg"), "");
    }

    #[test]
    fn str_parent_windows() {
        assert_eq!(str_parent(r"C:\Users\test\photo.jpg"), r"C:\Users\test");
        assert_eq!(str_parent(r"C:\photo.jpg"), "C:");
    }

    #[test]
    fn str_parent_no_separator() {
        assert_eq!(str_parent("photo.jpg"), "");
    }

    #[test]
    fn str_parent_mixed_separators() {
        // Picks whichever separator type appears last
        assert_eq!(
            str_parent(r"/mnt/c\Users\test\photo.jpg"),
            r"/mnt/c\Users\test"
        );
        assert_eq!(str_parent(r"C:\Users/test/photo.jpg"), r"C:\Users/test");
    }

    // ── dedup_nested ────────────────────────────────────────────────────

    fn d(path: &str, recursive: bool) -> (String, bool) {
        (path.to_string(), recursive)
    }

    #[test]
    fn dedup_no_overlap() {
        let dirs = vec![d("/a", true), d("/b", true)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_child_of_recursive_removed() {
        let dirs = vec![d("/photos", true), d("/photos/vacation", true)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "/photos");
    }

    #[test]
    fn dedup_child_of_recursive_nonrecursive_child_removed() {
        let dirs = vec![d("/photos", true), d("/photos/vacation", false)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "/photos");
    }

    #[test]
    fn dedup_nonrecursive_parent_keeps_child() {
        // /photos is non-recursive, so /photos/vacation is NOT covered
        let dirs = vec![d("/photos", false), d("/photos/vacation", true)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_no_false_prefix_match() {
        // /photo should NOT subsume /photos
        let dirs = vec![d("/photo", true), d("/photos", true)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn dedup_deeply_nested() {
        let dirs = vec![
            d("/a", true),
            d("/a/b", true),
            d("/a/b/c", false),
            d("/x", false),
        ];
        let result = dedup_nested(&dirs);
        // /a covers /a/b and /a/b/c; /x is independent
        assert_eq!(result.len(), 2);
        let paths: Vec<&str> = result.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"/a"));
        assert!(paths.contains(&"/x"));
    }

    #[test]
    fn dedup_empty() {
        let dirs: Vec<(String, bool)> = vec![];
        assert!(dedup_nested(&dirs).is_empty());
    }

    #[test]
    fn dedup_single() {
        let dirs = vec![d("/only", true)];
        let result = dedup_nested(&dirs);
        assert_eq!(result.len(), 1);
    }

    // ── WatchCmd / FsWatcher dynamic watch ──────────────────────────────

    #[test]
    fn watch_cmd_watch_variant() {
        let cmd = WatchCmd::Watch("/tmp/test".into());
        match cmd {
            WatchCmd::Watch(dir) => assert_eq!(dir, "/tmp/test"),
            _ => panic!("expected Watch variant"),
        }
    }

    #[test]
    fn watch_cmd_unwatch_variant() {
        let cmd = WatchCmd::Unwatch("/tmp/test".into());
        match cmd {
            WatchCmd::Unwatch(dir) => assert_eq!(dir, "/tmp/test"),
            _ => panic!("expected Unwatch variant"),
        }
    }

    #[test]
    fn fs_event_changed_variant() {
        let ev = FsEvent::Changed("/photos".into());
        match ev {
            FsEvent::Changed(dir) => assert_eq!(dir, "/photos"),
            _ => panic!("expected Changed"),
        }
    }

    #[test]
    fn fs_event_removed_variant() {
        let ev = FsEvent::Removed("/photos".into());
        match ev {
            FsEvent::Removed(dir) => assert_eq!(dir, "/photos"),
            _ => panic!("expected Removed"),
        }
    }

    #[test]
    fn fs_event_debug_impl() {
        let ev = FsEvent::Changed("/a".into());
        let dbg = format!("{:?}", ev);
        assert!(dbg.contains("Changed"));
        assert!(dbg.contains("/a"));
    }

    #[test]
    fn dynamic_watch_detects_new_file() {
        use crate::db::Db;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();

        // Set up DB with the dir tracked
        let db = Db::open_memory();
        db.ensure_schema();
        db.dir_track(&dir_str, false);

        let (watcher, rx) = FsWatcher::start(db);

        // Dynamically watch the directory
        watcher.watch_dir(&dir_str);

        // Give watcher thread time to register the watch
        std::thread::sleep(Duration::from_millis(300));

        // Create a media file
        std::fs::write(dir.path().join("new_photo.jpg"), b"fake").unwrap();

        // Wait for the event (up to 2s)
        let mut got_event = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
                match ev {
                    FsEvent::Changed(d) if d == dir_str => {
                        got_event = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(got_event, "should receive Changed event for new media file");
        drop(watcher);
    }

    #[test]
    fn dynamic_watch_ignores_non_media() {
        use crate::db::Db;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();

        let db = Db::open_memory();
        db.ensure_schema();
        db.dir_track(&dir_str, false);

        let (watcher, rx) = FsWatcher::start(db);
        watcher.watch_dir(&dir_str);
        std::thread::sleep(Duration::from_millis(300));

        // Create a non-media file
        std::fs::write(dir.path().join("readme.txt"), b"hello").unwrap();

        // Should NOT receive an event for non-media
        std::thread::sleep(Duration::from_millis(500));
        let got = rx.try_recv().is_ok();
        assert!(!got, "should not receive event for non-media file");
        drop(watcher);
    }

    #[test]
    fn dynamic_watch_detects_removal() {
        use crate::db::Db;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();

        // Pre-create a file so we can remove it
        let file_path = dir.path().join("old.jpg");
        std::fs::write(&file_path, b"fake").unwrap();

        let db = Db::open_memory();
        db.ensure_schema();
        db.dir_track(&dir_str, false);
        crate::scanner::discover(&db, dir.path());

        let (watcher, rx) = FsWatcher::start(db);
        watcher.watch_dir(&dir_str);
        std::thread::sleep(Duration::from_millis(300));

        // Remove the file
        std::fs::remove_file(&file_path).unwrap();

        // Wait for Removed event
        let mut got_removed = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
                match ev {
                    FsEvent::Removed(_) => {
                        got_removed = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert!(got_removed, "should receive Removed event");
        drop(watcher);
    }

    #[test]
    fn dynamic_unwatch_stops_events() {
        use crate::db::Db;
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();

        let db = Db::open_memory();
        db.ensure_schema();
        db.dir_track(&dir_str, false);

        let (watcher, rx) = FsWatcher::start(db);
        watcher.watch_dir(&dir_str);
        std::thread::sleep(Duration::from_millis(300));

        // Unwatch
        watcher.unwatch_dir(&dir_str);
        std::thread::sleep(Duration::from_millis(300));

        // Create a media file — should NOT trigger event
        std::fs::write(dir.path().join("after_unwatch.jpg"), b"fake").unwrap();
        std::thread::sleep(Duration::from_millis(500));

        let got = rx.try_recv().is_ok();
        assert!(!got, "should not receive events after unwatch");
        drop(watcher);
    }

    #[test]
    fn watcher_stop_is_clean() {
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let (mut watcher, _rx) = FsWatcher::start(db);
        // Should not hang or panic
        watcher.stop();
    }

    #[test]
    fn watcher_drop_is_clean() {
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let (watcher, _rx) = FsWatcher::start(db);
        drop(watcher); // should not hang
    }

    #[test]
    fn watch_dir_sends_command() {
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let (watcher, _rx) = FsWatcher::start(db);
        // Should not panic even with nonexistent dir
        watcher.watch_dir("/nonexistent/path/12345");
        watcher.unwatch_dir("/nonexistent/path/12345");
        drop(watcher);
    }

    // ── handle_event unit tests ─────────────────────────────────────────

    /// Helper: build a notify::Event with given kind and paths.
    fn make_event(kind: EventKind, paths: Vec<std::path::PathBuf>) -> notify::Event {
        notify::Event {
            kind,
            paths,
            attrs: Default::default(),
        }
    }

    #[test]
    fn handle_event_remove_with_win_prefix_mismatch() {
        // Simulate Windows: DB stores "C:\Users\test\photo.jpg" (clean_path),
        // but notify sends "\\?\C:\Users\test\photo.jpg" (prefixed).
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let clean = r"C:\Users\test\photo.jpg";
        let dir = r"C:\Users\test";
        db.dir_track(dir, false);
        db.file_insert(clean, dir, "photo.jpg", Some(100), None);
        assert!(db.file_lookup(clean).is_some());

        let (tx, rx) = mpsc::channel();

        // Notify gives the \\?\ prefixed path
        let prefixed = format!(r"\\?\{}", clean);
        let event = make_event(
            EventKind::Remove(notify::event::RemoveKind::File),
            vec![std::path::PathBuf::from(&prefixed)],
        );
        handle_event(&db, &tx, event);

        // File should be removed from DB
        assert!(
            db.file_lookup(clean).is_none(),
            "file should be removed from DB despite \\\\?\\ prefix mismatch"
        );
        // Should have sent a Removed event
        let ev = rx.try_recv().expect("should receive FsEvent::Removed");
        match ev {
            FsEvent::Removed(d) => assert_eq!(d, dir),
            _ => panic!("expected Removed, got {:?}", ev),
        }
    }

    #[test]
    fn handle_event_remove_without_prefix() {
        // Remove where path matches DB exactly (no prefix mismatch)
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();
        let file_path = dir.path().join("photo.jpg");
        let path_str = file_path.to_string_lossy().to_string();

        db.dir_track(&dir_str, false);
        db.file_insert(&path_str, &dir_str, "photo.jpg", Some(100), None);

        let (tx, rx) = mpsc::channel();
        let event = make_event(
            EventKind::Remove(notify::event::RemoveKind::File),
            vec![file_path.clone()],
        );
        handle_event(&db, &tx, event);

        assert!(
            db.file_lookup(&path_str).is_none(),
            "file should be removed from DB"
        );
        let ev = rx.try_recv().expect("should receive FsEvent::Removed");
        match ev {
            FsEvent::Removed(_) => {}
            _ => panic!("expected Removed"),
        }
    }

    #[test]
    fn handle_event_remove_nonexistent_file_no_event() {
        // Remove for a file not in DB should not send an event
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let (tx, rx) = mpsc::channel();
        let event = make_event(
            EventKind::Remove(notify::event::RemoveKind::File),
            vec![std::path::PathBuf::from("/nonexistent/photo.jpg")],
        );
        handle_event(&db, &tx, event);

        assert!(
            rx.try_recv().is_err(),
            "should not send event for unknown file"
        );
    }

    #[test]
    fn handle_event_remove_non_media_ignored() {
        // Remove for a non-media file should be ignored
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let (tx, rx) = mpsc::channel();
        let event = make_event(
            EventKind::Remove(notify::event::RemoveKind::File),
            vec![std::path::PathBuf::from("/some/dir/readme.txt")],
        );
        handle_event(&db, &tx, event);

        assert!(
            rx.try_recv().is_err(),
            "should not send event for non-media remove"
        );
    }

    #[test]
    fn handle_event_create_adds_to_db() {
        // Create event for a real media file should add it to DB
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().to_string();
        db.dir_track(&dir_str, false);

        let file_path = dir.path().join("new.jpg");
        std::fs::write(&file_path, b"fake image data").unwrap();

        let (tx, rx) = mpsc::channel();
        let event = make_event(
            EventKind::Create(notify::event::CreateKind::File),
            vec![file_path.clone()],
        );
        handle_event(&db, &tx, event);

        // File should now be in DB
        let canonical = std::fs::canonicalize(&file_path).unwrap();
        let canonical_str = canonical.to_string_lossy().to_string();
        assert!(
            db.file_lookup(&canonical_str).is_some(),
            "created file should be in DB"
        );
        let ev = rx.try_recv().expect("should receive FsEvent::Changed");
        match ev {
            FsEvent::Changed(_) => {}
            _ => panic!("expected Changed"),
        }
    }

    #[test]
    fn handle_event_create_non_media_ignored() {
        // Create event for non-media file should be ignored
        use crate::db::Db;

        let db = Db::open_memory();
        db.ensure_schema();

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("readme.txt");
        std::fs::write(&file_path, b"hello").unwrap();

        let (tx, rx) = mpsc::channel();
        let event = make_event(
            EventKind::Create(notify::event::CreateKind::File),
            vec![file_path],
        );
        handle_event(&db, &tx, event);

        assert!(
            rx.try_recv().is_err(),
            "should not send event for non-media create"
        );
    }
}
