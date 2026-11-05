// Claude — Date 11/03/2026
// Local state: one SQLite file per device, in the app's data folder (the CLI
// keeps its own under DBX_HOME). Apart from local changes not pushed yet,
// nothing in it is the only copy of anything. Losing it costs unfinished
// uploads, which start over, and the materialized index, which a fresh device
// rebuilds from the op logs in the store.
//
// What it holds:
// - The upload queue (M1). A row in `uploads` is an upload that hasn't written
//   its manifest yet; `upload_chunks` records each chunk swiftSearch has
//   acknowledged, which is what lets an upload resume after a quit or a crash.
// - The index (M2). `entries` holds the merged state of every file and folder
//   this device has heard of (see oplog.rs), and `blobs` every blob an applied
//   op named, which garbage collection works from.
// - Sync state. `meta` holds the device id, the clock and counters. `outbox`
//   holds local ops not in a batch yet, `batches` holds sealed batches not
//   pushed yet, and `peers` records how far each other device's log is applied.
//
// rusqlite is synchronous. Every call here is a few small statements, so they
// run directly on the calling task under a mutex, which is never held across an
// `.await`. Anything that changes more than one row runs in one IMMEDIATE
// transaction, so a crash leaves either all of it or none.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::blob::{Dest, Manifest};
use crate::error::Error;
use crate::oplog::{BATCH_VERSION, Batch, Body, Entry, Hlc, Op, View};
use crate::remote::Kind;

/// Most ops in one batch. An op is a few hundred bytes of JSON, so this keeps
/// every log far below swiftSearch's 64 MiB value limit, even after deleting
/// a folder with a hundred thousand files in it.
const MAX_BATCH_OPS: usize = 5000;

pub struct Db {
    conn: Mutex<Connection>,
}

/// This device, as its ops name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
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
    /// Where the blob goes in the index once it's uploaded.
    pub dest: Dest,
}

/// A sealed batch of local ops, waiting to be pushed. `body` is its exact JSON,
/// fixed when it was sealed, so a retried push sends the same bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unpushed {
    pub seq: u64,
    pub body: String,
    pub ops: usize,
}

/// How far this device has applied another device's log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Peer {
    /// The last seq applied; 0 for none.
    pub applied: u64,
    /// The ETag of that device's `head.json` when its log was last caught up
    /// with, for a conditional GET next time.
    pub head_etag: Option<String>,
}

/// What this device has published: its last pushed seq and its last checkpoint
/// (0 for none). Its `head.json` says the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Published {
    pub seq: u64,
    pub ckpt: u64,
}

/// The whole index at one moment, for writing a checkpoint.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub hlc: Hlc,
    /// For each device, the last seq of its log that `entries` includes.
    pub vector: BTreeMap<String, u64>,
    /// Every entry, placeholders included, by id.
    pub entries: Vec<Entry>,
    pub published: Published,
    /// Ops applied since the last checkpoint, as of this snapshot.
    pub ops: u64,
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
    // 2: the index and sync state. `dest` is a JSON `Dest`; uploads from
    // before it existed were for the blob alone.
    r#"ALTER TABLE uploads ADD COLUMN dest TEXT NOT NULL DEFAULT '{"to":"blob"}';
     CREATE TABLE meta (
         k TEXT PRIMARY KEY,
         v TEXT NOT NULL
     );
     CREATE TABLE entries (
         id    TEXT PRIMARY KEY,
         state TEXT NOT NULL
     );
     CREATE TABLE blobs (
         blob       TEXT PRIMARY KEY,
         entry      TEXT NOT NULL,
         garbage_ms INTEGER,
         gone       INTEGER NOT NULL DEFAULT 0
     );
     CREATE INDEX blobs_entry ON blobs(entry);
     CREATE TABLE outbox (
         n  INTEGER PRIMARY KEY AUTOINCREMENT,
         op TEXT NOT NULL
     );
     CREATE TABLE batches (
         seq  INTEGER PRIMARY KEY,
         body TEXT NOT NULL,
         ops  INTEGER NOT NULL
     );
     CREATE TABLE peers (
         device    TEXT PRIMARY KEY,
         applied   INTEGER NOT NULL DEFAULT 0,
         head_etag TEXT
     );"#,
];

const PENDING_COLUMNS: &str = "u.blob, u.path, u.size, u.mtime_ns, u.chunk_size, u.mime, u.kind,
     u.started_ms, (SELECT COUNT(*) FROM upload_chunks c WHERE c.blob = u.blob), u.dest";

// Keys in `meta`.
const DEVICE: &str = "device";
const DEVICE_NAME: &str = "device_name";
/// The HLC of the latest op this device has stamped or seen.
const HLC: &str = "hlc";
/// The seq of the last batch sealed from the outbox.
const SEALED: &str = "sealed";
/// The seq of the last batch pushed, and so the seq in `head.json`.
const PUSHED: &str = "pushed";
/// The number of the last checkpoint written.
const CKPT: &str = "ckpt";
/// Ops applied, local or remote, since the last checkpoint.
const SINCE_CKPT: &str = "since_ckpt";

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

    /// Run `f` in one write transaction: all of it lands, or none of it.
    fn write<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T, Error>) -> Result<T, Error> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    // ---- upload queue ----

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
            "INSERT INTO uploads (blob, path, size, mtime_ns, chunk_size, mime, kind, started_ms, dest)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                u.blob,
                path_text(&u.path),
                u.size as i64,
                u.mtime_ns,
                u.chunk_size,
                u.mime,
                u.kind.as_str(),
                u.started_ms,
                to_json(&u.dest)
            ],
        )?;
        Ok(())
    }

    /// Point an unfinished upload somewhere else in the index: the same file,
    /// put again with a different destination, resumes with the new one.
    pub(crate) fn set_upload_dest(&self, blob: &str, dest: &Dest) -> Result<(), Error> {
        self.conn()
            .execute("UPDATE uploads SET dest = ?2 WHERE blob = ?1", params![blob, to_json(dest)])?;
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

    /// Forget an abandoned upload, along with its chunk records.
    pub(crate) fn drop_upload(&self, blob: &str) -> Result<(), Error> {
        self.conn().execute("DELETE FROM uploads WHERE blob = ?1", [blob])?;
        Ok(())
    }

    /// An upload's last step, once its manifest is in the store: forget the
    /// upload and, unless it was for the blob alone, record the op that puts the
    /// blob in the index, in one transaction. Returns the entry's id.
    ///
    /// A file replaced here while another device deleted it stays deleted, as
    /// any concurrent edit and delete do; its new blob is collected as garbage.
    pub(crate) fn finish_upload(&self, job: &PendingUpload, m: &Manifest) -> Result<Option<String>, Error> {
        self.write(|tx| {
            tx.execute("DELETE FROM uploads WHERE blob = ?1", [&job.blob])?;
            let (blob, size, mime, sha256) = (m.blob.clone(), m.size, m.mime.clone(), m.sha256.clone());
            let (id, body) = match &job.dest {
                Dest::Blob => return Ok(None),
                Dest::New { parent, name } => {
                    let body = Body::File { name: name.clone(), parent: parent.clone(), blob, size, mime, sha256 };
                    (new_id(), body)
                }
                Dest::Replace { id } => (id.clone(), Body::Content { blob, size, mime, sha256 }),
            };
            commit_in(tx, vec![(id.clone(), body)])?;
            Ok(Some(id))
        })
    }

    // ---- this device ----

    pub fn device(&self) -> Result<Option<Device>, Error> {
        let conn = self.conn();
        let Some(id) = meta(&conn, DEVICE)? else { return Ok(None) };
        let name = meta(&conn, DEVICE_NAME)?.unwrap_or_default();
        Ok(Some(Device { id, name }))
    }

    /// Give this local state a device id, once. Calling it again returns the
    /// device already set up, under its original name.
    pub fn init_device(&self, name: &str) -> Result<Device, Error> {
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) || name.len() > 100 {
            return Err(Error::Invalid(format!("{name:?} can't be a device name")));
        }
        self.write(|tx| {
            if let Some(id) = meta(tx, DEVICE)? {
                let name = meta(tx, DEVICE_NAME)?.unwrap_or_default();
                return Ok(Device { id, name });
            }
            let device = Device { id: new_id(), name: name.to_owned() };
            set_meta(tx, DEVICE, &device.id)?;
            set_meta(tx, DEVICE_NAME, &device.name)?;
            Ok(device)
        })
    }

    // ---- the index ----

    /// Make local changes: stamp each with the next clock reading, apply it to
    /// the index and queue it for the next push, all in one transaction.
    pub fn commit(&self, changes: Vec<(String, Body)>) -> Result<Vec<Op>, Error> {
        self.write(|tx| commit_in(tx, changes))
    }

    /// One entry's merged state, placeholder or deleted ones included.
    pub fn entry(&self, id: &str) -> Result<Option<Entry>, Error> {
        load_entry(&self.conn(), id)
    }

    /// Every entry, placeholders and deleted ones included, by id.
    pub fn entries(&self) -> Result<Vec<Entry>, Error> {
        entries_in(&self.conn())
    }

    /// The folder tree as it stands on this device.
    pub fn view(&self) -> Result<View, Error> {
        Ok(View::new(self.entries()?))
    }

    // ---- sync: this device's log ----

    /// Seal every queued local op into batches, ready to push. Returns how many
    /// batches it sealed.
    pub fn seal_outbox(&self) -> Result<usize, Error> {
        self.write(|tx| {
            let device = device_id(tx)?;
            let queued: Vec<(i64, String)> = tx
                .prepare("SELECT n, op FROM outbox ORDER BY n")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            let Some(&(last, _)) = queued.last() else { return Ok(0) };
            let mut seq = meta_u64(tx, SEALED)?;
            let mut sealed = 0;
            for group in queued.chunks(MAX_BATCH_OPS) {
                let ops = group.iter().map(|(_, op)| from_json(op)).collect::<Result<Vec<Op>, _>>()?;
                seq += 1;
                let batch = Batch { v: BATCH_VERSION, device: device.clone(), seq, ops };
                tx.execute(
                    "INSERT INTO batches (seq, body, ops) VALUES (?1, ?2, ?3)",
                    params![seq as i64, to_json(&batch), group.len() as i64],
                )?;
                sealed += 1;
            }
            tx.execute("DELETE FROM outbox WHERE n <= ?1", [last])?;
            set_meta(tx, SEALED, &seq.to_string())?;
            Ok(sealed)
        })
    }

    /// Sealed batches not pushed yet, oldest first.
    pub fn unpushed(&self) -> Result<Vec<Unpushed>, Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT seq, body, ops FROM batches ORDER BY seq")?;
        let rows = stmt.query_map([], |r| {
            Ok(Unpushed { seq: r.get::<_, i64>(0)? as u64, body: r.get(1)?, ops: r.get::<_, i64>(2)? as usize })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Changes made here that the store doesn't have yet: ops still in the
    /// outbox, and ops in batches sealed but not pushed. What "3 changes
    /// waiting" counts while the store can't be reached.
    pub fn pending_changes(&self) -> Result<u64, Error> {
        let conn = self.conn();
        let n: i64 = conn.query_row(
            "SELECT (SELECT COUNT(*) FROM outbox) + (SELECT COALESCE(SUM(ops), 0) FROM batches)",
            [],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// Every batch up to `seq` is in the store and `head.json` says so.
    pub fn mark_pushed(&self, seq: u64) -> Result<(), Error> {
        self.write(|tx| {
            tx.execute("DELETE FROM batches WHERE seq <= ?1", [seq as i64])?;
            let pushed = meta_u64(tx, PUSHED)?.max(seq);
            set_meta(tx, PUSHED, &pushed.to_string())?;
            Ok(())
        })
    }

    pub fn published(&self) -> Result<Published, Error> {
        let conn = self.conn();
        Ok(Published { seq: meta_u64(&conn, PUSHED)?, ckpt: meta_u64(&conn, CKPT)? })
    }

    // ---- sync: other devices' logs ----

    pub fn peer(&self, device: &str) -> Result<Peer, Error> {
        peer_in(&self.conn(), device)
    }

    /// Every other device this one has applied ops from.
    pub fn peers(&self) -> Result<Vec<(String, Peer)>, Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT device, applied, head_etag FROM peers ORDER BY device")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, Peer { applied: r.get::<_, i64>(1)? as u64, head_etag: r.get(2)? }))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_head_etag(&self, device: &str, etag: &str) -> Result<(), Error> {
        self.conn().execute(
            "INSERT INTO peers (device, head_etag) VALUES (?1, ?2)
             ON CONFLICT(device) DO UPDATE SET head_etag = excluded.head_etag",
            params![device, etag],
        )?;
        Ok(())
    }

    /// Apply another device's batch, along with moving that device's cursor,
    /// in one transaction. Batches must come in seq order: one already applied
    /// is skipped (returns 0), and one that skips ahead is refused. Returns the
    /// number of ops applied.
    pub fn apply_batch(&self, batch: &Batch) -> Result<usize, Error> {
        self.write(|tx| {
            if batch.device == device_id(tx)? {
                return Err(Error::Invalid(format!("batch {} is this device's own", batch.seq)));
            }
            let applied = peer_in(tx, &batch.device)?.applied;
            if batch.seq <= applied {
                return Ok(0);
            }
            if batch.seq != applied + 1 {
                return Err(Error::Invalid(format!(
                    "batch {} of device {} is out of order: {} comes next",
                    batch.seq,
                    batch.device,
                    applied + 1
                )));
            }
            let mut clock = hlc_in(tx)?;
            let mut touched = HashSet::new();
            for op in &batch.ops {
                clock = clock.observe(op.t);
                apply_in(tx, op, &batch.device)?;
                touched.insert(op.id.clone());
            }
            set_meta(tx, HLC, &to_json(&clock))?;
            tx.execute(
                "INSERT INTO peers (device, applied) VALUES (?1, ?2)
                 ON CONFLICT(device) DO UPDATE SET applied = excluded.applied",
                params![batch.device, batch.seq as i64],
            )?;
            add_since_ckpt(tx, batch.ops.len() as u64)?;
            schedule_gc(tx, &touched, now_ms())?;
            Ok(batch.ops.len())
        })
    }

    /// Join a checkpoint into the index: its entries merge like any other
    /// state, and each device's cursor moves up to what the checkpoint already
    /// includes, so those logs needn't be replayed.
    pub fn merge_checkpoint(&self, hlc: Hlc, vector: &BTreeMap<String, u64>, entries: Vec<Entry>) -> Result<(), Error> {
        self.write(|tx| {
            let me = device_id(tx)?;
            let mut clock = hlc_in(tx)?.observe(hlc);
            let mut touched = HashSet::new();
            for e in entries {
                if let Some(t) = e.latest_stamp() {
                    clock = clock.observe(t);
                }
                if let Some(c) = &e.content {
                    name_blob(tx, &c.value.blob, &e.id)?;
                }
                touched.insert(e.id.clone());
                merge_in(tx, e)?;
            }
            for (device, &seq) in vector {
                if *device != me {
                    tx.execute(
                        "INSERT INTO peers (device, applied) VALUES (?1, ?2)
                         ON CONFLICT(device) DO UPDATE SET applied = max(applied, excluded.applied)",
                        params![device, seq as i64],
                    )?;
                }
            }
            set_meta(tx, HLC, &to_json(&clock))?;
            schedule_gc(tx, &touched, now_ms())?;
            Ok(())
        })
    }

    // ---- sync: checkpoints ----

    /// Whether `every` ops have been applied since the last checkpoint.
    pub fn checkpoint_due(&self, every: u64) -> Result<bool, Error> {
        Ok(every > 0 && meta_u64(&self.conn(), SINCE_CKPT)? >= every)
    }

    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let mut conn = self.conn();
        // A read transaction, so every part comes from the same moment.
        let tx = conn.transaction()?;
        let pushed = meta_u64(&tx, PUSHED)?;
        let mut vector: BTreeMap<String, u64> = tx
            .prepare("SELECT device, applied FROM peers WHERE applied > 0")?
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<Result<_, _>>()?;
        // The entries may also hold local ops not pushed yet. A reader that
        // later applies them again from the log changes nothing.
        if pushed > 0 {
            vector.insert(device_id(&tx)?, pushed);
        }
        Ok(Snapshot {
            hlc: hlc_in(&tx)?,
            vector,
            entries: entries_in(&tx)?,
            published: Published { seq: pushed, ckpt: meta_u64(&tx, CKPT)? },
            ops: meta_u64(&tx, SINCE_CKPT)?,
        })
    }

    /// Checkpoint `n`, made from a snapshot taken when `ops` ops had been
    /// applied since the one before, is in the store and in `head.json`.
    pub fn checkpoint_written(&self, n: u64, ops: u64) -> Result<(), Error> {
        self.write(|tx| {
            set_meta(tx, CKPT, &n.to_string())?;
            let since = meta_u64(tx, SINCE_CKPT)?.saturating_sub(ops);
            set_meta(tx, SINCE_CKPT, &since.to_string())?;
            Ok(())
        })
    }

    // ---- garbage collection ----

    /// Blobs that became garbage at or before `cutoff_ms` and are still in the
    /// store, oldest first.
    pub fn gc_due(&self, cutoff_ms: u64) -> Result<Vec<String>, Error> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT blob FROM blobs WHERE garbage_ms <= ?1 AND gone = 0 ORDER BY garbage_ms, blob",
        )?;
        let cutoff = i64::try_from(cutoff_ms).unwrap_or(i64::MAX);
        let rows = stmt.query_map([cutoff], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// A garbage blob is deleted from the store.
    pub fn gc_done(&self, blob: &str) -> Result<(), Error> {
        self.conn().execute("UPDATE blobs SET gone = 1 WHERE blob = ?1", [blob])?;
        Ok(())
    }
}

// ---- inside a transaction ----

fn commit_in(tx: &Connection, changes: Vec<(String, Body)>) -> Result<Vec<Op>, Error> {
    let device = device_id(tx)?;
    let now = now_ms();
    let mut clock = hlc_in(tx)?;
    let mut touched = HashSet::new();
    let mut ops = Vec::with_capacity(changes.len());
    let mut queue = tx.prepare_cached("INSERT INTO outbox (op) VALUES (?1)")?;
    for (id, body) in changes {
        clock = clock.tick(now);
        let op = Op { t: clock, id, body };
        apply_in(tx, &op, &device)?;
        queue.execute([to_json(&op)])?;
        touched.insert(op.id.clone());
        ops.push(op);
    }
    set_meta(tx, HLC, &to_json(&clock))?;
    add_since_ckpt(tx, ops.len() as u64)?;
    schedule_gc(tx, &touched, now)?;
    Ok(ops)
}

fn apply_in(conn: &Connection, op: &Op, device: &str) -> Result<(), Error> {
    merge_in(conn, Entry::from_op(op, device))?;
    if let Some(blob) = op.body.blob() {
        name_blob(conn, blob, &op.id)?;
    }
    Ok(())
}

fn merge_in(conn: &Connection, incoming: Entry) -> Result<(), Error> {
    let mut e = load_entry(conn, &incoming.id)?.unwrap_or_else(|| Entry::new(&incoming.id));
    e.merge(incoming);
    conn.prepare_cached(
        "INSERT INTO entries (id, state) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET state = excluded.state",
    )?
    .execute(params![e.id, to_json(&e)])?;
    Ok(())
}

fn name_blob(conn: &Connection, blob: &str, entry: &str) -> Result<(), Error> {
    conn.prepare_cached("INSERT OR IGNORE INTO blobs (blob, entry) VALUES (?1, ?2)")?
        .execute(params![blob, entry])?;
    Ok(())
}

/// Mark every blob of these entries that can never be shown again. A blob is
/// named by exactly one op, the one its uploader made, so once a later write
/// replaces it, or the entry is deleted (which is sticky), nothing can make it
/// current again. A placeholder's blobs are left alone: its creating op hasn't
/// arrived yet, and its content may turn out to be current.
fn schedule_gc(conn: &Connection, ids: &HashSet<String>, now_ms: u64) -> Result<(), Error> {
    let mut mark = conn.prepare_cached(
        "UPDATE blobs SET garbage_ms = ?3 WHERE entry = ?1 AND garbage_ms IS NULL AND blob IS NOT ?2",
    )?;
    for id in ids {
        let Some(e) = load_entry(conn, id)? else { continue };
        if e.created.is_none() && e.deleted.is_none() {
            continue;
        }
        let current = if e.is_live() { e.content().map(|c| c.blob.clone()) } else { None };
        mark.execute(params![id, current, now_ms as i64])?;
    }
    Ok(())
}

fn load_entry(conn: &Connection, id: &str) -> Result<Option<Entry>, Error> {
    let state: Option<String> = conn
        .prepare_cached("SELECT state FROM entries WHERE id = ?1")?
        .query_row([id], |r| r.get(0))
        .optional()?;
    state.map(|s| from_json(&s)).transpose()
}

fn entries_in(conn: &Connection) -> Result<Vec<Entry>, Error> {
    let mut stmt = conn.prepare("SELECT state FROM entries ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|s| from_json(&s?)).collect()
}

fn peer_in(conn: &Connection, device: &str) -> Result<Peer, Error> {
    let peer = conn
        .query_row("SELECT applied, head_etag FROM peers WHERE device = ?1", [device], |r| {
            Ok(Peer { applied: r.get::<_, i64>(0)? as u64, head_etag: r.get(1)? })
        })
        .optional()?;
    Ok(peer.unwrap_or_default())
}

fn meta(conn: &Connection, k: &str) -> Result<Option<String>, Error> {
    Ok(conn.query_row("SELECT v FROM meta WHERE k = ?1", [k], |r| r.get(0)).optional()?)
}

fn set_meta(conn: &Connection, k: &str, v: &str) -> Result<(), Error> {
    conn.execute(
        "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![k, v],
    )?;
    Ok(())
}

fn meta_u64(conn: &Connection, k: &str) -> Result<u64, Error> {
    match meta(conn, k)? {
        None => Ok(0),
        Some(v) => v.parse().map_err(|_| bad_row(format!("meta {k} is {v:?}, not a number"))),
    }
}

fn add_since_ckpt(conn: &Connection, n: u64) -> Result<(), Error> {
    let since = meta_u64(conn, SINCE_CKPT)? + n;
    set_meta(conn, SINCE_CKPT, &since.to_string())
}

fn device_id(conn: &Connection) -> Result<String, Error> {
    meta(conn, DEVICE)?.ok_or(Error::NoDevice)
}

fn hlc_in(conn: &Connection) -> Result<Hlc, Error> {
    meta(conn, HLC)?.map_or(Ok(Hlc::default()), |v| from_json(&v))
}

// Paths go in as text, so a file whose name isn't valid UTF-8 is matched by
// its lossy form. Two such files differing only in the invalid bytes would look
// alike to resume, which then notices the size or mtime differ and starts over.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn pending_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<PendingUpload> {
    let dest: String = r.get(9)?;
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
        dest: serde_json::from_str(&dest)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, Type::Text, Box::new(e)))?,
    })
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("index types always serialize")
}

fn from_json<T: DeserializeOwned>(s: &str) -> Result<T, Error> {
    serde_json::from_str(s).map_err(|e| bad_row(e.to_string()))
}

// A row this code wrote but can't read back: reported as a local-state error.
fn bad_row(why: String) -> Error {
    Error::Db(rusqlite::Error::FromSqlConversionFailure(0, Type::Text, why.into()))
}

/// A fresh id for an entry, device or blob. v7, so ids sort by age.
pub(crate) fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
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
            dest: Dest::Blob,
        }
    }

    fn device() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.init_device("test").unwrap();
        db
    }

    fn file(name: &str, blob: &str) -> Body {
        Body::File {
            name: name.into(),
            parent: None,
            blob: blob.into(),
            size: 1,
            mime: "text/plain".into(),
            sha256: "0".repeat(64),
        }
    }

    fn content(blob: &str) -> Body {
        Body::Content { blob: blob.into(), size: 2, mime: "text/plain".into(), sha256: "1".repeat(64) }
    }

    fn batch(device: &str, seq: u64, ops: Vec<Op>) -> Batch {
        Batch { v: BATCH_VERSION, device: device.into(), seq, ops }
    }

    fn op(ms: u64, id: &str, body: Body) -> Op {
        Op { t: Hlc(ms, 0), id: id.into(), body }
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

        let dest = Dest::New { parent: None, name: "a.mp4".into() };
        db.set_upload_dest("b1", &dest).unwrap();
        assert_eq!(db.pending_upload("b1").unwrap().unwrap().dest, dest);

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
        let db = Db::open(&path).unwrap();
        db.start_upload(&upload("b1", "/a", 1)).unwrap();
        let me = db.init_device("laptop").unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.pending_uploads().unwrap().len(), 1);
        assert_eq!(db.device().unwrap(), Some(me.clone()));
        assert_eq!(db.init_device("other name").unwrap(), me, "set up once");
    }

    #[test]
    fn no_device_no_changes() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.device().unwrap().is_none());
        assert!(matches!(db.commit(vec![("e".into(), Body::Delete)]), Err(Error::NoDevice)));
        assert!(matches!(db.init_device("  "), Err(Error::Invalid(_))));
    }

    #[test]
    fn local_changes_seal_into_batches() {
        let db = device();
        let me = db.device().unwrap().unwrap();
        let ops = db.commit(vec![("f".into(), file("a", "b1")), ("f".into(), Body::Rename { name: "b".into() })]).unwrap();
        assert!(ops[0].t < ops[1].t, "each op gets a later stamp");
        assert_eq!(db.view().unwrap().get("f").unwrap().name(), "b");

        assert_eq!(db.pending_changes().unwrap(), 2, "two ops in the outbox");
        assert_eq!(db.seal_outbox().unwrap(), 1);
        assert_eq!(db.pending_changes().unwrap(), 2, "now in a sealed batch, still not pushed");
        assert_eq!(db.seal_outbox().unwrap(), 0, "nothing new queued");
        db.commit(vec![("f".into(), Body::Delete)]).unwrap();
        assert_eq!(db.seal_outbox().unwrap(), 1);
        assert_eq!(db.pending_changes().unwrap(), 3);

        let unpushed = db.unpushed().unwrap();
        assert_eq!(unpushed.iter().map(|u| (u.seq, u.ops)).collect::<Vec<_>>(), [(1, 2), (2, 1)]);
        let first: Batch = serde_json::from_str(&unpushed[0].body).unwrap();
        assert_eq!((first.device, first.seq, first.ops), (me.id, 1, ops));

        db.mark_pushed(1).unwrap();
        assert_eq!(db.unpushed().unwrap().len(), 1);
        db.mark_pushed(2).unwrap();
        assert!(db.unpushed().unwrap().is_empty());
        assert_eq!(db.pending_changes().unwrap(), 0);
        assert_eq!(db.published().unwrap(), Published { seq: 2, ckpt: 0 });
    }

    #[test]
    fn remote_batches_apply_in_order() {
        let db = device();
        let far = now_ms() + 1_000_000;
        let first = batch("dev-x", 1, vec![op(far, "d", Body::Folder { name: "x".into(), parent: None })]);
        let second = batch("dev-x", 2, vec![op(far + 1, "d", Body::Rename { name: "y".into() })]);
        assert!(matches!(db.apply_batch(&second), Err(Error::Invalid(_))), "1 comes first");
        assert_eq!(db.apply_batch(&first).unwrap(), 1);
        assert_eq!(db.apply_batch(&first).unwrap(), 0, "already applied");
        assert_eq!(db.apply_batch(&second).unwrap(), 1);
        assert_eq!(db.peer("dev-x").unwrap().applied, 2);
        assert_eq!(db.view().unwrap().get("d").unwrap().name(), "y");

        // The clock has seen dev-x's stamps, so a local edit now beats them.
        db.commit(vec![("d".into(), Body::Rename { name: "mine".into() })]).unwrap();
        assert_eq!(db.view().unwrap().get("d").unwrap().name(), "mine");

        let me = db.device().unwrap().unwrap().id;
        assert!(db.apply_batch(&batch(&me, 1, vec![])).is_err(), "never its own");
    }

    #[test]
    fn replaced_and_deleted_blobs_become_garbage() {
        let db = device();
        db.commit(vec![("f".into(), file("a", "b1"))]).unwrap();
        assert!(db.gc_due(u64::MAX).unwrap().is_empty());
        db.commit(vec![("f".into(), content("b2"))]).unwrap();
        assert_eq!(db.gc_due(u64::MAX).unwrap(), ["b1"]);
        assert!(db.gc_due(0).unwrap().is_empty(), "not before the cutoff");
        db.gc_done("b1").unwrap();
        db.commit(vec![("f".into(), Body::Delete)]).unwrap();
        assert_eq!(db.gc_due(u64::MAX).unwrap(), ["b2"]);
    }

    #[test]
    fn placeholders_keep_their_blobs() {
        let db = device();
        // Content for an entry whose creating op hasn't arrived.
        db.apply_batch(&batch("dev-x", 1, vec![op(5, "f", content("b9"))])).unwrap();
        assert!(db.gc_due(u64::MAX).unwrap().is_empty());
        // The create, older than the content: the content is current.
        db.apply_batch(&batch("dev-y", 1, vec![op(1, "f", file("a", "b1"))])).unwrap();
        assert_eq!(db.gc_due(u64::MAX).unwrap(), ["b1"]);
        assert_eq!(db.view().unwrap().get("f").unwrap().content().unwrap().blob, "b9");
    }

    #[test]
    fn checkpoints_move_cursors_and_merge() {
        let db = device();
        let me = db.device().unwrap().unwrap().id;
        db.apply_batch(&batch("dev-x", 1, vec![op(1, "a", Body::Folder { name: "a".into(), parent: None })])).unwrap();

        let entry = Entry::from_op(&op(2, "b", Body::Folder { name: "b".into(), parent: None }), "dev-y");
        let vector = BTreeMap::from([("dev-x".to_owned(), 7), ("dev-y".to_owned(), 3), (me.clone(), 4)]);
        db.merge_checkpoint(Hlc(9, 0), &vector, vec![entry]).unwrap();
        assert_eq!(db.peer("dev-x").unwrap().applied, 7);
        assert_eq!(db.peer("dev-y").unwrap().applied, 3);
        assert_eq!(db.peer(&me).unwrap().applied, 0, "its own log isn't a peer");
        assert_eq!(db.view().unwrap().len(), 2);

        // Two ops applied so far: dev-x's folder and this one. A merged
        // checkpoint doesn't count.
        db.commit(vec![("c".into(), Body::Folder { name: "c".into(), parent: None })]).unwrap();
        assert!(db.checkpoint_due(2).unwrap());
        assert!(!db.checkpoint_due(3).unwrap());
        db.seal_outbox().unwrap();
        db.mark_pushed(1).unwrap();
        let snap = db.snapshot().unwrap();
        assert_eq!(snap.entries.len(), 3);
        assert_eq!(snap.vector, BTreeMap::from([("dev-x".to_owned(), 7), ("dev-y".to_owned(), 3), (me, 1)]));
        assert!(snap.hlc > Hlc(9, 0));
        db.checkpoint_written(1, snap.ops).unwrap();
        assert!(!db.checkpoint_due(1).unwrap());
        assert_eq!(db.published().unwrap(), Published { seq: 1, ckpt: 1 });
    }
}
