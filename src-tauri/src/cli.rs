use std::path::Path;
use std::time::Instant;

use crate::data::Db;
use crate::scanner;
use crate::worker;

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

/// Error patterns appearing more than this many times are considered systematic
/// and will NOT be retried automatically.
const SYSTEMATIC_THRESHOLD: i64 = 10;

pub fn doctor(db: &Db) {
    let t0 = Instant::now();
    let status = db.status();

    // ── 1. Library overview ──────────────────────────────────────────────
    println!("lv doctor");
    println!("=========");
    println!();
    println!("Library");
    println!("  files:   {}", status.files);
    println!("  hashed:  {}/{}", status.hashed, status.files);
    println!("  thumbs:  {}/{}", status.thumbs, status.files);
    println!("  dirs:    {}", status.dirs);
    println!("  watched: {}", status.watched);
    for p in &status.watched_paths {
        println!("    {}", p);
    }

    // ── 2. Job breakdown ─────────────────────────────────────────────────
    println!();
    println!("Jobs");
    let breakdown = db.jobs_by_type_status();
    if breakdown.is_empty() {
        println!("  (none)");
    } else {
        for (jtype, jstatus, count) in &breakdown {
            println!("  {:<12} {:<10} {}", jtype, jstatus, count);
        }
    }

    // ── 3. Orphan detection ──────────────────────────────────────────────
    let orphan_hash = db.jobs_orphan_hash_count();
    let orphan_thumb = db.jobs_orphan_thumb_count();
    if orphan_hash > 0 || orphan_thumb > 0 {
        println!();
        println!("Orphans (missing jobs)");
        if orphan_hash > 0 {
            println!("  {} files without hash and no pending job", orphan_hash);
        }
        if orphan_thumb > 0 {
            println!("  {} meta without thumb and no pending job", orphan_thumb);
        }
    }

    // ── 4. Error analysis ────────────────────────────────────────────────
    let errors = db.jobs_top_errors(20);
    if !errors.is_empty() {
        println!();
        println!("Top errors");
        for (jtype, error, count) in &errors {
            let tag = if *count > SYSTEMATIC_THRESHOLD {
                " [systematic — skipped]"
            } else {
                ""
            };
            let truncated = if error.len() > 80 {
                format!("{}…", &error[..80])
            } else {
                error.clone()
            };
            println!("  {:<12} ×{:<5} {}{}", jtype, count, truncated, tag);
        }
    }

    // ── 5. Actions ───────────────────────────────────────────────────────
    println!();
    println!("Actions");

    // 5a. Recover stale running → pending
    db.jobs_recover_stale();

    // 5b. Retry failed (skip systematic)
    let (retried, skipped) = db.jobs_retry_failed(SYSTEMATIC_THRESHOLD);
    if retried > 0 || skipped > 0 {
        println!("  retried:  {} failed jobs reset to pending", retried);
        if skipped > 0 {
            println!("  skipped:  {} systematic failures (same error >{}×)",
                     skipped, SYSTEMATIC_THRESHOLD);
        }
    }

    // 5c. Enqueue orphans
    let enqueued_hash = db.jobs_enqueue_missing_hashes();
    let enqueued_thumb = db.jobs_enqueue_missing_thumbs();
    if enqueued_hash > 0 {
        println!("  enqueued: {} hash jobs for unhashed files", enqueued_hash);
    }
    if enqueued_thumb > 0 {
        println!("  enqueued: {} thumb jobs for missing thumbnails", enqueued_thumb);
    }

    // 5d. Clean done jobs
    let cleaned = db.jobs_clean_done();
    if cleaned > 0 {
        println!("  cleaned:  {} completed job records", cleaned);
    }

    let total_pending = retried + enqueued_hash + enqueued_thumb;
    if total_pending == 0 && skipped == 0 {
        println!("  nothing to do ✓");
        println!();
        println!("Done in {:.1}s", t0.elapsed().as_secs_f64());
        return;
    }

    // ── 6. Run worker to drain ───────────────────────────────────────────
    if total_pending > 0 {
        println!();
        println!("Running worker (draining {} pending jobs)…", total_pending);
        worker::run_headless(db, true);
    }

    // ── 7. Final report ──────────────────────────────────────────────────
    let final_status = db.status();
    let final_errors = db.jobs_top_errors(5);
    println!();
    println!("Report");
    println!("  files:   {}", final_status.files);
    println!("  hashed:  {}/{}", final_status.hashed, final_status.files);
    println!("  thumbs:  {}/{}", final_status.thumbs, final_status.files);
    println!(
        "  jobs:    {} pending, {} running, {} done, {} failed",
        final_status.jobs_pending,
        final_status.jobs_running,
        final_status.jobs_done,
        final_status.jobs_failed
    );
    if !final_errors.is_empty() {
        println!();
        println!("  Remaining errors:");
        for (jtype, error, count) in &final_errors {
            let truncated = if error.len() > 72 {
                format!("{}…", &error[..72])
            } else {
                error.clone()
            };
            println!("    {:<12} ×{:<5} {}", jtype, count, truncated);
        }
    }
    let coverage_hash = if final_status.files > 0 {
        final_status.hashed as f64 / final_status.files as f64 * 100.0
    } else {
        100.0
    };
    let coverage_thumb = if final_status.files > 0 {
        final_status.thumbs as f64 / final_status.files as f64 * 100.0
    } else {
        100.0
    };
    println!();
    println!(
        "  coverage: hash {:.1}%, thumb {:.1}%",
        coverage_hash, coverage_thumb
    );
    println!("  elapsed:  {:.1}s", t0.elapsed().as_secs_f64());
}
