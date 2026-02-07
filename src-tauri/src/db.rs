use anyhow::Result;
use directories::ProjectDirs;
use rusqlite::Connection;
use std::path::PathBuf;

pub fn default_db_path() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from("dev", "lv", "lv") {
        let data = dirs.data_dir();
        std::fs::create_dir_all(data).ok();
        data.join("lv.db")
    } else {
        PathBuf::from("lv.db")
    }
}

pub fn open(path: &PathBuf) -> Result<Connection> {
    use crate::debug::dbg_log;
    dbg_log!("opening db: {}", path.display());
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode = WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrate(&conn)?;
    dbg_log!("db ready (WAL, FK on)");
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS files (
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

        CREATE TABLE IF NOT EXISTS thumbs (
            meta_id       INTEGER NOT NULL REFERENCES meta(id),
            size_tag      TEXT NOT NULL DEFAULT 'default',
            webp_data     BLOB NOT NULL,
            created_at    TEXT DEFAULT (datetime('now')),
            PRIMARY KEY (meta_id, size_tag)
        );

        CREATE TABLE IF NOT EXISTS history (
            id            INTEGER PRIMARY KEY,
            file_id       INTEGER REFERENCES files(id),
            action        TEXT NOT NULL,
            created_at    TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS watched (
            id            INTEGER PRIMARY KEY,
            path          TEXT NOT NULL UNIQUE,
            active        INTEGER DEFAULT 1,
            created_at    TEXT DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS jobs (
            id            INTEGER PRIMARY KEY,
            job_type      TEXT NOT NULL,
            file_id       INTEGER,
            meta_id       INTEGER,
            status        TEXT DEFAULT 'pending',
            priority      INTEGER DEFAULT 0,
            error         TEXT,
            created_at    TEXT DEFAULT (datetime('now')),
            updated_at    TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_files_dir ON files(dir);
        CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash_sha512);
        CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status, priority DESC);
        ",
    )?;

    // Incremental migrations for existing databases
    let has_size_tag: bool = conn.prepare("SELECT size_tag FROM thumbs LIMIT 0").is_ok();
    if !has_size_tag {
        // Old thumbs table had meta_id as PK without size_tag.
        // Recreate with composite PK.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS thumbs_new (
                meta_id   INTEGER NOT NULL REFERENCES meta(id),
                size_tag  TEXT NOT NULL DEFAULT 'default',
                webp_data BLOB NOT NULL,
                created_at TEXT DEFAULT (datetime('now')),
                PRIMARY KEY (meta_id, size_tag)
             );
             INSERT OR IGNORE INTO thumbs_new (meta_id, size_tag, webp_data, created_at)
                SELECT meta_id, 'default', webp_data, created_at FROM thumbs;
             DROP TABLE thumbs;
             ALTER TABLE thumbs_new RENAME TO thumbs;",
        )?;
    }

    Ok(())
}
