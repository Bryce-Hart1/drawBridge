// Claude — Date 11/03/2026
// The app's state, shared by every command, the sync loop, the transfer queue
// and the thumbnail protocol: the local Db, the store (once connected), the
// tree as last read from the Db, the sync status the title bar shows, and the
// chunk cache and stream server the viewer reads through.
//
// The tree is kept as one `Arc<View>`, rebuilt whenever the index changes, so
// listing a folder or searching never re-reads the Db.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use drawbridge_core::cache::Cache;
use drawbridge_core::stream::{self as streaming, StreamConfig};
use drawbridge_core::sync::{self, SyncOptions};
use drawbridge_core::{Db, Error, Remote, View, remote};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::commands::CmdError;
use crate::config::{self, Config};
use crate::sync_loop::{Kick, Syncer};
use crate::transfers::{Dir, Job, Transfers};

const DB_FILE: &str = "state.sqlite";

/// The chunk cache's cap until the settings say otherwise.
pub const DEFAULT_CACHE_CAP: u64 = 2 << 30;

/// The stream server stops after this long without a request.
const STREAM_IDLE: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    #[default]
    Idle,
    Syncing,
    Offline,
    Error,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: SyncState,
    pub last_ok_ms: Option<u64>,
    pub message: Option<String>,
    pub read_only: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub connected: bool,
    pub url: Option<String>,
    pub device: Option<String>,
    pub suggested_name: String,
    pub sync: SyncStatus,
}

pub struct App {
    pub handle: AppHandle,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    db: RwLock<Arc<Db>>,
    remote: RwLock<Option<Arc<Remote>>>,
    config: RwLock<Option<Config>>,
    view: RwLock<Arc<View>>,
    sync: Mutex<SyncStatus>,
    pub syncer: Syncer,
    pub transfers: Transfers,
    pub cache: Arc<Cache>,
    /// Started on the first stream, and again after it stops for being idle.
    stream: tokio::sync::Mutex<Option<streaming::Server>>,
}

impl App {
    pub fn open(handle: AppHandle) -> Result<(App, mpsc::UnboundedReceiver<Kick>), Box<dyn std::error::Error>> {
        let data_dir = handle.path().app_data_dir()?;
        let cache_dir = handle.path().app_cache_dir()?;
        fs::create_dir_all(&data_dir)?;
        fs::create_dir_all(&cache_dir)?;
        let db = Db::open(&data_dir.join(DB_FILE))?;
        let view = db.view()?;
        let config = config::load(&data_dir);
        let remote = config.as_ref().and_then(|c| Remote::new(&c.url, &config::load_key(&data_dir, &c.url)?).ok());
        let cap = config.as_ref().and_then(|c| c.cache_cap).unwrap_or(DEFAULT_CACHE_CAP);
        let cache = Cache::open(&cache_dir.join("chunks"), cap)?;
        let (syncer, kicks) = Syncer::new();
        let app = App {
            handle,
            data_dir,
            cache_dir,
            db: RwLock::new(Arc::new(db)),
            remote: RwLock::new(remote.map(Arc::new)),
            config: RwLock::new(config),
            view: RwLock::new(Arc::new(view)),
            sync: Mutex::new(SyncStatus::default()),
            syncer,
            transfers: Transfers::default(),
            cache,
            stream: tokio::sync::Mutex::default(),
        };
        Ok((app, kicks))
    }

    pub fn db(&self) -> Arc<Db> {
        read(&self.db).clone()
    }

    pub fn remote(&self) -> Option<Arc<Remote>> {
        read(&self.remote).clone()
    }

    pub fn require_remote(&self) -> Result<Arc<Remote>, CmdError> {
        self.remote().ok_or_else(|| CmdError::new("not_connected", "this device isn't connected to a store"))
    }

    /// Change and remember the chunk cache's cap.
    pub fn set_cache_cap(&self, cap: u64) -> std::io::Result<()> {
        self.cache.set_cap(cap);
        let mut config = write(&self.config);
        if let Some(c) = config.as_mut() {
            c.cache_cap = Some(cap);
            config::save(&self.data_dir, c)?;
        }
        Ok(())
    }

    /// Where the stream server serves `blob`, starting it if it isn't running.
    pub async fn stream_url(&self, blob: &str, name: &str) -> Result<String, CmdError> {
        let mut server = self.stream.lock().await;
        if !server.as_ref().is_some_and(streaming::Server::is_running) {
            let handle = self.handle.clone();
            let cfg = StreamConfig {
                cache: self.cache.clone(),
                remote: Arc::new(move || handle.try_state::<Arc<App>>()?.remote()),
                origins: app_origins(),
                idle: STREAM_IDLE,
            };
            // The same port and token as last time, so the page's URLs still work.
            let reuse = server.as_ref().map(|s| (s.port(), s.token().to_owned()));
            *server = Some(streaming::Server::start(cfg, reuse).await?);
        }
        Ok(server.as_ref().expect("just started").url(blob, name))
    }

    pub fn view(&self) -> Arc<View> {
        read(&self.view).clone()
    }

    pub fn status(&self) -> Status {
        let config = read(&self.config).clone();
        let device = self.db().device().ok().flatten();
        Status {
            connected: self.remote().is_some() && device.is_some(),
            url: config.as_ref().map(|c| c.url.clone()),
            device: device.map(|d| d.name).or(config.map(|c| c.device_name)),
            suggested_name: config::computer_name(),
            sync: lock(&self.sync).clone(),
        }
    }

    pub fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        // Only fails once the window is gone, when nobody is listening anyway.
        let _ = self.handle.emit(event, payload);
    }

    /// Re-read the tree and tell the UI. After anything that changes the index.
    pub fn index_changed(&self) {
        match self.db().view() {
            Ok(view) => *write(&self.view) = Arc::new(view),
            Err(e) => eprintln!("drawBridge: can't read the index: {e}"),
        }
        self.emit("index-changed", ());
    }

    /// A change made on this device: show it now, push it soon.
    pub fn changed_here(&self) {
        self.index_changed();
        self.syncer.soon();
    }

    fn set_sync(&self, change: impl FnOnce(&mut SyncStatus)) {
        let status = {
            let mut s = lock(&self.sync);
            change(&mut s);
            s.clone()
        };
        self.emit("sync-status", status);
    }

    /// The window is on screen: not hidden, not minimized. Syncing on a timer
    /// happens only then.
    pub fn visible(&self) -> bool {
        self.handle
            .get_webview_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(true))
    }

    /// One sync. Only the sync loop calls this, so syncs never overlap.
    pub async fn run_sync(&self) {
        let Some(remote) = self.remote() else { return };
        let db = self.db();
        if !matches!(db.device(), Ok(Some(_))) {
            return;
        }
        self.set_sync(|s| s.state = SyncState::Syncing);
        match sync::sync(&remote, &db, &SyncOptions::default()).await {
            Ok(report) => {
                if report.pulled > 0 || report.bootstrapped > 0 {
                    self.index_changed();
                }
                self.set_sync(|s| {
                    s.state = SyncState::Idle;
                    s.last_ok_ms = Some(now_ms());
                    s.message = None;
                    s.read_only = report.read_only;
                });
            }
            Err(e) => {
                let offline = matches!(e, Error::Remote(remote::Error::Transport(_)));
                self.set_sync(|s| {
                    s.state = if offline { SyncState::Offline } else { SyncState::Error };
                    s.message = Some(e.to_string());
                });
            }
        }
    }

    /// Start using `remote`, already checked by the caller. A different store
    /// from the one this device's index came from starts a fresh index.
    pub fn connect(&self, remote: Remote, key: &str, name: &str) -> Result<(), CmdError> {
        let url = remote.base().to_owned();
        let previous = read(&self.config).clone();
        if previous.as_ref().is_some_and(|c| c.url != url) {
            self.reset_index()?;
        }
        let name = if name.is_empty() { config::computer_name() } else { name.to_owned() };
        let me = self.db().init_device(&name)?;
        config::save_key(&self.data_dir, &url, key)
            .map_err(|e| CmdError::new("other", format!("couldn't save the key: {e}")))?;
        let config = Config { url, device_name: me.name, cache_cap: previous.and_then(|c| c.cache_cap) };
        config::save(&self.data_dir, &config)?;
        *write(&self.config) = Some(config);
        *write(&self.remote) = Some(Arc::new(remote));
        self.index_changed();
        Ok(())
    }

    /// Forget the key and stop talking to the store. The index and the config
    /// stay, so connecting again picks up where this left off.
    pub fn disconnect(&self) {
        if let Some(c) = read(&self.config).as_ref() {
            config::forget_key(&self.data_dir, &c.url);
        }
        *write(&self.remote) = None;
        self.transfers.stop_all(self);
        self.set_sync(|s| *s = SyncStatus::default());
    }

    fn reset_index(&self) -> Result<(), CmdError> {
        self.transfers.stop_all(self);
        let path = self.data_dir.join(DB_FILE);
        // Let go of the old file before deleting it, then start a new one.
        *write(&self.db) = Arc::new(Db::open_in_memory()?);
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", path.display()));
        }
        *write(&self.db) = Arc::new(Db::open(&path)?);
        Ok(())
    }

    /// Queue every upload a quit or crash interrupted. They pick up where they
    /// stopped, chunk by chunk.
    pub fn resume_uploads(&self) {
        if self.remote().is_none() {
            return;
        }
        let Ok(pending) = self.db().pending_uploads() else { return };
        for p in pending {
            let name = p.path.file_name().map_or_else(|| p.blob.clone(), |n| n.to_string_lossy().into_owned());
            self.transfers.add(self, Dir::Up, &name, p.size, Job::Resume { blob: p.blob });
        }
    }
}

/// The page's own origins, which may read streamed bytes from script (to draw
/// a video frame for a poster): the bundled app on macOS and Linux, on
/// Windows, and the dev server.
fn app_origins() -> Vec<String> {
    let mut origins = vec!["tauri://localhost".to_owned(), "http://tauri.localhost".to_owned()];
    if cfg!(debug_assertions) {
        origins.push("http://127.0.0.1:1420".to_owned());
    }
    origins
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// A panic while holding one of these leaves nothing half-written that matters
// (each guards a single value swap), so a poisoned lock is still usable.
pub fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|p| p.into_inner())
}

pub fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|p| p.into_inner())
}

pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
