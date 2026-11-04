// Claude — Date 11/03/2026
// The `thumb://` protocol: `<img src="thumb://localhost/<blob>">` in the
// webview loads that blob's thumbnail. Each one is fetched from the store once
// and then served from a folder in the app's cache: a blob never changes, so
// neither does its thumbnail.
//
// A miss is remembered too. Thumbnails are written before their blob's
// manifest (core's blob.rs), so a photo without one now never gets one; it was
// uploaded before thumbnails existed, or couldn't be decoded.

use std::fs;
use std::sync::Arc;

use drawbridge_core::keys;
use tauri::http::{Request, Response, StatusCode, header};
use tauri::{Manager, UriSchemeContext, UriSchemeResponder, Wry};
use tokio::sync::Semaphore;

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
