use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};

use crate::debug::dbg_log;

// ---------------------------------------------------------------------------
// Db — thin wrapper around Arc<Mutex<Connection>>
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn new(conn: Connection) -> Self {
        Self(Arc::new(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct FileDto {
    pub id: i64,
    pub path: String,
    pub dir: String,
    pub filename: String,
    pub meta_id: Option<i64>,
    pub thumb_ready: bool,
    pub shadow: Option<String>,
    pub liked: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct FileMetaDto {
    pub file_id: i64,
    pub path: String,
    pub dir: String,
    pub filename: String,
    pub size: Option<i64>,
    pub modified_at: Option<String>,
    pub hash_sha512: Option<String>,
    pub meta_id: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub format: Option<String>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i64>,
    pub codecs: Option<String>,
    pub tags: Vec<String>,
    pub thumb_ready: bool,
}

#[derive(Debug)]
pub struct Job {
    pub id: i64,
    pub job_type: String,
    pub file_id: Option<i64>,
    pub meta_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct StatusInfo {
    pub files: i64,
    pub dirs: i64,
    pub hashed: i64,
    pub thumbs: i64,
    pub watched: i64,
    pub jobs_pending: i64,
    pub jobs_running: i64,
    pub jobs_done: i64,
    pub jobs_failed: i64,
    pub watched_paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

impl Db {
    pub fn file_path(&self, file_id: i64) -> Option<String> {
        self.conn()
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
    }

    pub fn file_path_for_meta(&self, meta_id: i64) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT f.path FROM files f WHERE f.meta_id = ?1 LIMIT 1",
                [meta_id],
                |r| r.get(0),
            )
            .ok()
    }

    /// Check if file exists by path. Returns (id, size, modified_at) if found.
    pub fn file_lookup(&self, path: &str) -> Option<(i64, Option<i64>, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT id, size, modified_at FROM files WHERE path = ?1",
                [path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok()
    }

    /// Mark existing file as changed — clear hash/meta, update size/mtime.
    pub fn file_mark_changed(&self, file_id: i64, size: Option<i64>, mtime: Option<&str>) {
        self.conn()
            .execute(
                "UPDATE files SET size = ?1, modified_at = ?2, hash_sha512 = NULL, meta_id = NULL WHERE id = ?3",
                rusqlite::params![size, mtime, file_id],
            )
            .ok();
    }

    /// Insert a new file. Returns the new file_id, or None if already exists.
    pub fn file_insert(
        &self,
        path: &str,
        dir: &str,
        filename: &str,
        size: Option<i64>,
        mtime: Option<&str>,
    ) -> Option<i64> {
        let db = self.conn();
        let inserted = db
            .execute(
                "INSERT OR IGNORE INTO files (path, dir, filename, size, modified_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![path, dir, filename, size, mtime],
            )
            .unwrap_or(0);
        if inserted == 1 {
            db.query_row("SELECT id FROM files WHERE path = ?1", [path], |r| r.get(0))
                .ok()
        } else {
            None
        }
    }

    /// Link file to a hash and meta_id.
    pub fn file_set_hash(&self, file_id: i64, hash: &str, meta_id: i64) {
        self.conn()
            .execute(
                "UPDATE files SET hash_sha512 = ?1, meta_id = ?2 WHERE id = ?3",
                rusqlite::params![hash, meta_id, file_id],
            )
            .ok();
    }

    pub fn files_by_dir(&self, dir: &str) -> Vec<FileDto> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 WHERE f.dir = ?1
                 ORDER BY f.path",
            )
            .unwrap();
        stmt.query_map([dir], row_to_dto)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn files_all(&self) -> Vec<FileDto> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 ORDER BY f.path",
            )
            .unwrap();
        stmt.query_map([], row_to_dto)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn files_dirs(&self) -> Vec<String> {
        let db = self.conn();
        let mut stmt = db
            .prepare("SELECT DISTINCT dir FROM files ORDER BY dir")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn files_first_dir(&self) -> Option<String> {
        self.conn()
            .query_row("SELECT dir FROM files ORDER BY dir LIMIT 1", [], |r| {
                r.get(0)
            })
            .ok()
    }

    pub fn file_random(&self) -> Option<FileDto> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 ORDER BY RANDOM() LIMIT 1",
                [],
                row_to_dto,
            )
            .ok()
    }

    pub fn file_newest(&self) -> Option<FileDto> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
                 FROM files f LEFT JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 ORDER BY f.modified_at DESC LIMIT 1",
                [],
                row_to_dto,
            )
            .ok()
    }

    pub fn file_random_fav(&self) -> Option<FileDto> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        1
                 FROM files f
                 JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY RANDOM() LIMIT 1",
                [],
                row_to_dto,
            )
            .ok()
    }

    pub fn file_metadata(&self, file_id: i64) -> Option<FileMetaDto> {
        let db = self.conn();
        db.query_row(
            "SELECT f.id, f.path, f.dir, f.filename, f.size, f.modified_at, f.hash_sha512,
                    f.meta_id, m.width, m.height, m.format, m.duration_ms, m.bitrate,
                    m.codecs, m.tags, COALESCE(m.thumb_ready, 0)
             FROM files f LEFT JOIN meta m ON f.meta_id = m.id
             WHERE f.id = ?1",
            [file_id],
            |row| {
                let tags_str: String = row
                    .get::<_, Option<String>>(14)?
                    .unwrap_or_else(|| "[]".into());
                let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
                Ok(FileMetaDto {
                    file_id: row.get(0)?,
                    path: row.get(1)?,
                    dir: row.get(2)?,
                    filename: row.get(3)?,
                    size: row.get(4)?,
                    modified_at: row.get(5)?,
                    hash_sha512: row.get(6)?,
                    meta_id: row.get(7)?,
                    width: row.get(8)?,
                    height: row.get(9)?,
                    format: row.get(10)?,
                    duration_ms: row.get(11)?,
                    bitrate: row.get(12)?,
                    codecs: row.get(13)?,
                    tags,
                    thumb_ready: row.get::<_, i64>(15)? != 0,
                })
            },
        )
        .ok()
    }

    pub fn files_all_fav(&self) -> Vec<FileDto> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        1
                 FROM files f
                 JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 WHERE m.tags LIKE '%\"like\"%'
                 ORDER BY f.path",
            )
            .unwrap();
        stmt.query_map([], row_to_dto)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    pub fn file_latest_fav(&self) -> Option<FileDto> {
        self.conn()
            .query_row(
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, COALESCE(m.thumb_ready, 0), ts.webp_data,
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
                 FROM files f
                 JOIN meta m ON f.meta_id = m.id
                 LEFT JOIN thumbs ts ON ts.meta_id = f.meta_id AND ts.size_tag = 'shadow'
                 JOIN history h ON h.file_id = f.id AND h.action = 'like'
                 ORDER BY h.id DESC LIMIT 1",
                [],
                row_to_dto,
            )
            .ok()
    }
}

fn row_to_dto(row: &rusqlite::Row) -> rusqlite::Result<FileDto> {
    let shadow_blob: Option<Vec<u8>> = row.get(6)?;
    let shadow = shadow_blob.map(|b| format!("data:image/webp;base64,{}", B64.encode(&b)));
    Ok(FileDto {
        id: row.get(0)?,
        path: row.get(1)?,
        dir: row.get(2)?,
        filename: row.get(3)?,
        meta_id: row.get(4)?,
        thumb_ready: row.get::<_, i64>(5)? != 0,
        shadow,
        liked: row.get::<_, i64>(7)? != 0,
    })
}

// ---------------------------------------------------------------------------
// Meta
// ---------------------------------------------------------------------------

impl Db {
    /// Upsert meta by hash. Returns the meta_id.
    pub fn meta_upsert(&self, hash: &str) -> Option<i64> {
        let db = self.conn();
        db.execute(
            "INSERT OR IGNORE INTO meta (hash_sha512) VALUES (?1)",
            [hash],
        )
        .ok()?;
        db.query_row("SELECT id FROM meta WHERE hash_sha512 = ?1", [hash], |r| {
            r.get(0)
        })
        .ok()
    }

    pub fn meta_thumb_ready(&self, meta_id: i64) -> bool {
        self.conn()
            .query_row(
                "SELECT thumb_ready FROM meta WHERE id = ?1",
                [meta_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            != 0
    }

    pub fn meta_set_dimensions(&self, meta_id: i64, w: u32, h: u32, format: &str) {
        self.conn()
            .execute(
                "UPDATE meta SET width = ?1, height = ?2, format = ?3, thumb_ready = 1 WHERE id = ?4 AND width IS NULL",
                rusqlite::params![w as i64, h as i64, format, meta_id],
            )
            .ok();
    }

    pub fn meta_id_for_file(&self, file_id: i64) -> Option<i64> {
        self.conn()
            .query_row("SELECT meta_id FROM files WHERE id = ?1", [file_id], |r| {
                r.get(0)
            })
            .ok()
            .flatten()
    }

    pub fn meta_get_tags(&self, meta_id: i64) -> Vec<String> {
        let tags_str: String = self
            .conn()
            .query_row("SELECT tags FROM meta WHERE id = ?1", [meta_id], |r| {
                r.get(0)
            })
            .unwrap_or_else(|_| "[]".into());
        serde_json::from_str(&tags_str).unwrap_or_default()
    }

    pub fn meta_set_tags(&self, meta_id: i64, tags: &[String]) {
        let json = serde_json::to_string(tags).unwrap_or_else(|_| "[]".into());
        self.conn()
            .execute(
                "UPDATE meta SET tags = ?1 WHERE id = ?2",
                rusqlite::params![json, meta_id],
            )
            .ok();
    }

    /// Reset all thumbnails — clear thumb_ready, delete all thumb blobs, re-enqueue jobs.
    pub fn reset_thumbs(&self) -> usize {
        let db = self.conn();
        db.execute_batch(
            "UPDATE meta SET thumb_ready = 0, width = NULL, height = NULL;
             DELETE FROM thumbs;
             DELETE FROM jobs WHERE job_type = 'thumbnail';",
        )
        .ok();
        let mut stmt = db.prepare("SELECT id FROM meta").unwrap();
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        let count = ids.len();
        for meta_id in ids {
            db.execute(
                "INSERT INTO jobs (job_type, meta_id, priority) VALUES ('thumbnail', ?1, 0)",
                [meta_id],
            )
            .ok();
        }
        count
    }
}

// ---------------------------------------------------------------------------
// Thumbs
// ---------------------------------------------------------------------------

impl Db {
    pub fn thumb_save(&self, meta_id: i64, size_tag: &str, webp_data: &[u8]) {
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO thumbs (meta_id, size_tag, webp_data) VALUES (?1, ?2, ?3)",
                rusqlite::params![meta_id, size_tag, webp_data],
            )
            .ok();
    }

    pub fn thumb_get(&self, meta_id: i64, size_tag: &str) -> Option<Vec<u8>> {
        self.conn()
            .query_row(
                "SELECT webp_data FROM thumbs WHERE meta_id = ?1 AND size_tag = ?2",
                rusqlite::params![meta_id, size_tag],
                |r| r.get(0),
            )
            .ok()
    }
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

impl Db {
    /// Reset any 'running' jobs back to 'pending' — cleanup after crash/interrupt.
    pub fn jobs_recover_stale(&self) {
        let db = self.conn();
        let n = db
            .execute(
                "UPDATE jobs SET status = 'pending', updated_at = datetime('now') WHERE status = 'running'",
                [],
            )
            .unwrap_or(0);
        if n > 0 {
            dbg_log!("recovered {} stale running jobs", n);
            eprintln!("recovered {} interrupted jobs", n);
        }
    }

    /// Claim the next pending job of the given type, atomically setting status to 'running'.
    pub fn jobs_claim_next(&self, job_type: &str) -> Option<Job> {
        let db = self.conn();
        let mut stmt = db
            .prepare(
                "SELECT id, job_type, file_id, meta_id FROM jobs
                 WHERE status = 'pending' AND job_type = ?1
                 ORDER BY priority DESC, id ASC
                 LIMIT 1",
            )
            .ok()?;

        let job = stmt
            .query_row([job_type], |row| {
                Ok(Job {
                    id: row.get(0)?,
                    job_type: row.get(1)?,
                    file_id: row.get(2)?,
                    meta_id: row.get(3)?,
                })
            })
            .ok()?;

        db.execute(
            "UPDATE jobs SET status = 'running', updated_at = datetime('now') WHERE id = ?1",
            [job.id],
        )
        .ok()?;

        Some(job)
    }

    pub fn jobs_mark_done(&self, job_id: i64) {
        self.conn()
            .execute(
                "UPDATE jobs SET status = 'done', updated_at = datetime('now') WHERE id = ?1",
                [job_id],
            )
            .ok();
    }

    pub fn jobs_mark_failed(&self, job_id: i64, error: &str) {
        self.conn()
            .execute(
                "UPDATE jobs SET status = 'failed', error = ?2, updated_at = datetime('now') WHERE id = ?1",
                rusqlite::params![job_id, error],
            )
            .ok();
    }

    /// Boost priority for jobs matching the given file/meta ids (current view context).
    /// Resets all other pending jobs back to default priority so background work continues.
    pub fn jobs_boost(&self, file_ids: &[i64], meta_ids: &[i64]) {
        let db = self.conn();
        // Reset all boosted pending jobs back to 0
        db.execute(
            "UPDATE jobs SET priority = 0 WHERE status = 'pending' AND priority > 0",
            [],
        )
        .ok();

        if file_ids.is_empty() && meta_ids.is_empty() {
            return;
        }

        // Build dynamic IN clause for file_ids and meta_ids
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut clauses = Vec::new();

        if !file_ids.is_empty() {
            let placeholders: Vec<String> = file_ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + i + 1))
                .collect();
            clauses.push(format!("file_id IN ({})", placeholders.join(",")));
            for id in file_ids {
                params.push(Box::new(*id));
            }
        }

        if !meta_ids.is_empty() {
            let placeholders: Vec<String> = meta_ids
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", params.len() + i + 1))
                .collect();
            clauses.push(format!("meta_id IN ({})", placeholders.join(",")));
            for id in meta_ids {
                params.push(Box::new(*id));
            }
        }

        let sql = format!(
            "UPDATE jobs SET priority = 10 WHERE status = 'pending' AND ({})",
            clauses.join(" OR ")
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        db.execute(&sql, param_refs.as_slice()).ok();
    }

    pub fn jobs_enqueue_hash(&self, file_id: i64) {
        self.conn()
            .execute(
                "INSERT INTO jobs (job_type, file_id, priority) VALUES ('hash', ?1, 0)",
                [file_id],
            )
            .ok();
    }

    pub fn jobs_enqueue_thumb(&self, meta_id: i64, priority: i64) {
        self.conn()
            .execute(
                "INSERT INTO jobs (job_type, meta_id, priority) VALUES ('thumbnail', ?1, ?2)",
                rusqlite::params![meta_id, priority],
            )
            .ok();
    }
}

// ---------------------------------------------------------------------------
// Watched
// ---------------------------------------------------------------------------

impl Db {
    pub fn watched_add(&self, path: &str) {
        self.conn()
            .execute("INSERT OR IGNORE INTO watched (path) VALUES (?1)", [path])
            .ok();
    }

    pub fn watched_watch(&self, path: &str) {
        self.conn()
            .execute(
                "INSERT INTO watched (path) VALUES (?1) ON CONFLICT(path) DO UPDATE SET active = 1",
                [path],
            )
            .ok();
    }

    pub fn watched_unwatch(&self, path: &str) {
        self.conn()
            .execute("UPDATE watched SET active = 0 WHERE path = ?1", [path])
            .ok();
    }

    pub fn watched_list_active(&self) -> Vec<String> {
        let db = self.conn();
        let mut stmt = db
            .prepare("SELECT path FROM watched WHERE active = 1 ORDER BY path")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

impl Db {
    pub fn history_record(&self, file_id: i64, action: &str) {
        self.conn()
            .execute(
                "INSERT INTO history (file_id, action) VALUES (?1, ?2)",
                rusqlite::params![file_id, action],
            )
            .ok();
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

impl Db {
    pub fn status(&self) -> StatusInfo {
        let db = self.conn();
        let count = |sql: &str| -> i64 { db.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };

        let mut stmt = db
            .prepare("SELECT path FROM watched WHERE active = 1 ORDER BY path")
            .unwrap();
        let watched_paths: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        StatusInfo {
            files: count("SELECT COUNT(*) FROM files"),
            dirs: count("SELECT COUNT(DISTINCT dir) FROM files"),
            hashed: count("SELECT COUNT(*) FROM files WHERE hash_sha512 IS NOT NULL"),
            thumbs: count("SELECT COUNT(*) FROM thumbs"),
            watched: count("SELECT COUNT(*) FROM watched WHERE active = 1"),
            jobs_pending: count("SELECT COUNT(*) FROM jobs WHERE status = 'pending'"),
            jobs_running: count("SELECT COUNT(*) FROM jobs WHERE status = 'running'"),
            jobs_done: count("SELECT COUNT(*) FROM jobs WHERE status = 'done'"),
            jobs_failed: count("SELECT COUNT(*) FROM jobs WHERE status = 'failed'"),
            watched_paths,
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(
            "
            CREATE TABLE files (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE,
                dir TEXT NOT NULL, filename TEXT NOT NULL,
                size INTEGER, modified_at TEXT, hash_sha512 TEXT,
                meta_id INTEGER REFERENCES meta(id),
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE meta (
                id INTEGER PRIMARY KEY, hash_sha512 TEXT NOT NULL UNIQUE,
                width INTEGER, height INTEGER, format TEXT,
                exif_json TEXT, pnginfo TEXT, duration_ms INTEGER,
                bitrate INTEGER, codecs TEXT, tags TEXT DEFAULT '[]',
                thumb_ready INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE thumbs (
                meta_id INTEGER NOT NULL REFERENCES meta(id),
                size_tag TEXT NOT NULL DEFAULT 'default',
                webp_data BLOB NOT NULL,
                created_at TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (meta_id, size_tag)
            );
            CREATE TABLE history (
                id INTEGER PRIMARY KEY, file_id INTEGER REFERENCES files(id),
                action TEXT NOT NULL, created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE watched (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE,
                active INTEGER DEFAULT 1, created_at TEXT DEFAULT (datetime('now'))
            );
            CREATE TABLE jobs (
                id INTEGER PRIMARY KEY, job_type TEXT NOT NULL,
                file_id INTEGER, meta_id INTEGER,
                status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0,
                error TEXT, created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT
            );
            CREATE INDEX idx_files_dir ON files(dir);
            CREATE INDEX idx_files_hash ON files(hash_sha512);
            CREATE INDEX idx_jobs_status ON jobs(status, priority DESC);
            ",
        )
        .unwrap();
        Db::new(conn)
    }

    // -- Files ---------------------------------------------------------------

    #[test]
    fn file_insert_and_path() {
        let db = test_db();
        let id = db.file_insert(
            "/a/b/img.jpg",
            "/a/b",
            "img.jpg",
            Some(1024),
            Some("2025-01-01T00:00:00Z"),
        );
        assert!(id.is_some());
        let id = id.unwrap();
        assert_eq!(db.file_path(id).unwrap(), "/a/b/img.jpg");
    }

    #[test]
    fn file_insert_duplicate_returns_none() {
        let db = test_db();
        db.file_insert("/a/b/img.jpg", "/a/b", "img.jpg", Some(1024), None);
        let dup = db.file_insert("/a/b/img.jpg", "/a/b", "img.jpg", Some(1024), None);
        assert!(dup.is_none());
    }

    #[test]
    fn file_lookup_found_and_missing() {
        let db = test_db();
        assert!(db.file_lookup("/nope").is_none());
        db.file_insert("/x/y.png", "/x", "y.png", Some(512), Some("2025-06-01"));
        let (id, sz, mt) = db.file_lookup("/x/y.png").unwrap();
        assert!(id > 0);
        assert_eq!(sz, Some(512));
        assert_eq!(mt.as_deref(), Some("2025-06-01"));
    }

    #[test]
    fn file_mark_changed_clears_hash_and_meta() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", Some(100), Some("t1"))
            .unwrap();
        let mid = db.meta_upsert("hash1").unwrap();
        db.file_set_hash(fid, "hash1", mid);
        // Verify linked
        assert_eq!(db.meta_id_for_file(fid), Some(mid));
        // Mark changed
        db.file_mark_changed(fid, Some(200), Some("t2"));
        let (_, sz, mt) = db.file_lookup("/a/f.jpg").unwrap();
        assert_eq!(sz, Some(200));
        assert_eq!(mt.as_deref(), Some("t2"));
        assert_eq!(db.meta_id_for_file(fid), None);
    }

    #[test]
    fn file_set_hash_links_meta() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("abc").unwrap();
        db.file_set_hash(fid, "abc", mid);
        assert_eq!(db.meta_id_for_file(fid), Some(mid));
    }

    #[test]
    fn files_by_dir_and_all() {
        let db = test_db();
        db.file_insert("/a/1.jpg", "/a", "1.jpg", None, None);
        db.file_insert("/a/2.jpg", "/a", "2.jpg", None, None);
        db.file_insert("/b/3.jpg", "/b", "3.jpg", None, None);

        assert_eq!(db.files_by_dir("/a").len(), 2);
        assert_eq!(db.files_by_dir("/b").len(), 1);
        assert_eq!(db.files_by_dir("/c").len(), 0);
        assert_eq!(db.files_all().len(), 3);
    }

    #[test]
    fn files_dirs_returns_sorted_unique() {
        let db = test_db();
        db.file_insert("/b/1.jpg", "/b", "1.jpg", None, None);
        db.file_insert("/a/2.jpg", "/a", "2.jpg", None, None);
        db.file_insert("/b/3.jpg", "/b", "3.jpg", None, None);
        assert_eq!(db.files_dirs(), vec!["/a", "/b"]);
    }

    #[test]
    fn file_random_returns_something() {
        let db = test_db();
        assert!(db.file_random().is_none());
        db.file_insert("/a/1.jpg", "/a", "1.jpg", None, None);
        assert!(db.file_random().is_some());
    }

    #[test]
    fn file_newest_returns_latest() {
        let db = test_db();
        db.file_insert("/a/old.jpg", "/a", "old.jpg", None, Some("2020-01-01"));
        db.file_insert("/a/new.jpg", "/a", "new.jpg", None, Some("2025-06-01"));
        let f = db.file_newest().unwrap();
        assert_eq!(f.filename, "new.jpg");
    }

    #[test]
    fn file_path_for_meta_works() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("h1").unwrap();
        db.file_set_hash(fid, "h1", mid);
        assert_eq!(db.file_path_for_meta(mid).unwrap(), "/a/f.jpg");
        assert!(db.file_path_for_meta(9999).is_none());
    }

    #[test]
    fn file_path_missing_returns_none() {
        let db = test_db();
        assert!(db.file_path(9999).is_none());
    }

    // -- Meta ----------------------------------------------------------------

    #[test]
    fn meta_upsert_idempotent() {
        let db = test_db();
        let id1 = db.meta_upsert("abc123").unwrap();
        let id2 = db.meta_upsert("abc123").unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn meta_thumb_ready_default_false() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        assert!(!db.meta_thumb_ready(mid));
    }

    #[test]
    fn meta_set_dimensions_marks_ready() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.meta_set_dimensions(mid, 1920, 1080, "jpeg");
        assert!(db.meta_thumb_ready(mid));
    }

    #[test]
    fn meta_set_dimensions_no_overwrite() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.meta_set_dimensions(mid, 1920, 1080, "jpeg");
        db.meta_set_dimensions(mid, 100, 100, "png");
        // Should keep original because "AND width IS NULL"
        let db2 = db.conn();
        let w: i64 = db2
            .query_row("SELECT width FROM meta WHERE id = ?1", [mid], |r| r.get(0))
            .unwrap();
        assert_eq!(w, 1920);
    }

    #[test]
    fn meta_id_for_file_none_when_unlinked() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        assert!(db.meta_id_for_file(fid).is_none());
    }

    #[test]
    fn meta_tags_default_empty() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        assert!(db.meta_get_tags(mid).is_empty());
    }

    #[test]
    fn meta_set_and_get_tags() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        let tags = vec!["like".to_string(), "art".to_string()];
        db.meta_set_tags(mid, &tags);
        let got = db.meta_get_tags(mid);
        assert_eq!(got, tags);
    }

    // -- File Metadata -------------------------------------------------------

    #[test]
    fn file_metadata_returns_full_info() {
        let db = test_db();
        let fid = db
            .file_insert(
                "/a/photo.jpg",
                "/a",
                "photo.jpg",
                Some(4096),
                Some("2025-01-01"),
            )
            .unwrap();
        let mid = db.meta_upsert("h1").unwrap();
        db.file_set_hash(fid, "h1", mid);
        db.meta_set_dimensions(mid, 1920, 1080, "jpeg");
        db.meta_set_tags(mid, &["like".to_string()]);

        let m = db.file_metadata(fid).unwrap();
        assert_eq!(m.file_id, fid);
        assert_eq!(m.filename, "photo.jpg");
        assert_eq!(m.path, "/a/photo.jpg");
        assert_eq!(m.size, Some(4096));
        assert_eq!(m.modified_at.as_deref(), Some("2025-01-01"));
        assert_eq!(m.hash_sha512.as_deref(), Some("h1"));
        assert_eq!(m.meta_id, Some(mid));
        assert_eq!(m.width, Some(1920));
        assert_eq!(m.height, Some(1080));
        assert_eq!(m.format.as_deref(), Some("jpeg"));
        assert_eq!(m.tags, vec!["like"]);
        assert!(m.thumb_ready);
    }

    #[test]
    fn file_metadata_without_meta() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let m = db.file_metadata(fid).unwrap();
        assert_eq!(m.file_id, fid);
        assert!(m.meta_id.is_none());
        assert!(m.width.is_none());
        assert!(m.tags.is_empty());
        assert!(!m.thumb_ready);
    }

    #[test]
    fn file_metadata_missing_returns_none() {
        let db = test_db();
        assert!(db.file_metadata(9999).is_none());
    }

    // -- Thumbs --------------------------------------------------------------

    #[test]
    fn thumb_save_and_get() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        db.thumb_save(mid, "default", &data);
        assert_eq!(db.thumb_get(mid, "default").unwrap(), data);
    }

    #[test]
    fn thumb_get_missing_returns_none() {
        let db = test_db();
        assert!(db.thumb_get(9999, "default").is_none());
    }

    #[test]
    fn thumb_save_overwrites() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.thumb_save(mid, "default", &[1, 2]);
        db.thumb_save(mid, "default", &[3, 4]);
        assert_eq!(db.thumb_get(mid, "default").unwrap(), vec![3, 4]);
    }

    #[test]
    fn thumb_multiple_sizes() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.thumb_save(mid, "default", &[1, 2]);
        db.thumb_save(mid, "shadow", &[9, 8]);
        assert_eq!(db.thumb_get(mid, "default").unwrap(), vec![1, 2]);
        assert_eq!(db.thumb_get(mid, "shadow").unwrap(), vec![9, 8]);
        assert!(db.thumb_get(mid, "nonexistent").is_none());
    }

    #[test]
    fn file_dto_includes_shadow_base64() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("h").unwrap();
        db.file_set_hash(fid, "h", mid);
        // No shadow yet
        let files = db.files_by_dir("/a");
        assert!(files[0].shadow.is_none());
        // Add shadow
        db.thumb_save(mid, "shadow", &[0xFF, 0xAA]);
        let files = db.files_by_dir("/a");
        assert!(files[0]
            .shadow
            .as_ref()
            .unwrap()
            .starts_with("data:image/webp;base64,"));
    }

    // -- Jobs ----------------------------------------------------------------

    #[test]
    fn jobs_enqueue_hash_and_claim() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        let job = db.jobs_claim_next("hash").unwrap();
        assert_eq!(job.file_id, Some(fid));
        assert_eq!(job.job_type, "hash");
        // No more pending
        assert!(db.jobs_claim_next("hash").is_none());
    }

    #[test]
    fn jobs_enqueue_thumb_and_claim() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.jobs_enqueue_thumb(mid, 5);
        let job = db.jobs_claim_next("thumbnail").unwrap();
        assert_eq!(job.meta_id, Some(mid));
        assert_eq!(job.job_type, "thumbnail");
    }

    #[test]
    fn jobs_claim_respects_type() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        assert!(db.jobs_claim_next("thumbnail").is_none());
        assert!(db.jobs_claim_next("hash").is_some());
    }

    #[test]
    fn jobs_mark_done() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        let job = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_done(job.id);
        let s = db.status();
        assert_eq!(s.jobs_done, 1);
        assert_eq!(s.jobs_pending, 0);
    }

    #[test]
    fn jobs_mark_failed_stores_error() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        let job = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_failed(job.id, "boom");
        let s = db.status();
        assert_eq!(s.jobs_failed, 1);
        // Verify error stored
        let err: String = db
            .conn()
            .query_row("SELECT error FROM jobs WHERE id = ?1", [job.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(err, "boom");
    }

    #[test]
    fn jobs_recover_stale_resets_running() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        let _job = db.jobs_claim_next("hash").unwrap(); // now 'running'
        assert!(db.jobs_claim_next("hash").is_none());
        db.jobs_recover_stale();
        // Should be claimable again
        assert!(db.jobs_claim_next("hash").is_some());
    }

    #[test]
    fn jobs_priority_order() {
        let db = test_db();
        let mid1 = db.meta_upsert("h1").unwrap();
        let mid2 = db.meta_upsert("h2").unwrap();
        db.jobs_enqueue_thumb(mid1, 0);
        db.jobs_enqueue_thumb(mid2, 10);
        // Higher priority first
        let job = db.jobs_claim_next("thumbnail").unwrap();
        assert_eq!(job.meta_id, Some(mid2));
    }

    #[test]
    fn jobs_claim_empty_returns_none() {
        let db = test_db();
        assert!(db.jobs_claim_next("hash").is_none());
        assert!(db.jobs_claim_next("thumbnail").is_none());
    }

    // -- Watched -------------------------------------------------------------

    #[test]
    fn watched_add_and_list() {
        let db = test_db();
        db.watched_add("/home/pics");
        db.watched_add("/home/vids");
        let list = db.watched_list_active();
        assert_eq!(list, vec!["/home/pics", "/home/vids"]);
    }

    #[test]
    fn watched_add_duplicate_ignored() {
        let db = test_db();
        db.watched_add("/home/pics");
        db.watched_add("/home/pics");
        assert_eq!(db.watched_list_active().len(), 1);
    }

    #[test]
    fn watched_unwatch_removes_from_active() {
        let db = test_db();
        db.watched_add("/a");
        db.watched_add("/b");
        db.watched_unwatch("/a");
        assert_eq!(db.watched_list_active(), vec!["/b"]);
    }

    #[test]
    fn watched_watch_reactivates() {
        let db = test_db();
        db.watched_add("/a");
        db.watched_unwatch("/a");
        assert!(db.watched_list_active().is_empty());
        db.watched_watch("/a");
        assert_eq!(db.watched_list_active(), vec!["/a"]);
    }

    #[test]
    fn watched_watch_inserts_new() {
        let db = test_db();
        db.watched_watch("/new");
        assert_eq!(db.watched_list_active(), vec!["/new"]);
    }

    // -- History -------------------------------------------------------------

    #[test]
    fn history_record_inserts() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.history_record(fid, "view");
        db.history_record(fid, "like");
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM history WHERE file_id = ?1",
                [fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    // -- Favourites ----------------------------------------------------------

    #[test]
    fn file_random_fav_returns_liked() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("h").unwrap();
        db.file_set_hash(fid, "h", mid);

        // No fav yet
        assert!(db.file_random_fav().is_none());

        // Like it
        db.meta_set_tags(mid, &["like".to_string()]);
        let fav = db.file_random_fav().unwrap();
        assert_eq!(fav.id, fid);
    }

    #[test]
    fn file_latest_fav_returns_most_recent_like() {
        let db = test_db();
        let f1 = db
            .file_insert("/a/1.jpg", "/a", "1.jpg", None, None)
            .unwrap();
        let f2 = db
            .file_insert("/a/2.jpg", "/a", "2.jpg", None, None)
            .unwrap();
        let m1 = db.meta_upsert("h1").unwrap();
        let m2 = db.meta_upsert("h2").unwrap();
        db.file_set_hash(f1, "h1", m1);
        db.file_set_hash(f2, "h2", m2);
        db.meta_set_tags(m1, &["like".to_string()]);
        db.meta_set_tags(m2, &["like".to_string()]);

        db.history_record(f1, "like");
        db.history_record(f2, "like");

        let fav = db.file_latest_fav().unwrap();
        assert_eq!(fav.id, f2);
    }

    #[test]
    fn file_latest_fav_empty_when_no_likes() {
        let db = test_db();
        assert!(db.file_latest_fav().is_none());
    }

    // -- Status --------------------------------------------------------------

    #[test]
    fn status_empty_db() {
        let db = test_db();
        let s = db.status();
        assert_eq!(s.files, 0);
        assert_eq!(s.dirs, 0);
        assert_eq!(s.hashed, 0);
        assert_eq!(s.thumbs, 0);
        assert_eq!(s.watched, 0);
        assert_eq!(s.jobs_pending, 0);
        assert!(s.watched_paths.is_empty());
    }

    #[test]
    fn status_counts_correctly() {
        let db = test_db();
        db.file_insert("/a/1.jpg", "/a", "1.jpg", None, None);
        db.file_insert("/b/2.jpg", "/b", "2.jpg", None, None);
        db.watched_add("/a");
        db.watched_add("/b");
        db.watched_unwatch("/b");

        let fid = db
            .file_insert("/a/3.jpg", "/a", "3.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("hx").unwrap();
        db.file_set_hash(fid, "hx", mid);
        db.thumb_save(mid, "default", &[1]);

        db.jobs_enqueue_hash(1);
        db.jobs_enqueue_hash(2);

        let s = db.status();
        assert_eq!(s.files, 3);
        assert_eq!(s.dirs, 2);
        assert_eq!(s.hashed, 1);
        assert_eq!(s.thumbs, 1);
        assert_eq!(s.watched, 1);
        assert_eq!(s.watched_paths, vec!["/a"]);
        assert_eq!(s.jobs_pending, 2);
    }

    // -- Edge cases ----------------------------------------------------------

    #[test]
    fn file_insert_unicode_path() {
        let db = test_db();
        let id = db.file_insert(
            "/media/фото/画像.jpg",
            "/media/фото",
            "画像.jpg",
            Some(1),
            None,
        );
        assert!(id.is_some());
        assert_eq!(db.file_path(id.unwrap()).unwrap(), "/media/фото/画像.jpg");
    }

    #[test]
    fn file_insert_path_with_spaces_and_parens() {
        let db = test_db();
        let p = "/mnt/c/Users/me/Downloads/Movie (2019) [1080p]/file name (1).mp4";
        let id = db.file_insert(
            p,
            "/mnt/c/Users/me/Downloads/Movie (2019) [1080p]",
            "file name (1).mp4",
            None,
            None,
        );
        assert!(id.is_some());
        assert_eq!(db.file_path(id.unwrap()).unwrap(), p);
    }

    #[test]
    fn file_insert_zero_size() {
        let db = test_db();
        let id = db.file_insert("/a/empty.jpg", "/a", "empty.jpg", Some(0), None);
        assert!(id.is_some());
        let (_, sz, _) = db.file_lookup("/a/empty.jpg").unwrap();
        assert_eq!(sz, Some(0));
    }

    #[test]
    fn file_insert_null_size_and_mtime() {
        let db = test_db();
        let id = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let (_, sz, mt) = db.file_lookup("/a/f.jpg").unwrap();
        assert_eq!(sz, None);
        assert_eq!(mt, None);
        // mark_changed with nulls too
        db.file_mark_changed(id, None, None);
        let (_, sz2, mt2) = db.file_lookup("/a/f.jpg").unwrap();
        assert_eq!(sz2, None);
        assert_eq!(mt2, None);
    }

    #[test]
    fn file_mark_changed_nonexistent_id_is_noop() {
        let db = test_db();
        db.file_mark_changed(99999, Some(1), Some("t"));
        // Should not panic or create rows
        assert_eq!(db.status().files, 0);
    }

    #[test]
    fn file_set_hash_nonexistent_id_is_noop() {
        let db = test_db();
        db.file_set_hash(99999, "hash", 1);
        assert_eq!(db.status().hashed, 0);
    }

    #[test]
    fn file_lookup_changed_size_detected() {
        let db = test_db();
        db.file_insert("/a/f.jpg", "/a", "f.jpg", Some(100), Some("2025-01-01"));
        let (id, sz, mt) = db.file_lookup("/a/f.jpg").unwrap();
        // Simulate re-scan with different size
        let new_size: Option<i64> = Some(200);
        let changed = sz != new_size || mt.as_deref() != Some("2025-01-01");
        assert!(changed);
        db.file_mark_changed(id, new_size, Some("2025-01-01"));
        let (_, sz2, _) = db.file_lookup("/a/f.jpg").unwrap();
        assert_eq!(sz2, Some(200));
    }

    #[test]
    fn file_lookup_changed_mtime_detected() {
        let db = test_db();
        db.file_insert("/a/f.jpg", "/a", "f.jpg", Some(100), Some("2025-01-01"));
        let (_, sz, mt) = db.file_lookup("/a/f.jpg").unwrap();
        let changed = sz != Some(100) || mt.as_deref() != Some("2025-06-01");
        assert!(changed);
    }

    #[test]
    fn file_lookup_unchanged_not_detected() {
        let db = test_db();
        db.file_insert("/a/f.jpg", "/a", "f.jpg", Some(100), Some("2025-01-01"));
        let (_, sz, mt) = db.file_lookup("/a/f.jpg").unwrap();
        let changed = sz != Some(100) || mt.as_deref() != Some("2025-01-01");
        assert!(!changed);
    }

    #[test]
    fn files_by_dir_sorted_by_path() {
        let db = test_db();
        db.file_insert("/d/c.jpg", "/d", "c.jpg", None, None);
        db.file_insert("/d/a.jpg", "/d", "a.jpg", None, None);
        db.file_insert("/d/b.jpg", "/d", "b.jpg", None, None);
        let paths: Vec<String> = db
            .files_by_dir("/d")
            .iter()
            .map(|f| f.path.clone())
            .collect();
        assert_eq!(paths, vec!["/d/a.jpg", "/d/b.jpg", "/d/c.jpg"]);
    }

    #[test]
    fn files_all_sorted_by_full_path() {
        let db = test_db();
        db.file_insert("/b/z.jpg", "/b", "z.jpg", None, None);
        db.file_insert("/a/y.jpg", "/a", "y.jpg", None, None);
        db.file_insert("/a/x.jpg", "/a", "x.jpg", None, None);
        let paths: Vec<String> = db.files_all().iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, vec!["/a/x.jpg", "/a/y.jpg", "/b/z.jpg"]);
    }

    #[test]
    fn files_all_same_dir_files_grouped_together() {
        let db = test_db();
        db.file_insert("/z/img.jpg", "/z", "img.jpg", None, None);
        db.file_insert("/a/b/one.jpg", "/a/b", "one.jpg", None, None);
        db.file_insert("/a/b/two.jpg", "/a/b", "two.jpg", None, None);
        db.file_insert("/m/pic.jpg", "/m", "pic.jpg", None, None);
        let all = db.files_all();
        let dirs: Vec<&str> = all.iter().map(|f| f.dir.as_str()).collect();
        // Same-dir files must be adjacent
        assert_eq!(dirs, vec!["/a/b", "/a/b", "/m", "/z"]);
    }

    #[test]
    fn file_newest_empty_db_returns_none() {
        let db = test_db();
        assert!(db.file_newest().is_none());
    }

    #[test]
    fn file_random_fav_not_found_without_tag() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("h").unwrap();
        db.file_set_hash(fid, "h", mid);
        // Tags set but not "like"
        db.meta_set_tags(mid, &["art".into(), "nature".into()]);
        assert!(db.file_random_fav().is_none());
    }

    #[test]
    fn meta_upsert_different_hashes_get_different_ids() {
        let db = test_db();
        let id1 = db.meta_upsert("hash_a").unwrap();
        let id2 = db.meta_upsert("hash_b").unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn meta_thumb_ready_nonexistent_returns_false() {
        let db = test_db();
        assert!(!db.meta_thumb_ready(99999));
    }

    #[test]
    fn meta_get_tags_nonexistent_returns_empty() {
        let db = test_db();
        assert!(db.meta_get_tags(99999).is_empty());
    }

    #[test]
    fn meta_set_tags_empty_array() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.meta_set_tags(mid, &["like".into()]);
        assert!(!db.meta_get_tags(mid).is_empty());
        db.meta_set_tags(mid, &[]);
        assert!(db.meta_get_tags(mid).is_empty());
    }

    #[test]
    fn meta_set_tags_with_special_chars() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        let tags = vec!["like".into(), "it's \"great\"".into(), "日本語".into()];
        db.meta_set_tags(mid, &tags);
        assert_eq!(db.meta_get_tags(mid), tags);
    }

    #[test]
    fn meta_id_for_file_nonexistent_file() {
        let db = test_db();
        assert!(db.meta_id_for_file(99999).is_none());
    }

    #[test]
    fn thumb_save_large_blob() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        let big = vec![0xABu8; 1024 * 1024]; // 1 MB
        db.thumb_save(mid, "default", &big);
        let got = db.thumb_get(mid, "default").unwrap();
        assert_eq!(got.len(), 1024 * 1024);
        assert!(got.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn thumb_save_empty_blob() {
        let db = test_db();
        let mid = db.meta_upsert("h").unwrap();
        db.thumb_save(mid, "default", &[]);
        assert_eq!(db.thumb_get(mid, "default").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn jobs_mark_done_nonexistent_is_noop() {
        let db = test_db();
        db.jobs_mark_done(99999); // should not panic
    }

    #[test]
    fn jobs_mark_failed_nonexistent_is_noop() {
        let db = test_db();
        db.jobs_mark_failed(99999, "err"); // should not panic
    }

    #[test]
    fn jobs_multiple_hash_jobs_fifo_within_same_priority() {
        let db = test_db();
        let f1 = db
            .file_insert("/a/1.jpg", "/a", "1.jpg", None, None)
            .unwrap();
        let f2 = db
            .file_insert("/a/2.jpg", "/a", "2.jpg", None, None)
            .unwrap();
        let f3 = db
            .file_insert("/a/3.jpg", "/a", "3.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(f1);
        db.jobs_enqueue_hash(f2);
        db.jobs_enqueue_hash(f3);
        // Same priority → FIFO by id
        assert_eq!(db.jobs_claim_next("hash").unwrap().file_id, Some(f1));
        assert_eq!(db.jobs_claim_next("hash").unwrap().file_id, Some(f2));
        assert_eq!(db.jobs_claim_next("hash").unwrap().file_id, Some(f3));
        assert!(db.jobs_claim_next("hash").is_none());
    }

    #[test]
    fn jobs_recover_stale_no_running_is_noop() {
        let db = test_db();
        db.jobs_recover_stale(); // no jobs at all — no panic
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        db.jobs_recover_stale(); // pending job, not running — no change
        assert!(db.jobs_claim_next("hash").is_some());
    }

    #[test]
    fn jobs_done_and_failed_not_reclaimable() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(fid);
        let j = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_done(j.id);
        // Done jobs not reclaimable
        assert!(db.jobs_claim_next("hash").is_none());
        // Same for failed
        db.jobs_enqueue_hash(fid);
        let j2 = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_failed(j2.id, "err");
        assert!(db.jobs_claim_next("hash").is_none());
    }

    #[test]
    fn watched_unwatch_nonexistent_is_noop() {
        let db = test_db();
        db.watched_unwatch("/does/not/exist"); // no panic
        assert!(db.watched_list_active().is_empty());
    }

    #[test]
    fn watched_list_active_excludes_inactive() {
        let db = test_db();
        db.watched_add("/a");
        db.watched_add("/b");
        db.watched_add("/c");
        db.watched_unwatch("/b");
        let list = db.watched_list_active();
        assert_eq!(list, vec!["/a", "/c"]);
        assert!(!list.contains(&"/b".to_string()));
    }

    #[test]
    fn watched_list_active_sorted() {
        let db = test_db();
        db.watched_add("/z");
        db.watched_add("/a");
        db.watched_add("/m");
        assert_eq!(db.watched_list_active(), vec!["/a", "/m", "/z"]);
    }

    #[test]
    fn history_multiple_actions_same_file() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        for _ in 0..100 {
            db.history_record(fid, "view");
        }
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM history WHERE file_id = ?1",
                [fid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 100);
    }

    #[test]
    fn status_with_mixed_job_states() {
        let db = test_db();
        let f1 = db
            .file_insert("/a/1.jpg", "/a", "1.jpg", None, None)
            .unwrap();
        let f2 = db
            .file_insert("/a/2.jpg", "/a", "2.jpg", None, None)
            .unwrap();
        let f3 = db
            .file_insert("/a/3.jpg", "/a", "3.jpg", None, None)
            .unwrap();
        let f4 = db
            .file_insert("/a/4.jpg", "/a", "4.jpg", None, None)
            .unwrap();
        db.jobs_enqueue_hash(f1);
        db.jobs_enqueue_hash(f2);
        db.jobs_enqueue_hash(f3);
        db.jobs_enqueue_hash(f4);
        let _j1 = db.jobs_claim_next("hash").unwrap(); // running
        let j2 = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_done(j2.id);
        let j3 = db.jobs_claim_next("hash").unwrap();
        db.jobs_mark_failed(j3.id, "oops");
        // f4 still pending
        let s = db.status();
        assert_eq!(s.jobs_running, 1); // j1
        assert_eq!(s.jobs_done, 1); // j2
        assert_eq!(s.jobs_failed, 1); // j3
        assert_eq!(s.jobs_pending, 1); // f4
                                       // recover stale should reset running
        db.jobs_recover_stale();
        let s2 = db.status();
        assert_eq!(s2.jobs_running, 0);
        assert_eq!(s2.jobs_pending, 2); // j1 + f4
    }

    #[test]
    fn two_files_same_hash_share_meta() {
        let db = test_db();
        let f1 = db
            .file_insert("/a/dup1.jpg", "/a", "dup1.jpg", None, None)
            .unwrap();
        let f2 = db
            .file_insert("/b/dup2.jpg", "/b", "dup2.jpg", None, None)
            .unwrap();
        let mid = db.meta_upsert("same_hash").unwrap();
        db.file_set_hash(f1, "same_hash", mid);
        db.file_set_hash(f2, "same_hash", mid);
        assert_eq!(db.meta_id_for_file(f1), Some(mid));
        assert_eq!(db.meta_id_for_file(f2), Some(mid));
        // file_path_for_meta returns one of them
        let path = db.file_path_for_meta(mid).unwrap();
        assert!(path == "/a/dup1.jpg" || path == "/b/dup2.jpg");
    }

    #[test]
    fn file_dto_thumb_ready_reflects_meta() {
        let db = test_db();
        let fid = db
            .file_insert("/a/f.jpg", "/a", "f.jpg", None, None)
            .unwrap();
        // Before linking meta — thumb_ready should be false
        let files = db.files_by_dir("/a");
        assert!(!files[0].thumb_ready);
        // Link meta + set dimensions (marks thumb_ready=1)
        let mid = db.meta_upsert("h").unwrap();
        db.file_set_hash(fid, "h", mid);
        db.meta_set_dimensions(mid, 100, 100, "jpeg");
        let files2 = db.files_by_dir("/a");
        assert!(files2[0].thumb_ready);
    }

    // -- Full workflow -------------------------------------------------------

    #[test]
    fn full_hash_workflow() {
        let db = test_db();
        // 1. Insert file
        let fid = db
            .file_insert(
                "/pics/photo.jpg",
                "/pics",
                "photo.jpg",
                Some(4096),
                Some("2025-01-01"),
            )
            .unwrap();
        // 2. Enqueue hash job
        db.jobs_enqueue_hash(fid);
        // 3. Claim job
        let job = db.jobs_claim_next("hash").unwrap();
        assert_eq!(job.file_id, Some(fid));
        // 4. "Hash" the file
        let mid = db.meta_upsert("sha512:abc").unwrap();
        db.file_set_hash(fid, "sha512:abc", mid);
        // 5. Mark done
        db.jobs_mark_done(job.id);
        // 6. Not thumb ready yet
        assert!(!db.meta_thumb_ready(mid));
        // 7. Enqueue thumb
        db.jobs_enqueue_thumb(mid, 0);
        let tjob = db.jobs_claim_next("thumbnail").unwrap();
        // 8. Generate thumb
        db.meta_set_dimensions(mid, 4000, 3000, "jpeg");
        db.thumb_save(mid, "default", &[0xFF, 0xD8]);
        db.jobs_mark_done(tjob.id);
        // 9. Verify everything
        assert!(db.meta_thumb_ready(mid));
        assert_eq!(db.thumb_get(mid, "default").unwrap(), vec![0xFF, 0xD8]);
        assert_eq!(db.file_path(fid).unwrap(), "/pics/photo.jpg");
        assert_eq!(db.file_path_for_meta(mid).unwrap(), "/pics/photo.jpg");
        let s = db.status();
        assert_eq!(s.hashed, 1);
        assert_eq!(s.thumbs, 1);
        assert_eq!(s.jobs_done, 2);
    }

    // -- Performance ---------------------------------------------------------

    fn seed_files(db: &Db, n: usize) {
        let conn = db.conn();
        conn.execute_batch("BEGIN").unwrap();
        for i in 0..n {
            let dir = format!("/d{}", i / 1000);
            let filename = format!("f{}.jpg", i);
            let path = format!("{}/{}", dir, filename);
            conn.execute(
                "INSERT INTO files (path, dir, filename, size) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![path, dir, filename, 1024i64],
            )
            .unwrap();
        }
        conn.execute_batch("COMMIT").unwrap();
    }

    #[test]
    fn perf_files_first_dir() {
        let db = test_db();
        seed_files(&db, 100_000);
        let t0 = std::time::Instant::now();
        let dir = db.files_first_dir();
        let elapsed = t0.elapsed();
        assert!(dir.is_some());
        assert!(
            elapsed.as_millis() < 50,
            "files_first_dir took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_files_by_dir_1k() {
        let db = test_db();
        seed_files(&db, 100_000);
        // dir /d0 has files 0..999 = 1000 files
        let t0 = std::time::Instant::now();
        let files = db.files_by_dir("/d0");
        let elapsed = t0.elapsed();
        assert_eq!(files.len(), 1000);
        assert!(
            elapsed.as_millis() < 100,
            "files_by_dir(1k) took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_files_dirs_100k() {
        let db = test_db();
        seed_files(&db, 100_000);
        let t0 = std::time::Instant::now();
        let dirs = db.files_dirs();
        let elapsed = t0.elapsed();
        assert_eq!(dirs.len(), 100); // 100k files / 1000 per dir
        assert!(
            elapsed.as_millis() < 200,
            "files_dirs took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_status_100k() {
        let db = test_db();
        seed_files(&db, 100_000);
        let t0 = std::time::Instant::now();
        let s = db.status();
        let elapsed = t0.elapsed();
        assert_eq!(s.files, 100_000);
        assert!(
            elapsed.as_millis() < 200,
            "status took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_thumb_get_concurrent_simulation() {
        let db = test_db();
        // Create 100 thumbs
        for i in 0..100 {
            let mid = db.meta_upsert(&format!("h{}", i)).unwrap();
            db.thumb_save(mid, "default", &vec![0xABu8; 4096]);
        }
        // Simulate 1000 random thumb reads (what happens when scrolling fast)
        let t0 = std::time::Instant::now();
        for i in 0..1000 {
            let mid = (i % 100) + 1;
            let _ = db.thumb_get(mid, "default");
        }
        let elapsed = t0.elapsed();
        assert!(
            elapsed.as_millis() < 200,
            "1000 thumb reads took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_file_random_100k() {
        let db = test_db();
        seed_files(&db, 100_000);
        // ORDER BY RANDOM() is O(n) per call — single call should be fast enough
        let t0 = std::time::Instant::now();
        let f = db.file_random();
        let elapsed = t0.elapsed();
        assert!(f.is_some());
        assert!(
            elapsed.as_millis() < 200,
            "single random lookup took {}ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn perf_bulk_insert_10k() {
        let db = test_db();
        let t0 = std::time::Instant::now();
        seed_files(&db, 10_000);
        let elapsed = t0.elapsed();
        assert_eq!(db.status().files, 10_000);
        assert!(
            elapsed.as_millis() < 1000,
            "10k insert took {}ms",
            elapsed.as_millis()
        );
    }

    // -- jobs_boost tests ----------------------------------------------------

    #[test]
    fn jobs_boost_prioritizes_matching_jobs() {
        let db = test_db();
        // Insert 3 files with hash jobs
        for i in 1..=3 {
            db.file_insert(
                &format!("/d/f{}.jpg", i),
                "/d",
                &format!("f{}.jpg", i),
                Some(100),
                None,
            );
        }
        db.jobs_enqueue_hash(1);
        db.jobs_enqueue_hash(2);
        db.jobs_enqueue_hash(3);

        // Boost file 3 only
        db.jobs_boost(&[3], &[]);

        // First claimed should be file 3 (priority 10 > 0)
        let j1 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j1.file_id, Some(3));
        // Next two are unordered priority=0, but by id ASC
        let j2 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j2.file_id, Some(1));
        let j3 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j3.file_id, Some(2));
    }

    #[test]
    fn jobs_boost_resets_previous_boosts() {
        let db = test_db();
        for i in 1..=3 {
            db.file_insert(
                &format!("/d/f{}.jpg", i),
                "/d",
                &format!("f{}.jpg", i),
                Some(100),
                None,
            );
        }
        db.jobs_enqueue_hash(1);
        db.jobs_enqueue_hash(2);
        db.jobs_enqueue_hash(3);

        // Boost file 1
        db.jobs_boost(&[1], &[]);
        // Now boost file 3 — should reset file 1 back to 0
        db.jobs_boost(&[3], &[]);

        let j1 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j1.file_id, Some(3), "file 3 should be first after re-boost");
    }

    #[test]
    fn jobs_boost_with_meta_ids() {
        let db = test_db();
        let mid1 = db.meta_upsert("hash_a").unwrap();
        let mid2 = db.meta_upsert("hash_b").unwrap();
        db.jobs_enqueue_thumb(mid1, 0);
        db.jobs_enqueue_thumb(mid2, 0);

        // Boost mid2
        db.jobs_boost(&[], &[mid2]);

        let j1 = db.jobs_claim_next("thumbnail").unwrap();
        assert_eq!(j1.meta_id, Some(mid2));
        let j2 = db.jobs_claim_next("thumbnail").unwrap();
        assert_eq!(j2.meta_id, Some(mid1));
    }

    #[test]
    fn jobs_boost_empty_ids_just_resets() {
        let db = test_db();
        db.file_insert("/d/f1.jpg", "/d", "f1.jpg", Some(100), None);
        db.jobs_enqueue_hash(1);

        // Boost file 1
        db.jobs_boost(&[1], &[]);
        // Reset all boosts
        db.jobs_boost(&[], &[]);

        // Job should still be claimable (priority reset to 0, not deleted)
        let j = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j.file_id, Some(1));
    }

    #[test]
    fn jobs_boost_rapid_navigation_simulation() {
        let db = test_db();
        // Simulate 50 files with hash jobs (user navigates through them)
        for i in 1..=50 {
            db.file_insert(
                &format!("/d/f{}.jpg", i),
                "/d",
                &format!("f{}.jpg", i),
                Some(100),
                None,
            );
            db.jobs_enqueue_hash(i);
        }

        // Simulate rapid random navigation: boost different files each time
        // Only the last boost should matter
        db.jobs_boost(&[10, 11, 12], &[]);
        db.jobs_boost(&[30, 31, 32], &[]);
        db.jobs_boost(&[45, 46, 47], &[]); // final view position

        // First 3 claimed should be files 45, 46, 47
        let j1 = db.jobs_claim_next("hash").unwrap();
        let j2 = db.jobs_claim_next("hash").unwrap();
        let j3 = db.jobs_claim_next("hash").unwrap();
        let boosted: Vec<i64> = vec![
            j1.file_id.unwrap(),
            j2.file_id.unwrap(),
            j3.file_id.unwrap(),
        ];
        assert!(boosted.contains(&45));
        assert!(boosted.contains(&46));
        assert!(boosted.contains(&47));

        // Remaining jobs still claimable (background work continues)
        let mut remaining = 0;
        while db.jobs_claim_next("hash").is_some() {
            remaining += 1;
        }
        assert_eq!(remaining, 47); // 50 - 3 already claimed
    }

    #[test]
    fn jobs_boost_does_not_affect_running_jobs() {
        let db = test_db();
        db.file_insert("/d/f1.jpg", "/d", "f1.jpg", Some(100), None);
        db.file_insert("/d/f2.jpg", "/d", "f2.jpg", Some(100), None);
        db.jobs_enqueue_hash(1);
        db.jobs_enqueue_hash(2);

        // Claim job for file 1 (now running)
        let j1 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j1.file_id, Some(1));

        // Boost file 2 — should not affect the running job for file 1
        db.jobs_boost(&[2], &[]);

        // File 1's job is still running, not re-queued
        let j2 = db.jobs_claim_next("hash").unwrap();
        assert_eq!(j2.file_id, Some(2));
        assert!(db.jobs_claim_next("hash").is_none()); // no more pending
    }
}
