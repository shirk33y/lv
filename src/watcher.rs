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

/// Events sent from the watcher thread to the main loop.
#[derive(Debug)]
pub enum FsEvent {
    /// A file was created or modified — the dir it belongs to may need refreshing.
    Changed(String),
    /// A file was removed.
    Removed(String),
}

/// Handle to the running watcher. Drop to stop.
pub struct FsWatcher {
    quit: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FsWatcher {
    /// Start the filesystem watcher. Returns the handle and a receiver for events.
    pub fn start(db: Db) -> (Self, mpsc::Receiver<FsEvent>) {
        let (tx, rx) = mpsc::channel();
        let quit = Arc::new(AtomicBool::new(false));
        let quit2 = quit.clone();

        let thread = std::thread::Builder::new()
            .name("fs-watcher".into())
            .spawn(move || {
                run_watcher(db, tx, quit2);
            })
            .expect("failed to spawn fs-watcher thread");

        (
            FsWatcher {
                quit,
                thread: Some(thread),
            },
            rx,
        )
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

fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEDIA_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn run_watcher(db: Db, tx: mpsc::Sender<FsEvent>, quit: Arc<AtomicBool>) {
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
    if watched.is_empty() {
        eprintln!("watcher: no watched directories, exiting");
        return;
    }

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

    // Process events until quit
    while !quit.load(Ordering::Relaxed) {
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

fn handle_event(db: &Db, tx: &mpsc::Sender<FsEvent>, event: notify::Event) {
    for path in &event.paths {
        // Skip non-media files
        if path.is_file() && !is_media(path) {
            continue;
        }
        // Skip directories themselves (we only care about files)
        if path.is_dir() {
            continue;
        }

        let path_str = path.to_string_lossy().to_string();

        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                // Insert or update the file in DB
                let abs = match path.canonicalize() {
                    Ok(p) => p,
                    Err(_) => path.clone(),
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

                if !is_media(&abs) {
                    continue;
                }

                let abs_str = abs.to_string_lossy().to_string();
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
                // For removed files, path may not exist anymore so we can't canonicalize.
                // Try to match by the raw path string.
                if db.file_lookup(&path_str).is_some() {
                    db.remove_file_by_path(&path_str);
                    let dir = path
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_string_lossy()
                        .to_string();
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
    fn is_media_recognizes_images() {
        assert!(is_media(Path::new("/a/photo.jpg")));
        assert!(is_media(Path::new("/a/photo.PNG")));
        assert!(is_media(Path::new("/a/photo.webp")));
    }

    #[test]
    fn is_media_recognizes_videos() {
        assert!(is_media(Path::new("/a/clip.mp4")));
        assert!(is_media(Path::new("/a/clip.MKV")));
    }

    #[test]
    fn is_media_rejects_non_media() {
        assert!(!is_media(Path::new("/a/readme.txt")));
        assert!(!is_media(Path::new("/a/script.rs")));
        assert!(!is_media(Path::new("/a/.gitignore")));
    }

    #[test]
    fn is_media_no_extension() {
        assert!(!is_media(Path::new("/a/noext")));
        assert!(!is_media(Path::new("/a/")));
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
}
