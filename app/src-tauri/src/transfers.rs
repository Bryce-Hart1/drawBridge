// Claude — Date 11/03/2026
// The transfer queue: uploads and downloads, one file at a time, each with
// several chunks in flight inside it (core's blob.rs). Every change of state,
// and every chunk done, is a `transfer` event for the UI's transfer line.
//
// Uploads come in as paths from the file picker or a drop. Planning one turns
// those paths into folders to make and files to send, and finds names already
// taken in the folders they're going into. With `OnClash::Ask` those clashes
// come back to the UI instead of anything being queued, and it asks once:
// replace, keep both, or skip.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::oplog::check_name;
use drawbridge_core::{Dest, Entry, Error, Progress, View, index};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::commands::CmdError;
use crate::state::{App, lock};

/// Finished transfers kept for the UI to show.
const KEEP: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    Queued,
    Running,
    Done,
    Failed,
    Stopped,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: u64,
    pub dir: Dir,
    pub name: String,
    pub done: u64,
    pub total: u64,
    pub state: TransferState,
    pub error: Option<String>,
}

pub enum Job {
    Upload { path: PathBuf, dest: Dest },
    /// An upload a quit interrupted, by its blob id: it goes where it was going.
    Resume { blob: String },
    Download { blob: String, out: PathBuf },
}

#[derive(Default)]
pub struct Transfers {
    inner: Mutex<Inner>,
    wake: Notify,
}

#[derive(Default)]
struct Inner {
    next: u64,
    queue: VecDeque<(u64, Job)>,
    all: BTreeMap<u64, Transfer>,
    stops: HashMap<u64, Arc<AtomicBool>>,
    /// Stopped by the user, so the partial upload is deleted rather than kept
    /// for resuming.
    cancelled: HashSet<u64>,
}

impl Transfers {
    pub fn add(&self, app: &App, dir: Dir, name: &str, total: u64, job: Job) -> u64 {
        let t = {
            let mut g = lock(&self.inner);
            g.next += 1;
            let id = g.next;
            let t = Transfer { id, dir, name: name.to_owned(), done: 0, total, state: TransferState::Queued, error: None };
            g.all.insert(id, t.clone());
            g.queue.push_back((id, job));
            g.stops.insert(id, Arc::new(AtomicBool::new(false)));
            t
        };
        app.emit("transfer", &t);
        self.wake.notify_one();
        t.id
    }

    pub fn list(&self) -> Vec<Transfer> {
        lock(&self.inner).all.values().cloned().collect()
    }

    /// Stop a transfer and throw away what it did: a queued one never starts,
    /// a running one stops once its chunks in flight land.
    pub fn cancel(&self, app: &App, id: u64) {
        let was_queued = {
            let mut g = lock(&self.inner);
            if let Some(at) = g.queue.iter().position(|(i, _)| *i == id) {
                g.queue.remove(at);
                true
            } else {
                if let Some(stop) = g.stops.get(&id) {
                    stop.store(true, Ordering::Relaxed);
                    g.cancelled.insert(id);
                }
                false
            }
        };
        if was_queued {
            self.finish(app, id, TransferState::Stopped, None);
        }
    }

    /// Stop everything, keeping what uploads have sent, for disconnecting. The
    /// uploads resume when the device connects again.
    pub fn stop_all(&self, app: &App) {
        let queued: Vec<u64> = {
            let mut g = lock(&self.inner);
            for stop in g.stops.values() {
                stop.store(true, Ordering::Relaxed);
            }
            g.queue.drain(..).map(|(id, _)| id).collect()
        };
        for id in queued {
            self.finish(app, id, TransferState::Stopped, None);
        }
    }

    fn next(&self) -> Option<(u64, Job, Arc<AtomicBool>)> {
        let mut g = lock(&self.inner);
        let (id, job) = g.queue.pop_front()?;
        let stop = g.stops.get(&id).cloned().unwrap_or_default();
        Some((id, job, stop))
    }

    fn update(&self, app: &App, id: u64, change: impl FnOnce(&mut Transfer)) {
        let t = {
            let mut g = lock(&self.inner);
            let Some(t) = g.all.get_mut(&id) else { return };
            change(t);
            t.clone()
        };
        app.emit("transfer", &t);
    }

    fn finish(&self, app: &App, id: u64, state: TransferState, error: Option<String>) {
        self.update(app, id, |t| {
            t.state = state;
            t.error = error;
            if state == TransferState::Done {
                t.done = t.total;
            }
        });
        let mut g = lock(&self.inner);
        g.stops.remove(&id);
        g.cancelled.remove(&id);
        while g.all.len() > KEEP {
            let Some((&oldest, _)) = g.all.iter().find(|(_, t)| !matches!(t.state, TransferState::Queued | TransferState::Running)) else { break };
            g.all.remove(&oldest);
        }
    }

    fn take_cancelled(&self, id: u64) -> bool {
        lock(&self.inner).cancelled.remove(&id)
    }
}

pub fn spawn(app: Arc<App>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match app.transfers.next() {
                Some((id, job, stop)) => run(&app, id, job, &stop).await,
                None => app.transfers.wake.notified().await,
            }
        }
    });
}

async fn run(app: &App, id: u64, job: Job, stop: &AtomicBool) {
    let t = &app.transfers;
    if stop.load(Ordering::Relaxed) {
        return t.finish(app, id, TransferState::Stopped, None);
    }
    let Some(remote) = app.remote() else {
        return t.finish(app, id, TransferState::Failed, Some("not connected".into()));
    };
    let db = app.db();
    t.update(app, id, |t| t.state = TransferState::Running);
    let progress = |p: Progress| {
        t.update(app, id, |t| {
            t.done = p.done;
            t.total = p.total;
        })
    };
    let result = match &job {
        Job::Upload { path, dest } => {
            blob::upload(&remote, &db, path, dest.clone(), UploadOptions::default(), &progress, stop).await.map(drop)
        }
        Job::Resume { blob } => blob::resume(&remote, &db, blob, 3, &progress, stop).await.map(drop),
        Job::Download { blob, out } => match blob::manifest(&remote, blob).await {
            Ok(m) => blob::download(&remote, &m, out, 4, &progress, stop).await,
            Err(e) => Err(e),
        },
    };
    match result {
        Ok(()) => {
            if !matches!(job, Job::Download { .. }) {
                app.changed_here();
            }
            t.finish(app, id, TransferState::Done, None);
        }
        Err(Error::Stopped) => {
            if t.take_cancelled(id) {
                discard(&remote, &db, &job).await;
            }
            t.finish(app, id, TransferState::Stopped, None);
        }
        Err(e) => t.finish(app, id, TransferState::Failed, Some(e.to_string())),
    }
}

/// Delete what a cancelled upload sent. Downloads clean up after themselves.
async fn discard(remote: &drawbridge_core::Remote, db: &drawbridge_core::Db, job: &Job) {
    let blob = match job {
        Job::Resume { blob } => Some(blob.clone()),
        Job::Upload { path, .. } => {
            let path = fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            db.pending_uploads().ok().and_then(|ps| ps.into_iter().find(|p| p.path == path)).map(|p| p.blob)
        }
        Job::Download { .. } => None,
    };
    if let Some(blob) = blob {
        let _ = blob::cancel(remote, db, &blob).await;
    }
}

// ---- planning uploads ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnClash {
    Ask,
    Replace,
    Keep,
    Skip,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadPlan {
    pub queued: usize,
    pub skipped: usize,
    pub clashes: Vec<String>,
}

/// The folder something goes into: one that exists (`None` is the root), or
/// the n-th folder this plan makes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Target {
    Folder(Option<String>),
    New(usize),
}

/// What already has a planned file's name where it's going.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Taken {
    File(String),
    Folder,
    /// Another file in the same upload.
    Upload,
}

#[derive(Debug)]
struct NewFolder {
    parent: Target,
    name: String,
}

#[derive(Debug)]
struct PlannedFile {
    path: PathBuf,
    name: String,
    size: u64,
    target: Target,
    taken: Option<Taken>,
}

#[derive(Debug, Default)]
struct Plan {
    folders: Vec<NewFolder>,
    files: Vec<PlannedFile>,
    skipped: usize,
    claimed: HashMap<Target, HashSet<String>>,
}

/// Files that aren't anyone's content: Finder's and Windows' folder litter.
fn junk(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini") || name.starts_with("._")
}

fn child<'v>(view: &'v View, target: &Target, name: &str) -> Option<&'v Entry> {
    match target {
        Target::Folder(dir) => view.children(dir.as_deref()).find(|e| e.name() == name),
        Target::New(_) => None,
    }
}

impl Plan {
    fn new(view: &View, paths: &[PathBuf], parent: Option<&str>) -> Plan {
        let mut plan = Plan::default();
        for path in paths {
            plan.add(view, path, Target::Folder(parent.map(str::to_owned)));
        }
        plan
    }

    fn add(&mut self, view: &View, path: &Path, target: Target) {
        let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
        let Some(name) = name.filter(|n| check_name(n).is_ok()) else {
            self.skipped += 1;
            return;
        };
        if junk(&name) {
            return;
        }
        let Ok(meta) = fs::symlink_metadata(path) else {
            self.skipped += 1;
            return;
        };
        if meta.is_dir() {
            // Into a folder of the same name if there is one, as `cp -r` would.
            let folder = match child(view, &target, &name) {
                Some(e) if e.is_folder() => Target::Folder(Some(e.id.clone())),
                existing => {
                    // A file has the name: the folder becomes `name (2)`.
                    let name = if existing.is_some() {
                        index::free_name(&name, |n| child(view, &target, n).is_some())
                    } else {
                        name
                    };
                    self.folders.push(NewFolder { parent: target, name });
                    Target::New(self.folders.len() - 1)
                }
            };
            let mut inside: Vec<PathBuf> = fs::read_dir(path).into_iter().flatten().flatten().map(|d| d.path()).collect();
            inside.sort();
            for p in inside {
                self.add(view, &p, folder.clone());
            }
        } else if meta.is_file() {
            let claimed = self.claimed.entry(target.clone()).or_default();
            let taken = match child(view, &target, &name) {
                Some(e) if e.is_file() => Some(Taken::File(e.id.clone())),
                Some(_) => Some(Taken::Folder),
                None if claimed.contains(&name) => Some(Taken::Upload),
                None => None,
            };
            claimed.insert(name.clone());
            self.files.push(PlannedFile { path: path.to_owned(), name, size: meta.len(), target, taken });
        } else {
            // Symlinks, sockets, devices.
            self.skipped += 1;
        }
    }
}

/// Plan an upload of `paths` into `parent` and, unless it must ask first,
/// make its folders and queue its files.
pub fn upload(app: &App, paths: Vec<PathBuf>, parent: Option<String>, on_clash: OnClash) -> Result<UploadPlan, CmdError> {
    let view = app.view();
    if let Some(p) = &parent
        && !view.get(p).is_some_and(Entry::is_folder)
    {
        return Err(CmdError::new("not_found", "that folder isn't here any more"));
    }
    let plan = Plan::new(&view, &paths, parent.as_deref());
    let clashes: Vec<String> = plan.files.iter().filter(|f| f.taken.is_some()).map(|f| f.name.clone()).collect();
    if on_clash == OnClash::Ask && !clashes.is_empty() {
        return Ok(UploadPlan { queued: 0, skipped: 0, clashes });
    }

    let db = app.db();
    let mut made: Vec<String> = Vec::with_capacity(plan.folders.len());
    let dir_of = |target: &Target, made: &[String]| match target {
        Target::Folder(dir) => dir.clone(),
        Target::New(i) => Some(made[*i].clone()),
    };
    // Planned parents first, so each one's parent already exists.
    for f in &plan.folders {
        made.push(index::mkdir(&db, dir_of(&f.parent, &made).as_deref(), &f.name)?);
    }

    let mut skipped = plan.skipped;
    let mut queued = 0;
    // Names this upload has given out, per folder, for keep-both.
    let mut given: HashMap<Option<String>, HashSet<String>> = HashMap::new();
    for f in plan.files {
        let dir = dir_of(&f.target, &made);
        let used = given.entry(dir.clone()).or_default();
        let dest = match (&f.taken, on_clash) {
            (None, _) => Dest::New { parent: dir.clone(), name: f.name.clone() },
            (Some(Taken::File(id)), OnClash::Replace) => Dest::Replace { id: id.clone() },
            (Some(_), OnClash::Keep) => {
                let name = index::free_name(&f.name, |n| {
                    used.contains(n) || view.children(dir.as_deref()).any(|e| e.name() == n)
                });
                Dest::New { parent: dir.clone(), name }
            }
            // Skipped, or a folder (or another file of this upload) has the name.
            _ => {
                skipped += 1;
                continue;
            }
        };
        let shown = match &dest {
            Dest::New { name, .. } => name.clone(),
            _ => f.name.clone(),
        };
        used.insert(shown.clone());
        app.transfers.add(app, Dir::Up, &shown, f.size, Job::Upload { path: f.path, dest });
        queued += 1;
    }
    if !made.is_empty() {
        app.changed_here();
    }
    Ok(UploadPlan { queued, skipped, clashes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use drawbridge_core::Db;

    fn tree(root: &Path, files: &[&str]) {
        for f in files {
            let path = root.join(f);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }
    }

    #[test]
    fn plans_folders_files_and_clashes() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg", "trip/day2/b.jpg", "trip/.DS_Store", "trip/._a.jpg", "notes.md", "Photos/c.jpg"]);
        let db = Db::open_in_memory().unwrap();
        db.init_device("t").unwrap();
        let photos = index::mkdir(&db, None, "Photos").unwrap();
        index::mkdir(&db, None, "notes.md").unwrap(); // a folder with a file's name
        let view = db.view().unwrap();

        let paths = ["trip", "notes.md", "Photos"].map(|p| src.path().join(p));
        let plan = Plan::new(&view, &paths, None);

        let folders: Vec<(&str, &Target)> = plan.folders.iter().map(|f| (f.name.as_str(), &f.parent)).collect();
        assert_eq!(folders, [("trip", &Target::Folder(None)), ("day2", &Target::New(0))]);
        let files: Vec<(&str, &Target, &Option<Taken>)> =
            plan.files.iter().map(|f| (f.name.as_str(), &f.target, &f.taken)).collect();
        assert_eq!(
            files,
            [
                ("a.jpg", &Target::New(0), &None),
                ("b.jpg", &Target::New(1), &None),
                ("notes.md", &Target::Folder(None), &Some(Taken::Folder)),
                // Photos/ already exists: c.jpg goes into it.
                ("c.jpg", &Target::Folder(Some(photos)), &None),
            ]
        );
        assert_eq!(plan.skipped, 0, "litter is left out without counting");
    }

    #[test]
    fn the_same_name_twice_in_one_upload_clashes() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["one/x.jpg", "two/x.jpg"]);
        let db = Db::open_in_memory().unwrap();
        let view = db.view().unwrap();
        let paths = ["one/x.jpg", "two/x.jpg"].map(|p| src.path().join(p));
        let plan = Plan::new(&view, &paths, None);
        let taken: Vec<&Option<Taken>> = plan.files.iter().map(|f| &f.taken).collect();
        assert_eq!(taken, [&None, &Some(Taken::Upload)]);
    }

    #[test]
    fn odd_entries_are_skipped() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["real.txt"]);
        #[cfg(unix)]
        std::os::unix::fs::symlink(src.path().join("real.txt"), src.path().join("link.txt")).unwrap();
        let view = Db::open_in_memory().unwrap().view().unwrap();
        let paths = [src.path().join("link.txt"), src.path().join("gone.txt"), src.path().join("real.txt")];
        let plan = Plan::new(&view, &paths, None);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.skipped, 2);
    }
}
