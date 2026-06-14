//! CLI subcommand implementations.

use std::path::Path;

use crate::clean_path;
use crate::db::Db;
use crate::scanner;

fn resolve_path(path: &Path) -> Option<String> {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Some(clean_path(&abs.to_string_lossy()))
}

pub fn track(db: &Db, path: &Path) {
    let abs_str = match resolve_path(path) {
        Some(s) => s,
        None => {
            eprintln!("lv track: {}: could not resolve", path.display());
            return;
        }
    };
    db.dir_track(&abs_str, true);
    println!("Tracking {}", abs_str);
    println!("Scanning {}...", abs_str);
    let count = scanner::discover(db, Path::new(&abs_str));
    println!("Tracked {} ({} media files)", abs_str, count);
}

pub fn untrack(db: &Db, path: &Path) {
    let abs_str = resolve_path(path);
    let abs_str = match abs_str {
        Some(s) => s,
        None => {
            let s = clean_path(&path.to_string_lossy());
            eprintln!("lv remove: could not resolve path {}", s);
            return;
        }
    };
    db.dir_untrack(&abs_str);
    println!("Untracked {}", abs_str);
}

pub fn watch(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let abs_str = clean_path(&abs.to_string_lossy());
    db.dir_watch(&abs_str);
    println!("Watching {}", abs_str);
}

pub fn unwatch(db: &Db, path: &Path) {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let abs_str = clean_path(&abs.to_string_lossy());
    db.dir_unwatch(&abs_str);
    println!("Unwatched {}", abs_str);
}

pub fn scan(db: &Db, path: Option<&Path>) {
    let dirs: Vec<(String, bool)> = if let Some(p) = path {
        vec![(
            clean_path(
                &p.canonicalize()
                    .unwrap_or_else(|_| p.to_path_buf())
                    .to_string_lossy(),
            ),
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

pub fn get_props(db: &Db, path: &Path, keys: &[String]) {
    let abs_str = clean_path(
        &path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy(),
    );
    let dir_id = match db.dir_id_by_path(&abs_str) {
        Some(id) => id,
        None => {
            println!("Directory not tracked: {}", abs_str);
            return;
        }
    };
    println!("Properties for {}:", abs_str);
    for key in keys {
        let val = db.dir_get_prop(dir_id, key);
        match val {
            Some(v) => println!("  {} = {}", key, v),
            None => println!("  {} = (not set)", key),
        }
    }
}

pub fn set_props(db: &Db, path: &Path, props: &[String]) {
    let abs_str = clean_path(
        &path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy(),
    );
    let dir_id = match db.dir_id_by_path(&abs_str) {
        Some(id) => id,
        None => {
            println!("Directory not tracked: {}", abs_str);
            return;
        }
    };
    for prop in props {
        let (key, val) = match prop.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => {
                println!("Invalid property format: {} (expected KEY=VALUE)", prop);
                return;
            }
        };
        db.dir_set_prop(dir_id, key, val);
        println!("  {} → {}", key, val);
    }
}
