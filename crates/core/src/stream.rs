// Claude — Date 11/04/2026
// The stream server: blobs over HTTP on loopback, for the webview's <img>,
// <audio>, <video> and PDF frames, and for an external player like IINA, VLC
// or QuickTime. It listens on 127.0.0.1 only, on a port of its own, and every
// URL carries a 256-bit token:
//
//   http://127.0.0.1:<port>/<token>/<blob>/<name>
//
// The name is only for show: it gives a player a title and an extension.
//
// A Range request is answered with at most the rest of the chunk the range
// starts in. That is how media players read anyway, and it keeps each answer
// one chunk's worth. A chunk already in the cache is served from disk. A chunk
// that isn't is fetched whole into the cache in the background, while this
// answer gets just the bytes asked for (up to 1 MiB) straight from the store,
// so a seek starts playing before the whole chunk has arrived. Each answer also
// starts reading the next chunk ahead.
//
// A request without a Range gets the whole blob, chunk by chunk through the
// cache. That is how an <img> or a PDF loads.
//
// Safety:
// - The token stops other local programs and web pages from reading the store.
// - The Host check stops DNS rebinding: a page on evil.example resolving to
//   127.0.0.1 sends `Host: evil.example`.
// - Everything is served `nosniff`. Everything but PDFs is also served under
//   `Content-Security-Policy: sandbox`, and HTML as plain text, so a stored
//   page or SVG can't run script.
//
// The server stops after a stretch with no requests, so an unused app keeps
// no port open. Starting it again reuses its port and token where it can, so
// URLs handed out earlier keep working.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use bytes::Bytes;
use futures_util::stream::{self, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use crate::blob::{self, Manifest, Span};
use crate::cache::Cache;
use crate::error::Error;
use crate::keys;
use crate::remote::{ByteRange, Remote};

/// The most a cache miss answers with straight from the store.
const QUICK: u64 = 1 << 20;

/// Manifests kept in memory. Blobs never change, so this only bounds memory.
const MANIFESTS: usize = 256;

pub type RemoteSource = Arc<dyn Fn() -> Option<Arc<Remote>> + Send + Sync>;

#[derive(Clone)]
pub struct StreamConfig {
    pub cache: Arc<Cache>,
    /// The store as of each request: the app can connect or disconnect while
    /// the server runs.
    pub remote: RemoteSource,
    /// Web origins allowed to read responses from script (CORS): the app's own
    /// page, so it can draw video frames onto a canvas for posters.
    pub origins: Vec<String>,
    /// Stop after this long with no request in flight.
    pub idle: Duration,
}

pub struct Server {
    port: u16,
    token: String,
    running: Arc<AtomicBool>,
    stop: Option<oneshot::Sender<()>>,
}

struct Shared {
    cfg: StreamConfig,
    host: String,
    token: String,
    manifests: Mutex<HashMap<String, Arc<Manifest>>>,
    active: AtomicUsize,
    /// When the last request started or finished, in ms since `born`.
    last: AtomicU64,
    born: Instant,
}

impl Server {
    /// Start serving. With `reuse` (a stopped server's `port` and `token`),
    /// it tries that port first, so the old URLs work again.
    pub async fn start(cfg: StreamConfig, reuse: Option<(u16, String)>) -> Result<Server, Error> {
        let (listener, token) = match reuse {
            Some((port, token)) => match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
                Ok(l) => (l, token),
                Err(_) => (TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?, new_token()?),
            },
            None => (TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?, new_token()?),
        };
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Shared {
            cfg,
            host: format!("127.0.0.1:{port}"),
            token: token.clone(),
            manifests: Mutex::default(),
            active: AtomicUsize::new(0),
            last: AtomicU64::new(0),
            born: Instant::now(),
        });
        let app = Router::new()
            .route("/{token}/{blob}/{*name}", get(serve).options(preflight))
            .fallback(|| async { StatusCode::NOT_FOUND })
            .with_state(shared.clone());

        let (stop, stopped) = oneshot::channel::<()>();
        let running = Arc::new(AtomicBool::new(true));
        let idle = idle_watch(shared.clone());
        let flag = running.clone();
        tokio::spawn(async move {
            let quit = async {
                tokio::select! {
                    _ = stopped => {}
                    () = idle => {}
                }
            };
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(quit)
                .await;
            flag.store(false, Ordering::Relaxed);
        });
        Ok(Server { port, token, running, stop: Some(stop) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// `false` once it has stopped for being idle (or was stopped).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Where `blob` can be read, named `name`.
    pub fn url(&self, blob: &str, name: &str) -> String {
        format!("http://127.0.0.1:{}/{}/{blob}/{}", self.port, self.token, encode(name))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Resolves once no request has been in flight for `idle`.
async fn idle_watch(s: Arc<Shared>) {
    let tick = (s.cfg.idle / 4).clamp(Duration::from_millis(10), Duration::from_secs(60));
    loop {
        tokio::time::sleep(tick).await;
        let quiet = s.since_last();
        if s.active.load(Ordering::Relaxed) == 0 && quiet >= s.cfg.idle {
            return;
        }
    }
}

impl Shared {
    fn mark(&self) {
        self.last.store(self.born.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    fn since_last(&self) -> Duration {
        self.born.elapsed().saturating_sub(Duration::from_millis(self.last.load(Ordering::Relaxed)))
    }

    async fn manifest(&self, remote: &Remote, blob: &str) -> Result<Arc<Manifest>, Error> {
        if let Some(m) = self.manifests.lock().unwrap_or_else(|e| e.into_inner()).get(blob) {
            return Ok(m.clone());
        }
        let m = Arc::new(blob::manifest(remote, blob).await?);
        let mut all = self.manifests.lock().unwrap_or_else(|e| e.into_inner());
        if all.len() >= MANIFESTS {
            all.clear();
        }
        all.insert(blob.to_owned(), m.clone());
        Ok(m)
    }
}

/// A request in flight, for the idle check. Moved into a streamed body, so a
/// long download counts until its last byte.
struct Busy(Arc<Shared>);

impl Busy {
    fn new(s: &Arc<Shared>) -> Busy {
        s.active.fetch_add(1, Ordering::Relaxed);
        s.mark();
        Busy(s.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.mark();
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn preflight(State(s): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let mut r = StatusCode::NO_CONTENT.into_response();
    cors(&s, &headers, r.headers_mut());
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, HEAD"));
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Range"));
    r
}

async fn serve(
    State(s): State<Arc<Shared>>,
    Path((token, blob, name)): Path<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let busy = Busy::new(&s);
    if headers.get(header::HOST).and_then(|h| h.to_str().ok()) != Some(s.host.as_str()) {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if !same(&token, &s.token) || keys::manifest(&blob).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(remote) = (s.cfg.remote)() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "not connected to a store").into_response();
    };
    let m = match s.manifest(&remote, &blob).await {
        Ok(m) => m,
        Err(e) => return failure(e),
    };

    let mut r = match headers.get(header::RANGE).and_then(|h| h.to_str().ok()).and_then(parse_range) {
        None => whole(&s, remote, m.clone(), method == Method::HEAD, busy),
        Some(range) => match blob::resolve(range, m.size) {
            None => {
                let mut r = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
                put(r.headers_mut(), header::CONTENT_RANGE, &format!("bytes */{}", m.size));
                r
            }
            Some((first, last)) => match part(&s, remote, m.clone(), first, last, method == Method::HEAD).await {
                Ok(r) => r,
                Err(e) => return failure(e),
            },
        },
    };
    let h = r.headers_mut();
    put(h, header::CONTENT_TYPE, served_as(&m.mime));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    if !m.mime.starts_with("application/pdf") {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    }
    put(h, header::CONTENT_DISPOSITION, &format!("inline; filename*=UTF-8''{}", encode(&name)));
    cors(&s, &headers, h);
    r
}

/// The whole blob, 200, streamed one chunk ahead.
fn whole(s: &Arc<Shared>, remote: Arc<Remote>, m: Arc<Manifest>, head: bool, busy: Busy) -> Response {
    let size = m.size;
    let body = if head || size == 0 {
        Body::empty()
    } else {
        let cache = s.cfg.cache.clone();
        let chunks = stream::iter(0..m.chunks.len() as u32)
            .map(move |i| {
                let (cache, remote, m) = (cache.clone(), remote.clone(), m.clone());
                async move { cache.get(&remote, &m, i).await.map_err(std::io::Error::other) }
            })
            .buffered(2)
            // Holds `busy` until the body is done or dropped.
            .chain(stream::once(async move {
                drop(busy);
                Ok(Bytes::new())
            }));
        Body::from_stream(chunks)
    };
    let mut r = Response::new(body);
    put(r.headers_mut(), header::CONTENT_LENGTH, &size.to_string());
    r
}

/// Bytes `first..=last`, cut at the end of `first`'s chunk, 206.
async fn part(
    s: &Arc<Shared>,
    remote: Arc<Remote>,
    m: Arc<Manifest>,
    first: u64,
    last: u64,
    head: bool,
) -> Result<Response, Error> {
    let span = m.spans(first, last).next().expect("a resolved range has a span");
    let cache = &s.cfg.cache;
    let (bytes, span) = if head {
        (Bytes::new(), span)
    } else if let Some(bytes) = cache.read(&m, span.index, span.start, span.end).await {
        (bytes, span)
    } else if m.chunk_len(span.index) <= QUICK {
        // Small enough to fetch whole now.
        let chunk = cache.get(&remote, &m, span.index).await?;
        (chunk.slice(span.start as usize..=span.end as usize), span)
    } else {
        let whole_chunk = (cache.clone(), remote.clone(), m.clone());
        tokio::spawn(async move {
            let (cache, remote, m) = whole_chunk;
            let _ = cache.get(&remote, &m, span.index).await;
        });
        let quick = Span { end: span.end.min(span.start + QUICK - 1), ..span };
        (blob::read_span(&remote, &m, quick).await?, quick)
    };
    if !head {
        cache.prefetch(remote, m.clone(), span.index + 1);
    }
    let base = span.index as u64 * m.chunk_size as u64;
    let mut r = Response::new(Body::from(bytes));
    *r.status_mut() = StatusCode::PARTIAL_CONTENT;
    let h = r.headers_mut();
    put(h, header::CONTENT_LENGTH, &span.len().to_string());
    put(h, header::CONTENT_RANGE, &format!("bytes {}-{}/{}", base + span.start, base + span.end, m.size));
    Ok(r)
}

fn failure(e: Error) -> Response {
    let status = match &e {
        Error::NotFound(_) => StatusCode::NOT_FOUND,
        Error::Corrupt(_) => StatusCode::UNPROCESSABLE_ENTITY,
        _ => StatusCode::BAD_GATEWAY,
    };
    (status, e.to_string()).into_response()
}

/// What a stored type is served as. Stored HTML is shown, never run.
fn served_as(mime: &str) -> &str {
    if mime.starts_with("text/html") || mime.starts_with("application/xhtml") {
        "text/plain; charset=utf-8"
    } else {
        mime
    }
}

fn cors(s: &Shared, request: &HeaderMap, out: &mut HeaderMap) {
    out.insert(header::VARY, HeaderValue::from_static("Origin"));
    let Some(origin) = request.get(header::ORIGIN) else { return };
    if s.cfg.origins.iter().any(|o| origin.as_bytes() == o.as_bytes()) {
        out.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        out.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("Content-Range, Content-Length, Accept-Ranges"),
        );
    }
}

fn put(h: &mut HeaderMap, name: header::HeaderName, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        h.insert(name, v);
    }
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n`. Anything else, a list of ranges
/// included, is ignored and the whole blob is sent, as HTTP allows.
pub fn parse_range(header: &str) -> Option<ByteRange> {
    let spec = header.trim().strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (a, b) = spec.split_once('-')?;
    let num = |s: &str| s.trim().parse::<u64>().ok();
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, false) => Some(ByteRange::Last(num(b)?)),
        (false, true) => Some(ByteRange::From(num(a)?)),
        (false, false) => {
            let (first, last) = (num(a)?, num(b)?);
            (first <= last).then_some(ByteRange::Span(first, last))
        }
        (true, true) => None,
    }
}

/// Percent-encode a path segment: everything but RFC 3986's unreserved bytes.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Equal, in time that doesn't depend on where they first differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn new_token() -> Result<String, Error> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| Error::Io(std::io::Error::other(e.to_string())))?;
    Ok(raw.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99"), Some(ByteRange::Span(0, 99)));
        assert_eq!(parse_range("bytes=100-"), Some(ByteRange::From(100)));
        assert_eq!(parse_range("bytes=-10"), Some(ByteRange::Last(10)));
        assert_eq!(parse_range(" bytes= 5 - 6 "), Some(ByteRange::Span(5, 6)));
        for ignored in ["bytes=0-1,5-6", "bytes=-", "bytes=9-3", "items=0-1", "bytes=a-b", ""] {
            assert_eq!(parse_range(ignored), None, "{ignored:?}");
        }
    }

    #[test]
    fn names_are_encoded() {
        assert_eq!(encode("beach day.mp4"), "beach%20day.mp4");
        assert_eq!(encode("a/b?#"), "a%2Fb%3F%23");
        assert_eq!(encode("é"), "%C3%A9");
    }

    #[test]
    fn html_is_served_as_text() {
        assert_eq!(served_as("text/html; charset=utf-8"), "text/plain; charset=utf-8");
        assert_eq!(served_as("video/mp4"), "video/mp4");
    }

    #[test]
    fn tokens() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(same(&a, &a.clone()) && !same(&a, &b) && !same(&a, &a[1..]));
    }
}
