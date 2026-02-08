//! CLI subcommand implementations.

use std::path::Path;

use crate::db::Db;
use crate::scanner;

pub fn add(db: &Db, path: &Path) {
    let abs = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lv add: {}: {}", path.display(), e);
            return;
        }
    };

    db.watched_add(&abs.to_string_lossy());

    println!("Scanning {}...", abs.display());
    let count = scanner::discover(db, &abs);
    println!("Added {} ({} media files)", abs.display(), count);
}

pub fn scan(db: &Db, path: Option<&Path>) {
    let dirs: Vec<String> = if let Some(p) = path {
        vec![p
            .canonicalize()
            .unwrap_or(p.to_path_buf())
            .to_string_lossy()
            .into()]
    } else {
        db.watched_list()
    };

    if dirs.is_empty() {
        println!("No watched directories. Use `lv add PATH` first.");
        return;
    }

    let mut total = 0usize;
    for dir in &dirs {
        println!("Scanning {}...", dir);
        let count = scanner::discover(db, Path::new(dir));
        println!("  {} new/changed", count);
        total += count;
    }
    println!("Done. {} new/changed files.", total);
}

pub fn status(db: &Db) {
    let stats = db.collection_stats();
    let watched = db.watched_list();

    println!("lv status");
    println!("=========");
    println!("files:   {} ({} dirs)", stats.total_files, stats.total_dirs);
    println!(
        "hashed:  {}/{}",
        stats.hashed, stats.total_files
    );
    println!(
        "exif:    {}/{}",
        stats.with_exif, stats.total_files
    );
    println!("failed:  {}", stats.failed);
    println!("watched: {}", watched.len());
    for p in &watched {
        println!("  {}", p);
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
