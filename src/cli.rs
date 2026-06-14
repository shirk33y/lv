//! CLI subcommand implementations.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::clean_path;
use crate::db::{row_to_entry, Db, FileEntry};
use crate::scanner;
use rusqlite::types::Value;

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

fn glob_to_like(pattern: &str) -> String {
    let mut like = String::with_capacity(pattern.len() + 4);
    let has_glob = pattern.contains('*') || pattern.contains('?');
    if !has_glob {
        like.push('%');
        for ch in pattern.chars() {
            match ch {
                '%' | '_' => {
                    like.push('/');
                    like.push(ch);
                }
                c => like.push(c),
            }
        }
        like.push('%');
    } else {
        for ch in pattern.chars() {
            match ch {
                '*' => like.push('%'),
                '?' => like.push('_'),
                '%' | '_' => {
                    like.push('/');
                    like.push(ch);
                }
                c => like.push(c),
            }
        }
    }
    like
}

fn parse_size_spec(spec: &str) -> Option<(&'static str, i64)> {
    let spec = spec.trim();
    let (op, rest) = if let Some(r) = spec.strip_prefix('+') {
        (">", r)
    } else if let Some(r) = spec.strip_prefix('-') {
        ("<", r)
    } else if let Some(r) = spec.strip_prefix('=') {
        ("=", r)
    } else {
        ("=", spec)
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let (val_str, unit) = if rest.ends_with(|c: char| c.is_alphabetic()) {
        let (num, unit) = rest.split_at(rest.len() - 1);
        (num, unit)
    } else {
        (rest, "")
    };
    let base: i64 = val_str.parse().ok()?;
    let multiplier: i64 = match unit {
        "K" | "k" => 1024,
        "M" | "m" => 1024 * 1024,
        "G" | "g" => 1024 * 1024 * 1024,
        "B" | "b" => 1,
        "" => 1,
        _ => return None,
    };
    Some((op, base * multiplier))
}

fn parse_duration_spec(spec: &str) -> Option<(&'static str, i64)> {
    let spec = spec.trim();
    let (op, rest) = if let Some(r) = spec.strip_prefix('+') {
        (">", r)
    } else if let Some(r) = spec.strip_prefix('-') {
        ("<", r)
    } else if let Some(r) = spec.strip_prefix('=') {
        ("=", r)
    } else {
        ("=", spec)
    };
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let (val_str, unit) = if rest.ends_with(|c: char| c.is_alphabetic()) {
        let (num, unit) = rest.split_at(rest.len() - 1);
        (num, unit)
    } else {
        (rest, "")
    };
    let base: i64 = val_str.parse().ok()?;
    let multiplier: i64 = match unit {
        "s" => 1000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "" => 1000,
        _ => return None,
    };
    Some((op, base * multiplier))
}

fn add_resolution(spec: &str, conditions: &mut Vec<String>, params: &mut Vec<Value>) {
    let val: i64;
    let (left, op): (&str, &str) = match spec {
        "thumb" => {
            val = 640;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "vga" => {
            val = 800;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "sd" => {
            val = 1280;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "hd" => {
            val = 1920;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "4k" => {
            val = 3840;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "8k" => {
            val = 7680;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", "<=")
        }
        "photo" => {
            let n = params.len() + 1;
            conditions.push(format!(
                "COALESCE(m.width,0) >= ?{n} AND COALESCE(m.height,0) >= ?{n}"
            ));
            params.push(Value::Integer(2160));
            return;
        }
        raw => {
            let (o, n) = if let Some(r) = raw.strip_prefix('+') {
                (">=", r)
            } else if let Some(r) = raw.strip_prefix('-') {
                ("<", r)
            } else if let Some(r) = raw.strip_prefix('=') {
                ("=", r)
            } else {
                ("=", raw)
            };
            let v: i64 = match n.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("Invalid resolution spec: {raw}");
                    return;
                }
            };
            val = v;
            ("MAX(COALESCE(m.width,0),COALESCE(m.height,0))", o)
        }
    };
    let n = params.len() + 1;
    conditions.push(format!("{left} {op} ?{n}"));
    params.push(Value::Integer(val));
}

#[allow(clippy::too_many_arguments)]
pub fn find_files(
    db: &Db,
    pattern: Option<String>,
    size: Option<String>,
    duration: Option<String>,
    resolution: Option<String>,
    tags: &[String],
    sort: Option<String>,
    count: bool,
    print0: bool,
) {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    // Pattern (glob→SQL LIKE)
    if let Some(p) = pattern {
        let like = glob_to_like(&p);
        let n = params.len() + 1;
        conditions.push(format!("f.filename LIKE ?{n} ESCAPE '/'"));
        params.push(Value::Text(like));
    }

    // Size
    if let Some(ref s) = size {
        if let Some((op, val)) = parse_size_spec(s) {
            let n = params.len() + 1;
            conditions.push(format!("f.size {op} ?{n}"));
            params.push(Value::Integer(val));
        }
    }

    // Duration
    if let Some(ref d) = duration {
        if let Some((op, val)) = parse_duration_spec(d) {
            let n = params.len() + 1;
            conditions.push(format!("m.duration_ms {op} ?{n}"));
            params.push(Value::Integer(val));
        }
    }

    // Resolution
    if let Some(ref r) = resolution {
        add_resolution(r, &mut conditions, &mut params);
    }

    // Tags
    for tag in tags {
        let n = params.len() + 1;
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?{n})"
        ));
        params.push(Value::Text(tag.clone()));
    }

    // Build SQL
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let order = match sort.as_deref() {
        Some("name") => "ORDER BY f.path COLLATE NOCASE",
        Some("size") => "ORDER BY f.size",
        Some("duration") => "ORDER BY m.duration_ms",
        Some("resolution") => "ORDER BY COALESCE(m.width,0) DESC, COALESCE(m.height,0) DESC",
        Some("random") => "ORDER BY RANDOM()",
        _ => "ORDER BY f.path COLLATE NOCASE",
    };

    if count {
        // Fast path: COUNT(*) only
        let sql = format!(
            "SELECT COUNT(*) FROM files f LEFT JOIN meta m ON f.meta_id = m.id {where_clause}"
        );
        let conn = db.conn();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Query error: {e}");
                return;
            }
        };
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        match stmt.query_row(param_refs.as_slice(), |r| r.get::<_, i64>(0)) {
            Ok(n) => println!("{n}"),
            Err(e) => eprintln!("Query error: {e}"),
        }
        return;
    }

    let sql = format!(
        "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, \
         EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = 'like'), f.temporary \
         FROM files f LEFT JOIN meta m ON f.meta_id = m.id {where_clause} {order}"
    );

    let conn = db.conn();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Query error: {e}");
            return;
        }
    };

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
        .iter()
        .map(|p| p as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = match stmt.query_map(param_refs.as_slice(), row_to_entry) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Query error: {e}");
            return;
        }
    };

    let results: Vec<FileEntry> = rows.filter_map(|r| r.ok()).collect();

    if print0 {
        for f in &results {
            print!("{}\0", f.path);
        }
        return;
    }

    println!("{}", results.len());
    for f in &results {
        println!("{}", f.path);
    }
}

static DAEMON_RUNNING: AtomicBool = AtomicBool::new(true);

extern "C" fn daemon_signal(_: i32) {
    DAEMON_RUNNING.store(false, Ordering::Release);
}

pub fn daemon(db: &Db) {
    eprintln!("daemon: starting...");

    DAEMON_RUNNING.store(true, Ordering::Release);
    unsafe {
        libc::signal(
            libc::SIGINT,
            daemon_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            daemon_signal as *const () as libc::sighandler_t,
        );
    }

    let (fs_watcher, _fs_rx) = crate::watcher::FsWatcher::start(db.clone());
    let mut engine = crate::jobs::JobEngine::start(db.clone());
    engine.stats.turbo.store(true, Ordering::Relaxed);

    let mut last_data_ver = db.data_version() - 1; // force initial refresh
    let mut watched: HashSet<String> = db.watched_dirs().into_iter().map(|(p, _)| p).collect();

    eprintln!("daemon: running (SIGINT/SIGTERM to stop)");

    while DAEMON_RUNNING.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(500));

        let dv = db.data_version();
        if dv == last_data_ver {
            continue;
        }
        last_data_ver = dv;

        // Process pending commands
        while let Some((id, action, payload)) = db.claim_next_command() {
            match action.as_str() {
                "scan" => {
                    if let Some(ref p) = payload {
                        eprintln!("daemon: scanning {}...", p);
                        scanner::discover(db, Path::new(p));
                    } else {
                        let dirs = db.tracked_list();
                        for (dir, ..) in &dirs {
                            eprintln!("daemon: scanning {}...", dir);
                            scanner::discover(db, Path::new(dir));
                        }
                    }
                    eprintln!("daemon: scan done");
                }
                "shutdown" => {
                    eprintln!("daemon: shutdown requested via commands");
                    DAEMON_RUNNING.store(false, Ordering::Release);
                }
                other => eprintln!("daemon: unknown command: {other}"),
            }
            db.delete_command(id);
            if !DAEMON_RUNNING.load(Ordering::Acquire) {
                break;
            }
        }

        // Sync filesystem watches with DB state
        let new_watched: HashSet<String> = db.watched_dirs().into_iter().map(|(p, _)| p).collect();

        for path in new_watched.difference(&watched) {
            fs_watcher.watch_dir(path);
        }
        for path in watched.difference(&new_watched) {
            fs_watcher.unwatch_dir(path);
        }
        watched = new_watched;
    }

    engine.stop();
    eprintln!("daemon: stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::types::Value;

    // ── glob_to_like ─────────────────────────────────────────────────────

    #[test]
    fn glob_to_like_substring_no_glob_chars() {
        assert_eq!(glob_to_like("hello"), "%hello%");
    }

    #[test]
    fn glob_to_like_asterisk_to_percent() {
        assert_eq!(glob_to_like("*.jpg"), "%.jpg");
    }

    #[test]
    fn glob_to_like_question_to_underscore() {
        assert_eq!(glob_to_like("photo_?.png"), "photo/__.png");
    }

    #[test]
    fn glob_to_like_literal_percent_escaped() {
        assert_eq!(glob_to_like("100%"), "%100/%%");
    }

    #[test]
    fn glob_to_like_escape_special_in_substring_mode() {
        assert_eq!(glob_to_like("foo_bar"), "%foo/_bar%");
    }

    #[test]
    fn glob_to_like_empty_string() {
        assert_eq!(glob_to_like(""), "%%");
    }

    #[test]
    fn glob_to_like_only_glob() {
        assert_eq!(glob_to_like("*"), "%");
    }

    #[test]
    fn glob_to_like_unicode() {
        assert_eq!(glob_to_like("café.jpg"), "%café.jpg%");
    }

    #[test]
    fn glob_to_like_mixed_glob_and_literal() {
        assert_eq!(glob_to_like("img_*_?.txt"), "img/_%/__.txt");
    }

    // ── parse_size_spec ──────────────────────────────────────────────────

    #[test]
    fn size_plus_10m() {
        assert_eq!(parse_size_spec("+10M"), Some((">", 10 * 1024 * 1024)));
    }

    #[test]
    fn size_minus_500k() {
        assert_eq!(parse_size_spec("-500K"), Some(("<", 500 * 1024)));
    }

    #[test]
    fn size_eq_1g() {
        assert_eq!(parse_size_spec("=1G"), Some(("=", 1024 * 1024 * 1024)));
    }

    #[test]
    fn size_bare_number() {
        assert_eq!(parse_size_spec("42"), Some(("=", 42)));
    }

    #[test]
    fn size_plus_0() {
        assert_eq!(parse_size_spec("+0"), Some((">", 0)));
    }

    #[test]
    fn size_with_b_suffix() {
        assert_eq!(parse_size_spec("+500B"), Some((">", 500)));
    }

    #[test]
    fn size_lowercase_suffix() {
        assert_eq!(parse_size_spec("+10m"), Some((">", 10 * 1024 * 1024)));
    }

    #[test]
    fn size_whitespace() {
        assert_eq!(parse_size_spec("  +10M  "), Some((">", 10 * 1024 * 1024)));
    }

    #[test]
    fn size_empty() {
        assert_eq!(parse_size_spec(""), None);
    }

    #[test]
    fn size_just_operator() {
        assert_eq!(parse_size_spec("+"), None);
        assert_eq!(parse_size_spec("-"), None);
    }

    #[test]
    fn size_invalid_unit() {
        assert_eq!(parse_size_spec("+10X"), None);
    }

    #[test]
    fn size_invalid_number() {
        assert_eq!(parse_size_spec("+abcM"), None);
    }

    #[test]
    fn size_multiple_operators() {
        // ++10M: first + stripped → "+10M" which parses as > 10M
        assert_eq!(parse_size_spec("++10M"), Some((">", 10 * 1024 * 1024)));
    }

    // ── parse_duration_spec ──────────────────────────────────────────────

    #[test]
    fn duration_plus_30s() {
        assert_eq!(parse_duration_spec("+30s"), Some((">", 30_000)));
    }

    #[test]
    fn duration_minus_5m() {
        assert_eq!(parse_duration_spec("-5m"), Some(("<", 5 * 60_000)));
    }

    #[test]
    fn duration_eq_1h() {
        assert_eq!(parse_duration_spec("=1h"), Some(("=", 3_600_000)));
    }

    #[test]
    fn duration_bare_days() {
        assert_eq!(parse_duration_spec("7d"), Some(("=", 7 * 86_400_000)));
    }

    #[test]
    fn duration_bare_milliseconds() {
        // bare numbers treated as seconds (multiplier 1000)
        assert_eq!(parse_duration_spec("500"), Some(("=", 500_000)));
    }

    #[test]
    fn duration_empty() {
        assert_eq!(parse_duration_spec(""), None);
    }

    #[test]
    fn duration_just_operator() {
        assert_eq!(parse_duration_spec("+"), None);
    }

    #[test]
    fn duration_invalid_unit() {
        assert_eq!(parse_duration_spec("+30x"), None);
    }

    #[test]
    fn duration_invalid_number() {
        assert_eq!(parse_duration_spec("+abc"), None);
    }

    #[test]
    fn duration_whitespace() {
        assert_eq!(parse_duration_spec("  +30s  "), Some((">", 30_000)));
    }

    // ── add_resolution ───────────────────────────────────────────────────

    fn check_resolution(spec: &str, expected_cond: &str, expected_val: i64) {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        add_resolution(spec, &mut conditions, &mut params);
        assert_eq!(conditions.len(), 1);
        assert_eq!(params.len(), 1);
        assert_eq!(conditions[0], expected_cond);
        assert_eq!(params[0], Value::Integer(expected_val));
    }

    #[test]
    fn res_thumb() {
        check_resolution(
            "thumb",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            640,
        );
    }

    #[test]
    fn res_vga() {
        check_resolution(
            "vga",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            800,
        );
    }

    #[test]
    fn res_sd() {
        check_resolution(
            "sd",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            1280,
        );
    }

    #[test]
    fn res_hd() {
        check_resolution(
            "hd",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            1920,
        );
    }

    #[test]
    fn res_4k() {
        check_resolution(
            "4k",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            3840,
        );
    }

    #[test]
    fn res_8k() {
        check_resolution(
            "8k",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) <= ?1",
            7680,
        );
    }

    #[test]
    fn res_photo() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        add_resolution("photo", &mut conditions, &mut params);
        assert_eq!(conditions.len(), 1);
        // ?1 is reused for both width/height, so only 1 param needed
        assert_eq!(params.len(), 1);
        assert_eq!(
            conditions[0],
            "COALESCE(m.width,0) >= ?1 AND COALESCE(m.height,0) >= ?1"
        );
        assert_eq!(params[0], Value::Integer(2160));
    }

    #[test]
    fn res_raw_plus() {
        check_resolution(
            "+1920",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) >= ?1",
            1920,
        );
    }

    #[test]
    fn res_raw_minus() {
        check_resolution(
            "-1080",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) < ?1",
            1080,
        );
    }

    #[test]
    fn res_raw_eq() {
        check_resolution(
            "=4000",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) = ?1",
            4000,
        );
    }

    #[test]
    fn res_raw_bare() {
        check_resolution(
            "3840",
            "MAX(COALESCE(m.width,0),COALESCE(m.height,0)) = ?1",
            3840,
        );
    }

    #[test]
    fn res_invalid_raw() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        add_resolution("+abc", &mut conditions, &mut params);
        assert!(conditions.is_empty());
        assert!(params.is_empty());
    }

    #[test]
    fn res_multiple_accumulates() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        add_resolution("hd", &mut conditions, &mut params);
        add_resolution("+2000", &mut conditions, &mut params);
        assert_eq!(conditions.len(), 2);
        assert_eq!(params.len(), 2);
        assert!(conditions[0].contains("<= ?1"));
        assert!(conditions[1].contains(">= ?2"));
        assert_eq!(params[0], Value::Integer(1920));
        assert_eq!(params[1], Value::Integer(2000));
    }

    // ── SQL query building (find_files internals) ────────────────────────

    #[test]
    fn query_build_pattern_no_glob() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let like = glob_to_like("hello");
        let n = params.len() + 1;
        conditions.push(format!("f.filename LIKE ?{n} ESCAPE '/'"));
        params.push(Value::Text(like));
        assert_eq!(conditions[0], "f.filename LIKE ?1 ESCAPE '/'");
        assert_eq!(params[0], Value::Text("%hello%".to_string()));
    }

    #[test]
    fn query_build_pattern_with_glob() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let like = glob_to_like("*.jpg");
        let n = params.len() + 1;
        conditions.push(format!("f.filename LIKE ?{n} ESCAPE '/'"));
        params.push(Value::Text(like));
        assert!(conditions[0].starts_with("f.filename LIKE ?"));
        assert_eq!(params[0], Value::Text("%.jpg".to_string()));
    }

    #[test]
    fn query_build_size() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let (op, val) = parse_size_spec("+10M").unwrap();
        let n = params.len() + 1;
        conditions.push(format!("f.size {op} ?{n}"));
        params.push(Value::Integer(val));
        assert_eq!(conditions[0], "f.size > ?1");
        assert_eq!(params[0], Value::Integer(10 * 1024 * 1024));
    }

    #[test]
    fn query_build_duration() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let (op, val) = parse_duration_spec("-5m").unwrap();
        let n = params.len() + 1;
        conditions.push(format!("m.duration_ms {op} ?{n}"));
        params.push(Value::Integer(val));
        assert_eq!(conditions[0], "m.duration_ms < ?1");
        assert_eq!(params[0], Value::Integer(300_000));
    }

    #[test]
    fn query_build_tag() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        let tag = "like".to_string();
        let n = params.len() + 1;
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?{n})"
        ));
        params.push(Value::Text(tag));
        assert_eq!(
            conditions[0],
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?1)"
        );
        assert_eq!(params[0], Value::Text("like".to_string()));
    }

    #[test]
    fn query_build_multiple_tags() {
        let tags = ["like", "c2"];
        let mut conditions = Vec::new();
        let mut params = Vec::new();
        for tag in &tags {
            let n = params.len() + 1;
            conditions.push(format!(
                "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?{n})"
            ));
            params.push(Value::Text(tag.to_string()));
        }
        assert_eq!(conditions.len(), 2);
        assert_eq!(
            conditions[0],
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?1)"
        );
        assert_eq!(
            conditions[1],
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?2)"
        );
        assert_eq!(params[0], Value::Text("like".to_string()));
        assert_eq!(params[1], Value::Text("c2".to_string()));
    }

    #[test]
    fn query_build_combined_filters() {
        let mut conditions = Vec::new();
        let mut params = Vec::new();

        // pattern
        let like = glob_to_like("*.png");
        let n = params.len() + 1;
        conditions.push(format!("f.filename LIKE ?{n} ESCAPE '/'"));
        params.push(Value::Text(like));

        // size
        let (op, val) = parse_size_spec("+1M").unwrap();
        let n = params.len() + 1;
        conditions.push(format!("f.size {op} ?{n}"));
        params.push(Value::Integer(val));

        // tag
        let n = params.len() + 1;
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?{n})"
        ));
        params.push(Value::Text("like".to_string()));

        assert_eq!(conditions.len(), 3);
        assert_eq!(params.len(), 3);
        let where_clause = conditions.join(" AND ");
        assert!(where_clause.contains("f.filename LIKE ?1"));
        assert!(where_clause.contains("f.size > ?2"));
        assert!(where_clause.contains("meta_tags mt WHERE mt.meta_id = m.id AND mt.tag = ?3"));
        assert_eq!(params[0], Value::Text("%.png".to_string()));
        assert_eq!(params[1], Value::Integer(1024 * 1024));
        assert_eq!(params[2], Value::Text("like".to_string()));
    }

    #[test]
    fn query_build_no_filters() {
        let conditions: Vec<String> = Vec::new();
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        assert_eq!(where_clause, "");
    }

    #[test]
    fn sort_keys() {
        assert_eq!(
            match Some("name") {
                Some("name") => "ORDER BY f.path COLLATE NOCASE",
                _ => "",
            },
            "ORDER BY f.path COLLATE NOCASE"
        );
        assert_eq!(
            match Some("size") {
                Some("size") => "ORDER BY f.size",
                _ => "",
            },
            "ORDER BY f.size"
        );
        assert_eq!(
            match Some("duration") {
                Some("duration") => "ORDER BY m.duration_ms",
                _ => "",
            },
            "ORDER BY m.duration_ms"
        );
        assert_eq!(
            match Some("resolution") {
                Some("resolution") =>
                    "ORDER BY COALESCE(m.width,0) DESC, COALESCE(m.height,0) DESC",
                _ => "",
            },
            "ORDER BY COALESCE(m.width,0) DESC, COALESCE(m.height,0) DESC"
        );
        assert_eq!(
            match Some("random") {
                Some("random") => "ORDER BY RANDOM()",
                _ => "",
            },
            "ORDER BY RANDOM()"
        );
    }

    #[test]
    fn sort_default() {
        let sort: Option<&str> = None;
        assert_eq!(
            match sort {
                None => "ORDER BY f.path COLLATE NOCASE",
                _ => "",
            },
            "ORDER BY f.path COLLATE NOCASE"
        );
    }

    // ── daemon / commands ────────────────────────────────────────────────

    #[test]
    fn daemon_signal_sets_flag_false() {
        use std::sync::atomic::Ordering;
        DAEMON_RUNNING.store(true, Ordering::Release);
        daemon_signal(0);
        assert!(!DAEMON_RUNNING.load(Ordering::Acquire));
        // Reset for subsequent tests
        DAEMON_RUNNING.store(true, Ordering::Release);
    }

    #[test]
    fn watch_sync_difference_new_dirs() {
        use std::collections::HashSet;
        let current: HashSet<String> = ["/a".into(), "/b".into()].into();
        let new: HashSet<String> = ["/a".into(), "/b".into(), "/c".into()].into();
        let to_add: Vec<&String> = new.difference(&current).collect();
        let to_remove: Vec<&String> = current.difference(&new).collect();
        assert_eq!(to_add.len(), 1);
        assert_eq!(to_add[0], "/c");
        assert!(to_remove.is_empty());
    }

    #[test]
    fn watch_sync_difference_removed_dirs() {
        use std::collections::HashSet;
        let mut current: HashSet<String> = ["/a".into(), "/b".into(), "/c".into()].into();
        let new: HashSet<String> = ["/a".into(), "/b".into()].into();
        let to_remove: Vec<&String> = current.difference(&new).collect();
        assert_eq!(to_remove.len(), 1);
        assert!(to_remove.contains(&&"/c".to_string()));
    }

    #[test]
    fn watch_sync_difference_both() {
        use std::collections::HashSet;
        let current: HashSet<String> = ["/a".into(), "/b".into()].into();
        let new: HashSet<String> = ["/b".into(), "/c".into()].into();
        let to_add: Vec<&String> = new.difference(&current).collect();
        let to_remove: Vec<&String> = current.difference(&new).collect();
        assert_eq!(to_add.len(), 1);
        assert_eq!(to_add[0], "/c");
        assert_eq!(to_remove.len(), 1);
        assert_eq!(to_remove[0], "/a");
    }

    #[test]
    fn watch_sync_difference_no_change() {
        use std::collections::HashSet;
        let current: HashSet<String> = ["/a".into(), "/b".into()].into();
        let new: HashSet<String> = ["/b".into(), "/a".into()].into();
        let to_add: Vec<&String> = new.difference(&current).collect();
        let to_remove: Vec<&String> = current.difference(&new).collect();
        assert!(to_add.is_empty());
        assert!(to_remove.is_empty());
    }
}
