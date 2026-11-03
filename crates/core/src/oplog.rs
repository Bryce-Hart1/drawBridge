// Claude — Date 11/03/2026
// The index's data model: what one change (an op) looks like, how changes from
// every device combine into one state, and how that state reads as a folder
// tree. Pure: no I/O and no clock. db.rs persists it; sync.rs moves it between
// devices.
//
// The merge is a join. Every entry is a handful of registers, and each register
// keeps the write with the greatest stamp, so applying the same ops in any order,
// any number of times, gives the same state. That is what lets each device write
// only its own log and still end up agreeing with every other device.
//
// The rules, all decided by stamps alone:
// - name, parent, content: the latest write wins. A stamp is (HLC, device id),
//   so two devices can never write with equal stamps.
// - created: the earliest creating op. There is only ever one per id, since ids
//   are fresh UUIDs, but taking the minimum keeps even a duplicate deterministic.
// - deleted: sticky. Once any device has deleted an entry, nothing revives it.
// - An op can arrive before the op that created its entry (device B's rename,
//   pulled before device A's log). It lands on a placeholder that stays hidden
//   until the creating op turns up.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A hybrid logical clock reading: wall-clock milliseconds, and a counter for
/// several events within one millisecond (or after a clock that went
/// backwards). Serialized as `[ms, counter]`, two numbers any JSON reader holds
/// exactly; one u64 would lose precision past 2^53 in JavaScript.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Hlc(pub u64, pub u32);

impl Hlc {
    /// The reading for the next local event: later than this one, and no
    /// earlier than `now_ms`.
    pub fn tick(self, now_ms: u64) -> Hlc {
        if now_ms > self.0 {
            Hlc(now_ms, 0)
        } else if self.1 < u32::MAX {
            Hlc(self.0, self.1 + 1)
        } else {
            Hlc(self.0 + 1, 0)
        }
    }

    /// Fold in a reading seen on another device's op, so this device's next
    /// local event is stamped after everything it has already seen.
    pub fn observe(self, seen: Hlc) -> Hlc {
        self.max(seen)
    }
}

/// When, and on which device, a write happened. Ordered by time, then device.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Stamp {
    pub t: Hlc,
    pub device: String,
}

/// One register: a value and the stamp of the write that set it. Ordered by
/// stamp, then value, so even an impossible tie is settled the same way
/// everywhere.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Reg<T> {
    pub at: Stamp,
    pub value: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Folder,
    File,
}

/// What a file holds: one blob, and what the index shows without fetching it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Content {
    pub blob: String,
    pub size: u64,
    pub mime: String,
    pub sha256: String,
}

/// What an op does. `parent: None` is the root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Body {
    Folder { name: String, parent: Option<String> },
    File { name: String, parent: Option<String>, blob: String, size: u64, mime: String, sha256: String },
    Content { blob: String, size: u64, mime: String, sha256: String },
    Rename { name: String },
    Move { parent: Option<String> },
    Delete,
}

impl Body {
    /// The blob this op points its entry at, if any. Each blob is named by
    /// exactly one op, the one its uploader made, which is what makes garbage
    /// collection safe (see db.rs).
    pub fn blob(&self) -> Option<&str> {
        match self {
            Body::File { blob, .. } | Body::Content { blob, .. } => Some(blob),
            _ => None,
        }
    }
}

/// One change to one entry. On the wire: `{"t":[ms,c],"id":"…","op":"rename",…}`.
/// The device isn't repeated in every op; it comes from the batch that carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub t: Hlc,
    pub id: String,
    #[serde(flatten)]
    pub body: Body,
}

/// One device's ops, as written to `dev/<device>/log/<seq>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub v: u32,
    pub device: String,
    pub seq: u64,
    pub ops: Vec<Op>,
}

pub const BATCH_VERSION: u32 = 1;

/// The merged state of one file or folder.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<Reg<EntryKind>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Reg<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Reg<Option<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Reg<Content>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Stamp>,
}

impl Entry {
    pub fn new(id: &str) -> Entry {
        Entry { id: id.to_owned(), ..Entry::default() }
    }

    /// The entry as one op alone would leave it.
    pub fn from_op(op: &Op, device: &str) -> Entry {
        let at = Stamp { t: op.t, device: device.to_owned() };
        fn reg<T>(at: &Stamp, value: T) -> Option<Reg<T>> {
            Some(Reg { at: at.clone(), value })
        }
        let mut e = Entry::new(&op.id);
        match &op.body {
            Body::Folder { name, parent } => {
                e.created = reg(&at, EntryKind::Folder);
                e.name = reg(&at, name.clone());
                e.parent = reg(&at, parent.clone());
            }
            Body::File { name, parent, blob, size, mime, sha256 } => {
                e.created = reg(&at, EntryKind::File);
                e.name = reg(&at, name.clone());
                e.parent = reg(&at, parent.clone());
                let c = Content { blob: blob.clone(), size: *size, mime: mime.clone(), sha256: sha256.clone() };
                e.content = reg(&at, c);
            }
            Body::Content { blob, size, mime, sha256 } => {
                let c = Content { blob: blob.clone(), size: *size, mime: mime.clone(), sha256: sha256.clone() };
                e.content = reg(&at, c);
            }
            Body::Rename { name } => e.name = reg(&at, name.clone()),
            Body::Move { parent } => e.parent = reg(&at, parent.clone()),
            Body::Delete => e.deleted = Some(at),
        }
        e
    }

    pub fn apply(&mut self, op: &Op, device: &str) {
        self.merge(Entry::from_op(op, device));
    }

    /// Join with another state of the same entry: every register keeps the
    /// greater write. Commutative, associative and idempotent.
    pub fn merge(&mut self, other: Entry) {
        debug_assert_eq!(self.id, other.id);
        join_min(&mut self.created, other.created);
        self.name = self.name.take().max(other.name);
        self.parent = self.parent.take().max(other.parent);
        self.content = self.content.take().max(other.content);
        join_min(&mut self.deleted, other.deleted);
    }

    /// Created, and not deleted.
    pub fn is_live(&self) -> bool {
        self.created.is_some() && self.deleted.is_none()
    }

    pub fn kind(&self) -> Option<EntryKind> {
        self.created.as_ref().map(|r| r.value)
    }

    pub fn is_folder(&self) -> bool {
        self.kind() == Some(EntryKind::Folder)
    }

    pub fn is_file(&self) -> bool {
        self.kind() == Some(EntryKind::File)
    }

    pub fn name(&self) -> &str {
        self.name.as_ref().map_or("", |r| &r.value)
    }

    /// A file's current content. Folders have none, whatever an op said.
    pub fn content(&self) -> Option<&Content> {
        self.content.as_ref().filter(|_| self.is_file()).map(|r| &r.value)
    }

    /// Every stamp in the entry, so a device loading a checkpoint can move its
    /// clock past all of them.
    pub fn latest_stamp(&self) -> Option<Hlc> {
        [
            self.created.as_ref().map(|r| r.at.t),
            self.name.as_ref().map(|r| r.at.t),
            self.parent.as_ref().map(|r| r.at.t),
            self.content.as_ref().map(|r| r.at.t),
            self.deleted.as_ref().map(|s| s.t),
        ]
        .into_iter()
        .flatten()
        .max()
    }
}

// `None` means "never written": the identity, not the least value.
fn join_min<T: Ord>(a: &mut Option<T>, b: Option<T>) {
    *a = match (a.take(), b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    };
}

/// A name a new or renamed entry may have.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("{name:?} can't be a name"));
    }
    if name.contains('/') {
        return Err(format!("{name:?}: a name can't contain `/`"));
    }
    if name.chars().any(char::is_control) {
        return Err(format!("{name:?}: a name can't contain control characters"));
    }
    if name.len() > 255 {
        return Err(format!("a name can be at most 255 bytes, this one is {}", name.len()));
    }
    Ok(())
}

/// The merged state read as a folder tree: only live entries, each with the
/// parent it is shown under. A pure function of the entries, so every device
/// holding the same entries shows the same tree.
///
/// Where an entry is shown:
/// - under its parent register, if that names a live folder;
/// - at the root otherwise: its folder was deleted (moved in on one device while
///   another deleted the folder), or the parent is a file or not here yet;
/// - at the root if it is part of a loop. Two devices can each move one folder
///   into the other; of the folders in the loop, the one moved most recently is
///   shown at the root.
pub struct View {
    live: HashMap<String, Entry>,
    parent: HashMap<String, Option<String>>,
    children: HashMap<Option<String>, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// Nothing at this path.
    NotFound(String),
    /// A path that goes through a file as if it were a folder.
    NotAFolder(String),
    /// More than one entry has this name in the same folder.
    Ambiguous { path: String, ids: Vec<String> },
}

impl std::fmt::Display for Lookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lookup::NotFound(p) => write!(f, "{p}: no such file or folder"),
            Lookup::NotAFolder(p) => write!(f, "{p} is a file, not a folder"),
            Lookup::Ambiguous { path, ids } => {
                write!(f, "{path} matches {} entries; name one by id: {}", ids.len(), ids.join(", "))
            }
        }
    }
}

impl View {
    pub fn new(entries: impl IntoIterator<Item = Entry>) -> View {
        let live: HashMap<String, Entry> =
            entries.into_iter().filter(Entry::is_live).map(|e| (e.id.clone(), e)).collect();
        let mut parent: HashMap<String, Option<String>> = live
            .values()
            .map(|e| {
                let p = e.parent.as_ref().and_then(|r| r.value.clone());
                let p = p.filter(|p| live.get(p).is_some_and(Entry::is_folder));
                (e.id.clone(), p)
            })
            .collect();
        break_loops(&live, &mut parent);

        let mut children: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for (id, p) in &parent {
            children.entry(p.clone()).or_default().push(id.clone());
        }
        for kids in children.values_mut() {
            kids.sort_by(|a, b| (live[a].name(), a).cmp(&(live[b].name(), b)));
        }
        View { live, parent, children }
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// A live entry by id.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.live.get(id)
    }

    /// The folder an entry is shown in; `None` is the root.
    pub fn parent_of(&self, id: &str) -> Option<&str> {
        self.parent.get(id).and_then(|p| p.as_deref())
    }

    /// What a folder shows (`None` is the root), by name, then id.
    pub fn children(&self, dir: Option<&str>) -> impl Iterator<Item = &Entry> {
        self.children
            .get(&dir.map(str::to_owned))
            .into_iter()
            .flatten()
            .map(|id| &self.live[id])
    }

    /// `/Photos/2026/beach.jpg`.
    pub fn path(&self, id: &str) -> Option<String> {
        let mut names = Vec::new();
        let mut cur = Some(self.live.get(id)?);
        while let Some(e) = cur {
            names.push(e.name());
            cur = self.parent_of(&e.id).map(|p| &self.live[p]);
        }
        names.reverse();
        Some(format!("/{}", names.join("/")))
    }

    /// The entry at a path. `Ok(None)` is the root (`/` or empty). A leading
    /// `/` is optional, and empty components are skipped.
    pub fn resolve(&self, path: &str) -> Result<Option<&Entry>, Lookup> {
        let mut cur: Option<&Entry> = None;
        let mut walked = String::new();
        for name in path.split('/').filter(|c| !c.is_empty()) {
            if let Some(e) = cur
                && !e.is_folder()
            {
                return Err(Lookup::NotAFolder(walked));
            }
            walked.push('/');
            walked.push_str(name);
            let matches: Vec<&Entry> =
                self.children(cur.map(|e| e.id.as_str())).filter(|e| e.name() == name).collect();
            cur = match matches[..] {
                [] => return Err(Lookup::NotFound(walked)),
                [one] => Some(one),
                _ => {
                    let ids = matches.iter().map(|e| e.id.clone()).collect();
                    return Err(Lookup::Ambiguous { path: walked, ids });
                }
            };
        }
        Ok(cur)
    }

    /// An entry and everything under it, parents before children.
    pub fn subtree(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        if self.live.contains_key(id) {
            out.push(id.to_owned());
        }
        let mut i = 0;
        while i < out.len() {
            let kids: Vec<String> = self.children(Some(&out[i])).map(|e| e.id.clone()).collect();
            out.extend(kids);
            i += 1;
        }
        out
    }

    /// Whether `id` is `ancestor` or somewhere under it.
    pub fn is_within(&self, id: &str, ancestor: &str) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.parent_of(c);
        }
        false
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.live.values()
    }
}

/// Each entry has one shown parent, so the tree is a functional graph and every
/// loop is a cycle in it. Walk from each entry, in id order so the walk itself
/// is deterministic; a walk that comes back to itself has found a loop, and the
/// member whose parent register is newest is moved to the root.
fn break_loops(live: &HashMap<String, Entry>, parent: &mut HashMap<String, Option<String>>) {
    #[derive(Clone, Copy, PartialEq)]
    enum Seen {
        OnWalk,
        Done,
    }
    let mut seen: HashMap<String, Seen> = HashMap::new();
    let mut ids: Vec<String> = parent.keys().cloned().collect();
    ids.sort();
    for start in ids {
        let mut walk: Vec<String> = Vec::new();
        let mut cur = Some(start);
        while let Some(id) = cur {
            match seen.get(&id) {
                Some(Seen::Done) => break,
                Some(Seen::OnWalk) => {
                    let at = walk.iter().position(|w| *w == id).expect("on the walk");
                    let newest = walk[at..]
                        .iter()
                        .max_by(|a, b| live[*a].parent.cmp(&live[*b].parent))
                        .expect("a loop has members")
                        .clone();
                    parent.insert(newest, None);
                    break;
                }
                None => {}
            }
            seen.insert(id.clone(), Seen::OnWalk);
            cur = parent[&id].clone();
            walk.push(id);
        }
        for w in walk {
            seen.insert(w, Seen::Done);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn op(ms: u64, id: &str, body: Body) -> Op {
        Op { t: Hlc(ms, 0), id: id.into(), body }
    }

    fn folder(name: &str, parent: Option<&str>) -> Body {
        Body::Folder { name: name.into(), parent: parent.map(Into::into) }
    }

    fn file(name: &str, parent: Option<&str>, blob: &str) -> Body {
        Body::File {
            name: name.into(),
            parent: parent.map(Into::into),
            blob: blob.into(),
            size: 1,
            mime: "text/plain".into(),
            sha256: "0".repeat(64),
        }
    }

    /// Apply (device, op) pairs in order.
    fn state(ops: &[(&str, Op)]) -> HashMap<String, Entry> {
        let mut m: HashMap<String, Entry> = HashMap::new();
        for (dev, op) in ops {
            m.entry(op.id.clone()).or_insert_with(|| Entry::new(&op.id)).apply(op, dev);
        }
        m
    }

    fn view(ops: &[(&str, Op)]) -> View {
        View::new(state(ops).into_values())
    }

    /// The tree as sorted `(path, kind)` lines, for comparing views.
    fn dump(v: &View) -> Vec<String> {
        let mut lines: Vec<String> =
            v.entries().map(|e| format!("{} {:?} {}", v.path(&e.id).unwrap(), e.kind(), e.id)).collect();
        lines.sort();
        lines
    }

    #[test]
    fn hlc_ticks_and_observes() {
        assert_eq!(Hlc(5, 0).tick(9), Hlc(9, 0));
        assert_eq!(Hlc(9, 0).tick(9), Hlc(9, 1));
        assert_eq!(Hlc(9, 3).tick(2), Hlc(9, 4), "a clock going backwards never goes back");
        assert_eq!(Hlc(9, u32::MAX).tick(9), Hlc(10, 0));
        assert_eq!(Hlc(5, 0).observe(Hlc(7, 2)), Hlc(7, 2));
        assert_eq!(Hlc(8, 0).observe(Hlc(7, 2)), Hlc(8, 0));
    }

    #[test]
    fn op_json_shape() {
        let o = op(1730000000000, "e1", file("a.txt", None, "b1"));
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["t"], serde_json::json!([1730000000000u64, 0]));
        assert_eq!(json["op"], "file");
        assert_eq!(json["parent"], serde_json::Value::Null);
        assert_eq!(serde_json::from_value::<Op>(json).unwrap(), o);
        let d: Op = serde_json::from_str(r#"{"t":[1,2],"id":"x","op":"delete"}"#).unwrap();
        assert_eq!(d.body, Body::Delete);
    }

    #[test]
    fn latest_write_wins_per_register() {
        let v = view(&[
            ("a", op(1, "f", folder("one", None))),
            ("b", op(3, "f", Body::Rename { name: "three".into() })),
            ("a", op(2, "f", Body::Rename { name: "two".into() })),
        ]);
        assert_eq!(v.get("f").unwrap().name(), "three");
    }

    #[test]
    fn equal_times_are_settled_by_device() {
        let v = view(&[
            ("a", op(1, "f", folder("x", None))),
            ("b", op(5, "f", Body::Rename { name: "from b".into() })),
            ("a", op(5, "f", Body::Rename { name: "from a".into() })),
        ]);
        assert_eq!(v.get("f").unwrap().name(), "from b");
    }

    #[test]
    fn delete_is_sticky() {
        let v = view(&[
            ("a", op(1, "f", folder("x", None))),
            ("a", op(2, "f", Body::Delete)),
            ("b", op(9, "f", Body::Rename { name: "later".into() })),
        ]);
        assert!(v.get("f").is_none());
        // Deleted before its creation even arrived.
        let v = view(&[("b", op(2, "g", Body::Delete)), ("a", op(1, "g", folder("x", None)))]);
        assert!(v.get("g").is_none());
    }

    #[test]
    fn ops_before_their_create_wait_hidden() {
        let early = [("b", op(5, "f", Body::Rename { name: "renamed".into() }))];
        assert!(view(&early).is_empty());
        let v = view(&[early[0].clone(), ("a", op(1, "f", folder("orig", None)))]);
        assert_eq!(v.get("f").unwrap().name(), "renamed");
    }

    #[test]
    fn orphans_show_at_the_root() {
        let v = view(&[
            ("a", op(1, "d", folder("dir", None))),
            ("a", op(2, "f", file("x", None, "b1"))),
            ("b", op(3, "f", Body::Move { parent: Some("d".into()) })),
            ("a", op(4, "d", Body::Delete)),
            ("a", op(5, "g", file("y", Some("f"), "b2"))),
            ("a", op(6, "h", file("z", Some("not-here"), "b3"))),
        ]);
        assert_eq!(v.path("f").unwrap(), "/x", "moved into a folder deleted elsewhere");
        assert_eq!(v.path("g").unwrap(), "/y", "a file is not a folder");
        assert_eq!(v.path("h").unwrap(), "/z", "parent not seen yet");
    }

    #[test]
    fn crossed_moves_break_at_the_newest() {
        let base = [("a", op(1, "p", folder("p", None))), ("a", op(2, "q", folder("q", None)))];
        let ops = [
            base[0].clone(),
            base[1].clone(),
            ("a", op(10, "p", Body::Move { parent: Some("q".into()) })),
            ("b", op(11, "q", Body::Move { parent: Some("p".into()) })),
        ];
        let v = view(&ops);
        assert_eq!(v.parent_of("q"), None, "q's move is newer, so q goes to the root");
        assert_eq!(v.parent_of("p"), Some("q"));
        assert_eq!(v.path("p").unwrap(), "/q/p");

        let into_itself = [base[0].clone(), ("a", op(3, "p", Body::Move { parent: Some("p".into()) }))];
        assert_eq!(view(&into_itself).parent_of("p"), None);
    }

    #[test]
    fn paths_and_lookup() {
        let v = view(&[
            ("a", op(1, "d", folder("Photos", None))),
            ("a", op(2, "f", file("beach.jpg", Some("d"), "b1"))),
            ("a", op(3, "g", file("dup", None, "b2"))),
            ("a", op(4, "h", file("dup", None, "b3"))),
        ]);
        assert_eq!(v.path("f").unwrap(), "/Photos/beach.jpg");
        assert_eq!(v.resolve("/Photos/beach.jpg").unwrap().unwrap().id, "f");
        assert_eq!(v.resolve("Photos//beach.jpg/").unwrap().unwrap().id, "f");
        assert!(v.resolve("/").unwrap().is_none());
        assert_eq!(v.resolve("/Photos/x"), Err(Lookup::NotFound("/Photos/x".into())));
        assert_eq!(v.resolve("/Photos/beach.jpg/x"), Err(Lookup::NotAFolder("/Photos/beach.jpg".into())));
        assert!(matches!(v.resolve("/dup"), Err(Lookup::Ambiguous { ids, .. }) if ids == ["g", "h"]));
        assert_eq!(v.subtree("d"), ["d", "f"]);
        assert!(v.is_within("f", "d") && !v.is_within("d", "f"));
        let root: Vec<&str> = v.children(None).map(Entry::name).collect();
        assert_eq!(root, ["Photos", "dup", "dup"]);
    }

    #[test]
    fn names() {
        for ok in ["a", "beach.jpg", ".hidden", "with space", "ünïcødé"] {
            assert!(check_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "tab\there", &"x".repeat(256)] {
            assert!(check_name(bad).is_err(), "{bad:?}");
        }
    }

    // Random ops over a few ids and devices, with colliding times on purpose.
    fn arb_op() -> impl Strategy<Value = (String, Op)> {
        let id = prop::sample::select(vec!["e1", "e2", "e3", "e4"]);
        let parent = prop::option::of(prop::sample::select(vec!["e1", "e2", "e3", "e4"]));
        let name = prop::sample::select(vec!["a", "b", "c"]);
        let body = prop_oneof![
            (name.clone(), parent.clone())
                .prop_map(|(n, p)| Body::Folder { name: n.into(), parent: p.map(Into::into) }),
            (name.clone(), parent.clone(), 0u8..4).prop_map(|(n, p, b)| Body::File {
                name: n.into(),
                parent: p.map(Into::into),
                blob: format!("blob{b}"),
                size: b as u64,
                mime: "x".into(),
                sha256: "0".repeat(64),
            }),
            (0u8..4).prop_map(|b| Body::Content {
                blob: format!("blob{b}"),
                size: b as u64,
                mime: "x".into(),
                sha256: "0".repeat(64),
            }),
            name.prop_map(|n| Body::Rename { name: n.into() }),
            parent.prop_map(|p| Body::Move { parent: p.map(Into::into) }),
            Just(Body::Delete),
        ];
        (prop::sample::select(vec!["dev-a", "dev-b", "dev-c"]), id, 0u64..12, 0u32..3, body)
            .prop_map(|(dev, id, ms, c, body)| (dev.to_owned(), Op { t: Hlc(ms, c), id: id.into(), body }))
    }

    fn run(ops: &[(String, Op)]) -> HashMap<String, Entry> {
        let pairs: Vec<(&str, Op)> = ops.iter().map(|(d, o)| (d.as_str(), o.clone())).collect();
        state(&pairs)
    }

    proptest! {
        #[test]
        fn any_order_gives_the_same_state(
            (ops, shuffled) in prop::collection::vec(arb_op(), 0..40)
                .prop_flat_map(|ops| (Just(ops.clone()), Just(ops).prop_shuffle()))
        ) {
            let a = run(&ops);
            let b = run(&shuffled);
            prop_assert_eq!(&a, &b);
            prop_assert_eq!(dump(&View::new(a.into_values())), dump(&View::new(b.into_values())));
        }

        #[test]
        fn applying_twice_changes_nothing(ops in prop::collection::vec(arb_op(), 0..40)) {
            let doubled: Vec<_> = ops.iter().chain(ops.iter()).cloned().collect();
            prop_assert_eq!(run(&ops), run(&doubled));
        }

        #[test]
        fn merging_states_equals_applying_ops(
            left in prop::collection::vec(arb_op(), 0..20),
            right in prop::collection::vec(arb_op(), 0..20),
        ) {
            // What a checkpoint load does: join two whole states.
            let mut joined = run(&left);
            for (id, e) in run(&right) {
                joined.entry(id.clone()).or_insert_with(|| Entry::new(&id)).merge(e);
            }
            let all: Vec<_> = left.iter().chain(right.iter()).cloned().collect();
            prop_assert_eq!(joined, run(&all));
        }

        #[test]
        fn every_view_is_a_tree(ops in prop::collection::vec(arb_op(), 0..40)) {
            let v = View::new(run(&ops).into_values());
            for e in v.entries() {
                // path() terminates and every shown parent is a live folder.
                prop_assert!(v.path(&e.id).is_some());
                if let Some(p) = v.parent_of(&e.id) {
                    prop_assert!(v.get(p).is_some_and(Entry::is_folder));
                }
            }
        }
    }
}
