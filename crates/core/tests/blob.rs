// Claude — Date 11/03/2026
// Chunked blobs against a real swiftSearch: round trips at chunk edges, range
// reads across chunks, stop-and-resume, a file changing under an upload, and
// damage in the store being caught. Most tests use 64 KiB chunks so a
// many-chunk file stays small.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use common::{Server, bytes};
use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::{ByteRange, Db, Error, Kind, Progress, keys};
use sha2::{Digest, Sha256};

const SMALL: u32 = 64 * 1024;

fn opts(chunk_size: u32, parallel: usize) -> UploadOptions {
    UploadOptions { chunk_size, parallel }
}

fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, data).unwrap();
    path
}

fn sha_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn quiet(_: Progress) {}

/// A progress callback that sets `stop` once `after` bytes are done.
fn stop_after(stop: &AtomicBool, after: u64) -> impl Fn(Progress) + Sync + '_ {
    move |p: Progress| {
        if p.done >= after {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

#[tokio::test]
async fn round_trips_at_chunk_edges() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let go = AtomicBool::new(false);
    let sizes = [0, 1, SMALL as usize - 1, SMALL as usize, SMALL as usize + 1, 3 * SMALL as usize + 17];
    for (i, len) in sizes.into_iter().enumerate() {
        let data = bytes(len, i as u64 + 1);
        let src = write(dir.path(), &format!("in{i}.jpg"), &data);
        let up = blob::upload(&r, &db, &src, opts(SMALL, 3), &quiet, &go).await.unwrap();
        let m = &up.manifest;
        assert_eq!(m.size, len as u64);
        assert_eq!(m.chunks.len(), len.div_ceil(SMALL as usize));
        assert_eq!((up.sent, up.skipped), (m.chunks.len() as u32, 0));
        assert_eq!(m.sha256, sha_hex(&data));
        assert_eq!((m.mime.as_str(), m.kind), ("image/jpeg", Kind::Img));

        assert_eq!(&blob::manifest(&r, &m.blob).await.unwrap(), m);

        let out = dir.path().join(format!("out{i}.jpg"));
        blob::download(&r, m, &out, 4, &quiet, &go).await.unwrap();
        assert!(std::fs::read(&out).unwrap() == data, "{len} bytes came back different");
        assert!(!dir.path().join(format!("out{i}.jpg.part")).exists());
    }
    assert!(db.pending_uploads().unwrap().is_empty());
}

#[tokio::test]
async fn default_chunk_size_on_a_large_file() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let go = AtomicBool::new(false);
    let data = bytes((20 << 20) + 3, 9);
    let src = write(dir.path(), "clip.mp4", &data);

    let reports = std::sync::Mutex::new(Vec::new());
    let record = |p: Progress| reports.lock().unwrap().push(p);
    let up = blob::upload(&r, &db, &src, UploadOptions::default(), &record, &go).await.unwrap();
    assert_eq!(up.manifest.chunk_size, 8 << 20);
    assert_eq!(up.manifest.chunks.len(), 3);
    let reports = reports.into_inner().unwrap();
    assert_eq!(reports.len(), 3);
    assert_eq!(reports.last().unwrap(), &Progress { done: data.len() as u64, total: data.len() as u64 });

    let chunk0 = r.head(&keys::chunk(&up.manifest.blob, 0).unwrap()).await.unwrap().unwrap();
    assert_eq!((chunk0.size, chunk0.kind), (8 << 20, Kind::Video));

    let out = dir.path().join("back.mp4");
    blob::download(&r, &up.manifest, &out, 4, &quiet, &go).await.unwrap();
    assert!(std::fs::read(&out).unwrap() == data);
}

#[tokio::test]
async fn ranges_cross_chunks() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let go = AtomicBool::new(false);
    let data = bytes(200_000, 4);
    let src = write(dir.path(), "song.mp3", &data);
    let m = blob::upload(&r, &db, &src, opts(SMALL, 3), &quiet, &go).await.unwrap().manifest;

    let cs = SMALL as u64;
    for (range, first, last) in [
        (ByteRange::Span(0, 0), 0, 0),
        (ByteRange::Span(cs - 1, cs), cs - 1, cs),
        (ByteRange::Span(cs, 2 * cs - 1), cs, 2 * cs - 1),
        (ByteRange::Span(10, 199_999), 10, 199_999),
        (ByteRange::Span(150_000, 10_000_000), 150_000, 199_999),
        (ByteRange::From(2 * cs), 2 * cs, 199_999),
        (ByteRange::Last(1), 199_999, 199_999),
        (ByteRange::Last(300_000), 0, 199_999),
    ] {
        let got = blob::read_range(&r, &m, range).await.unwrap();
        assert!(got == data[first as usize..=last as usize], "{range:?}");
    }
    assert!(matches!(
        blob::read_range(&r, &m, ByteRange::From(200_000)).await,
        Err(Error::Range { size: 200_000 })
    ));
}

#[tokio::test]
async fn stops_and_resumes_where_it_left_off() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let data = bytes(10 * SMALL as usize, 5);
    let src = write(dir.path(), "movie.mov", &data);

    let stop = AtomicBool::new(false);
    let first = blob::upload(&r, &db, &src, opts(SMALL, 1), &stop_after(&stop, 3 * SMALL as u64), &stop).await;
    assert!(matches!(first, Err(Error::Stopped)), "{first:?}");

    let pending = db.pending_uploads().unwrap();
    assert_eq!(pending.len(), 1);
    let (id, done) = (pending[0].blob.clone(), pending[0].chunks_done);
    assert!((3..10).contains(&done), "{done} chunks done");
    assert!(matches!(blob::manifest(&r, &id).await, Err(Error::NotFound(_))), "no manifest until the end");

    // The same file again picks the upload back up, whatever path names it.
    let go = AtomicBool::new(false);
    let via = dir.path().join(".").join("movie.mov");
    let up = blob::upload(&r, &db, &via, opts(SMALL, 3), &quiet, &go).await.unwrap();
    assert_eq!(up.manifest.blob, id);
    assert_eq!((up.skipped, up.sent), (done, 10 - done));
    assert!(db.pending_uploads().unwrap().is_empty());

    let out = dir.path().join("back.mov");
    blob::download(&r, &up.manifest, &out, 4, &quiet, &go).await.unwrap();
    assert!(std::fs::read(&out).unwrap() == data);
}

#[tokio::test]
async fn resumes_by_blob_id() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let data = bytes(6 * SMALL as usize, 6);
    let src = write(dir.path(), "a.bin", &data);
    let stop = AtomicBool::new(false);
    let _ = blob::upload(&r, &db, &src, opts(SMALL, 1), &stop_after(&stop, 2 * SMALL as u64), &stop).await;
    let id = db.pending_uploads().unwrap()[0].blob.clone();

    let go = AtomicBool::new(false);
    let up = blob::resume(&r, &db, &id, 3, &quiet, &go).await.unwrap();
    assert_eq!(up.manifest.sha256, sha_hex(&data));
    assert!(up.skipped >= 2);
    assert!(matches!(blob::resume(&r, &db, &id, 3, &quiet, &go).await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn a_changed_file_starts_over() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let src = write(dir.path(), "notes.md", &bytes(8 * SMALL as usize, 7));
    let stop = AtomicBool::new(false);
    let _ = blob::upload(&r, &db, &src, opts(SMALL, 1), &stop_after(&stop, 2 * SMALL as u64), &stop).await;
    let old = db.pending_uploads().unwrap()[0].blob.clone();

    let data = bytes(8 * SMALL as usize + 1, 8);
    std::fs::write(&src, &data).unwrap();
    let go = AtomicBool::new(false);
    let up = blob::upload(&r, &db, &src, opts(SMALL, 3), &quiet, &go).await.unwrap();
    assert_ne!(up.manifest.blob, old);
    assert_eq!(up.skipped, 0);
    assert_eq!(up.manifest.sha256, sha_hex(&data));
    for i in 0..8 {
        assert!(r.get(&keys::chunk(&old, i).unwrap()).await.unwrap().is_none(), "old chunk {i} left behind");
    }

    // Changed between stopping and an explicit resume: refused, and cleaned up.
    let stop = AtomicBool::new(false);
    let _ = blob::upload(&r, &db, &src, opts(SMALL, 1), &stop_after(&stop, 1), &stop).await;
    let mid = db.pending_uploads().unwrap()[0].blob.clone();
    std::fs::write(&src, b"short now").unwrap();
    assert!(matches!(blob::resume(&r, &db, &mid, 3, &quiet, &go).await, Err(Error::SourceChanged(_))));
    assert!(db.pending_uploads().unwrap().is_empty());
    assert!(r.get(&keys::chunk(&mid, 0).unwrap()).await.unwrap().is_none());
}

#[tokio::test]
async fn cancel_deletes_what_was_sent() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let src = write(dir.path(), "x.bin", &bytes(10 * SMALL as usize, 9));
    let stop = AtomicBool::new(false);
    let _ = blob::upload(&r, &db, &src, opts(SMALL, 2), &stop_after(&stop, 3 * SMALL as u64), &stop).await;
    let id = db.pending_uploads().unwrap()[0].blob.clone();

    blob::cancel(&r, &db, &id).await.unwrap();
    assert!(db.pending_uploads().unwrap().is_empty());
    for i in 0..10 {
        assert!(r.get(&keys::chunk(&id, i).unwrap()).await.unwrap().is_none(), "chunk {i} left behind");
    }
    assert!(matches!(blob::cancel(&r, &db, &id).await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn damage_in_the_store_is_caught() {
    let s = Server::start();
    let (r, db, dir) = (s.writer(), Db::open_in_memory().unwrap(), tempfile::tempdir().unwrap());
    let go = AtomicBool::new(false);
    let src = write(dir.path(), "p.png", &bytes(3 * SMALL as usize, 10));
    let m = blob::upload(&r, &db, &src, opts(SMALL, 3), &quiet, &go).await.unwrap().manifest;

    // Same length, different bytes: only the sha256 can tell.
    let k1 = keys::chunk(&m.blob, 1).unwrap();
    r.put(&k1, Kind::Img, Bytes::from(bytes(SMALL as usize, 99))).await.unwrap();
    let out = dir.path().join("out.png");
    let got = blob::download(&r, &m, &out, 4, &quiet, &go).await;
    assert!(matches!(got, Err(Error::Corrupt(_))), "{got:?}");
    assert!(!out.exists() && !dir.path().join("out.png.part").exists());
    let whole_chunk = ByteRange::Span(SMALL as u64, 2 * SMALL as u64 - 1);
    assert!(matches!(blob::read_range(&r, &m, whole_chunk).await, Err(Error::Corrupt(_))));

    // Wrong length, and missing.
    let k2 = keys::chunk(&m.blob, 2).unwrap();
    r.put(&k2, Kind::Img, Bytes::from_static(b"short")).await.unwrap();
    assert!(matches!(blob::chunk(&r, &m, 2).await, Err(Error::Corrupt(_))));
    r.delete(&k2).await.unwrap();
    assert!(matches!(blob::chunk(&r, &m, 2).await, Err(Error::Corrupt(_))));

    // A manifest that isn't one.
    r.put(&keys::manifest(&m.blob).unwrap(), Kind::Str, Bytes::from_static(b"{}")).await.unwrap();
    assert!(matches!(blob::manifest(&r, &m.blob).await, Err(Error::Corrupt(_))));
}

#[tokio::test]
async fn unknown_blob() {
    let s = Server::start();
    let got = blob::manifest(&s.writer(), "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f").await;
    assert!(matches!(got, Err(Error::NotFound(_))));
}
