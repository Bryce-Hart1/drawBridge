// Claude — Date 11/03/2026
// The commands the UI invokes (app/src/lib/api.ts), one function each. The
// shapes they return mirror app/src/lib/types.ts; change both together.
//
// Each is async, so none runs on the main thread: those that touch the Db are
// quick but still disk I/O, and the rest wait on the store.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::{ByteRange, Dest, Entry, EntryKind, Error, Remote, View, index, keys, remote};
use serde::{Deserialize, Serialize};

use crate::state::{App, Status};
use crate::transfers::{self, Dir, Job, OnClash, Transfer, UploadPlan};

type State<'a> = tauri::State<'a, Arc<App>>;
type Result<T> = std::result::Result<T, CmdError>;

/// Notes opened in the editor stay this small; the UI says the same.
const NOTE_LIMIT: u64 = 1 << 20;

/// How a command fails, for the UI: a `kind` it can act on, and a message for
/// people. Kinds: invalid, not_found, not_connected, unauthorized, forbidden,
/// offline, remote, other.
#[derive(Debug, Serialize)]
pub struct CmdError {
    kind: &'static str,
    message: String,
}

impl CmdError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> CmdError {
        CmdError { kind, message: message.into() }
    }
}

impl From<Error> for CmdError {
    fn from(e: Error) -> CmdError {
        let kind = match &e {
            Error::Invalid(_) => "invalid",
            Error::NotFound(_) => "not_found",
            Error::NoDevice => "not_connected",
            Error::Remote(remote::Error::Unauthorized) => "unauthorized",
            Error::Remote(remote::Error::Forbidden) => "forbidden",
            Error::Remote(remote::Error::Transport(_)) => "offline",
            Error::Remote(_) => "remote",
            _ => "other",
        };
        CmdError::new(kind, e.to_string())
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> CmdError {
        CmdError::from(Error::Io(e))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    id: String,
    name: String,
    kind: EntryKind,
    size: Option<u64>,
    mime: Option<String>,
    blob: Option<String>,
    count: Option<usize>,
}

fn item(view: &View, e: &Entry) -> Item {
    let content = e.content();
    Item {
        id: e.id.clone(),
        name: e.name().to_owned(),
        kind: e.kind().unwrap_or(EntryKind::File),
        size: content.map(|c| c.size),
        mime: content.map(|c| c.mime.clone()),
        blob: content.map(|c| c.blob.clone()),
        count: e.is_folder().then(|| view.children(Some(&e.id)).count()),
    }
}

#[derive(Serialize)]
pub struct Crumb {
    id: String,
    name: String,
}

#[derive(Serialize)]
pub struct Listing {
    dir: Option<String>,
    path: Vec<Crumb>,
    items: Vec<Item>,
}

#[derive(Serialize)]
pub struct Hit {
    item: Item,
    parent: Option<String>,
    path: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum NoteTarget {
    Existing { id: String },
    New { parent: Option<String>, name: String },
}

#[tauri::command]
pub async fn status(app: State<'_>) -> Result<Status> {
    Ok(app.status())
}

/// Check the address answers and the key is accepted, then save both and
/// start syncing. Nothing is saved if either check fails.
#[tauri::command]
pub async fn connect(app: State<'_>, url: String, key: String, device_name: String) -> Result<Status> {
    let (key, name) = (key.trim(), device_name.trim());
    let remote = Remote::new(url.trim(), key).map_err(|e| CmdError::from(Error::Remote(e)))?;
    if let Err(e) = remote.health().await {
        return Err(CmdError::new("offline", format!("can't reach {}: {e}", remote.base())));
    }
    match remote.head(&keys::devices()).await {
        Ok(_) => {}
        Err(remote::Error::Unauthorized) => return Err(CmdError::new("unauthorized", "the server rejected this key")),
        Err(e) => return Err(Error::Remote(e).into()),
    }
    app.connect(remote, key, name)?;
    app.syncer.now();
    app.resume_uploads();
    Ok(app.status())
}

#[tauri::command]
pub async fn disconnect(app: State<'_>) -> Result<()> {
    app.disconnect();
    Ok(())
}

#[tauri::command]
pub async fn list(app: State<'_>, dir: Option<String>) -> Result<Listing> {
    let view = app.view();
    if let Some(d) = &dir
        && !view.get(d).is_some_and(Entry::is_folder)
    {
        return Err(CmdError::new("not_found", "that folder isn't here any more"));
    }
    let mut items: Vec<Item> = view.children(dir.as_deref()).map(|e| item(&view, e)).collect();
    // Folders first, then by name ignoring case.
    items.sort_by_cached_key(|i| (i.kind != EntryKind::Folder, i.name.to_lowercase(), i.id.clone()));
    let mut path = Vec::new();
    let mut at = dir.clone();
    while let Some(id) = at {
        let name = view.get(&id).map(|e| e.name().to_owned()).unwrap_or_default();
        at = view.parent_of(&id).map(str::to_owned);
        path.push(Crumb { id, name });
    }
    path.reverse();
    Ok(Listing { dir, path, items })
}

#[tauri::command]
pub async fn search(app: State<'_>, query: String) -> Result<Vec<Hit>> {
    let view = app.view();
    let hits = index::search(&view, &query, 200)
        .into_iter()
        .map(|h| Hit {
            item: item(&view, h.entry),
            parent: h.parent.map(str::to_owned),
            path: h.parent.and_then(|p| view.path(p)).unwrap_or_else(|| "/".into()),
        })
        .collect();
    Ok(hits)
}

#[tauri::command]
pub async fn mkdir(app: State<'_>, parent: Option<String>, name: String) -> Result<String> {
    let id = index::mkdir(&app.db(), parent.as_deref(), name.trim())?;
    app.changed_here();
    Ok(id)
}

#[tauri::command]
pub async fn rename(app: State<'_>, id: String, name: String) -> Result<()> {
    index::rename(&app.db(), &id, name.trim())?;
    app.changed_here();
    Ok(())
}

#[tauri::command]
pub async fn move_to(app: State<'_>, id: String, parent: Option<String>) -> Result<()> {
    index::move_to(&app.db(), &id, parent.as_deref(), None)?;
    app.changed_here();
    Ok(())
}

#[tauri::command]
pub async fn remove(app: State<'_>, id: String) -> Result<usize> {
    let n = index::remove(&app.db(), &id)?;
    app.changed_here();
    Ok(n)
}

#[tauri::command]
pub async fn upload(app: State<'_>, paths: Vec<PathBuf>, parent: Option<String>, on_clash: OnClash) -> Result<UploadPlan> {
    let app = app.inner().clone();
    // Walking a dropped folder reads the disk: keep it off the async threads.
    tauri::async_runtime::spawn_blocking(move || transfers::upload(&app, paths, parent, on_clash))
        .await
        .map_err(|e| CmdError::new("other", e.to_string()))?
}

#[tauri::command]
pub async fn download(app: State<'_>, id: String, path: PathBuf) -> Result<u64> {
    let view = app.view();
    let e = view.get(&id).ok_or_else(|| CmdError::new("not_found", "that file isn't here any more"))?;
    let c = e.content().ok_or_else(|| CmdError::new("invalid", format!("{} is a folder", e.name())))?;
    let job = Job::Download { blob: c.blob.clone(), out: path };
    Ok(app.transfers.add(&app, Dir::Down, e.name(), c.size, job))
}

#[tauri::command]
pub async fn cancel(app: State<'_>, transfer: u64) -> Result<()> {
    app.transfers.cancel(&app, transfer);
    Ok(())
}

#[tauri::command]
pub async fn transfers(app: State<'_>) -> Result<Vec<Transfer>> {
    Ok(app.transfers.list())
}

#[tauri::command]
pub async fn note_read(app: State<'_>, id: String) -> Result<String> {
    let (name, blob, size) = {
        let view = app.view();
        let e = view.get(&id).ok_or_else(|| CmdError::new("not_found", "that note isn't here any more"))?;
        let c = e.content().ok_or_else(|| CmdError::new("invalid", format!("{} is a folder", e.name())))?;
        (e.name().to_owned(), c.blob.clone(), c.size)
    };
    if size > NOTE_LIMIT {
        return Err(CmdError::new("invalid", format!("{name} is too big to edit here")));
    }
    let remote = app.require_remote()?;
    let m = blob::manifest(&remote, &blob).await?;
    let bytes = if m.size == 0 { Default::default() } else { blob::read_range(&remote, &m, ByteRange::From(0)).await? };
    String::from_utf8(bytes.to_vec()).map_err(|_| CmdError::new("invalid", format!("{name} isn't text")))
}

/// Save a note: its text becomes the file's new content, or a new file. It
/// goes up like any upload, from a temporary file, so it gets the same
/// manifest and resumability; it's just small enough to wait for.
#[tauri::command]
pub async fn note_save(app: State<'_>, target: NoteTarget, text: String) -> Result<String> {
    let remote = app.require_remote()?;
    let db = app.db();
    let (dest, name) = match target {
        NoteTarget::Existing { id } => {
            let name = app.view().get(&id).map(|e| e.name().to_owned());
            let name = name.ok_or_else(|| CmdError::new("not_found", "that note isn't here any more"))?;
            (Dest::Replace { id }, name)
        }
        NoteTarget::New { parent, name } => (Dest::New { parent, name: name.clone() }, name),
    };
    // Named for the type the extension gives (mime.rs); nothing else of the
    // name reaches the file system.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let ext: String = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("").chars().filter(char::is_ascii_alphanumeric).collect();
    let dir = app.cache_dir.join("notes").join(format!("{}-{}", crate::state::now_ms(), NEXT.fetch_add(1, Ordering::Relaxed)));
    fs::create_dir_all(&dir)?;
    let path = dir.join(if ext.is_empty() { "note".to_owned() } else { format!("note.{ext}") });
    fs::write(&path, text)?;
    let up = blob::upload(&remote, &db, &path, dest, UploadOptions::default(), &|_| {}, &AtomicBool::new(false)).await;
    let _ = fs::remove_dir_all(&dir);
    let up = up?;
    app.changed_here();
    up.entry.ok_or_else(|| CmdError::new("other", "the note was saved without an entry"))
}

#[tauri::command]
pub async fn sync_now(app: State<'_>) -> Result<()> {
    app.syncer.now();
    Ok(())
}
