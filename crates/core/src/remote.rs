// Claude — Date 11/03/2026
// The swiftSearch client. Every byte drawBridge stores or reads goes through
// here. In development it talks to swiftSearch directly on loopback; in
// production it goes through castle+ over HTTPS. The routes are the same either
// way, because castle+ forwards the vault host to swiftSearch's root
// (swiftSearch USE.md is the reference for every status and header used below).
//
// What swiftSearch and castle+ require, and where it shows up here:
// - Every PUT needs Content-Length; both answer a chunked body with 501. Bodies
//   are always `Bytes`, so reqwest always knows the length.
// - The access key rides on every request except /healthz, in plain text. A
//   base URL that isn't loopback must therefore be https (`check_base`).
// - castle+ forwards `Authorization` only on a route without `auth =`; with one,
//   it consumes the header and swiftSearch answers every request with 401.
// - A 503 (swiftSearch's queue is full) or a connection dropped under us is
//   worth retrying. Every call here is idempotent, so retrying is safe: a PUT of
//   the same bytes to the same key stores the same value.

use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url};
use serde::Deserialize;

use crate::keys::Key;

/// What kind of value a blob holds. Sent as `X-SwiftSearch-Type` on every PUT,
/// so the store stays self-describing to anyone reading it with curl.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Img,
    Video,
    Str,
    Bin,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Img => "img",
            Kind::Video => "video",
            Kind::Str => "str",
            Kind::Bin => "bin",
        }
    }

    /// swiftSearch's scalar types (`int`, `float`, …) are never written by
    /// drawBridge, so anything but the four names above reads back as `Bin`.
    pub fn parse(s: &str) -> Kind {
        match s.to_ascii_lowercase().as_str() {
            "img" => Kind::Img,
            "video" => Kind::Video,
            "str" => Kind::Str,
            _ => Kind::Bin,
        }
    }
}

/// One byte range. swiftSearch takes a single range per request; it ignores a
/// list and answers with the whole value, which `get_range` would then report
/// as offset 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteRange {
    /// `bytes=first-`: from `first` to the end.
    From(u64),
    /// `bytes=first-last`, both ends inclusive.
    Span(u64, u64),
    /// `bytes=-n`: the last `n` bytes.
    Last(u64),
}

impl ByteRange {
    fn header(self) -> String {
        match self {
            ByteRange::From(first) => format!("bytes={first}-"),
            ByteRange::Span(first, last) => format!("bytes={first}-{last}"),
            ByteRange::Last(n) => format!("bytes=-{n}"),
        }
    }
}

/// What a HEAD (or the headers of a GET) says about a stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    /// Size of the whole value, not of a range of it.
    pub size: u64,
    pub kind: Kind,
    /// The `ETag` exactly as sent, quotes included, ready for `If-None-Match`.
    pub etag: String,
    /// `X-SwiftSearch-Hash`: HMAC-SHA256 under the store's salt, in hex. Stable
    /// within one store; not the file's sha256.
    pub hash: String,
    pub content_type: String,
}

#[derive(Clone, Debug)]
pub struct Value {
    pub meta: Meta,
    pub bytes: Bytes,
}

/// Part of a value. `bytes` starts at `offset` within a value of `total` bytes.
#[derive(Clone, Debug)]
pub struct Partial {
    pub offset: u64,
    pub total: u64,
    pub etag: String,
    pub bytes: Bytes,
}

/// The answer to a conditional read.
#[derive(Clone, Debug)]
pub enum Conditional {
    /// The value still has the tag that was sent.
    Unchanged,
    Changed(Value),
    Missing,
}

/// What swiftSearch reports after a PUT is on disk.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Stored {
    pub hash: String,
    pub size: u64,
    /// `true` for a new key, `false` when an existing value was replaced.
    pub created: bool,
}

#[derive(Debug)]
pub enum Error {
    /// The base URL can't be used. The message says why.
    BaseUrl(String),
    /// An access key that can't be sent as a header.
    AccessKey,
    /// 401: no key, an unknown key, or a revoked one.
    Unauthorized,
    /// 403: a valid key without enough access, e.g. a `read` key on a PUT.
    Forbidden,
    /// 413: over swiftSearch's 64 MiB value limit (or castle+'s `max_body`).
    TooLarge,
    /// 416: the range starts past the end. `size` is the value's real size.
    RangeNotSatisfiable { size: u64 },
    /// Any other error status, with swiftSearch's stable `code` and its sentence.
    Status { status: u16, code: String, message: String },
    /// The request got no answer: connect, TLS, a dropped connection, a timeout.
    Transport(reqwest::Error),
    /// The server answered something this client can't read.
    Protocol(String),
}

impl Error {
    fn retryable(&self) -> bool {
        match self {
            Error::Status { status, .. } => *status == 503,
            Error::Transport(e) => !e.is_builder(),
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BaseUrl(why) => write!(f, "server address: {why}"),
            Error::AccessKey => f.write_str("the access key can't be sent as a header"),
            Error::Unauthorized => f.write_str("access key refused (401)"),
            Error::Forbidden => f.write_str("access key lacks permission for this (403)"),
            Error::TooLarge => f.write_str("value too large for the store (413)"),
            Error::RangeNotSatisfiable { size } => {
                write!(f, "range past the end of a {size}-byte value (416)")
            }
            Error::Status { status, code, message } => write!(f, "{message} ({status} {code})"),
            Error::Transport(e) => write!(f, "could not reach the store: {e}"),
            Error::Protocol(why) => write!(f, "unexpected answer from the store: {why}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Transport(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Error {
        Error::Transport(e)
    }
}

/// How hard to try before giving up on a retryable failure. The delay grows
/// fourfold each time: 200 ms, 800 ms, 3.2 s by default.
#[derive(Clone, Copy, Debug)]
pub struct Retry {
    pub attempts: u32,
    pub first_delay: Duration,
}

impl Default for Retry {
    fn default() -> Retry {
        Retry { attempts: 4, first_delay: Duration::from_millis(200) }
    }
}

pub struct Remote {
    http: reqwest::Client,
    /// `scheme://host[:port]`, no trailing slash.
    base: String,
    auth: HeaderValue,
    retry: Retry,
}

// Hand-written so the access key can never end up in a log line.
impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Remote").field("base", &self.base).finish_non_exhaustive()
    }
}

const TYPE_HEADER: &str = "x-swiftsearch-type";
const HASH_HEADER: &str = "x-swiftsearch-hash";

impl Remote {
    pub fn new(base_url: &str, access_key: &str) -> Result<Remote, Error> {
        let base = check_base(base_url)?;
        if access_key.is_empty() {
            return Err(Error::AccessKey);
        }
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {access_key}")).map_err(|_| Error::AccessKey)?;
        auth.set_sensitive(true);
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // swiftSearch drops a client that reads nothing for 20 s; this is
            // the same courtesy the other way, for a server that has stalled.
            .read_timeout(Duration::from_secs(30))
            // swiftSearch closes a keep-alive connection after 15 s idle. Give
            // pooled ones up first, so a request never lands on a closing socket.
            .pool_idle_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Remote { http, base, auth, retry: Retry::default() })
    }

    pub fn with_retry(mut self, retry: Retry) -> Remote {
        self.retry = retry;
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// `GET /healthz`, the one route that needs no key. Says the server is up,
    /// nothing about whether the key is good; a HEAD on any key checks that.
    pub async fn health(&self) -> Result<(), Error> {
        let url = format!("{}/healthz", self.base);
        let url = &url;
        self.retrying(|| async move {
            let resp = self.http.get(url).send().await?;
            if resp.status() == StatusCode::OK { Ok(()) } else { Err(fail(resp).await) }
        })
        .await
    }

    /// Store `body` under `key`, replacing any value already there. On success
    /// the value is flushed to disk on the server.
    pub async fn put(&self, key: &Key, kind: Kind, body: Bytes) -> Result<Stored, Error> {
        let url = self.blob_url(key);
        let (url, body) = (&url, &body);
        self.retrying(|| async move {
            let resp = self
                .request(Method::PUT, url)
                .header(TYPE_HEADER, kind.as_str())
                // Set by hand: hyper leaves it off an empty body, and swiftSearch
                // answers a PUT without it with 411. hyper adds none of its own
                // when one is present, so there is never a duplicate (which
                // castle+ refuses with 400).
                .header(header::CONTENT_LENGTH, body.len())
                .body(body.clone())
                .send()
                .await?;
            match resp.status() {
                StatusCode::OK | StatusCode::CREATED => {
                    let text = resp.bytes().await?;
                    serde_json::from_slice(&text)
                        .map_err(|e| Error::Protocol(format!("PUT response body: {e}")))
                }
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    /// The whole value, or `None` if there is no such key.
    pub async fn get(&self, key: &Key) -> Result<Option<Value>, Error> {
        let url = self.blob_url(key);
        let url = &url;
        self.retrying(|| async move {
            let resp = self.request(Method::GET, url).send().await?;
            match resp.status() {
                StatusCode::OK => Ok(Some(read_value(resp).await?)),
                StatusCode::NOT_FOUND => Ok(None),
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    /// The value only if its tag is no longer `etag`. This is how the index is
    /// polled: an unchanged `head.json` costs a header-only 304.
    pub async fn get_if_changed(&self, key: &Key, etag: &str) -> Result<Conditional, Error> {
        let url = self.blob_url(key);
        let url = &url;
        self.retrying(|| async move {
            let resp = self
                .request(Method::GET, url)
                .header(header::IF_NONE_MATCH, etag)
                .send()
                .await?;
            match resp.status() {
                StatusCode::NOT_MODIFIED => Ok(Conditional::Unchanged),
                StatusCode::OK => Ok(Conditional::Changed(read_value(resp).await?)),
                StatusCode::NOT_FOUND => Ok(Conditional::Missing),
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    /// Part of a value, or `None` if there is no such key. swiftSearch sends
    /// only the requested bytes, though it reads the whole value from its disk
    /// to do so; that is one reason drawBridge keeps chunks small.
    pub async fn get_range(&self, key: &Key, range: ByteRange) -> Result<Option<Partial>, Error> {
        let url = self.blob_url(key);
        let url = &url;
        self.retrying(|| async move {
            let resp = self
                .request(Method::GET, url)
                .header(header::RANGE, range.header())
                .send()
                .await?;
            match resp.status() {
                StatusCode::PARTIAL_CONTENT => {
                    let (first, last, total) = header_str(resp.headers(), header::CONTENT_RANGE)
                        .and_then(parse_content_range)
                        .ok_or_else(|| Error::Protocol("206 without a usable Content-Range".into()))?;
                    let etag = header_str(resp.headers(), header::ETAG).unwrap_or_default().to_owned();
                    let bytes = resp.bytes().await?;
                    if bytes.len() as u64 != last - first + 1 {
                        return Err(Error::Protocol(format!(
                            "Content-Range says {} bytes, body has {}",
                            last - first + 1,
                            bytes.len()
                        )));
                    }
                    Ok(Some(Partial { offset: first, total, etag, bytes }))
                }
                // The server ignored the range and sent everything.
                StatusCode::OK => {
                    let v = read_value(resp).await?;
                    Ok(Some(Partial { offset: 0, total: v.meta.size, etag: v.meta.etag, bytes: v.bytes }))
                }
                StatusCode::NOT_FOUND => Ok(None),
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    /// Size, kind and tag of a value without its bytes, or `None` if there is
    /// no such key.
    pub async fn head(&self, key: &Key) -> Result<Option<Meta>, Error> {
        let url = self.blob_url(key);
        let url = &url;
        self.retrying(|| async move {
            let resp = self.request(Method::HEAD, url).send().await?;
            match resp.status() {
                StatusCode::OK => {
                    // Not resp.content_length(): that is the size of the body
                    // received, which for a HEAD is always zero.
                    let size = header_str(resp.headers(), header::CONTENT_LENGTH)
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| Error::Protocol("HEAD without Content-Length".into()))?;
                    Ok(Some(meta(resp.headers(), size)))
                }
                StatusCode::NOT_FOUND => Ok(None),
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    /// Remove a value. `true` if it was there, `false` if it wasn't. A retry
    /// after a delete that landed but whose answer was lost reports `false`.
    pub async fn delete(&self, key: &Key) -> Result<bool, Error> {
        let url = self.blob_url(key);
        let url = &url;
        self.retrying(|| async move {
            let resp = self.request(Method::DELETE, url).send().await?;
            match resp.status() {
                StatusCode::NO_CONTENT => Ok(true),
                StatusCode::NOT_FOUND => Ok(false),
                _ => Err(fail(resp).await),
            }
        })
        .await
    }

    fn blob_url(&self, key: &Key) -> String {
        // Key's character set needs no percent-encoding (see keys.rs).
        format!("{}/v1/blobs/{}", self.base, key.as_str())
    }

    fn request(&self, method: Method, url: &str) -> RequestBuilder {
        self.http.request(method, url).header(header::AUTHORIZATION, self.auth.clone())
    }

    async fn retrying<T, F, Fut>(&self, op: F) -> Result<T, Error>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<T, Error>>,
    {
        let mut delay = self.retry.first_delay;
        let mut attempt = 1;
        loop {
            match op().await {
                Err(e) if e.retryable() && attempt < self.retry.attempts => {
                    tokio::time::sleep(delay).await;
                    delay *= 4;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }
}

/// Accept `https://host[:port]` anywhere, and `http://` only to loopback: the
/// access key crosses the wire in plain text, so plain http over a network
/// would hand it to anyone on the path.
fn check_base(raw: &str) -> Result<String, Error> {
    let url = Url::parse(raw.trim()).map_err(|e| Error::BaseUrl(format!("{raw:?}: {e}")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::BaseUrl(
            "no credentials in the address; the access key is kept separately".into(),
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(Error::BaseUrl(format!(
            "{raw:?} must be just scheme://host[:port]; swiftSearch's routes start at the root"
        )));
    }
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        "http" => {
            return Err(Error::BaseUrl(format!(
                "{raw:?} is plain http to another machine; use https (castle+) so the access key isn't sent in the clear"
            )));
        }
        other => return Err(Error::BaseUrl(format!("unsupported scheme {other:?}"))),
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn is_loopback(url: &Url) -> bool {
    match url.host_str() {
        Some(h) if h.eq_ignore_ascii_case("localhost") => true,
        Some(h) => h
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn meta(headers: &HeaderMap, size: u64) -> Meta {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default();
    Meta {
        size,
        kind: Kind::parse(get(TYPE_HEADER)),
        etag: get("etag").to_owned(),
        hash: get(HASH_HEADER).to_owned(),
        content_type: get("content-type").to_owned(),
    }
}

async fn read_value(resp: Response) -> Result<Value, Error> {
    let headers = resp.headers().clone();
    let bytes = resp.bytes().await?;
    Ok(Value { meta: meta(&headers, bytes.len() as u64), bytes })
}

/// `bytes first-last/total` → (first, last, total).
fn parse_content_range(v: &str) -> Option<(u64, u64, u64)> {
    let (span, total) = v.strip_prefix("bytes ")?.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    let (first, last, total) = (first.parse().ok()?, last.parse().ok()?, total.parse().ok()?);
    (first <= last && last < total).then_some((first, last, total))
}

/// Turn an error status into an `Error`. swiftSearch's error bodies are
/// `{"error": sentence, "code": stable_name, "status": n}`; castle+ answers some
/// statuses itself with a plain-text body, which lands in `message`.
async fn fail(resp: Response) -> Error {
    #[derive(Deserialize)]
    struct Body {
        error: String,
        code: String,
    }

    let status = resp.status();
    match status {
        StatusCode::UNAUTHORIZED => return Error::Unauthorized,
        StatusCode::FORBIDDEN => return Error::Forbidden,
        StatusCode::PAYLOAD_TOO_LARGE => return Error::TooLarge,
        StatusCode::RANGE_NOT_SATISFIABLE => {
            // `Content-Range: bytes */size`.
            let size = header_str(resp.headers(), header::CONTENT_RANGE)
                .and_then(|v| v.strip_prefix("bytes */"))
                .and_then(|n| n.parse().ok());
            return match size {
                Some(size) => Error::RangeNotSatisfiable { size },
                None => Error::Protocol("416 without Content-Range".into()),
            };
        }
        _ => {}
    }
    let text = resp.bytes().await.unwrap_or_default();
    match serde_json::from_slice::<Body>(&text) {
        Ok(b) => Error::Status { status: status.as_u16(), code: b.code, message: b.error },
        Err(_) => Error::Status {
            status: status.as_u16(),
            code: String::new(),
            message: String::from_utf8_lossy(&text).trim().to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls() {
        for (raw, want) in [
            ("https://vault.example.com", "https://vault.example.com"),
            ("https://vault.example.com/", "https://vault.example.com"),
            ("https://vault.example.com:8443", "https://vault.example.com:8443"),
            ("http://127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("http://127.9.9.9:1", "http://127.9.9.9:1"),
            ("http://localhost:8080/", "http://localhost:8080"),
            ("http://[::1]:8080", "http://[::1]:8080"),
            ("  https://a.b  ", "https://a.b"),
        ] {
            assert_eq!(check_base(raw).unwrap(), want, "{raw}");
        }
        for bad in [
            "http://192.168.1.10:8080",
            "http://vault.example.com",
            "http://0.0.0.0:8080",
            "https://vault.example.com/v1",
            "https://vault.example.com/?x=1",
            "https://user:pw@vault.example.com",
            "ftp://vault.example.com",
            "vault.example.com",
            "",
        ] {
            assert!(matches!(check_base(bad), Err(Error::BaseUrl(_))), "{bad}");
        }
    }

    #[test]
    fn range_headers() {
        assert_eq!(ByteRange::From(100).header(), "bytes=100-");
        assert_eq!(ByteRange::Span(0, 4).header(), "bytes=0-4");
        assert_eq!(ByteRange::Last(100).header(), "bytes=-100");
    }

    #[test]
    fn content_range() {
        assert_eq!(parse_content_range("bytes 0-4/19"), Some((0, 4, 19)));
        assert_eq!(parse_content_range("bytes 18-18/19"), Some((18, 18, 19)));
        assert_eq!(parse_content_range("bytes */19"), None);
        assert_eq!(parse_content_range("bytes 5-4/19"), None);
        assert_eq!(parse_content_range("bytes 0-19/19"), None);
        assert_eq!(parse_content_range("items 0-4/19"), None);
    }

    #[test]
    fn kinds() {
        for k in [Kind::Img, Kind::Video, Kind::Str, Kind::Bin] {
            assert_eq!(Kind::parse(k.as_str()), k);
        }
        assert_eq!(Kind::parse("IMG"), Kind::Img);
        assert_eq!(Kind::parse("big_int"), Kind::Bin);
        assert_eq!(Kind::parse(""), Kind::Bin);
    }

    #[test]
    fn access_key_never_in_debug() {
        let r = Remote::new("http://127.0.0.1:1", "ssk_secret_value").unwrap();
        assert!(!format!("{r:?}").contains("ssk_secret_value"));
    }

    #[test]
    fn refuses_unusable_keys() {
        assert!(matches!(Remote::new("http://127.0.0.1:1", ""), Err(Error::AccessKey)));
        assert!(matches!(Remote::new("http://127.0.0.1:1", "a\nb"), Err(Error::AccessKey)));
    }
}
