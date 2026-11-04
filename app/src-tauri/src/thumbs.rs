// Claude — Date 11/03/2026
// The `thumb://` protocol: `<img src="thumb://localhost/<blob>">` in the
// webview loads that blob's thumbnail. Each one is fetched from the store once
// and then served from a folder in the app's cache: a blob never changes, so
// neither does its thumbnail.
//
// A miss is remembered too, so a photo or video without one isn't asked for
// again. Photo thumbnails are made at upload (core's thumb.rs). The rest,
// video posters and HEIC thumbnails, are drawn by the page from the stream
// (app/src/lib/posters.ts) and handed to `save`, which replaces the miss and
// stores the thumbnail for every other device too.

use std::fs;
use std::sync::Arc;

use drawbridge_core::{Kind, keys, remote};
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};
use tokio::sync::Semaphore;

use crate::commands::CmdError;
use crate::config::write_atomic;
use crate::state::App;

/// Thumbnail fetches in flight at once. A grid of photos asks for many at a
/// time; through castle+ every one is its own TLS connection.
static IN_FLIGHT: Semaphore = Semaphore::const_new(4);

pub fn protocol(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let handle = ctx.app_handle().clone();
    let blob = request.uri().path().trim_start_matches('/').to_owned();
    tauri::async_runtime::spawn(async move {
        let found = match handle.try_state::<Arc<App>>() {
            Some(app) => {
                let app = app.inner().clone();
                fetch(&app, &blob).await
            }
            None => None,
        };
        let response = match found {
            Some(jpeg) => Response::builder()
                .header(header::CONTENT_TYPE, "image/jpeg")
                .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                .body(jpeg),
            None => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
        };
        responder.respond(response.expect("a response from fixed parts is valid"));
    });
}

/// The most `save` takes: 256 px JPEGs are a few tens of KiB.
const MAX_THUMB: usize = 256 << 10;

/// Keep a thumbnail the page drew, here and (if this device may write) in the
/// store, where every device finds it. `true` if it reached the store.
pub async fn save(app: &App, blob: &str, jpeg: Vec<u8>) -> Result<bool, CmdError> {
    let key = keys::thumb(blob).map_err(|e| CmdError::new("invalid", e.to_string()))?;
    if jpeg.len() > MAX_THUMB || !jpeg.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err(CmdError::new("invalid", "a thumbnail must be a JPEG under 256 KiB"));
    }
    let dir = app.cache_dir.join("thumbs");
    fs::create_dir_all(&dir)?;
    write_atomic(&dir.join(format!("{blob}.jpg")), &jpeg)?;
    let _ = fs::remove_file(dir.join(format!("{blob}.none")));
    let Some(remote) = app.remote() else { return Ok(false) };
    match remote.put(&key, Kind::Img, jpeg.into()).await {
        Ok(_) => Ok(true),
        // A read-only key: it stays a local thumbnail.
        Err(remote::Error::Forbidden) => Ok(false),
        Err(e) => Err(drawbridge_core::Error::Remote(e).into()),
    }
}

/// Bytes the thumbnail cache holds.
pub fn usage(app: &App) -> u64 {
    let Ok(dir) = fs::read_dir(app.cache_dir.join("thumbs")) else { return 0 };
    dir.filter_map(|e| e.ok()?.metadata().ok()).map(|m| m.len()).sum()
}

pub fn clear(app: &App) {
    let _ = fs::remove_dir_all(app.cache_dir.join("thumbs"));
}

async fn fetch(app: &App, blob: &str) -> Option<Vec<u8>> {
    // Also what keeps `blob` safe to use in a file name: one key segment.
    let key = keys::thumb(blob).ok()?;
    let dir = app.cache_dir.join("thumbs");
    let hit = dir.join(format!("{blob}.jpg"));
    let miss = dir.join(format!("{blob}.none"));
    if let Ok(jpeg) = fs::read(&hit) {
        return Some(jpeg);
    }
    if miss.exists() {
        return None;
    }
    let remote = app.remote()?;
    let _turn = IN_FLIGHT.acquire().await.ok()?;
    match remote.get(&key).await {
        Ok(Some(value)) => {
            let _ = fs::create_dir_all(&dir).and_then(|()| write_atomic(&hit, &value.bytes));
            Some(value.bytes.to_vec())
        }
        Ok(None) => {
            let _ = fs::create_dir_all(&dir).and_then(|()| fs::write(&miss, b""));
            None
        }
        // Offline, say: try again next time it's shown.
        Err(_) => None,
    }
}
