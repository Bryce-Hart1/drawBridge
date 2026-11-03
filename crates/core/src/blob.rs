// Claude — Date 11/03/2026
// Blobs: one immutable version of a file's bytes, stored as fixed-size chunks
// under `obj/<blob>/<i>`, plus `obj/<blob>/manifest.json`, which says how to put
// them back together.
//
// Why chunks: swiftSearch caps a value at 64 MiB and holds each request body
// whole in memory, and a Range read loads the whole value from its disk. 8 MiB
// chunks keep large videos storable, bound swiftSearch's memory per request,
// and let a seek fetch one chunk's worth instead of one file's worth.
//
// Write order is what makes a blob safe to read: every chunk, then the
// manifest, then the op that puts it in the index. A manifest exists only once
// all of its chunks are on disk in the store, and no op points at a blob
// before its manifest. Uploads are resumable at chunk granularity: each chunk
// swiftSearch acknowledges is recorded in the local Db, and a later upload of
// the same unchanged file skips those chunks.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use bytes::{Bytes, BytesMut};
use futures_util::stream::{self, FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::db::{self, Db, PendingUpload};
use crate::error::Error;
use crate::index;
use crate::keys;
use crate::mime;
use crate::remote::{self, ByteRange, Kind, Remote};

/// The default chunk size. Each manifest records its own, so this can change
/// without touching blobs already stored.
pub const CHUNK_SIZE: u32 = 8 << 20;

/// swiftSearch's largest value.
const MAX_CHUNK_SIZE: u32 = 64 << 20;

/// Most chunk PUTs one upload keeps in flight. Also the bound `discard` relies
/// on to find chunks that were sent but never recorded (see there).
const MAX_PARALLEL: usize = 16;

const MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest format version.
    pub v: u32,
    pub blob: String,
    pub size: u64,
    pub chunk_size: u32,
    /// sha256 of each chunk, lowercase hex, in order.
    pub chunks: Vec<String>,
    /// sha256 of the whole content, lowercase hex.
    pub sha256: String,
    pub mime: String,
    pub kind: Kind,
    /// How the chunks are encrypted: always "none" for now. A reader refuses
    /// any other value rather than hand back ciphertext as content.
    pub enc: String,
}

impl Manifest {
    /// Length of chunk `index`: `chunk_size`, except the last, which is shorter.
    pub fn chunk_len(&self, index: u32) -> u64 {
        chunk_len(self.size, self.chunk_size, index)
    }

    /// The chunk pieces that make up bytes `first..=last`.
    pub fn spans(&self, first: u64, last: u64) -> impl Iterator<Item = Span> + use<> {
        spans(self.chunk_size, first, last)
    }

    fn check(&self) -> Result<(), Error> {
        let bad = |why: String| Err(Error::Corrupt(format!("manifest of {}: {why}", self.blob)));
        if self.v != MANIFEST_VERSION {
            return bad(format!("format v{}, this version reads v{MANIFEST_VERSION}", self.v));
        }
        if self.enc != "none" {
            return bad(format!("encrypted with {:?}, which this version can't read", self.enc));
        }
        if self.chunk_size == 0 || self.chunk_size > MAX_CHUNK_SIZE {
            return bad(format!("chunk size {}", self.chunk_size));
        }
        if chunk_count(self.size, self.chunk_size) != Some(self.chunks.len() as u32) {
            return bad(format!("{} chunk hashes for {} bytes", self.chunks.len(), self.size));
        }
        if !self.chunks.iter().chain([&self.sha256]).all(|h| is_sha256_hex(h)) {
            return bad("a hash isn't 64 hex digits".into());
        }
        Ok(())
    }
}

/// One chunk's share of a byte range: bytes `start..=end` of chunk `index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub index: u32,
    pub start: u64,
    pub end: u64,
}

impl Span {
    // A span always covers at least one byte, so there is no `is_empty`.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// How many chunks `size` bytes take. `None` past u32::MAX chunks, which at
/// 8 MiB a chunk is 32 PiB.
pub fn chunk_count(size: u64, chunk_size: u32) -> Option<u32> {
    size.div_ceil(chunk_size as u64).try_into().ok()
}

fn chunk_len(size: u64, chunk_size: u32, index: u32) -> u64 {
    let start = index as u64 * chunk_size as u64;
    (size - start).min(chunk_size as u64)
}

/// Split the inclusive byte range `first..=last` at chunk boundaries.
pub fn spans(chunk_size: u32, first: u64, last: u64) -> impl Iterator<Item = Span> {
    let cs = chunk_size as u64;
    ((first / cs) as u32..=(last / cs) as u32).map(move |index| {
        let base = index as u64 * cs;
        Span { index, start: first.max(base) - base, end: last.min(base + cs - 1) - base }
    })
}

/// A range made concrete against a value's size, as inclusive `(first, last)`,
/// with the same rules swiftSearch applies: an end past the last byte is cut
/// to it, and a start past the end (or any range of an empty value) is `None`.
pub fn resolve(range: ByteRange, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    match range {
        ByteRange::From(first) if first < size => Some((first, size - 1)),
        ByteRange::Span(first, last) if first < size && first <= last => {
            Some((first, last.min(size - 1)))
        }
        ByteRange::Last(n) if n > 0 => Some((size - n.min(size), size - 1)),
        _ => None,
    }
}

/// Where a finished upload goes in the index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "lowercase")]
pub enum Dest {
    /// Nowhere: just the blob, which nothing in the index points at.
    Blob,
    /// A new file with this name in this folder (`None` is the root).
    New { parent: Option<String>, name: String },
    /// New content for the file with this entry id.
    Replace { id: String },
}

#[derive(Clone, Copy, Debug)]
pub struct UploadOptions {
    /// Bytes per chunk, at most 64 MiB. Ignored when resuming: a resumed
    /// upload keeps the chunk size it started with.
    pub chunk_size: u32,
    /// Chunk PUTs in flight at once, 1 to 16. Memory in use is about
    /// `(parallel + 1) × chunk_size`.
    pub parallel: usize,
}

impl Default for UploadOptions {
    fn default() -> UploadOptions {
        UploadOptions { chunk_size: CHUNK_SIZE, parallel: 3 }
    }
}

/// Bytes done out of the total, reported after each chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct Uploaded {
    pub manifest: Manifest,
    /// The file's entry id, unless the upload was for the blob alone.
    pub entry: Option<String>,
    /// Chunks sent in this call.
    pub sent: u32,
    /// Chunks an earlier, interrupted call had already sent.
    pub skipped: u32,
}

/// Upload the file at `path` as a new blob and put it at `dest` in the index,
/// or carry on with an unfinished upload of the same file. If the file has
/// changed since that upload started, the old partial upload is deleted and
/// this starts over.
///
/// The index changes on this device only; the next sync pushes it.
///
/// Setting `stop` makes it finish the chunks in flight and return
/// `Error::Stopped`; calling this again picks up from there.
pub async fn upload(
    remote: &Remote,
    db: &Db,
    path: &Path,
    dest: Dest,
    opts: UploadOptions,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> Result<Uploaded, Error> {
    if opts.chunk_size == 0 || opts.chunk_size > MAX_CHUNK_SIZE {
        return Err(Error::Invalid(format!("chunk size {} is outside 1..=64 MiB", opts.chunk_size)));
    }
    index::check_dest(db, &dest)?;
    // Canonical, so the same file resumes whatever directory it's named from.
    let path = std::fs::canonicalize(path)?;
    let (size, mtime_ns) = stat(&path)?;
    if chunk_count(size, opts.chunk_size).is_none() {
        return Err(Error::Invalid(format!("{} is too large", path.display())));
    }

    let job = match db.pending_for_path(&path)? {
        Some(p) if p.size == size && p.mtime_ns == mtime_ns => {
            if p.dest != dest {
                db.set_upload_dest(&p.blob, &dest)?;
            }
            PendingUpload { dest, ..p }
        }
        stale => {
            if let Some(p) = stale {
                discard(remote, db, &p).await?;
            }
            let (mime, kind) = mime::detect(&path);
            let p = PendingUpload {
                blob: db::new_id(),
                path,
                size,
                mtime_ns,
                chunk_size: opts.chunk_size,
                mime: mime.to_owned(),
                kind,
                started_ms: db::now_ms() as i64,
                chunks_done: 0,
                dest,
            };
            db.start_upload(&p)?;
            p
        }
    };
    run(remote, db, job, opts.parallel, progress, stop).await
}

/// Carry on with an unfinished upload by its blob id, e.g. every pending upload
/// when the app starts. It goes where it was headed when it started. A file
/// that has changed since is `SourceChanged`, and its partial upload is deleted.
pub async fn resume(
    remote: &Remote,
    db: &Db,
    blob: &str,
    parallel: usize,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> Result<Uploaded, Error> {
    let job =
        db.pending_upload(blob)?.ok_or_else(|| Error::NotFound(format!("upload {blob}")))?;
    match stat(&job.path) {
        Ok((size, mtime_ns)) if size == job.size && mtime_ns == job.mtime_ns => {}
        _ => {
            discard(remote, db, &job).await?;
            return Err(Error::SourceChanged(job.path));
        }
    }
    run(remote, db, job, parallel, progress, stop).await
}

/// Abandon an unfinished upload and delete what it sent.
pub async fn cancel(remote: &Remote, db: &Db, blob: &str) -> Result<(), Error> {
    let job =
        db.pending_upload(blob)?.ok_or_else(|| Error::NotFound(format!("upload {blob}")))?;
    discard(remote, db, &job).await
}

async fn run(
    remote: &Remote,
    db: &Db,
    job: PendingUpload,
    parallel: usize,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> Result<Uploaded, Error> {
    let parallel = parallel.clamp(1, MAX_PARALLEL);
    let count = chunk_count(job.size, job.chunk_size).expect("checked when the upload started");
    let already = db.uploaded_chunks(&job.blob)?;
    let kind = job.kind;

    let mut file = File::open(&job.path)?;
    let mut whole = Sha256::new();
    let mut hashes = Vec::with_capacity(count as usize);
    let mut tally = Tally { done: 0, total: job.size, sent: 0, skipped: 0, progress };
    let mut inflight = FuturesUnordered::new();
    let mut failure = None;

    'chunks: for index in 0..count {
        if stop.load(Ordering::Relaxed) {
            failure = Some(Error::Stopped);
            break;
        }
        let len = chunk_len(job.size, job.chunk_size, index);
        let read;
        (file, whole, read) = read_chunk(file, whole, len).await;
        let (bytes, sha) = match read {
            Ok(read) => read,
            // Shorter than it was when we started: it changed under us.
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                failure = Some(Error::SourceChanged(job.path.clone()));
                break;
            }
            Err(e) => {
                failure = Some(e.into());
                break;
            }
        };

        if let Some(sent_before) = already.get(&index) {
            if *sent_before != sha {
                // Same size and mtime, different bytes: edited in place.
                failure = Some(Error::SourceChanged(job.path.clone()));
                break;
            }
            hashes.push(sha);
            tally.add(len, false);
            continue;
        }

        while inflight.len() >= parallel {
            let done = inflight.next().await.expect("inflight is not empty");
            if let Err(e) = settle(db, &job.blob, done, &mut tally) {
                failure = Some(e);
                break 'chunks;
            }
        }
        let key = keys::chunk(&job.blob, index)?;
        hashes.push(sha.clone());
        inflight.push(async move {
            remote.put(&key, kind, bytes).await.map(|_| (index, sha, len))
        });
    }

    // Whatever happened, record every chunk that did land before returning.
    while let Some(done) = inflight.next().await {
        if let Err(e) = settle(db, &job.blob, done, &mut tally) {
            failure.get_or_insert(e);
        }
    }

    if failure.is_none() {
        // Grown since we started, or touched while we read it.
        let mut probe = [0u8; 1];
        let grown = file.read(&mut probe)? != 0;
        if grown || stat(&job.path)? != (job.size, job.mtime_ns) {
            failure = Some(Error::SourceChanged(job.path.clone()));
        }
    }
    match failure {
        Some(Error::SourceChanged(path)) => {
            discard(remote, db, &job).await?;
            return Err(Error::SourceChanged(path));
        }
        Some(e) => return Err(e),
        None => {}
    }

    let manifest = Manifest {
        v: MANIFEST_VERSION,
        blob: job.blob.clone(),
        size: job.size,
        chunk_size: job.chunk_size,
        chunks: hashes,
        sha256: hex(&whole.finalize()),
        mime: job.mime.clone(),
        kind,
        enc: "none".into(),
    };
    let json = serde_json::to_vec(&manifest).expect("a manifest always serializes");
    remote.put(&keys::manifest(&job.blob)?, Kind::Str, Bytes::from(json)).await?;
    let entry = db.finish_upload(&job, &manifest)?;
    Ok(Uploaded { manifest, entry, sent: tally.sent, skipped: tally.skipped })
}

struct Tally<'a> {
    done: u64,
    total: u64,
    sent: u32,
    skipped: u32,
    progress: &'a (dyn Fn(Progress) + Sync),
}

impl Tally<'_> {
    fn add(&mut self, len: u64, sent: bool) {
        if sent {
            self.sent += 1;
        } else {
            self.skipped += 1;
        }
        self.done += len;
        (self.progress)(Progress { done: self.done, total: self.total });
    }
}

fn settle(
    db: &Db,
    blob: &str,
    done: Result<(u32, String, u64), remote::Error>,
    tally: &mut Tally<'_>,
) -> Result<(), Error> {
    let (index, sha, len) = done?;
    db.mark_chunk(blob, index, &sha)?;
    tally.add(len, true);
    Ok(())
}

/// Read the next `len` bytes and hash them, off the async threads. The file and
/// the running whole-file hash go in and always come back out, read or not.
async fn read_chunk(
    mut file: File,
    mut whole: Sha256,
    len: u64,
) -> (File, Sha256, io::Result<(Bytes, String)>) {
    tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; len as usize];
        let read = file.read_exact(&mut buf).map(|()| {
            whole.update(&buf);
            let sha = hex(&Sha256::digest(&buf));
            (Bytes::from(buf), sha)
        });
        (file, whole, read)
    })
    .await
    // Only a panic inside the closure lands here, and it can't panic.
    .expect("chunk read task")
}

/// Delete everything an unfinished upload may have put in the store, then
/// forget it.
///
/// That includes chunks that were sent but never recorded: a crash between
/// swiftSearch's answer and `mark_chunk`. Chunks are read and sent in index
/// order with at most MAX_PARALLEL unanswered at a time, so every chunk ever
/// sent has an index below the highest recorded one plus MAX_PARALLEL + 1.
/// The manifest goes too, in case a crash came after it was written but before
/// the upload was forgotten.
async fn discard(remote: &Remote, db: &Db, job: &PendingUpload) -> Result<(), Error> {
    let count = chunk_count(job.size, job.chunk_size).unwrap_or(0);
    let highest = db.uploaded_chunks(&job.blob)?.into_keys().max();
    let reach = highest.map_or(0, |h| h + 1).saturating_add(MAX_PARALLEL as u32).min(count);
    remote.delete(&keys::manifest(&job.blob)?).await?;
    let mut deletes = stream::iter(0..reach)
        .map(|index| async move { remote.delete(&keys::chunk(&job.blob, index)?).await.map_err(Error::from) })
        .buffer_unordered(8);
    while let Some(deleted) = deletes.next().await {
        deleted?;
    }
    db.drop_upload(&job.blob)
}

/// Delete a finished blob from the store: its chunks, its thumbnail, and the
/// manifest last, so a blob whose manifest is gone has nothing else left. A
/// blob already gone is fine. Garbage collection calls this; nothing must
/// point at the blob any more.
pub async fn delete(remote: &Remote, blob: &str) -> Result<(), Error> {
    let chunks = match manifest(remote, blob).await {
        Ok(m) => m.chunks.len() as u32,
        Err(Error::NotFound(_)) => return Ok(()),
        // Unreadable, so how many chunks it had is unknown. Any stay behind;
        // the manifest and thumbnail still go, so this isn't retried forever.
        Err(Error::Corrupt(_)) => 0,
        Err(e) => return Err(e),
    };
    let mut deletes = stream::iter(0..chunks)
        .map(|index| async move { remote.delete(&keys::chunk(blob, index)?).await.map_err(Error::from) })
        .buffer_unordered(8);
    while let Some(deleted) = deletes.next().await {
        deleted?;
    }
    remote.delete(&keys::thumb(blob)?).await?;
    remote.delete(&keys::manifest(blob)?).await?;
    Ok(())
}

/// Read a blob's manifest.
pub async fn manifest(remote: &Remote, blob: &str) -> Result<Manifest, Error> {
    let value = remote
        .get(&keys::manifest(blob)?)
        .await?
        .ok_or_else(|| Error::NotFound(format!("blob {blob}")))?;
    let m: Manifest = serde_json::from_slice(&value.bytes)
        .map_err(|e| Error::Corrupt(format!("manifest of {blob}: {e}")))?;
    if m.blob != blob {
        return Err(Error::Corrupt(format!("manifest of {blob} names blob {}", m.blob)));
    }
    m.check()?;
    Ok(m)
}

/// One whole chunk, checked against its length and sha256.
pub async fn chunk(remote: &Remote, m: &Manifest, index: u32) -> Result<Bytes, Error> {
    let missing = || Error::Corrupt(format!("chunk {index} of {} is missing", m.blob));
    let want_hash = m.chunks.get(index as usize).ok_or_else(missing)?.clone();
    let bytes = remote.get(&keys::chunk(&m.blob, index)?).await?.ok_or_else(missing)?.bytes;
    if bytes.len() as u64 != m.chunk_len(index) {
        return Err(Error::Corrupt(format!(
            "chunk {index} of {} is {} bytes, the manifest says {}",
            m.blob,
            bytes.len(),
            m.chunk_len(index)
        )));
    }
    let (bytes, sha) = tokio::task::spawn_blocking(move || {
        let sha = hex(&Sha256::digest(&bytes));
        (bytes, sha)
    })
    .await
    .map_err(io::Error::other)?;
    if sha != want_hash {
        return Err(Error::Corrupt(format!("chunk {index} of {} fails its sha256", m.blob)));
    }
    Ok(bytes)
}

/// The bytes of one span. A span covering a whole chunk is fetched and checked
/// like `chunk`. A smaller one asks swiftSearch for just those bytes, which
/// can't be hashed against the manifest. It relies on TLS and on swiftSearch's
/// own record checksums, which never let a damaged value out.
pub async fn read_span(remote: &Remote, m: &Manifest, span: Span) -> Result<Bytes, Error> {
    let len = m.chunk_len(span.index);
    if span.start == 0 && span.end + 1 == len {
        return chunk(remote, m, span.index).await;
    }
    let key = keys::chunk(&m.blob, span.index)?;
    let part = remote
        .get_range(&key, ByteRange::Span(span.start, span.end))
        .await?
        .ok_or_else(|| Error::Corrupt(format!("chunk {} of {} is missing", span.index, m.blob)))?;
    if part.offset != span.start || part.total != len || part.bytes.len() as u64 != span.len() {
        return Err(Error::Corrupt(format!(
            "chunk {} of {} answered bytes {}+{} of {}, asked for {}+{} of {len}",
            span.index,
            m.blob,
            part.offset,
            part.bytes.len(),
            part.total,
            span.start,
            span.len()
        )));
    }
    Ok(part.bytes)
}

/// Bytes of a blob by range, gathered in memory. Meant for the small ranges a
/// player or viewer asks for; to read a whole large blob, use `download`, or
/// walk `Manifest::spans` with `read_span`.
pub async fn read_range(remote: &Remote, m: &Manifest, range: ByteRange) -> Result<Bytes, Error> {
    let (first, last) = resolve(range, m.size).ok_or(Error::Range { size: m.size })?;
    let mut out = BytesMut::with_capacity((last - first + 1) as usize);
    for span in m.spans(first, last) {
        out.extend_from_slice(&read_span(remote, m, span).await?);
    }
    Ok(out.freeze())
}

/// Download a whole blob to `out`, fetching up to `parallel` chunks at once and
/// checking each one's sha256. Bytes go to `<out>.part`, which is renamed into
/// place only when everything has arrived, so `out` never holds a partial file.
///
/// The whole-file sha256 isn't recomputed: every chunk matching its hash in the
/// manifest already means the file is the bytes that were uploaded.
pub async fn download(
    remote: &Remote,
    m: &Manifest,
    out: &Path,
    parallel: usize,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> Result<(), Error> {
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let result = write_chunks(remote, m, &part, parallel, progress, stop).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
        return result;
    }
    tokio::fs::rename(&part, out).await?;
    Ok(())
}

async fn write_chunks(
    remote: &Remote,
    m: &Manifest,
    part: &Path,
    parallel: usize,
    progress: &(dyn Fn(Progress) + Sync),
    stop: &AtomicBool,
) -> Result<(), Error> {
    let mut file = tokio::fs::File::create(part).await?;
    // `buffered` runs up to `parallel` fetches at once but yields them in
    // order, so the file is written front to back with at most `parallel`
    // chunks held in memory.
    let mut chunks = stream::iter(0..m.chunks.len() as u32)
        .map(|index| chunk(remote, m, index))
        .buffered(parallel.clamp(1, MAX_PARALLEL));
    let mut done = 0;
    while let Some(bytes) = chunks.next().await {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let bytes = bytes?;
        file.write_all(&bytes).await?;
        done += bytes.len() as u64;
        progress(Progress { done, total: m.size });
    }
    file.sync_all().await?;
    Ok(())
}

fn stat(path: &Path) -> Result<(u64, i64), Error> {
    let md = std::fs::metadata(path)?;
    if !md.is_file() {
        return Err(Error::Invalid(format!("{} is not a regular file", path.display())));
    }
    let mtime_ns = md
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    Ok((md.len(), mtime_ns))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_lengths() {
        assert_eq!(chunk_count(0, 10), Some(0));
        assert_eq!(chunk_count(1, 10), Some(1));
        assert_eq!(chunk_count(10, 10), Some(1));
        assert_eq!(chunk_count(11, 10), Some(2));
        assert_eq!(chunk_count(u64::MAX, 1), None);
        assert_eq!(chunk_len(25, 10, 0), 10);
        assert_eq!(chunk_len(25, 10, 2), 5);
        assert_eq!(chunk_len(20, 10, 1), 10);
    }

    #[test]
    fn spans_split_at_chunk_edges() {
        let s = |first, last| spans(10, first, last).collect::<Vec<_>>();
        let sp = |index, start, end| Span { index, start, end };
        assert_eq!(s(0, 0), [sp(0, 0, 0)]);
        assert_eq!(s(0, 9), [sp(0, 0, 9)]);
        assert_eq!(s(9, 10), [sp(0, 9, 9), sp(1, 0, 0)]);
        assert_eq!(s(5, 24), [sp(0, 5, 9), sp(1, 0, 9), sp(2, 0, 4)]);
        assert_eq!(s(20, 20), [sp(2, 0, 0)]);
        assert_eq!(s(5, 24).iter().map(Span::len).sum::<u64>(), 20);
    }

    #[test]
    fn resolve_like_swiftsearch() {
        use ByteRange::*;
        assert_eq!(resolve(From(0), 10), Some((0, 9)));
        assert_eq!(resolve(From(9), 10), Some((9, 9)));
        assert_eq!(resolve(From(10), 10), None);
        assert_eq!(resolve(Span(2, 4), 10), Some((2, 4)));
        assert_eq!(resolve(Span(8, 100), 10), Some((8, 9)));
        assert_eq!(resolve(Span(4, 2), 10), None);
        assert_eq!(resolve(Last(3), 10), Some((7, 9)));
        assert_eq!(resolve(Last(30), 10), Some((0, 9)));
        assert_eq!(resolve(Last(0), 10), None);
        assert_eq!(resolve(From(0), 0), None);
        assert_eq!(resolve(Last(5), 0), None);
    }

    fn manifest(size: u64, chunk_size: u32) -> Manifest {
        let n = chunk_count(size, chunk_size).unwrap() as usize;
        Manifest {
            v: 1,
            blob: "b".into(),
            size,
            chunk_size,
            chunks: vec!["0".repeat(64); n],
            sha256: "f".repeat(64),
            mime: "video/mp4".into(),
            kind: Kind::Video,
            enc: "none".into(),
        }
    }

    #[test]
    fn manifest_checks() {
        assert!(manifest(25, 10).check().is_ok());
        assert!(manifest(0, 10).check().is_ok());
        let broken = [
            Manifest { v: 2, ..manifest(25, 10) },
            Manifest { enc: "aes-gcm".into(), ..manifest(25, 10) },
            Manifest { chunk_size: 0, chunks: vec![], ..manifest(0, 10) },
            Manifest { chunk_size: MAX_CHUNK_SIZE + 1, ..manifest(25, 10) },
            Manifest { size: 31, ..manifest(25, 10) },
            Manifest { sha256: "F".repeat(64), ..manifest(25, 10) },
            Manifest { chunks: vec!["0".repeat(63); 3], ..manifest(25, 10) },
        ];
        for m in broken {
            assert!(matches!(m.check(), Err(Error::Corrupt(_))), "{m:?}");
        }
    }

    #[test]
    fn manifest_json_shape() {
        let json = serde_json::to_value(manifest(25, 10)).unwrap();
        assert_eq!(json["kind"], "video");
        assert_eq!(json["chunks"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    // The Tauri app runs these on a multi-threaded runtime, so they must be Send.
    #[allow(dead_code)]
    fn futures_are_send(remote: &Remote, db: &Db, m: &Manifest, stop: &AtomicBool) {
        fn send<T: Send>(_: T) {}
        let progress = |_: Progress| {};
        send(upload(remote, db, Path::new("x"), Dest::Blob, UploadOptions::default(), &progress, stop));
        send(delete(remote, "b"));
        send(resume(remote, db, "b", 3, &progress, stop));
        send(cancel(remote, db, "b"));
        send(download(remote, m, Path::new("x"), 4, &progress, stop));
        send(read_range(remote, m, ByteRange::From(0)));
    }
}
