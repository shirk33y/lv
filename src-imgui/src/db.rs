//! Minimal database layer for the imgui POC.
//! Opens the existing lv.db and provides read/write queries.
//! This will be replaced by src-core when extracted from src-tauri.

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

pub struct FileEntry {
    pub id: i64,
    pub path: String,
    pub dir: String,
    pub filename: String,
    #[allow(dead_code)]
    pub meta_id: Option<i64>,
    pub liked: bool,
    #[allow(dead_code)]
    pub temporary: bool,
}

/// Aggregate stats for the info sidebar.
pub struct CollectionStats {
    pub total_files: i64,
    pub total_dirs: i64,
    pub hashed: i64,
    pub with_exif: i64,
    pub failed: i64,
}

/// Extended metadata for the info sidebar.
pub struct FileMeta {
    pub filename: String,
    pub path: String,
    pub dir: String,
    pub size: Option<i64>,
    pub modified_at: Option<String>,
    pub hash_sha512: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub format: Option<String>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i64>,
    pub codecs: Option<String>,
    pub tags: Vec<String>,
    pub pnginfo: Option<String>,
}

impl Db {
    pub fn open_default() -> Self {
        let path = default_db_path();
        eprintln!("db: {}", path.display());
        let conn = Connection::open(&path).expect("failed to open lv.db");
        conn.execute_batch("PRAGMA journal_mode = WAL;").ok();
        conn.execute_batch("PRAGMA foreign_keys = ON;").ok();
        Db(Arc::new(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }

    pub fn ensure_schema(&self) {
        self.conn()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS files (
                    id            INTEGER PRIMARY KEY,
                    path          TEXT NOT NULL UNIQUE,
                    dir           TEXT NOT NULL,
                    filename      TEXT NOT NULL,
                    size          INTEGER,
                    modified_at   TEXT,
                    hash_sha512   TEXT,
                    meta_id       INTEGER REFERENCES meta(id),
                    created_at    TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS meta (
                    id            INTEGER PRIMARY KEY,
                    hash_sha512   TEXT NOT NULL UNIQUE,
                    width         INTEGER,
                    height        INTEGER,
                    format        TEXT,
                    exif_json     TEXT,
                    pnginfo       TEXT,
                    duration_ms   INTEGER,
                    bitrate       INTEGER,
                    codecs        TEXT,
                    tags          TEXT DEFAULT '[]',
                    thumb_ready   INTEGER DEFAULT 0,
                    created_at    TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS history (
                    id            INTEGER PRIMARY KEY,
                    file_id       INTEGER REFERENCES files(id),
                    action        TEXT NOT NULL,
                    created_at    TEXT DEFAULT (datetime('now'))
                );
                CREATE TABLE IF NOT EXISTS directories (
                    id            INTEGER PRIMARY KEY,
                    path          TEXT NOT NULL UNIQUE,
                    tracked       INTEGER NOT NULL DEFAULT 1,
                    watched       INTEGER NOT NULL DEFAULT 0,
                    recursive     INTEGER NOT NULL DEFAULT 1,
                    created_at    TEXT DEFAULT (datetime('now'))
                );
                CREATE INDEX IF NOT EXISTS idx_files_dir ON files(dir);
                CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);",
            )
            .expect("schema creation failed");

        // Migrations
        let db = self.conn();
        // Add temporary column if missing
        let has_temp: bool = db.prepare("SELECT temporary FROM files LIMIT 0").is_ok();
        if !has_temp {
            db.execute_batch("ALTER TABLE files ADD COLUMN temporary INTEGER NOT NULL DEFAULT 0;")
                .ok();
        }
        // Migrate old watched table → directories
        let has_old: bool = db.prepare("SELECT path FROM watched LIMIT 0").is_ok();
        if has_old {
            db.execute_batch(
                "INSERT OR IGNORE INTO directories (path, tracked, watched, recursive)
                 SELECT path, 1, active, 1 FROM watched;
                 DROP TABLE watched;",
            )
            .ok();
        }
    }

    // ── Directories (track / watch) ────────────────────────────────────

    pub fn dir_track(&self, path: &str, recursive: bool) {
        self.conn()
            .execute(
                "INSERT INTO directories (path, tracked, watched, recursive)
                 VALUES (?1, 1, 0, ?2)
                 ON CONFLICT(path) DO UPDATE SET tracked = 1, recursive = ?2",
                rusqlite::params![path, recursive as i32],
            )
            .ok();
    }

    pub fn dir_is_tracked(&self, path: &str) -> bool {
        self.conn()
            .query_row(
                "SELECT 1 FROM directories WHERE path = ?1 AND tracked = 1",
                [path],
                |_| Ok(true),
            )
            .unwrap_or(false)
    }

    /// Check if a parent directory (or ancestor) is already tracked recursively.
    pub fn dir_is_covered(&self, path: &str) -> bool {
        let db = self.conn();
        let mut stmt = db
            .prepare("SELECT path FROM directories WHERE tracked = 1 AND recursive = 1")
            .unwrap();
        let tracked: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let p = path.to_string();
        for dir in &tracked {
            if p == *dir || p.starts_with(&format!("{}/", dir)) {
                return true;
            }
        }
        false
    }

    pub fn dir_untrack(&self, path: &str) {
        self.conn()
            .execute(
                "UPDATE directories SET tracked = 0, watched = 0 WHERE path = ?1",
                [path],
            )
            .ok();
    }

    pub fn dir_watch(&self, path: &str) {
        self.conn()
            .execute(
                "UPDATE directories SET watched = 1 WHERE path = ?1 AND tracked = 1",
                [path],
            )
            .ok();
    }

    pub fn dir_unwatch(&self, path: &str) {
        self.conn()
            .execute("UPDATE directories SET watched = 0 WHERE path = ?1", [path])
            .ok();
    }

    #[allow(dead_code)]
    pub fn tracked_list(&self) -> Vec<(String, bool, bool)> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT path, recursive, watched FROM directories WHERE tracked = 1 ORDER BY path",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, i32>(1)? != 0,
                r.get::<_, i32>(2)? != 0,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    #[allow(dead_code)]
    pub fn watched_list(&self) -> Vec<String> {
        let db = self.conn();
        let mut stmt = db
            .prepare("SELECT path FROM directories WHERE tracked = 1 AND watched = 1 ORDER BY path")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    // ── Collections (tag-based) ──────────────────────────────────────────

    /// Toggle collection tag (c2-c8) on a file. Returns new state.
    #[allow(dead_code)]
    pub fn toggle_collection(&self, file_id: i64, collection: u8) -> bool {
        let tag = collection_tag(collection);
        let db = self.conn();
        let meta_id: Option<i64> = db
            .query_row("SELECT meta_id FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
            .flatten();
        let meta_id = match meta_id {
            Some(id) => id,
            None => return false,
        };
        let tags_str: String = db
            .query_row("SELECT tags FROM meta WHERE id = ?1", [meta_id], |r| {
                r.get(0)
            })
            .unwrap_or_else(|_| "[]".into());
        let mut tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

        let now_in = if tags.contains(&tag) {
            tags.retain(|t| t != &tag);
            false
        } else {
            tags.push(tag);
            true
        };
        let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
        db.execute(
            "UPDATE meta SET tags = ?1 WHERE id = ?2",
            rusqlite::params![json, meta_id],
        )
        .ok();
        now_in
    }

    /// Check if file belongs to a collection.
    #[allow(dead_code)]
    pub fn file_in_collection(&self, file_id: i64, collection: u8) -> bool {
        match collection {
            0 => self
                .conn()
                .query_row(
                    "SELECT temporary FROM files WHERE id = ?1",
                    [file_id],
                    |r| r.get::<_, i32>(0),
                )
                .map(|t| t == 0)
                .unwrap_or(false),
            1 => self
                .conn()
                .query_row(
                    "SELECT temporary FROM files WHERE id = ?1",
                    [file_id],
                    |r| r.get::<_, i32>(0),
                )
                .map(|t| t != 0)
                .unwrap_or(false),
            9 => self
                .conn()
                .query_row(
                    "SELECT 1 FROM files f JOIN meta m ON f.meta_id = m.id
                         WHERE f.id = ?1 AND m.tags LIKE '%\"like\"%'",
                    [file_id],
                    |_| Ok(true),
                )
                .unwrap_or(false),
            2..=8 => {
                let pattern = format!("%\"{}\"%%", collection_tag(collection));
                self.conn()
                    .query_row(
                        "SELECT 1 FROM files f JOIN meta m ON f.meta_id = m.id
                         WHERE f.id = ?1 AND m.tags LIKE ?2",
                        rusqlite::params![file_id, pattern],
                        |_| Ok(true),
                    )
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Get files for a collection.
    #[allow(dead_code)]
    /// Collection 0 = all non-temporary. 1 = temporary.
    /// 2-8 = tag c2-c8. 9 = tag like.
    pub fn files_by_collection(&self, collection: u8) -> Vec<FileEntry> {
        let db = self.conn();
        let (sql, param): (&str, Option<String>) = match collection {
            0 => (
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 WHERE f.temporary = 0
                 ORDER BY f.path",
                None,
            ),
            1 => (
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 WHERE f.temporary = 1
                 ORDER BY f.path",
                None,
            ),
            9 => (
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1, f.temporary
                 FROM files f JOIN meta m ON f.meta_id = m.id
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY f.path",
                None,
            ),
            c @ 2..=8 => (
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f JOIN meta m ON f.meta_id = m.id
                 WHERE m.tags LIKE ?1
                 ORDER BY f.path",
                Some(format!("%\"{}\"%%", collection_tag(c))),
            ),
            _ => return vec![],
        };
        let mut stmt = db.prepare(sql).unwrap();
        let rows = if let Some(ref p) = param {
            stmt.query_map([p.as_str()], row_to_entry)
        } else {
            stmt.query_map([], row_to_entry)
        };
        rows.unwrap().filter_map(|r| r.ok()).collect()
    }

    /// Random file within a collection.
    #[allow(dead_code)]
    pub fn random_in_collection(&self, collection: u8) -> Option<FileEntry> {
        let db = self.conn();
        match collection {
            0 => db
                .query_row(
                    "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 WHERE f.temporary = 0
                 ORDER BY RANDOM() LIMIT 1",
                    [],
                    row_to_entry,
                )
                .ok(),
            1 => db
                .query_row(
                    "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 WHERE f.temporary = 1
                 ORDER BY RANDOM() LIMIT 1",
                    [],
                    row_to_entry,
                )
                .ok(),
            9 => db
                .query_row(
                    "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1, f.temporary
                 FROM files f JOIN meta m ON f.meta_id = m.id
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY RANDOM() LIMIT 1",
                    [],
                    row_to_entry,
                )
                .ok(),
            c @ 2..=8 => {
                let pattern = format!("%\"{}\"%%", collection_tag(c));
                db.query_row(
                    "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                            (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                     FROM files f JOIN meta m ON f.meta_id = m.id
                     WHERE m.tags LIKE ?1
                     ORDER BY RANDOM() LIMIT 1",
                    [&pattern],
                    row_to_entry,
                )
                .ok()
            }
            _ => None,
        }
    }

    /// Count files + total size for a collection.
    #[allow(dead_code)]
    pub fn collection_count_size(&self, collection: u8) -> (i64, i64) {
        let db = self.conn();
        let (sql, param): (&str, Option<String>) = match collection {
            0 => ("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files WHERE temporary = 0", None),
            1 => ("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files WHERE temporary = 1", None),
            9 => (
                "SELECT COUNT(*), COALESCE(SUM(f.size),0) FROM files f JOIN meta m ON f.meta_id = m.id WHERE m.tags LIKE '%\"like\"%'",
                None,
            ),
            c @ 2..=8 => (
                "SELECT COUNT(*), COALESCE(SUM(f.size),0) FROM files f JOIN meta m ON f.meta_id = m.id WHERE m.tags LIKE ?1",
                Some(format!("%\"{}\"%%", collection_tag(c))),
            ),
            _ => return (0, 0),
        };
        if let Some(ref p) = param {
            db.query_row(sql, [p.as_str()], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap_or((0, 0))
        } else {
            db.query_row(sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap_or((0, 0))
        }
    }

    #[allow(dead_code)]
    pub fn set_temporary(&self, file_id: i64, temp: bool) {
        self.conn()
            .execute(
                "UPDATE files SET temporary = ?1 WHERE id = ?2",
                rusqlite::params![temp as i32, file_id],
            )
            .ok();
    }

    pub fn file_lookup(&self, path: &str) -> Option<(i64, Option<i64>, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT id, size, modified_at FROM files WHERE path = ?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()
    }

    pub fn file_insert(
        &self,
        path: &str,
        dir: &str,
        filename: &str,
        size: Option<i64>,
        modified_at: Option<&str>,
    ) -> Option<i64> {
        let db = self.conn();
        db.execute(
            "INSERT OR IGNORE INTO files (path, dir, filename, size, modified_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![path, dir, filename, size, modified_at],
        )
        .ok()?;
        Some(db.last_insert_rowid())
    }

    pub fn file_update_meta(&self, file_id: i64, size: Option<i64>, modified_at: Option<&str>) {
        self.conn()
            .execute(
                "UPDATE files SET size = ?1, modified_at = ?2, hash_sha512 = NULL, meta_id = NULL WHERE id = ?3",
                rusqlite::params![size, modified_at, file_id],
            )
            .ok();
    }

    // ── Directory listing ───────────────────────────────────────────────

    pub fn dirs(&self) -> Vec<String> {
        let db = self.conn();
        let mut stmt = db
            .prepare("SELECT DISTINCT dir FROM files ORDER BY dir")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn first_dir(&self) -> Option<String> {
        self.conn()
            .query_row("SELECT dir FROM files ORDER BY dir LIMIT 1", [], |r| {
                r.get(0)
            })
            .ok()
    }

    // ── File queries ────────────────────────────────────────────────────

    pub fn files_by_dir(&self, dir: &str) -> Vec<FileEntry> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 WHERE f.dir = ?1
                 ORDER BY f.path",
            )
            .unwrap();
        stmt.query_map([dir], row_to_entry)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn navigate_dir(&self, current_dir: &str, delta: i32) -> Option<String> {
        let dirs = self.dirs();
        if dirs.is_empty() {
            return None;
        }
        let cur_idx = dirs.iter().position(|d| d == current_dir).unwrap_or(0);
        let new_idx = (cur_idx as i64 + delta as i64).clamp(0, dirs.len() as i64 - 1) as usize;
        if new_idx == cur_idx {
            return None;
        }
        Some(dirs[new_idx].clone())
    }

    pub fn random_file(&self) -> Option<FileEntry> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 ORDER BY RANDOM() LIMIT 1",
                [],
                row_to_entry,
            )
            .ok()
    }

    pub fn newest_file(&self) -> Option<FileEntry> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%'), f.temporary
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 ORDER BY f.modified_at DESC LIMIT 1",
                [],
                row_to_entry,
            )
            .ok()
    }

    pub fn random_fav(&self) -> Option<FileEntry> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1, f.temporary
                 FROM files f JOIN meta m ON f.meta_id = m.id
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY RANDOM() LIMIT 1",
                [],
                row_to_entry,
            )
            .ok()
    }

    pub fn latest_fav(&self) -> Option<FileEntry> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1, f.temporary
                 FROM files f JOIN meta m ON f.meta_id = m.id
                 JOIN history h ON h.file_id = f.id AND h.action = 'like'
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY h.id DESC LIMIT 1",
                [],
                row_to_entry,
            )
            .ok()
    }

    // ── Mutations ───────────────────────────────────────────────────────

    pub fn toggle_like(&self, file_id: i64) -> bool {
        let db = self.conn();
        let meta_id: Option<i64> = db
            .query_row("SELECT meta_id FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
            .flatten();

        let meta_id = match meta_id {
            Some(id) => id,
            None => return false,
        };

        let tags_str: String = db
            .query_row("SELECT tags FROM meta WHERE id = ?1", [meta_id], |r| {
                r.get(0)
            })
            .unwrap_or_else(|_| "[]".into());
        let mut tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

        let liked = if tags.contains(&"like".to_string()) {
            tags.retain(|t| t != "like");
            db.execute(
                "INSERT INTO history (file_id, action) VALUES (?1, 'unlike')",
                [file_id],
            )
            .ok();
            false
        } else {
            tags.push("like".to_string());
            db.execute(
                "INSERT INTO history (file_id, action) VALUES (?1, 'like')",
                [file_id],
            )
            .ok();
            true
        };

        let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
        db.execute(
            "UPDATE meta SET tags = ?1 WHERE id = ?2",
            rusqlite::params![json, meta_id],
        )
        .ok();

        liked
    }

    pub fn record_view(&self, file_id: i64) {
        self.conn()
            .execute(
                "INSERT INTO history (file_id, action) VALUES (?1, 'view')",
                [file_id],
            )
            .ok();
    }

    // ── Metadata ─────────────────────────────────────────────────────────

    pub fn get_file_metadata(&self, file_id: i64) -> Option<FileMeta> {
        let db = self.conn();
        db.query_row(
            "SELECT f.filename, f.path, f.dir, f.size, f.modified_at, f.hash_sha512,
                    m.width, m.height, m.format, m.duration_ms, m.bitrate, m.codecs,
                    COALESCE(m.tags, '[]'), m.pnginfo
             FROM files f LEFT JOIN meta m ON f.meta_id = m.id
             WHERE f.id = ?1",
            [file_id],
            |row| {
                let tags_str: String = row.get(12)?;
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                Ok(FileMeta {
                    filename: row.get(0)?,
                    path: row.get(1)?,
                    dir: row.get(2)?,
                    size: row.get(3)?,
                    modified_at: row.get(4)?,
                    hash_sha512: row.get(5)?,
                    width: row.get(6)?,
                    height: row.get(7)?,
                    format: row.get(8)?,
                    duration_ms: row.get(9)?,
                    bitrate: row.get(10)?,
                    codecs: row.get(11)?,
                    tags,
                    pnginfo: row.get(13)?,
                })
            },
        )
        .ok()
    }

    // ── Status ──────────────────────────────────────────────────────────

    pub fn file_count(&self) -> i64 {
        self.conn()
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn dir_count(&self) -> i64 {
        self.conn()
            .query_row("SELECT COUNT(DISTINCT dir) FROM files", [], |r| r.get(0))
            .unwrap_or(0)
    }

    // ── Jobs / Layers ───────────────────────────────────────────────────

    pub fn ensure_jobs_schema(&self) {
        self.conn()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS job_fails (
                    file_id INTEGER NOT NULL,
                    layer TEXT NOT NULL,
                    error TEXT,
                    created_at TEXT DEFAULT (datetime('now')),
                    PRIMARY KEY (file_id, layer)
                );",
            )
            .ok();
    }

    pub fn next_missing_hash(&self) -> Option<(i64, String)> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path FROM files f
                 WHERE f.hash_sha512 IS NULL
                 AND f.id NOT IN (SELECT file_id FROM job_fails WHERE layer = 'hash')
                 ORDER BY RANDOM() LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    pub fn next_missing_exif(&self) -> Option<(i64, String)> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path FROM files f
                 JOIN meta m ON f.meta_id = m.id
                 WHERE m.width IS NULL
                 AND f.id NOT IN (SELECT file_id FROM job_fails WHERE layer = 'exif')
                 AND (LOWER(f.path) LIKE '%.jpg' OR LOWER(f.path) LIKE '%.jpeg'
                   OR LOWER(f.path) LIKE '%.png' OR LOWER(f.path) LIKE '%.webp'
                   OR LOWER(f.path) LIKE '%.gif' OR LOWER(f.path) LIKE '%.bmp'
                   OR LOWER(f.path) LIKE '%.tiff')
                 ORDER BY RANDOM() LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    pub fn file_set_hash_meta(&self, file_id: i64, hash: &str) {
        let db = self.conn();
        db.execute(
            "INSERT OR IGNORE INTO meta (hash_sha512) VALUES (?1)",
            [hash],
        )
        .ok();
        if let Ok(meta_id) =
            db.query_row("SELECT id FROM meta WHERE hash_sha512 = ?1", [hash], |r| {
                r.get::<_, i64>(0)
            })
        {
            db.execute(
                "UPDATE files SET hash_sha512 = ?1, meta_id = ?2 WHERE id = ?3",
                rusqlite::params![hash, meta_id, file_id],
            )
            .ok();
        }
    }

    pub fn meta_set_dimensions(&self, file_id: i64, w: u32, h: u32, format: &str) {
        let db = self.conn();
        let meta_id: Option<i64> = db
            .query_row("SELECT meta_id FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
            .flatten();
        if let Some(mid) = meta_id {
            db.execute(
                "UPDATE meta SET width = ?1, height = ?2, format = ?3 WHERE id = ?4",
                rusqlite::params![w, h, format, mid],
            )
            .ok();
        }
    }

    pub fn meta_set_pnginfo(&self, file_id: i64, pnginfo: &str) {
        let db = self.conn();
        let meta_id: Option<i64> = db
            .query_row("SELECT meta_id FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
            .flatten();
        if let Some(mid) = meta_id {
            db.execute(
                "UPDATE meta SET pnginfo = ?1 WHERE id = ?2",
                rusqlite::params![pnginfo, mid],
            )
            .ok();
        }
    }

    pub fn next_missing_pnginfo(&self) -> Option<(i64, String)> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path FROM files f
                 JOIN meta m ON f.meta_id = m.id
                 WHERE m.pnginfo IS NULL
                 AND f.id NOT IN (SELECT file_id FROM job_fails WHERE layer = 'ai_basic')
                 AND LOWER(f.path) LIKE '%.png'
                 ORDER BY RANDOM() LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    pub fn record_job_fail(&self, file_id: i64, layer: &str, error: &str) {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO job_fails (file_id, layer, error) VALUES (?1, ?2, ?3)",
                rusqlite::params![file_id, layer, error],
            )
            .ok();
    }

    pub fn collection_stats(&self) -> CollectionStats {
        let db = self.conn();
        CollectionStats {
            total_files: db
                .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
                .unwrap_or(0),
            total_dirs: db
                .query_row("SELECT COUNT(DISTINCT dir) FROM files", [], |r| r.get(0))
                .unwrap_or(0),
            hashed: db
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE hash_sha512 IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            with_exif: db
                .query_row(
                    "SELECT COUNT(*) FROM files f JOIN meta m ON f.meta_id = m.id WHERE m.width IS NOT NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0),
            failed: db
                .query_row("SELECT COUNT(*) FROM job_fails", [], |r| r.get(0))
                .unwrap_or(0),
        }
    }

    #[allow(dead_code)]
    pub fn file_path_by_id(&self, file_id: i64) -> Option<String> {
        self.conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        dir: row.get(2)?,
        filename: row.get(3)?,
        meta_id: row.get(4)?,
        liked: row.get::<_, i64>(5)? != 0,
        temporary: row.get::<_, i32>(6).unwrap_or(0) != 0,
    })
}

#[allow(dead_code)]
fn collection_tag(c: u8) -> String {
    match c {
        9 => "like".into(),
        n @ 2..=8 => format!("c{n}"),
        _ => String::new(),
    }
}

fn default_db_path() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("dev", "lv", "lv") {
        let data = dirs.data_dir();
        data.join("lv.db")
    } else {
        PathBuf::from("lv.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an in-memory Db with the minimal schema needed for tests.
    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (id INTEGER PRIMARY KEY, tags TEXT DEFAULT '[]');
             CREATE TABLE files (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL,
                 dir TEXT NOT NULL,
                 filename TEXT NOT NULL,
                 meta_id INTEGER REFERENCES meta(id),
                 modified_at TEXT DEFAULT '',
                 size INTEGER,
                 temporary INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY,
                 file_id INTEGER NOT NULL,
                 action TEXT NOT NULL
             );
             CREATE TABLE directories (
                 id INTEGER PRIMARY KEY,
                 path TEXT NOT NULL UNIQUE,
                 tracked INTEGER NOT NULL DEFAULT 1,
                 watched INTEGER NOT NULL DEFAULT 0,
                 recursive INTEGER NOT NULL DEFAULT 1
             );",
        )
        .unwrap();
        Db(Arc::new(Mutex::new(conn)))
    }

    fn insert_file(db: &Db, id: i64, path: &str, dir: &str, filename: &str) {
        let conn = db.conn();
        conn.execute("INSERT INTO meta (id, tags) VALUES (?1, '[]')", [id])
            .ok();
        conn.execute(
            "INSERT INTO files (id, path, dir, filename, meta_id) VALUES (?1, ?2, ?3, ?4, ?1)",
            rusqlite::params![id, path, dir, filename],
        )
        .unwrap();
    }

    #[test]
    fn empty_db() {
        let db = test_db();
        assert_eq!(db.file_count(), 0);
        assert_eq!(db.dir_count(), 0);
        assert!(db.dirs().is_empty());
        assert!(db.first_dir().is_none());
        assert!(db.random_file().is_none());
    }

    #[test]
    fn files_by_dir_returns_sorted() {
        let db = test_db();
        insert_file(&db, 1, "/pics/b.jpg", "/pics", "b.jpg");
        insert_file(&db, 2, "/pics/a.jpg", "/pics", "a.jpg");
        insert_file(&db, 3, "/vids/c.mp4", "/vids", "c.mp4");

        let files = db.files_by_dir("/pics");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "a.jpg"); // sorted by path
        assert_eq!(files[1].filename, "b.jpg");

        let vids = db.files_by_dir("/vids");
        assert_eq!(vids.len(), 1);
        assert_eq!(vids[0].filename, "c.mp4");

        assert!(db.files_by_dir("/nonexistent").is_empty());
    }

    #[test]
    fn dirs_and_counts() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/b/2.jpg", "/b", "2.jpg");
        insert_file(&db, 3, "/b/3.jpg", "/b", "3.jpg");

        assert_eq!(db.file_count(), 3);
        assert_eq!(db.dir_count(), 2);

        let dirs = db.dirs();
        assert_eq!(dirs, vec!["/a", "/b"]);
        assert_eq!(db.first_dir(), Some("/a".to_string()));
    }

    #[test]
    fn navigate_dir_forward_backward() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/b/2.jpg", "/b", "2.jpg");
        insert_file(&db, 3, "/c/3.jpg", "/c", "3.jpg");

        assert_eq!(db.navigate_dir("/a", 1), Some("/b".to_string()));
        assert_eq!(db.navigate_dir("/b", 1), Some("/c".to_string()));
        assert_eq!(db.navigate_dir("/c", 1), None); // at end
        assert_eq!(db.navigate_dir("/c", -1), Some("/b".to_string()));
        assert_eq!(db.navigate_dir("/a", -1), None); // at start
    }

    #[test]
    fn toggle_like() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        // Initially not liked
        let files = db.files_by_dir("/a");
        assert!(!files[0].liked);

        // Like it
        let liked = db.toggle_like(1);
        assert!(liked);
        let files = db.files_by_dir("/a");
        assert!(files[0].liked);

        // Unlike it
        let liked = db.toggle_like(1);
        assert!(!liked);
        let files = db.files_by_dir("/a");
        assert!(!files[0].liked);
    }

    #[test]
    fn record_view_inserts_history() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        db.record_view(1);
        db.record_view(1);

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM history WHERE file_id = 1 AND action = 'view'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn random_file_returns_something() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        let f = db.random_file();
        assert!(f.is_some());
        assert_eq!(f.unwrap().id, 1);
    }

    // ── Directory tracking tests ────────────────────────────────────────

    #[test]
    fn dir_track_and_untrack() {
        let db = test_db();
        assert!(!db.dir_is_tracked("/photos"));

        db.dir_track("/photos", true);
        assert!(db.dir_is_tracked("/photos"));

        db.dir_untrack("/photos");
        assert!(!db.dir_is_tracked("/photos"));
    }

    #[test]
    fn dir_track_idempotent() {
        let db = test_db();
        db.dir_track("/photos", true);
        db.dir_track("/photos", true);
        assert!(db.dir_is_tracked("/photos"));

        // Re-track with different recursive flag updates it
        db.dir_track("/photos", false);
        let list = db.tracked_list();
        assert_eq!(list.len(), 1);
        assert!(!list[0].1); // recursive = false now
    }

    #[test]
    fn dir_track_updates_recursive_flag() {
        let db = test_db();
        db.dir_track("/a", true);
        assert!(db.tracked_list()[0].1); // recursive

        db.dir_track("/a", false);
        assert!(!db.tracked_list()[0].1); // now non-recursive
    }

    #[test]
    fn dir_watch_requires_tracked() {
        let db = test_db();
        // Watching a non-tracked dir does nothing
        db.dir_watch("/photos");
        assert!(db.watched_list().is_empty());

        // Track then watch
        db.dir_track("/photos", true);
        db.dir_watch("/photos");
        assert_eq!(db.watched_list(), vec!["/photos"]);

        // Unwatch
        db.dir_unwatch("/photos");
        assert!(db.watched_list().is_empty());
        // Still tracked
        assert!(db.dir_is_tracked("/photos"));
    }

    #[test]
    fn dir_untrack_also_unwatches() {
        let db = test_db();
        db.dir_track("/photos", true);
        db.dir_watch("/photos");
        assert_eq!(db.watched_list().len(), 1);

        db.dir_untrack("/photos");
        assert!(db.watched_list().is_empty());
        assert!(!db.dir_is_tracked("/photos"));
    }

    #[test]
    fn tracked_list_shows_flags() {
        let db = test_db();
        db.dir_track("/a", true);
        db.dir_track("/b", false);
        db.dir_watch("/a");

        let list = db.tracked_list();
        assert_eq!(list.len(), 2);
        // /a: recursive=true, watched=true
        assert_eq!(list[0].0, "/a");
        assert!(list[0].1);
        assert!(list[0].2);
        // /b: recursive=false, watched=false
        assert_eq!(list[1].0, "/b");
        assert!(!list[1].1);
        assert!(!list[1].2);
    }

    // ── dir_is_covered tests (ancestor tracking) ────────────────────────

    #[test]
    fn dir_is_covered_exact_match() {
        let db = test_db();
        db.dir_track("/photos", true);
        assert!(db.dir_is_covered("/photos"));
    }

    #[test]
    fn dir_is_covered_child_of_recursive() {
        let db = test_db();
        db.dir_track("/photos", true);
        assert!(db.dir_is_covered("/photos/vacation"));
        assert!(db.dir_is_covered("/photos/vacation/day1"));
    }

    #[test]
    fn dir_is_covered_not_child_of_nonrecursive() {
        let db = test_db();
        db.dir_track("/photos", false);
        // Non-recursive: exact match is covered, children are NOT
        assert!(!db.dir_is_covered("/photos/vacation"));
        // Exact match is not covered either since recursive=0
        assert!(!db.dir_is_covered("/photos"));
    }

    #[test]
    fn dir_is_covered_no_false_prefix_match() {
        let db = test_db();
        db.dir_track("/photo", true);
        // "/photos" should NOT match "/photo" — it's not a child
        assert!(!db.dir_is_covered("/photos"));
        assert!(!db.dir_is_covered("/photography"));
        // But "/photo/x" should match
        assert!(db.dir_is_covered("/photo/x"));
    }

    #[test]
    fn dir_is_covered_untracked_not_covered() {
        let db = test_db();
        db.dir_track("/photos", true);
        db.dir_untrack("/photos");
        assert!(!db.dir_is_covered("/photos/vacation"));
    }

    // ── Temporary flag tests ────────────────────────────────────────────

    #[test]
    fn set_temporary_flag() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        // Default: not temporary
        let f = db.files_by_dir("/a");
        assert!(!f[0].temporary);

        // Set temporary
        db.set_temporary(1, true);
        let f = db.files_by_dir("/a");
        assert!(f[0].temporary);

        // Unset
        db.set_temporary(1, false);
        let f = db.files_by_dir("/a");
        assert!(!f[0].temporary);
    }

    // ── Collection tests (tag-based) ────────────────────────────────────

    #[test]
    fn collection_0_excludes_temporary() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/a/2.jpg", "/a", "2.jpg");
        db.set_temporary(2, true);

        let c0 = db.files_by_collection(0);
        assert_eq!(c0.len(), 1);
        assert_eq!(c0[0].id, 1);

        assert!(db.file_in_collection(1, 0));
        assert!(!db.file_in_collection(2, 0));
    }

    #[test]
    fn collection_1_only_temporary() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/a/2.jpg", "/a", "2.jpg");
        db.set_temporary(2, true);

        let c1 = db.files_by_collection(1);
        assert_eq!(c1.len(), 1);
        assert_eq!(c1[0].id, 2);

        assert!(!db.file_in_collection(1, 1));
        assert!(db.file_in_collection(2, 1));
    }

    #[test]
    fn collection_9_is_liked() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/a/2.jpg", "/a", "2.jpg");
        db.toggle_like(1);

        let c9 = db.files_by_collection(9);
        assert_eq!(c9.len(), 1);
        assert_eq!(c9[0].id, 1);

        assert!(db.file_in_collection(1, 9));
        assert!(!db.file_in_collection(2, 9));
    }

    #[test]
    fn toggle_collection_tag_c2_through_c8() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        // Toggle c3 on
        let on = db.toggle_collection(1, 3);
        assert!(on);
        assert!(db.file_in_collection(1, 3));

        let c3 = db.files_by_collection(3);
        assert_eq!(c3.len(), 1);

        // Toggle c3 off
        let off = db.toggle_collection(1, 3);
        assert!(!off);
        assert!(!db.file_in_collection(1, 3));
        assert!(db.files_by_collection(3).is_empty());
    }

    #[test]
    fn multiple_collection_tags_independent() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        db.toggle_collection(1, 2);
        db.toggle_collection(1, 5);

        assert!(db.file_in_collection(1, 2));
        assert!(db.file_in_collection(1, 5));
        assert!(!db.file_in_collection(1, 3));

        // Removing c2 doesn't affect c5
        db.toggle_collection(1, 2);
        assert!(!db.file_in_collection(1, 2));
        assert!(db.file_in_collection(1, 5));
    }

    #[test]
    fn collection_count_size() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/a/2.jpg", "/a", "2.jpg");
        insert_file(&db, 3, "/a/3.jpg", "/a", "3.jpg");

        // All non-temporary → collection 0
        let (count, _size) = db.collection_count_size(0);
        assert_eq!(count, 3);

        // Mark one temporary
        db.set_temporary(3, true);
        let (c0, _) = db.collection_count_size(0);
        let (c1, _) = db.collection_count_size(1);
        assert_eq!(c0, 2);
        assert_eq!(c1, 1);

        // Tag collection
        db.toggle_collection(1, 4);
        let (c4, _) = db.collection_count_size(4);
        assert_eq!(c4, 1);
    }

    #[test]
    fn random_in_collection_respects_filter() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        insert_file(&db, 2, "/a/2.jpg", "/a", "2.jpg");
        db.set_temporary(1, true);

        // Random in collection 0 should never return file 1
        for _ in 0..20 {
            let f = db.random_in_collection(0);
            assert!(f.is_some());
            assert_eq!(f.unwrap().id, 2);
        }

        // Random in collection 1 should only return file 1
        for _ in 0..20 {
            let f = db.random_in_collection(1);
            assert!(f.is_some());
            assert_eq!(f.unwrap().id, 1);
        }
    }

    #[test]
    fn random_in_empty_collection_returns_none() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        // No temporary files → collection 1 is empty
        assert!(db.random_in_collection(1).is_none());
        // No tagged files → collection 3 is empty
        assert!(db.random_in_collection(3).is_none());
        // No liked files → collection 9 is empty
        assert!(db.random_in_collection(9).is_none());
    }

    #[test]
    fn collection_tag_helper() {
        assert_eq!(collection_tag(2), "c2");
        assert_eq!(collection_tag(8), "c8");
        assert_eq!(collection_tag(9), "like");
        assert_eq!(collection_tag(0), "");
        assert_eq!(collection_tag(1), "");
        assert_eq!(collection_tag(10), "");
    }

    #[test]
    fn toggle_collection_on_file_without_meta_returns_false() {
        let db = test_db();
        // Insert file without meta_id
        db.conn()
            .execute(
                "INSERT INTO files (id, path, dir, filename) VALUES (99, '/x/y.jpg', '/x', 'y.jpg')",
                [],
            )
            .unwrap();
        let result = db.toggle_collection(99, 3);
        assert!(!result);
    }

    #[test]
    fn file_in_collection_nonexistent_file() {
        let db = test_db();
        assert!(!db.file_in_collection(999, 0));
        assert!(!db.file_in_collection(999, 1));
        assert!(!db.file_in_collection(999, 5));
        assert!(!db.file_in_collection(999, 9));
    }

    #[test]
    fn files_by_collection_invalid_returns_empty() {
        let db = test_db();
        assert!(db.files_by_collection(10).is_empty());
        assert!(db.files_by_collection(255).is_empty());
    }

    #[test]
    fn like_and_collection_tag_coexist() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");

        db.toggle_like(1);
        db.toggle_collection(1, 4);

        assert!(db.file_in_collection(1, 9)); // liked
        assert!(db.file_in_collection(1, 4)); // c4
        assert!(db.file_in_collection(1, 0)); // non-temporary

        // Unlike doesn't remove c4
        db.toggle_like(1);
        assert!(!db.file_in_collection(1, 9));
        assert!(db.file_in_collection(1, 4));
    }

    #[test]
    fn temporary_file_in_tagged_collection() {
        let db = test_db();
        insert_file(&db, 1, "/a/1.jpg", "/a", "1.jpg");
        db.set_temporary(1, true);
        db.toggle_collection(1, 3);

        // Temporary file can still be in tag collections
        assert!(db.file_in_collection(1, 1)); // temporary
        assert!(db.file_in_collection(1, 3)); // c3
        assert!(!db.file_in_collection(1, 0)); // NOT in collection 0
    }

    // ── to_string_lossy / Unicode path tests ────────────────────────────
    //
    // Path::to_string_lossy() replaces invalid UTF-8 bytes with U+FFFD (�).
    // It does NOT strip characters. The replacement is deterministic, so
    // two calls on the same OsStr always produce the same String. Our code
    // relies on this: the scanner stores paths via to_string_lossy(), and
    // later lookups compare against the same lossy conversion.
    //
    // These tests verify that:
    // 1. Valid UTF-8 paths (including emoji, CJK, accents) round-trip perfectly.
    // 2. The replacement character is stored and matched consistently.
    // 3. dir_is_tracked / dir_is_covered work with Unicode paths.

    #[test]
    fn unicode_paths_roundtrip() {
        let db = test_db();
        // Emoji dir
        insert_file(&db, 1, "/📸/photo.jpg", "/📸", "photo.jpg");
        // CJK
        insert_file(&db, 2, "/写真/img.png", "/写真", "img.png");
        // Accented
        insert_file(&db, 3, "/café/latté.jpg", "/café", "latté.jpg");

        let files = db.files_by_dir("/📸");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "/📸/photo.jpg");

        let files = db.files_by_dir("/写真");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "img.png");

        let files = db.files_by_dir("/café");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "latté.jpg");

        assert_eq!(db.file_count(), 3);
        assert_eq!(db.dir_count(), 3);
    }

    #[test]
    fn unicode_dir_tracking() {
        let db = test_db();
        db.dir_track("/données/photos", true);
        assert!(db.dir_is_tracked("/données/photos"));
        assert!(db.dir_is_covered("/données/photos/été"));
        assert!(!db.dir_is_covered("/donnees/photos")); // different string

        db.dir_untrack("/données/photos");
        assert!(!db.dir_is_tracked("/données/photos"));
    }

    #[test]
    fn replacement_char_is_consistent() {
        let db = test_db();
        // Simulate what to_string_lossy produces for invalid UTF-8:
        // the replacement character U+FFFD is a valid UTF-8 string.
        let lossy_path = "/pics/caf\u{FFFD}.jpg";
        let lossy_dir = "/pics";
        insert_file(&db, 1, lossy_path, lossy_dir, "caf\u{FFFD}.jpg");

        // Lookup with the same lossy string succeeds
        let files = db.files_by_dir(lossy_dir);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, lossy_path);
        assert_eq!(files[0].filename, "caf\u{FFFD}.jpg");

        // Lookup with the "correct" UTF-8 does NOT match the lossy version
        let files = db.files_by_dir("/pics_other");
        assert!(files.is_empty());
    }

    #[test]
    fn lossy_path_does_not_match_original() {
        let db = test_db();
        // A path stored with replacement char won't match the "intended" name
        insert_file(&db, 1, "/a/caf\u{FFFD}.jpg", "/a", "caf\u{FFFD}.jpg");

        let files = db.files_by_dir("/a");
        assert_eq!(files.len(), 1);
        // The stored filename contains the replacement char, not the original byte
        assert!(files[0].filename.contains('\u{FFFD}'));
        assert_ne!(files[0].filename, "café.jpg");
    }

    #[test]
    fn to_string_lossy_deterministic() {
        use std::ffi::OsStr;
        // Valid UTF-8: to_string_lossy returns identical string
        let s = OsStr::new("/photos/café/日本語.jpg");
        let a = s.to_string_lossy();
        let b = s.to_string_lossy();
        assert_eq!(a, b);

        // On Unix, we can test with raw invalid bytes
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            // 0xFF is not valid UTF-8
            let raw: &[u8] = b"/pics/caf\xff.jpg";
            let os = OsStr::from_bytes(raw);
            let lossy1 = os.to_string_lossy().to_string();
            let lossy2 = os.to_string_lossy().to_string();
            assert_eq!(lossy1, lossy2); // deterministic
            assert!(lossy1.contains('\u{FFFD}')); // replacement char present
            assert!(!lossy1.contains('\u{FF}')); // original byte gone
        }
    }
}
