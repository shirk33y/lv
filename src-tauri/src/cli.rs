use std::path::Path;

use crate::data::Db;
use crate::scanner;

pub fn add(db: &Db, path: &Path) {
    use crate::debug::dbg_log;
    dbg_log!("add: {}", path.display());
    let abs = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lv add: {}: {}", path.display(), e);
            return;
        }
    };

    db.watched_add(&abs.to_string_lossy());

    println!("Added {} to library.", abs.display());
    println!("Run `lv -s` to start scanning.");
}

pub fn scan(db: &Db, path: Option<&Path>, rescan_all: bool) {
    use crate::debug::dbg_log;
    dbg_log!("scan: path={:?} all={}", path, rescan_all);
    if rescan_all {
        println!("Full re-scan...");
    }

    let dirs: Vec<String> = if let Some(p) = path {
        vec![p
            .canonicalize()
            .unwrap_or(p.to_path_buf())
            .to_string_lossy()
            .into()]
    } else {
        db.watched_list_active()
    };

    if dirs.is_empty() {
        println!("No watched directories. Use `lv add PATH` first.");
        return;
    }

    for dir in &dirs {
        println!("Scanning {}...", dir);
        let t = std::time::Instant::now();
        let count = scanner::discover(db, Path::new(dir));
        dbg_log!("scanned in {:?}", t.elapsed());
        println!("  found {} media files", count);
    }
}

pub fn watch(db: &Db, path: &Path) {
    let abs = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lv watch: {}: {}", path.display(), e);
            return;
        }
    };

    db.watched_watch(&abs.to_string_lossy());
    println!("Watching {}", abs.display());
}

pub fn status(db: &Db) {
    let s = db.status();
    println!(
        "files:   {} ({} dirs, {} hashed, {} thumbs)",
        s.files, s.dirs, s.hashed, s.thumbs
    );
    println!("watched: {}", s.watched);
    for p in &s.watched_paths {
        println!("  {}", p);
    }
    println!(
        "jobs:    {} pending, {} running, {} done, {} failed",
        s.jobs_pending, s.jobs_running, s.jobs_done, s.jobs_failed
    );
}

pub fn unwatch(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or(path.to_path_buf());
    db.watched_unwatch(&abs.to_string_lossy());
    println!("Unwatched {}", abs.display());
}

pub fn reset_thumbs(db: &Db) {
    let count = db.reset_thumbs();
    println!("Reset thumbnails. Re-enqueued {} jobs.", count);
    println!("Run `lv worker --once` to regenerate.");
}
