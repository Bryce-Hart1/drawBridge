// Claude — Date 11/03/2026
// Local state: one SQLite file per device, in the app's data folder (the CLI
// keeps its own under DBX_HOME). Nothing in it is the only copy of anything in
// the store. Losing it costs unfinished uploads, which start over, and (from
// M2) the materialized index, which is rebuilt from the op logs.
//
// So far it holds the upload queue. A row in `uploads` is an upload that hasn't
// written its manifest yet; `upload_chunks` records each chunk swiftSearch has
// acknowledged, which is what lets an upload resume after a quit or a crash.
//
// rusqlite is synchronous. Every call here is a few small statements, so they
// run directly on the calling task under a mutex, which is never held across an
// `.await`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::Error;
use crate::remote::Kind;

pub struct Db {
    conn: Mutex<Connection>,
}

/// An upload that has started but not finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingUpload {
    pub blob: String,
    pub path: PathBuf,
    pub size: u64,
    /// The file's modification time when the upload started, in nanoseconds
    /// since the epoch. With `size`, it tells a resumed upload whether the file
    /// is still the one it was uploading.
    pub mtime_ns: i64,
    pub chunk_size: u32,
    pub mime: String,
    pub kind: Kind,
    pub started_ms: i64,
    pub chunks_done: u32,
}

/// Schema versions, applied in order and recorded in `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    // 1: the upload queue.
    "CREATE TABLE uploads (
         blob       TEXT PRIMARY KEY,
         path       TEXT NOT NULL,
         size       INTEGER NOT NULL,
         mtime_ns   INTEGER NOT NULL,
         chunk_size INTEGER NOT NULL,
         mime       TEXT NOT NULL,
         kind       TEXT NOT NULL,
         started_ms INTEGER NOT NULL
     );
     CREATE INDEX uploads_path ON uploads(path);
     CREATE TABLE upload_chunks (
         blob   TEXT NOT NULL REFERENCES uploads(blob) ON DELETE CASCADE,
         idx    INTEGER NOT NULL,
         sha256 TEXT NOT NULL,
         PRIMARY KEY (blob, idx)
     );",
];

const PENDING_COLUMNS: &str = "u.blob, u.path, u.size, u.mtime_ns, u.chunk_size, u.mime, u.kind,
     u.started_ms, (SELECT COUNT(*) FROM upload_chunks c WHERE c.blob = u.blob)";

impl Db {
    pub fn open(path: &Path) -> Result<Db, Error> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Db::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Db, Error> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db, Error> {
        // In memory this answers "memory" instead; either is fine.
        let _mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // The CLI and the app can share one file; wait out the other's write.
        conn.busy_timeout(Duration::from_secs(5))?;

        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)?;
            tx.commit()?;
        }
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic mid-statement leaves nothing half-done in SQLite (the
        // transaction rolls back), so a poisoned lock is safe to keep using.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn pending_uploads(&self) -> Result<Vec<PendingUpload>, Error> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!("SELECT {PENDING_COLUMNS} FROM uploads u ORDER BY u.started_ms"))?;
        let rows = stmt.query_map([], pending_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn pending_upload(&self, blob: &str) -> Result<Option<PendingUpload>, Error> {
        let conn = self.conn();
        let sql = format!("SELECT {PENDING_COLUMNS} FROM uploads u WHERE u.blob = ?1");
        Ok(conn.query_row(&sql, [blob], pending_row).optional()?)
    }

    pub(crate) fn pending_for_path(&self, path: &Path) -> Result<Option<PendingUpload>, Error> {
        let conn = self.conn();
        let sql = format!(
            "SELECT {PENDING_COLUMNS} FROM uploads u WHERE u.path = ?1
             ORDER BY u.started_ms DESC LIMIT 1"
        );
        Ok(conn.query_row(&sql, [path_text(path)], pending_row).optional()?)
    }

    pub(crate) fn start_upload(&self, u: &PendingUpload) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO uploads (blob, path, size, mtime_ns, chunk_size, mime, kind, started_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                u.blob,
                path_text(&u.path),
                u.size as i64,
                u.mtime_ns,
                u.chunk_size,
                u.mime,
                u.kind.as_str(),
                u.started_ms
            ],
        )?;
        Ok(())
    }

    /// Chunk index → sha256 of every chunk swiftSearch has acknowledged.
    pub(crate) fn uploaded_chunks(&self, blob: &str) -> Result<HashMap<u32, String>, Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT idx, sha256 FROM upload_chunks WHERE blob = ?1")?;
        let rows = stmt.query_map([blob], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub(crate) fn mark_chunk(&self, blob: &str, idx: u32, sha256: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT OR REPLACE INTO upload_chunks (blob, idx, sha256) VALUES (?1, ?2, ?3)",
            params![blob, idx, sha256],
        )?;
        Ok(())
    }

    /// Forget an upload, finished or abandoned, along with its chunk records.
    pub(crate) fn drop_upload(&self, blob: &str) -> Result<(), Error> {
        self.conn().execute("DELETE FROM uploads WHERE blob = ?1", [blob])?;
        Ok(())
    }
}

// Paths go in as text, so a file whose name isn't valid UTF-8 is matched by
// its lossy form. Two such files differing only in the invalid bytes would look
// alike to resume, which then notices the size or mtime differ and starts over.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn pending_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PendingUpload> {
    Ok(PendingUpload {
        blob: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        size: r.get::<_, i64>(2)? as u64,
        mtime_ns: r.get(3)?,
        chunk_size: r.get(4)?,
        mime: r.get(5)?,
        kind: Kind::parse(&r.get::<_, String>(6)?),
        started_ms: r.get(7)?,
        chunks_done: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload(blob: &str, path: &str, started_ms: i64) -> PendingUpload {
        PendingUpload {
            blob: blob.into(),
            path: path.into(),
            size: 100,
            mtime_ns: 7,
            chunk_size: 64,
            mime: "video/mp4".into(),
            kind: Kind::Video,
            started_ms,
            chunks_done: 0,
        }
    }

    #[test]
    fn upload_queue() {
        let db = Db::open_in_memory().unwrap();
        db.start_upload(&upload("b1", "/a.mp4", 1)).unwrap();
        db.start_upload(&upload("b2", "/b.mp4", 2)).unwrap();
        db.mark_chunk("b1", 0, "aa").unwrap();
        db.mark_chunk("b1", 1, "bb").unwrap();
        db.mark_chunk("b1", 1, "bb").unwrap();

        let pending = db.pending_uploads().unwrap();
        assert_eq!(pending.iter().map(|p| &p.blob[..]).collect::<Vec<_>>(), ["b1", "b2"]);
        assert_eq!(pending[0].chunks_done, 2);
        assert_eq!(pending[0], PendingUpload { chunks_done: 2, ..upload("b1", "/a.mp4", 1) });

        let found = db.pending_for_path(Path::new("/a.mp4")).unwrap().unwrap();
        assert_eq!(found.blob, "b1");
        assert!(db.pending_for_path(Path::new("/c.mp4")).unwrap().is_none());

        let chunks = db.uploaded_chunks("b1").unwrap();
        assert_eq!(chunks, HashMap::from([(0, "aa".into()), (1, "bb".into())]));

        db.drop_upload("b1").unwrap();
        assert!(db.pending_upload("b1").unwrap().is_none());
        assert!(db.uploaded_chunks("b1").unwrap().is_empty(), "chunk rows cascade");
    }

    #[test]
    fn reopens_without_migrating_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/drawbridge.sqlite");
        Db::open(&path).unwrap().start_upload(&upload("b1", "/a", 1)).unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(db.pending_uploads().unwrap().len(), 1);
    }
}
