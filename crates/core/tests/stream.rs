// Claude — Date 11/04/2026
// The stream server and chunk cache against a real swiftSearch: ranges cut at
// chunk ends, whole-blob reads, HEAD, the token and Host checks, how stored
// HTML is served, CORS for the app's origins, stopping when idle and coming
// back on the same port, and damaged chunks never reaching the cache.
//
// The test blob is 5 MiB in 2 MiB chunks (2 + 2 + 1), so the first two chunks
// are bigger than the 1 MiB the server answers with on a cache miss and the
// last one isn't.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{Server, bytes};
use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::cache::Cache;
use drawbridge_core::stream::{self, StreamConfig};
use drawbridge_core::{Db, Dest, Error, Kind, Manifest, Progress, Remote, keys};
use reqwest::StatusCode;
use reqwest::header::{self, HeaderMap};

const MIB: usize = 1 << 20;
const CHUNK: u32 = 2 << 20;
const APP: &str = "tauri://localhost";

struct Fixture {
    _store: Server,
    remote: Arc<Remote>,
    cache: Arc<Cache>,
    _dir: tempfile::TempDir,
    data: Vec<u8>,
    m: Manifest,
}

async fn fixture() -> Fixture {
    let store = Server::start();
    let remote = Arc::new(store.writer());
    let dir = tempfile::tempdir().unwrap();
    let data = bytes(5 * MIB, 7);
    let m = upload(&remote, &dir, "clip.mp4", &data).await;
    let cache = Cache::open(&dir.path().join("cache"), 1 << 30).unwrap();
    Fixture { _store: store, remote, cache, _dir: dir, data, m }
}

async fn upload(remote: &Remote, dir: &tempfile::TempDir, name: &str, data: &[u8]) -> Manifest {
    let path = dir.path().join(name);
    std::fs::write(&path, data).unwrap();
    let db = Db::open_in_memory().unwrap();
    let opts = UploadOptions { chunk_size: CHUNK, parallel: 3 };
    let quiet = |_: Progress| {};
    blob::upload(remote, &db, &path, Dest::Blob, opts, &quiet, &AtomicBool::new(false))
        .await
        .unwrap()
        .manifest
}

async fn serve(f: &Fixture, idle: Duration, reuse: Option<(u16, String)>) -> stream::Server {
    let remote = f.remote.clone();
    let cfg = StreamConfig {
        cache: f.cache.clone(),
        remote: Arc::new(move || Some(remote.clone())),
        origins: vec![APP.into()],
        idle,
    };
    stream::Server::start(cfg, reuse).await.unwrap()
}

async fn get(url: &str, headers: &[(header::HeaderName, &str)]) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = reqwest::Client::new().get(url);
    for (k, v) in headers {
        req = req.header(k, *v);
    }
    let r = req.send().await.unwrap();
    let (status, h) = (r.status(), r.headers().clone());
    (status, h, r.bytes().await.unwrap().to_vec())
}

fn text(h: &HeaderMap, name: header::HeaderName) -> &str {
    h.get(name).map(|v| v.to_str().unwrap()).unwrap_or("")
}

async fn wait_until(what: &str, ok: impl Fn() -> bool) {
    for _ in 0..500 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn ranges_are_cut_at_chunk_ends() {
    let f = fixture().await;
    let s = serve(&f, Duration::from_secs(60), None).await;
    let url = s.url(&f.m.blob, "clip.mp4");
    let size = f.data.len();

    let (status, h, body) = get(&url, &[(header::RANGE, "bytes=0-99")]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(text(&h, header::CONTENT_RANGE), format!("bytes 0-99/{size}"));
    assert_eq!(text(&h, header::CONTENT_TYPE), "video/mp4");
    assert_eq!(text(&h, header::ACCEPT_RANGES), "bytes");
    assert_eq!(text(&h, header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert_eq!(body, &f.data[..100]);

    // The last 10 bytes, in the 1 MiB last chunk.
    let (status, h, body) = get(&url, &[(header::RANGE, "bytes=-10")]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(text(&h, header::CONTENT_RANGE), format!("bytes {}-{}/{size}", size - 10, size - 1));
    assert_eq!(body, &f.data[size - 10..]);

    // Open-ended, into the middle of chunk 1: answered up to 1 MiB on a miss,
    // and the whole chunk is fetched into the cache meanwhile.
    let start = CHUNK as usize + 5;
    let (status, h, body) = get(&url, &[(header::RANGE, &format!("bytes={start}-"))]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    let end = start + body.len() - 1;
    assert!(body.len() <= MIB || end == 2 * CHUNK as usize - 1, "{} bytes", body.len());
    assert_eq!(text(&h, header::CONTENT_RANGE), format!("bytes {start}-{end}/{size}"));
    assert_eq!(body, &f.data[start..=end]);
    wait_until("chunk 1 in the cache", || f.cache.has(&f.m.blob, 1)).await;
    // Reading chunk 1 also read chunk 2 ahead.
    wait_until("chunk 2 read ahead", || f.cache.has(&f.m.blob, 2)).await;

    // Now cached: the rest of the chunk, from disk.
    let (_, h, body) = get(&url, &[(header::RANGE, &format!("bytes={start}-"))]).await;
    let end = 2 * CHUNK as usize - 1;
    assert_eq!(text(&h, header::CONTENT_RANGE), format!("bytes {start}-{end}/{size}"));
    assert_eq!(body, &f.data[start..=end]);

    let (status, h, body) = get(&url, &[(header::RANGE, &format!("bytes={size}-"))]).await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(text(&h, header::CONTENT_RANGE), format!("bytes */{size}"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn whole_blob_and_head() {
    let f = fixture().await;
    let s = serve(&f, Duration::from_secs(60), None).await;
    let url = s.url(&f.m.blob, "my clip.mp4");
    assert!(url.ends_with("/my%20clip.mp4"), "{url}");

    let (status, h, body) = get(&url, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&h, header::CONTENT_LENGTH), f.data.len().to_string());
    assert_eq!(text(&h, header::CONTENT_DISPOSITION), "inline; filename*=UTF-8''my%20clip.mp4");
    assert!(body == f.data, "whole body differs");
    for i in 0..3 {
        assert!(f.cache.has(&f.m.blob, i), "chunk {i} cached");
    }

    let r = reqwest::Client::new().head(&url).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(text(r.headers(), header::CONTENT_LENGTH), f.data.len().to_string());
    assert!(r.bytes().await.unwrap().is_empty());
}

#[tokio::test]
async fn token_and_host_are_checked() {
    let f = fixture().await;
    let s = serve(&f, Duration::from_secs(60), None).await;
    let url = s.url(&f.m.blob, "clip.mp4");

    let wrong = url.replace(s.token(), &"0".repeat(64));
    assert_eq!(get(&wrong, &[]).await.0, StatusCode::NOT_FOUND);
    let short = url.replace(s.token(), "abc");
    assert_eq!(get(&short, &[]).await.0, StatusCode::NOT_FOUND);
    let bad_blob = url.replace(&f.m.blob, "..");
    assert_eq!(get(&bad_blob, &[]).await.0, StatusCode::NOT_FOUND);
    let missing = url.replace(&f.m.blob, "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f");
    assert_eq!(get(&missing, &[]).await.0, StatusCode::NOT_FOUND);

    let rebound = (header::HOST, "evil.example");
    assert_eq!(get(&url, &[rebound]).await.0, StatusCode::MISDIRECTED_REQUEST);
    let other_port = (header::HOST, "127.0.0.1:1");
    assert_eq!(get(&url, &[other_port]).await.0, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn html_is_text_and_cors_is_for_the_app_only() {
    let f = fixture().await;
    let page = upload(&f.remote, &f._dir, "page.html", b"<script>alert(1)</script>").await;
    let pdf = upload(&f.remote, &f._dir, "doc.pdf", b"%PDF-1.4").await;
    let s = serve(&f, Duration::from_secs(60), None).await;

    let (_, h, _) = get(&s.url(&page.blob, "page.html"), &[(header::ORIGIN, APP)]).await;
    assert_eq!(text(&h, header::CONTENT_TYPE), "text/plain; charset=utf-8");
    assert_eq!(text(&h, header::CONTENT_SECURITY_POLICY), "sandbox");
    assert_eq!(text(&h, header::ACCESS_CONTROL_ALLOW_ORIGIN), APP);

    // PDFs can't render in a sandbox, and can't run script in WebKit anyway.
    let (_, h, _) = get(&s.url(&pdf.blob, "doc.pdf"), &[]).await;
    assert_eq!(text(&h, header::CONTENT_TYPE), "application/pdf");
    assert_eq!(text(&h, header::CONTENT_SECURITY_POLICY), "");

    let (_, h, _) = get(&s.url(&page.blob, "page.html"), &[(header::ORIGIN, "https://evil.example")]).await;
    assert_eq!(text(&h, header::ACCESS_CONTROL_ALLOW_ORIGIN), "");
}

#[tokio::test]
async fn stops_when_idle_and_restarts_on_the_same_port() {
    let f = fixture().await;
    let s = serve(&f, Duration::from_millis(200), None).await;
    let url = s.url(&f.m.blob, "clip.mp4");
    assert_eq!(get(&url, &[(header::RANGE, "bytes=0-9")]).await.0, StatusCode::PARTIAL_CONTENT);
    wait_until("the server to stop", || !s.is_running()).await;
    assert!(reqwest::get(&url).await.is_err(), "port still open");

    let again = serve(&f, Duration::from_secs(60), Some((s.port(), s.token().to_owned()))).await;
    assert_eq!(again.url(&f.m.blob, "clip.mp4"), url);
    let (status, _, body) = get(&url, &[(header::RANGE, "bytes=0-9")]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, &f.data[..10]);
    drop(again);
}

#[tokio::test]
async fn damaged_chunks_never_reach_the_cache() {
    let f = fixture().await;
    let len = f.m.chunk_len(1) as usize;
    let key = keys::chunk(&f.m.blob, 1).unwrap();
    f.remote.put(&key, Kind::Video, vec![0u8; len].into()).await.unwrap();
    assert!(matches!(f.cache.get(&f.remote, &f.m, 1).await, Err(Error::Corrupt(_))));
    assert!(!f.cache.has(&f.m.blob, 1));

    // Two readers of one chunk at once both get it, from one cached copy.
    let (a, b) = tokio::join!(f.cache.get(&f.remote, &f.m, 0), f.cache.get(&f.remote, &f.m, 0));
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(f.cache.usage().chunks, 1);
}
