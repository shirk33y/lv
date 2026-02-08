//! CLI subcommand implementations.

use std::path::Path;

use crate::db::Db;
use crate::scanner;

pub fn track(db: &Db, path: &Path) {
    let abs = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lv track: {}: {}", path.display(), e);
            return;
        }
    };
    db.dir_track(&abs.to_string_lossy(), true);
    println!("Scanning {}...", abs.display());
    let count = scanner::discover(db, &abs);
    println!("Tracked {} ({} media files)", abs.display(), count);
}

pub fn untrack(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    db.dir_untrack(&abs.to_string_lossy());
    println!("Untracked {}", abs.display());
}

pub fn watch(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    db.dir_watch(&abs.to_string_lossy());
    println!("Watching {}", abs.display());
}

pub fn unwatch(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    db.dir_unwatch(&abs.to_string_lossy());
    println!("Unwatched {}", abs.display());
}

pub fn scan(db: &Db, path: Option<&Path>) {
    let dirs: Vec<(String, bool)> = if let Some(p) = path {
        vec![(
            p.canonicalize()
                .unwrap_or_else(|_| p.to_path_buf())
                .to_string_lossy()
                .into(),
            true,
        )]
    } else {
        db.tracked_list()
            .into_iter()
            .map(|(p, _recursive, _watched)| (p, true))
            .collect()
    };

    if dirs.is_empty() {
        println!("No tracked directories. Use `lv track PATH` first.");
        return;
    }

    let mut total = 0usize;
    for (dir, _recursive) in &dirs {
        println!("Scanning {}...", dir);
        let count = scanner::discover(db, Path::new(dir));
        println!("  {} new/changed", count);
        total += count;
    }
    println!("Done. {} new/changed files.", total);
}

pub fn status(db: &Db) {
    let stats = db.collection_stats();
    let tracked = db.tracked_list();

    println!("lv status");
    println!("=========");
    println!("files:   {} ({} dirs)", stats.total_files, stats.total_dirs);
    println!("hashed:  {}/{}", stats.hashed, stats.total_files);
    println!("exif:    {}/{}", stats.with_exif, stats.total_files);
    println!("failed:  {}", stats.failed);
    println!("tracked: {}", tracked.len());
    for (p, recursive, watched) in &tracked {
        let flags = match (*recursive, *watched) {
            (true, true) => " [recursive, watched]",
            (true, false) => " [recursive]",
            (false, true) => " [watched]",
            (false, false) => "",
        };
        println!("  {}{}", p, flags);
    }
}

pub fn worker(db: &Db) {
    use std::sync::atomic::Ordering;

    println!("Running jobs (turbo mode)...");
    let mut engine = crate::jobs::JobEngine::start(db.clone());
    engine.stats.turbo.store(true, Ordering::Relaxed);

    // Poll until no more work
    loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        let done = engine.stats.done.load(Ordering::Relaxed);
        let failed = engine.stats.failed.load(Ordering::Relaxed);
        let active = engine.stats.active.load(Ordering::Relaxed);

        if active == 0 {
            // Double-check after a short pause
            std::thread::sleep(std::time::Duration::from_millis(500));
            let active2 = engine.stats.active.load(Ordering::Relaxed);
            if active2 == 0 {
                engine.stop();
                println!("Done. {} ok, {} failed.", done, failed);
                return;
            }
        }

        eprint!("\r  {} ok, {} failed, {} active...", done, failed, active);
    }
}
