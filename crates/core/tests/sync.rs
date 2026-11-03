// Claude — Date 11/03/2026
// Several devices, each with its own local Db, sharing one real swiftSearch
// store: what one device does shows up on the others after a sync, and
// concurrent changes settle the same way on every device.

mod common;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use bytes::Bytes;
use common::{Server, bytes};
use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::sync::{self, Report, SyncOptions};
use drawbridge_core::{Db, Dest, Error, Kind, Progress, Remote, View, index, keys};

static GO: AtomicBool = AtomicBool::new(false);

struct Dev {
    db: Db,
    remote: Remote,
    opts: SyncOptions,
}

impl Dev {
    fn new(server: &Server, name: &str) -> Dev {
        Dev::with(server.writer(), name)
    }

    fn with(remote: Remote, name: &str) -> Dev {
        let db = Db::open_in_memory().unwrap();
        db.init_device(name).unwrap();
        let opts = SyncOptions { checkpoint_every: 0, gc_grace: Duration::ZERO, parallel: 4 };
        Dev { db, remote, opts }
    }

    async fn sync(&self) -> Report {
        sync::sync(&self.remote, &self.db, &self.opts).await.unwrap()
    }

    fn view(&self) -> View {
        self.db.view().unwrap()
    }

    fn id_of(&self, path: &str) -> String {
        self.view().resolve(path).unwrap().unwrap().id.clone()
    }

    fn me(&self) -> String {
        self.db.device().unwrap().unwrap().id
    }
}

/// The tree as sorted `path kind` lines.
fn tree(dev: &Dev) -> Vec<String> {
    let v = dev.view();
    let mut lines: Vec<String> =
        v.entries().map(|e| format!("{} {:?}", v.path(&e.id).unwrap(), e.kind().unwrap())).collect();
    lines.sort();
    lines
}

fn quiet(_: Progress) {}

fn small() -> UploadOptions {
    UploadOptions { chunk_size: 64 << 10, parallel: 3 }
}

/// A fixed point after concurrent changes: everyone pushes, then everyone
/// pulls what the others pushed.
async fn settle(devs: &[&Dev]) {
    for d in devs {
        d.sync().await;
    }
    for d in devs {
        d.sync().await;
    }
}

#[tokio::test]
async fn an_upload_on_one_device_appears_on_another() {
    let server = Server::start();
    let (a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    let dir = tempfile::tempdir().unwrap();
    let data = bytes(300_000, 7);
    let src = dir.path().join("clip.bin");
    std::fs::write(&src, &data).unwrap();

    let photos = index::mkdir(&a.db, None, "Photos").unwrap();
    let dest = Dest::New { parent: Some(photos), name: "clip.bin".into() };
    let up = blob::upload(&a.remote, &a.db, &src, dest, small(), &quiet, &GO).await.unwrap();

    let pushed = a.sync().await;
    assert_eq!((pushed.pushed, pushed.registered), (2, true));
    let pulled = b.sync().await;
    assert_eq!((pulled.pulled, pulled.devices), (2, 1));

    let view = b.view();
    let file = view.resolve("/Photos/clip.bin").unwrap().unwrap();
    assert_eq!(Some(&file.id), up.entry.as_ref());
    let m = blob::manifest(&b.remote, &file.content().unwrap().blob).await.unwrap();
    let out = dir.path().join("back.bin");
    blob::download(&b.remote, &m, &out, 4, &quiet, &GO).await.unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), data);

    // Nothing new on either side: the heads answer 304 and nothing is pushed.
    let again = b.sync().await;
    assert_eq!((again.pushed, again.pulled, again.registered), (0, 0, false), "b registers once");
    let again = a.sync().await;
    assert_eq!((again.pushed, again.pulled, again.devices), (0, 0, 1));
}

#[tokio::test]
async fn concurrent_renames_settle_on_the_newest() {
    let server = Server::start();
    let (a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    index::mkdir(&a.db, None, "docs").unwrap();
    settle(&[&a, &b]).await;

    let id = a.id_of("/docs");
    index::rename(&a.db, &id, "from-a").unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    index::rename(&b.db, &id, "from-b").unwrap();
    settle(&[&a, &b]).await;

    assert_eq!(tree(&a), ["/from-b Folder"]);
    assert_eq!(tree(&b), tree(&a));
}

#[tokio::test]
async fn crossed_moves_settle_into_one_tree() {
    let server = Server::start();
    let (a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    let p = index::mkdir(&a.db, None, "p").unwrap();
    let q = index::mkdir(&a.db, None, "q").unwrap();
    settle(&[&a, &b]).await;

    // Each is fine on its own device; together they would make a loop.
    index::move_to(&a.db, &p, Some(&q), None).unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    index::move_to(&b.db, &q, Some(&p), None).unwrap();
    settle(&[&a, &b]).await;

    // q moved last, so q is the one shown at the root.
    assert_eq!(tree(&a), ["/q Folder", "/q/p Folder"]);
    assert_eq!(tree(&b), tree(&a));
}

#[tokio::test]
async fn moving_into_a_folder_deleted_elsewhere_lands_at_the_root() {
    let server = Server::start();
    let (a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    let d = index::mkdir(&a.db, None, "d").unwrap();
    let f = index::mkdir(&a.db, None, "f").unwrap();
    settle(&[&a, &b]).await;

    index::remove(&a.db, &d).unwrap();
    index::move_to(&b.db, &f, Some(&d), None).unwrap();
    settle(&[&a, &b]).await;

    assert_eq!(tree(&a), ["/f Folder"]);
    assert_eq!(tree(&b), tree(&a));
}

#[tokio::test]
async fn a_new_device_starts_from_a_checkpoint() {
    let server = Server::start();
    let mut a = Dev::new(&server, "a");
    a.opts.checkpoint_every = 5;
    for i in 0..8 {
        index::mkdir(&a.db, None, &format!("f{i}")).unwrap();
    }
    assert_eq!(a.sync().await.checkpoint, Some(1));
    index::mkdir(&a.db, None, "g1").unwrap();
    index::mkdir(&a.db, None, "g2").unwrap();
    assert_eq!(a.sync().await.checkpoint, None, "2 ops since the last one");

    // The checkpoint covers log 1; only log 2 is replayed.
    let c = Dev::new(&server, "c");
    let r = c.sync().await;
    assert_eq!((r.bootstrapped, r.pulled), (1, 2));
    assert_eq!(tree(&c).len(), 10);
    assert_eq!(tree(&c), tree(&a));

    // The next checkpoint replaces the first.
    for i in 0..3 {
        index::mkdir(&a.db, None, &format!("h{i}")).unwrap();
    }
    assert_eq!(a.sync().await.checkpoint, Some(2));
    let me = a.me();
    assert!(a.remote.get(&keys::checkpoint(&me, 1).unwrap()).await.unwrap().is_none());
    assert!(a.remote.get(&keys::checkpoint(&me, 2).unwrap()).await.unwrap().is_some());

    // A device that already follows a just replays the logs.
    let r = c.sync().await;
    assert_eq!((r.bootstrapped, r.pulled), (0, 3));
    assert_eq!(tree(&c), tree(&a));
}

#[tokio::test]
async fn a_device_dropped_from_the_registry_adds_itself_back() {
    let server = Server::start();
    let (a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    settle(&[&a, &b]).await;
    index::mkdir(&b.db, None, "from-b").unwrap();
    b.sync().await;

    // What a lost race looks like: a and b both read the registry before
    // either wrote it, and a's write went last, so b's line is gone.
    let only_a = sync::Devices {
        v: sync::FORMAT_VERSION,
        devices: sync::devices(&a.remote).await.unwrap().into_iter().filter(|d| d.id == a.me()).collect(),
    };
    let json = Bytes::from(serde_json::to_vec(&only_a).unwrap());
    a.remote.put(&keys::devices(), Kind::Str, json).await.unwrap();

    let c = Dev::new(&server, "c");
    assert_eq!(c.sync().await.devices, 1, "c can't see b");
    assert!(tree(&c).is_empty());

    assert!(b.sync().await.registered);
    let r = c.sync().await;
    assert_eq!((r.devices, r.pulled), (2, 1));
    assert_eq!(tree(&c), ["/from-b Folder"]);
    let names: Vec<String> = sync::devices(&a.remote).await.unwrap().into_iter().map(|d| d.name).collect();
    assert_eq!(names, ["a", "c", "b"]);
}

#[tokio::test]
async fn replaced_and_deleted_content_is_collected() {
    let server = Server::start();
    let (mut a, b) = (Dev::new(&server, "a"), Dev::new(&server, "b"));
    let dir = tempfile::tempdir().unwrap();
    let v1 = dir.path().join("v1.txt");
    let v2 = dir.path().join("v2.txt");
    std::fs::write(&v1, bytes(200_000, 1)).unwrap();
    std::fs::write(&v2, b"second version").unwrap();

    let dest = Dest::New { parent: None, name: "note.txt".into() };
    let first = blob::upload(&a.remote, &a.db, &v1, dest, small(), &quiet, &GO).await.unwrap();
    let id = first.entry.unwrap();
    let old = first.manifest.blob;
    a.sync().await;
    let second = blob::upload(&a.remote, &a.db, &v2, Dest::Replace { id: id.clone() }, small(), &quiet, &GO)
        .await
        .unwrap();
    assert_eq!(second.entry.as_deref(), Some(id.as_str()));
    let new = second.manifest.blob;

    // Within the grace period, the old content stays.
    a.opts.gc_grace = Duration::from_secs(3600);
    assert_eq!(a.sync().await.collected, 0);
    assert!(blob::manifest(&a.remote, &old).await.is_ok());

    a.opts.gc_grace = Duration::ZERO;
    assert_eq!(a.sync().await.collected, 1);
    assert!(matches!(blob::manifest(&a.remote, &old).await, Err(Error::NotFound(_))));
    assert!(a.remote.head(&keys::chunk(&old, 0).unwrap()).await.unwrap().is_none());
    assert!(a.remote.head(&keys::chunk(&old, 3).unwrap()).await.unwrap().is_none());
    assert!(blob::manifest(&a.remote, &new).await.is_ok(), "the current content stays");

    // b learns of both versions at once; the old one is already gone.
    b.sync().await;
    assert_eq!(b.view().get(&id).unwrap().content().unwrap().blob, new);

    index::remove(&a.db, &id).unwrap();
    assert_eq!(a.sync().await.collected, 1);
    assert!(matches!(blob::manifest(&a.remote, &new).await, Err(Error::NotFound(_))));
}

#[tokio::test]
async fn a_read_only_key_follows_without_registering() {
    let server = Server::start();
    let a = Dev::new(&server, "a");
    index::mkdir(&a.db, None, "shared").unwrap();
    a.sync().await;

    let viewer = Dev::with(server.reader(), "viewer");
    let r = viewer.sync().await;
    assert!(r.read_only && !r.registered);
    assert_eq!(r.pulled, 1);
    assert_eq!(tree(&viewer), ["/shared Folder"]);
    assert_eq!(sync::devices(&a.remote).await.unwrap().len(), 1);

    // A change made on it can't be pushed.
    index::mkdir(&viewer.db, None, "mine").unwrap();
    let pushed = sync::sync(&viewer.remote, &viewer.db, &viewer.opts).await;
    assert!(matches!(pushed, Err(Error::Remote(drawbridge_core::remote::Error::Forbidden))), "{pushed:?}");
}
