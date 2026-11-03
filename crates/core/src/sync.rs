// Claude — Date 11/03/2026
// Sync: how devices share the index through the store, with no server of
// their own. Each device writes only the keys under its own `dev/<device>/`,
// plus its line in devices.json, so swiftSearch never needs a compare-and-swap.
//
// One sync, in order:
// 1. Push. Seal queued local ops into batches, PUT each as `log/<seq>.json`,
//    then PUT `head.json` with the new seq. A reader that sees a head can
//    always find every log it counts. After a crash in between, the next sync
//    sends the same bytes again.
// 2. Register. Make sure devices.json lists this device. Two devices that
//    register at the same moment can each overwrite the other's line; the one
//    left out adds itself back on its next sync.
// 3. Pull. For every other device, a conditional GET of its head (a 304 when
//    nothing changed), then its new logs, applied in order. A device seen for
//    the first time is loaded from its latest checkpoint, if it has one,
//    instead of from log 1.
// 4. Checkpoint. Every `checkpoint_every` ops, write the whole index as
//    `ckpt/<n>.json` and point the head at it.
// 5. Collect. Delete blobs that no entry can show any more, once `gc_grace`
//    has passed. db.rs (`schedule_gc`) says why that is safe.
//
// Run one sync at a time per Db.

use std::collections::BTreeMap;
use std::time::Duration;

use bytes::Bytes;
use futures_util::stream::{self, StreamExt};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::blob;
use crate::db::{self, Db, Device};
use crate::error::Error;
use crate::keys;
use crate::oplog::{BATCH_VERSION, Batch, Entry, Hlc};
use crate::remote::{self, Conditional, Kind, Remote};

/// The version of `head.json`, `devices.json` and checkpoints.
pub const FORMAT_VERSION: u32 = 1;

/// swiftSearch's largest value.
const MAX_VALUE: usize = 64 << 20;

/// `dev/<device>/head.json`: how far that device's log goes, and its latest
/// checkpoint (0 for none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub v: u32,
    pub seq: u64,
    pub ckpt: u64,
}

/// `devices.json`: every device that has synced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Devices {
    pub v: u32,
    pub devices: Vec<Registered>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registered {
    pub id: String,
    pub name: String,
    pub added_ms: u64,
}

/// `dev/<device>/ckpt/<n>.json`: the whole index as that device had it, and
/// how far into each device's log that goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub v: u32,
    pub device: String,
    pub n: u64,
    pub hlc: Hlc,
    pub vector: BTreeMap<String, u64>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug)]
pub struct SyncOptions {
    /// Write a checkpoint once this many ops, local or pulled, have been
    /// applied since the last one. 0 never writes one.
    pub checkpoint_every: u64,
    /// How long a blob stays in the store after nothing shows it any more, so
    /// a player still streaming it, or a device that hasn't synced, isn't cut
    /// off at once.
    pub gc_grace: Duration,
    /// Log fetches in flight at once.
    pub parallel: usize,
}

impl Default for SyncOptions {
    fn default() -> SyncOptions {
        SyncOptions { checkpoint_every: 500, gc_grace: Duration::from_secs(600), parallel: 4 }
    }
}

/// What one sync did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Local ops pushed.
    pub pushed: usize,
    /// Ops pulled from other devices and applied.
    pub pulled: usize,
    /// Other devices in devices.json.
    pub devices: usize,
    /// Other devices loaded from a checkpoint instead of from their first log.
    pub bootstrapped: usize,
    /// devices.json didn't list this device, and now does.
    pub registered: bool,
    /// The checkpoint written, if one was due.
    pub checkpoint: Option<u64>,
    /// Blobs deleted from the store as garbage.
    pub collected: usize,
    /// The key can read but not write. This device follows the others but has
    /// nothing to share, so it doesn't register, checkpoint or collect.
    pub read_only: bool,
}

pub async fn sync(remote: &Remote, db: &Db, opts: &SyncOptions) -> Result<Report, Error> {
    let me = db.device()?.ok_or(Error::NoDevice)?;
    let mut report = Report { pushed: push(remote, db, &me.id).await?, ..Report::default() };
    let registry = register(remote, &me, &mut report).await?;
    pull(remote, db, &me.id, &registry, opts.parallel, &mut report).await?;
    if !report.read_only {
        report.checkpoint = checkpoint(remote, db, &me.id, opts.checkpoint_every).await?;
        report.collected = collect(remote, db, opts.gc_grace).await?;
    }
    Ok(report)
}

/// Every device in devices.json.
pub async fn devices(remote: &Remote) -> Result<Vec<Registered>, Error> {
    Ok(read_registry(remote).await?.devices)
}

async fn push(remote: &Remote, db: &Db, me: &str) -> Result<usize, Error> {
    db.seal_outbox()?;
    let batches = db.unpushed()?;
    let Some(last) = batches.last().map(|b| b.seq) else { return Ok(0) };
    for b in &batches {
        remote.put(&keys::log(me, b.seq)?, Kind::Str, Bytes::from(b.body.clone())).await?;
    }
    let head = Head { v: FORMAT_VERSION, seq: last, ckpt: db.published()?.ckpt };
    remote.put(&keys::head(me)?, Kind::Str, json(&head)).await?;
    db.mark_pushed(last)?;
    Ok(batches.iter().map(|b| b.ops).sum())
}

async fn read_registry(remote: &Remote) -> Result<Devices, Error> {
    let Some(value) = remote.get(&keys::devices()).await? else {
        return Ok(Devices { v: FORMAT_VERSION, devices: Vec::new() });
    };
    let registry: Devices = parse(&value.bytes, "devices.json")?;
    // Writing back an older format would drop whatever the newer one added.
    check_version(registry.v, "devices.json")?;
    Ok(registry)
}

async fn register(remote: &Remote, me: &Device, report: &mut Report) -> Result<Devices, Error> {
    let mut registry = read_registry(remote).await?;
    if registry.devices.iter().any(|d| d.id == me.id) {
        return Ok(registry);
    }
    registry.devices.push(Registered { id: me.id.clone(), name: me.name.clone(), added_ms: db::now_ms() });
    match remote.put(&keys::devices(), Kind::Str, json(&registry)).await {
        Ok(_) => report.registered = true,
        Err(remote::Error::Forbidden) => report.read_only = true,
        Err(e) => return Err(e.into()),
    }
    Ok(registry)
}

async fn pull(
    remote: &Remote,
    db: &Db,
    me: &str,
    registry: &Devices,
    parallel: usize,
    report: &mut Report,
) -> Result<(), Error> {
    // Every head first, so devices seen for the first time are loaded from
    // checkpoints before any log is replayed: one checkpoint can cover the
    // logs of several devices.
    let mut heads = Vec::new();
    for d in &registry.devices {
        // An id that can't be a key segment was never written by drawBridge.
        let Ok(key) = keys::head(&d.id) else { continue };
        if d.id == me {
            continue;
        }
        report.devices += 1;
        let answer = match db.peer(&d.id)?.head_etag {
            Some(tag) => remote.get_if_changed(&key, &tag).await?,
            None => remote.get(&key).await?.map_or(Conditional::Missing, Conditional::Changed),
        };
        // Unchanged: caught up already. Missing: registered, nothing pushed yet.
        let Conditional::Changed(value) = answer else { continue };
        let what = format!("head of device {}", d.id);
        let head: Head = parse(&value.bytes, &what)?;
        check_version(head.v, &what)?;
        heads.push((d.id.as_str(), head, value.meta.etag));
    }
    for (device, head, _) in &heads {
        if head.ckpt > 0 && db.peer(device)?.applied == 0 && bootstrap(remote, db, device, head.ckpt).await? {
            report.bootstrapped += 1;
        }
    }
    for (device, head, etag) in heads {
        report.pulled += replay(remote, db, device, head.seq, parallel).await?;
        // Only once caught up: until then the head must be fetched again.
        db.set_head_etag(device, &etag)?;
    }
    Ok(())
}

/// Load another device's checkpoint `n`. False if it's gone, which happens
/// when the device wrote a newer one after its head was read; its logs are
/// replayed instead.
async fn bootstrap(remote: &Remote, db: &Db, device: &str, n: u64) -> Result<bool, Error> {
    let Some(value) = remote.get(&keys::checkpoint(device, n)?).await? else { return Ok(false) };
    let what = format!("checkpoint {n} of device {device}");
    let cp: Checkpoint = parse(&value.bytes, &what)?;
    check_version(cp.v, &what)?;
    if cp.device != device || cp.n != n {
        return Err(Error::Corrupt(format!("{what} says it is checkpoint {} of device {}", cp.n, cp.device)));
    }
    db.merge_checkpoint(cp.hlc, &cp.vector, cp.entries)?;
    Ok(true)
}

/// Apply a device's logs after its cursor, through `seq`, in order, with up to
/// `parallel` fetches in flight. Returns the number of ops applied.
async fn replay(remote: &Remote, db: &Db, device: &str, seq: u64, parallel: usize) -> Result<usize, Error> {
    let from = db.peer(device)?.applied + 1;
    let mut logs = stream::iter(from..=seq)
        .map(|seq| fetch_log(remote, device, seq))
        .buffered(parallel.clamp(1, 16));
    let mut applied = 0;
    while let Some(batch) = logs.next().await {
        applied += db.apply_batch(&batch?)?;
    }
    Ok(applied)
}

async fn fetch_log(remote: &Remote, device: &str, seq: u64) -> Result<Batch, Error> {
    let what = format!("log {seq} of device {device}");
    let value = remote
        .get(&keys::log(device, seq)?)
        .await?
        .ok_or_else(|| Error::Corrupt(format!("{what} is missing, though its head counts it")))?;
    let batch: Batch = parse(&value.bytes, &what)?;
    if batch.v != BATCH_VERSION {
        return Err(Error::Corrupt(format!("{what} is format v{}, this version reads v{BATCH_VERSION}", batch.v)));
    }
    if batch.device != device || batch.seq != seq {
        return Err(Error::Corrupt(format!("{what} says it is log {} of device {}", batch.seq, batch.device)));
    }
    Ok(batch)
}

async fn checkpoint(remote: &Remote, db: &Db, me: &str, every: u64) -> Result<Option<u64>, Error> {
    if !db.checkpoint_due(every)? {
        return Ok(None);
    }
    let snap = db.snapshot()?;
    let n = snap.published.ckpt + 1;
    let cp = Checkpoint {
        v: FORMAT_VERSION,
        device: me.to_owned(),
        n,
        hlc: snap.hlc,
        vector: snap.vector,
        entries: snap.entries,
    };
    let body = json(&cp);
    if body.len() > MAX_VALUE {
        // Too big for one value. New devices replay the logs instead; reset
        // the count so the next try comes after another `every` ops.
        db.checkpoint_written(snap.published.ckpt, snap.ops)?;
        return Ok(None);
    }
    remote.put(&keys::checkpoint(me, n)?, Kind::Str, body).await?;
    let head = Head { v: FORMAT_VERSION, seq: snap.published.seq, ckpt: n };
    remote.put(&keys::head(me)?, Kind::Str, json(&head)).await?;
    db.checkpoint_written(n, snap.ops)?;
    if n > 1 {
        // A device that read the old head finds this gone and replays logs.
        remote.delete(&keys::checkpoint(me, n - 1)?).await?;
    }
    Ok(Some(n))
}

async fn collect(remote: &Remote, db: &Db, grace: Duration) -> Result<usize, Error> {
    let cutoff = db::now_ms().saturating_sub(grace.as_millis() as u64);
    let due = db.gc_due(cutoff)?;
    for id in &due {
        match blob::delete(remote, id).await {
            // An id that can't be a key was never stored, so there's nothing to delete.
            Ok(()) | Err(Error::Key(_)) => db.gc_done(id)?,
            Err(e) => return Err(e),
        }
    }
    Ok(due.len())
}

fn check_version(v: u32, what: &str) -> Result<(), Error> {
    if v == FORMAT_VERSION {
        Ok(())
    } else {
        Err(Error::Corrupt(format!("{what} is format v{v}, this version reads v{FORMAT_VERSION}")))
    }
}

fn json<T: Serialize>(value: &T) -> Bytes {
    Bytes::from(serde_json::to_vec(value).expect("sync types always serialize"))
}

fn parse<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|e| Error::Corrupt(format!("{what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let head = Head { v: 1, seq: 4, ckpt: 0 };
        assert_eq!(serde_json::to_value(head).unwrap(), serde_json::json!({"v": 1, "seq": 4, "ckpt": 0}));
        let reg: Devices =
            serde_json::from_str(r#"{"v":1,"devices":[{"id":"d1","name":"laptop","added_ms":5}]}"#).unwrap();
        assert_eq!(reg.devices[0].name, "laptop");
        assert!(check_version(2, "x").is_err());
    }

    // The Tauri app runs these on a multi-threaded runtime, so they must be Send.
    #[allow(dead_code)]
    fn futures_are_send(remote: &Remote, db: &Db) {
        fn send<T: Send>(_: T) {}
        send(sync(remote, db, &SyncOptions::default()));
        send(devices(remote));
    }
}
