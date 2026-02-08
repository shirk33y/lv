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
    pub meta_id: Option<i64>,
    pub liked: bool,
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
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
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
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
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
                        (COALESCE(m.tags, '[]') LIKE '%\"like\"%')
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
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1
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
                "SELECT f.id, f.path, f.dir, f.filename, f.meta_id, 1
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
                    COALESCE(m.tags, '[]')
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
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<FileEntry> {
    Ok(FileEntry {
        id: row.get(0)?,
        path: row.get(1)?,
        dir: row.get(2)?,
        filename: row.get(3)?,
        meta_id: row.get(4)?,
        liked: row.get::<_, i64>(5)? != 0,
    })
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
                 modified_at TEXT DEFAULT ''
             );
             CREATE TABLE history (
                 id INTEGER PRIMARY KEY,
                 file_id INTEGER NOT NULL,
                 action TEXT NOT NULL
             );",
        )
        .unwrap();
        Db(Arc::new(Mutex::new(conn)))
    }

    fn insert_file(db: &Db, id: i64, path: &str, dir: &str, filename: &str) {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO meta (id, tags) VALUES (?1, '[]')",
            [id],
        )
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
}
