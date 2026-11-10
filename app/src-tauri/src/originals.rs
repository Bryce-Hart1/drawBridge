// Claude — Date 11/09/2026
// "Trash the originals": when an upload was started with that choice, the files
// and folders it was made from go to the Trash once they're safely on the
// store. Nothing here ever deletes for good.
//
// Nothing is trashed on hope. When the last upload of a batch has ended, each
// dropped item is checked, and goes only if everything inside it
//   - was planned for upload (no symlink, bad name or name clash left out),
//   - finished uploading (none failed, was cancelled or is still waiting), and
//   - still has the size and modified time it had when it was planned (an edit
//     made since would be lost with the original).
// A dropped folder goes whole or not at all: one stray file and it stays, with
// the reason in the `originals` event the UI shows. Finder's litter (.DS_Store
// and the like, never uploaded) is the one thing allowed to go along.
//
// A batch that uploads nothing (everything skipped) trashes nothing. If the
// app quits mid-batch the uploads resume on the next launch, but this choice
// isn't remembered: those originals stay.

use std::collections::{HashMap, HashSet};
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;

use crate::state::lock;
use crate::transfers::junk;

/// A file as planning saw it. If either part differs when the batch ends, it
/// was edited since, and its original holds an edit the store doesn't.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    pub fn of(meta: &Metadata) -> Stamp {
        Stamp { len: meta.len(), modified: meta.modified().ok() }
    }
}

/// What became of the originals, for the UI to say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// Names (folders with a trailing slash) now in the Trash.
    pub trashed: Vec<String>,
    pub kept: Vec<Kept>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Kept {
    pub name: String,
    pub why: String,
}

/// How a path is moved to the Trash, and how the outcome is told: swapped for
/// recorders in the tests.
pub type Trasher = Box<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;
pub type Reporter = Box<dyn Fn(Outcome) + Send + Sync>;

/// One batch's originals, shared by every upload job in it.
pub struct Originals {
    /// What was dropped or picked, the top level only.
    roots: Vec<PathBuf>,
    /// Every file queued for upload, as planned.
    stamps: HashMap<PathBuf, Stamp>,
    state: Mutex<State>,
    trash: Trasher,
    report: Reporter,
}

struct State {
    /// Upload jobs of the batch not yet ended.
    waiting: usize,
    /// Those that ended by finishing.
    uploaded: HashSet<PathBuf>,
}

impl Originals {
    /// `files` are the uploads queued, each with its stamp: all of them, before
    /// the first is queued, so a quick one can't end the batch early. With none
    /// there's no batch to settle: nothing uploaded, so nothing is trashed.
    pub fn new(mut roots: Vec<PathBuf>, files: Vec<(PathBuf, Stamp)>, trash: Trasher, report: Reporter) -> Arc<Originals> {
        roots.sort();
        roots.dedup();
        // A folder and something inside it both dropped: the folder covers it.
        let all = roots.clone();
        roots.retain(|r| !all.iter().any(|o| o != r && r.starts_with(o)));
        let waiting = files.len();
        Arc::new(Originals {
            roots,
            stamps: files.into_iter().collect(),
            state: Mutex::new(State { waiting, uploaded: HashSet::new() }),
            trash,
            report,
        })
    }

    /// An upload of the batch ended: finished (`ok`), or didn't. The last one
    /// to end settles the batch.
    pub fn ended(self: &Arc<Self>, path: &Path, ok: bool) {
        let last = {
            let mut s = lock(&self.state);
            if ok {
                s.uploaded.insert(path.to_owned());
            }
            s.waiting = s.waiting.saturating_sub(1);
            s.waiting == 0
        };
        if last {
            self.settle_later();
        }
    }

    /// Walking a tree and trashing it are disk work: not on the transfer queue's thread.
    fn settle_later(self: &Arc<Self>) {
        let this = self.clone();
        std::thread::spawn(move || (this.report)(this.settle()));
    }

    fn settle(&self) -> Outcome {
        let uploaded = lock(&self.state).uploaded.clone();
        let mut out = Outcome::default();
        for root in &self.roots {
            let mut name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
            if fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
                name.push('/');
            }
            let result = self.check(root, root, &uploaded).and_then(|()| {
                (self.trash)(root).map_err(|e| {
                    eprintln!("drawBridge: can't trash {}: {e}", root.display());
                    "the Trash wouldn't take it".to_owned()
                })
            });
            match result {
                Ok(()) => out.trashed.push(name),
                Err(why) => out.kept.push(Kept { name, why }),
            }
        }
        out
    }

    /// Whether everything at and under `path` is as uploaded. The error names
    /// the first thing that isn't.
    fn check(&self, path: &Path, root: &Path, uploaded: &HashSet<PathBuf>) -> Result<(), String> {
        let shown = || path.strip_prefix(root.parent().unwrap_or(root)).unwrap_or(path).display().to_string();
        let unreadable = |e: io::Error| format!("can't read {}: {}", shown(), e.kind());
        let meta = fs::symlink_metadata(path).map_err(unreadable)?;
        if meta.is_dir() {
            let mut inside = fs::read_dir(path)
                .and_then(|d| d.map(|e| e.map(|e| e.path())).collect::<io::Result<Vec<_>>>())
                .map_err(unreadable)?;
            inside.sort();
            inside.iter().try_for_each(|p| self.check(p, root, uploaded))
        } else if meta.is_file() {
            let litter = path != root && path.file_name().and_then(|n| n.to_str()).is_some_and(junk);
            match self.stamps.get(path) {
                _ if litter => Ok(()),
                None => Err(format!("{} wasn't uploaded", shown())),
                Some(_) if !uploaded.contains(path) => Err(format!("{} didn't finish uploading", shown())),
                Some(planned) if *planned != Stamp::of(&meta) => Err(format!("{} changed after it was queued", shown())),
                Some(_) => Ok(()),
            }
        } else {
            Err(format!("{} isn't a plain file", shown()))
        }
    }
}

/// Move `path` to the Trash. On macOS through NSFileManager, which asks for no
/// permission (the Finder route would), at the price of "Put Back" not
/// always working: the files can still be dragged out.
pub fn to_trash(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(path).map_err(io::Error::other)
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(path).map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch over `roots` whose queued files are `queued`, all stamped as they are now.
    /// Trashing is recorded, not done.
    fn batch(src: &Path, roots: &[&str], queued: &[&str]) -> (Arc<Originals>, Arc<Mutex<Vec<PathBuf>>>) {
        let trashed = Arc::new(Mutex::new(Vec::new()));
        let log = trashed.clone();
        let files = queued.iter().map(|f| src.join(f)).map(|p| {
            let stamp = Stamp::of(&fs::symlink_metadata(&p).unwrap());
            (p, stamp)
        });
        let o = Originals::new(
            roots.iter().map(|r| src.join(r)).collect(),
            files.collect(),
            Box::new(move |p| {
                log.lock().unwrap().push(p.to_owned());
                Ok(())
            }),
            Box::new(|_| {}),
        );
        (o, trashed)
    }

    fn tree(root: &Path, files: &[&str]) {
        for f in files {
            let path = root.join(f);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }
    }

    /// Mark every queued file of `o` as uploaded, as the queue does.
    fn upload_all(o: &Originals) {
        let paths: Vec<PathBuf> = o.stamps.keys().cloned().collect();
        lock(&o.state).uploaded.extend(paths);
    }

    #[test]
    fn a_folder_goes_whole_once_everything_in_it_is_up() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg", "trip/day2/b.jpg", "trip/.DS_Store", "one.txt"]);
        let (o, trashed) = batch(src.path(), &["trip", "one.txt"], &["trip/a.jpg", "trip/day2/b.jpg", "one.txt"]);
        upload_all(&o);
        let out = o.settle();
        assert_eq!(out.trashed, ["one.txt", "trip/"]);
        assert!(out.kept.is_empty());
        // The folder, litter and all, in one go: not file by file.
        assert_eq!(*trashed.lock().unwrap(), [src.path().join("one.txt"), src.path().join("trip")]);
    }

    #[test]
    fn one_file_that_didnt_finish_keeps_its_whole_folder() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg", "trip/b.jpg"]);
        let (o, trashed) = batch(src.path(), &["trip"], &["trip/a.jpg", "trip/b.jpg"]);
        lock(&o.state).uploaded.insert(src.path().join("trip/a.jpg"));
        let out = o.settle();
        assert!(out.trashed.is_empty());
        assert_eq!(out.kept, [Kept { name: "trip/".into(), why: "trip/b.jpg didn't finish uploading".into() }]);
        assert!(trashed.lock().unwrap().is_empty());
    }

    #[test]
    fn something_never_queued_keeps_the_folder() {
        // A name clash skipped, a file added since, anything not planned.
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg", "trip/skipped.jpg"]);
        let (o, trashed) = batch(src.path(), &["trip"], &["trip/a.jpg"]);
        upload_all(&o);
        let out = o.settle();
        assert_eq!(out.kept[0].why, "trip/skipped.jpg wasn't uploaded");
        assert!(trashed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_file_edited_after_it_was_queued_stays() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["a.txt"]);
        let (o, trashed) = batch(src.path(), &["a.txt"], &["a.txt"]);
        upload_all(&o);
        fs::write(src.path().join("a.txt"), b"edited since, and longer").unwrap();
        let out = o.settle();
        assert_eq!(out.kept[0].why, "a.txt changed after it was queued");
        assert!(trashed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_symlink_keeps_the_folder_and_is_never_followed() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg", "elsewhere/secret.txt"]);
        #[cfg(unix)]
        std::os::unix::fs::symlink(src.path().join("elsewhere"), src.path().join("trip/link")).unwrap();
        let (o, trashed) = batch(src.path(), &["trip"], &["trip/a.jpg"]);
        upload_all(&o);
        let out = o.settle();
        assert_eq!(out.kept[0].why, "trip/link isn't a plain file");
        assert!(trashed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_trash_that_refuses_keeps_the_original() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["a.txt"]);
        let stamp = Stamp::of(&fs::symlink_metadata(src.path().join("a.txt")).unwrap());
        let o = Originals::new(
            vec![src.path().join("a.txt")],
            vec![(src.path().join("a.txt"), stamp)],
            Box::new(|_| Err(io::Error::other("no"))),
            Box::new(|_| {}),
        );
        upload_all(&o);
        let out = o.settle();
        assert_eq!(out.kept, [Kept { name: "a.txt".into(), why: "the Trash wouldn't take it".into() }]);
        assert!(src.path().join("a.txt").exists());
    }

    #[test]
    fn a_folder_dropped_with_something_inside_it_is_one_root() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["trip/a.jpg"]);
        let (o, trashed) = batch(src.path(), &["trip/a.jpg", "trip", "trip"], &["trip/a.jpg"]);
        upload_all(&o);
        assert_eq!(o.settle().trashed, ["trip/"]);
        assert_eq!(trashed.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_last_upload_to_end_settles_the_batch() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["a.txt", "b.txt"]);
        let (tx, rx) = std::sync::mpsc::channel();
        let files = ["a.txt", "b.txt"].map(|f| {
            let p = src.path().join(f);
            let stamp = Stamp::of(&fs::symlink_metadata(&p).unwrap());
            (p, stamp)
        });
        let o = Originals::new(
            vec![src.path().join("a.txt"), src.path().join("b.txt")],
            files.to_vec(),
            Box::new(|_| Ok(())),
            Box::new(move |out| tx.send(out).unwrap()),
        );
        o.ended(&src.path().join("a.txt"), true);
        assert!(rx.try_recv().is_err(), "one upload is still going");
        o.ended(&src.path().join("b.txt"), false);
        let out = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(out.trashed, ["a.txt"]);
        assert_eq!(out.kept[0].name, "b.txt");
    }

    /// Touches the real Trash, so it stays out of a plain `cargo test`:
    /// `cargo test -p drawbridge-app -- --ignored trashes_for_real`.
    /// It leaves a small folder named `drawbridge-trash-test-…` there.
    #[test]
    #[ignore = "puts a folder in the real Trash"]
    fn trashes_for_real() {
        let src = tempfile::tempdir().unwrap();
        let dir = src.path().join(format!("drawbridge-trash-test-{}", std::process::id()));
        tree(&dir, &["inner/a.txt"]);
        // From a thread of its own, as the batch settles.
        let moved = dir.clone();
        std::thread::spawn(move || to_trash(&moved)).join().unwrap().unwrap();
        assert!(!dir.exists(), "still at {}", dir.display());
    }

    #[test]
    fn an_empty_folder_dropped_with_a_file_goes_with_it() {
        let src = tempfile::tempdir().unwrap();
        tree(src.path(), &["a.txt"]);
        fs::create_dir(src.path().join("empty")).unwrap();
        let (o, trashed) = batch(src.path(), &["a.txt", "empty"], &["a.txt"]);
        upload_all(&o);
        assert_eq!(o.settle().trashed, ["a.txt", "empty/"]);
        assert_eq!(trashed.lock().unwrap().len(), 2);
    }
}
