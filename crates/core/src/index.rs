// Claude — Date 11/03/2026
// Changes to the index made on this device: new folders, renames, moves and
// deletes. Each checks the request against the tree as this device sees it,
// then commits ops to the local Db; the next sync pushes them. Files come in
// through `blob::upload`, whose destination is checked here too.
//
// The checks are for the person asking, not for the merge. Another device can
// make a conflicting change at the same moment (move something into a folder
// this device just deleted, say), and oplog.rs settles that the same way on
// every device, whatever was checked here.

use crate::blob::Dest;
use crate::db::{self, Db};
use crate::error::Error;
use crate::oplog::{Body, Entry, View, check_name};

/// Make a folder named `name` in `parent` (`None` is the root). Returns its id.
pub fn mkdir(db: &Db, parent: Option<&str>, name: &str) -> Result<String, Error> {
    check_new(&db.view()?, parent, name)?;
    let id = db::new_id();
    let body = Body::Folder { name: name.to_owned(), parent: parent.map(str::to_owned) };
    db.commit(vec![(id.clone(), body)])?;
    Ok(id)
}

/// Rename an entry, leaving it in its folder.
pub fn rename(db: &Db, id: &str, name: &str) -> Result<(), Error> {
    let view = db.view()?;
    let parent = view.parent_of(id).map(str::to_owned);
    relocate(db, &view, id, parent.as_deref(), Some(name))
}

/// Move an entry into `parent` (`None` is the root) and, given a `name`,
/// rename it in the same step.
pub fn move_to(db: &Db, id: &str, parent: Option<&str>, name: Option<&str>) -> Result<(), Error> {
    relocate(db, &db.view()?, id, parent, name)
}

/// Delete an entry and, for a folder, everything shown in it. Returns how
/// many entries that was.
pub fn remove(db: &Db, id: &str) -> Result<usize, Error> {
    let view = db.view()?;
    live(&view, id)?;
    let ids = view.subtree(id);
    db.commit(ids.iter().map(|id| (id.clone(), Body::Delete)).collect())?;
    Ok(ids.len())
}

/// One search result: an entry and the folder it's shown in (`None` is the root).
pub struct Hit<'v> {
    pub entry: &'v Entry,
    pub parent: Option<&'v str>,
}

/// Entries whose name contains `query`, ignoring case, best first: names that
/// start with it, then names with a word that does (`2026 beach.jpg` for
/// `beach`), then any others; shorter paths first within each. A query with a
/// `/` in it matches whole paths instead (`trips/lis`). Reads only this
/// device's copy of the index, so it is instant and works offline.
pub fn search<'v>(view: &'v View, query: &str, limit: usize) -> Vec<Hit<'v>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u8, String, &Entry)> = view
        .entries()
        .filter_map(|e| {
            let rank = if q.contains('/') {
                let path = view.path(&e.id)?.to_lowercase();
                if !path.contains(&q) {
                    return None;
                }
                0
            } else {
                let name = e.name().to_lowercase();
                if name.starts_with(&q) {
                    0
                } else if name.match_indices(&q).any(|(i, _)| !name[..i].ends_with(char::is_alphanumeric)) {
                    1
                } else if name.contains(&q) {
                    2
                } else {
                    return None;
                }
            };
            Some((rank, view.path(&e.id)?, e))
        })
        .collect();
    found.sort_by(|a, b| (a.0, a.1.len(), &a.1).cmp(&(b.0, b.1.len(), &b.1)));
    found.truncate(limit);
    found.into_iter().map(|(_, _, entry)| Hit { entry, parent: view.parent_of(&entry.id) }).collect()
}

/// `name`, or the first of `name (2)`, `name (3)`, … that `taken` says is free,
/// with the number before the extension as Finder puts it: `beach (2).jpg`.
pub fn free_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (2..).map(|n| format!("{stem} ({n}){ext}")).find(|n| !taken(n)).expect("some number is free")
}

/// Check an upload's destination before any bytes are sent.
pub(crate) fn check_dest(db: &Db, dest: &Dest) -> Result<(), Error> {
    if *dest == Dest::Blob {
        return Ok(());
    }
    db.device()?.ok_or(Error::NoDevice)?;
    let view = db.view()?;
    match dest {
        Dest::Blob => Ok(()),
        Dest::New { parent, name } => check_new(&view, parent.as_deref(), name),
        Dest::Replace { id } => match live(&view, id)? {
            e if e.is_file() => Ok(()),
            _ => Err(Error::Invalid(format!("{} is a folder, not a file", shown(&view, id)))),
        },
    }
}

fn relocate(db: &Db, view: &View, id: &str, parent: Option<&str>, name: Option<&str>) -> Result<(), Error> {
    let e = live(view, id)?;
    let name = name.unwrap_or(e.name());
    check_name(name).map_err(Error::Invalid)?;
    check_folder(view, parent)?;
    if let Some(p) = parent
        && view.is_within(p, id)
    {
        return Err(Error::Invalid(format!("can't move {} into itself", shown(view, id))));
    }
    if view.children(parent).any(|other| other.id != id && other.name() == name) {
        return Err(Error::Invalid(format!("{} already exists", join(view, parent, name))));
    }
    let mut changes = Vec::new();
    if view.parent_of(id) != parent {
        changes.push((id.to_owned(), Body::Move { parent: parent.map(str::to_owned) }));
    }
    if e.name() != name {
        changes.push((id.to_owned(), Body::Rename { name: name.to_owned() }));
    }
    if !changes.is_empty() {
        db.commit(changes)?;
    }
    Ok(())
}

fn live<'v>(view: &'v View, id: &str) -> Result<&'v Entry, Error> {
    view.get(id).ok_or_else(|| Error::NotFound(format!("entry {id}")))
}

fn check_folder(view: &View, parent: Option<&str>) -> Result<(), Error> {
    let Some(p) = parent else { return Ok(()) };
    match live(view, p)? {
        e if e.is_folder() => Ok(()),
        _ => Err(Error::Invalid(format!("{} is a file, not a folder", shown(view, p)))),
    }
}

/// A new entry named `name` can go in `parent`. Names in a folder are kept
/// unique here, as in any file system. Two devices can still each add the same
/// name at once; the tree shows both, and paths ask for an id between them.
fn check_new(view: &View, parent: Option<&str>, name: &str) -> Result<(), Error> {
    check_name(name).map_err(Error::Invalid)?;
    check_folder(view, parent)?;
    if view.children(parent).any(|e| e.name() == name) {
        return Err(Error::Invalid(format!("{} already exists", join(view, parent, name))));
    }
    Ok(())
}

fn shown(view: &View, id: &str) -> String {
    view.path(id).unwrap_or_else(|| id.to_owned())
}

fn join(view: &View, parent: Option<&str>, name: &str) -> String {
    match parent {
        Some(p) => format!("{}/{name}", shown(view, p)),
        None => format!("/{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.init_device("test").unwrap();
        db
    }

    fn path(db: &Db, id: &str) -> Option<String> {
        db.view().unwrap().path(id)
    }

    #[test]
    fn folders() {
        let db = device();
        let a = mkdir(&db, None, "a").unwrap();
        let b = mkdir(&db, Some(&a), "b").unwrap();
        assert_eq!(path(&db, &b).unwrap(), "/a/b");
        assert!(matches!(mkdir(&db, None, "a"), Err(Error::Invalid(m)) if m == "/a already exists"));
        assert!(matches!(mkdir(&db, Some(&a), "x/y"), Err(Error::Invalid(_))));
        assert!(matches!(mkdir(&db, Some("nope"), "x"), Err(Error::NotFound(_))));
    }

    #[test]
    fn moves_and_renames() {
        let db = device();
        let a = mkdir(&db, None, "a").unwrap();
        let b = mkdir(&db, Some(&a), "b").unwrap();
        let c = mkdir(&db, None, "c").unwrap();

        assert!(matches!(move_to(&db, &a, Some(&b), None), Err(Error::Invalid(m)) if m.contains("into itself")));
        assert!(matches!(move_to(&db, &a, Some(&a), None), Err(Error::Invalid(_))));
        assert!(matches!(rename(&db, &c, "a"), Err(Error::Invalid(m)) if m == "/a already exists"));

        move_to(&db, &b, None, Some("b2")).unwrap();
        assert_eq!(path(&db, &b).unwrap(), "/b2");
        rename(&db, &b, "b3").unwrap();
        assert_eq!(path(&db, &b).unwrap(), "/b3");
        move_to(&db, &c, Some(&a), None).unwrap();
        assert_eq!(path(&db, &c).unwrap(), "/a/c");

        // Nothing changes, so nothing is queued.
        db.seal_outbox().unwrap();
        move_to(&db, &c, Some(&a), Some("c")).unwrap();
        assert_eq!(db.seal_outbox().unwrap(), 0);
    }

    #[test]
    fn remove_takes_the_subtree() {
        let db = device();
        let a = mkdir(&db, None, "a").unwrap();
        let b = mkdir(&db, Some(&a), "b").unwrap();
        mkdir(&db, Some(&b), "c").unwrap();
        let keep = mkdir(&db, None, "keep").unwrap();
        assert_eq!(remove(&db, &a).unwrap(), 3);
        let view = db.view().unwrap();
        assert_eq!(view.len(), 1);
        assert!(view.get(&keep).is_some());
        assert!(matches!(remove(&db, &a), Err(Error::NotFound(_))));
    }

    #[test]
    fn search_ranks_and_finds_by_path() {
        let db = device();
        let photos = mkdir(&db, None, "Photos").unwrap();
        let trips = mkdir(&db, Some(&photos), "trips").unwrap();
        let deep = mkdir(&db, Some(&trips), "beach").unwrap();
        mkdir(&db, None, "Beach day").unwrap();
        mkdir(&db, None, "old beach").unwrap();
        mkdir(&db, None, "seabeach").unwrap();
        mkdir(&db, None, "harbor").unwrap();
        let view = db.view().unwrap();
        let names = |q: &str| -> Vec<String> {
            search(&view, q, 10).iter().map(|h| view.path(&h.entry.id).unwrap()).collect()
        };
        assert_eq!(
            names("BEACH"),
            ["/Beach day", "/Photos/trips/beach", "/old beach", "/seabeach"],
            "prefix, then word start, then anywhere; shorter paths first"
        );
        assert_eq!(names("trips/be"), ["/Photos/trips/beach"]);
        assert!(names("  ").is_empty());
        assert!(names("zzz").is_empty());
        assert_eq!(search(&view, "beach", 1).len(), 1);
        let hit = &search(&view, "trips/beach", 10)[0];
        assert_eq!((hit.entry.id.as_str(), hit.parent), (deep.as_str(), Some(trips.as_str())));
    }

    #[test]
    fn free_names() {
        let taken = ["beach.jpg", "beach (2).jpg", "notes", ".env"];
        let is_taken = |n: &str| taken.contains(&n);
        assert_eq!(free_name("pier.jpg", is_taken), "pier.jpg");
        assert_eq!(free_name("beach.jpg", is_taken), "beach (3).jpg");
        assert_eq!(free_name("notes", is_taken), "notes (2)");
        assert_eq!(free_name(".env", is_taken), ".env (2)");
    }

    #[test]
    fn upload_destinations() {
        let db = device();
        let a = mkdir(&db, None, "a").unwrap();
        assert!(check_dest(&db, &Dest::Blob).is_ok());
        assert!(check_dest(&db, &Dest::New { parent: Some(a.clone()), name: "x".into() }).is_ok());
        assert!(check_dest(&db, &Dest::New { parent: None, name: "a".into() }).is_err());
        assert!(check_dest(&db, &Dest::New { parent: Some("nope".into()), name: "x".into() }).is_err());
        assert!(matches!(check_dest(&db, &Dest::Replace { id: a }), Err(Error::Invalid(_))));

        let fresh = Db::open_in_memory().unwrap();
        assert!(check_dest(&fresh, &Dest::Blob).is_ok(), "a blob alone needs no device");
        let to_root = Dest::New { parent: None, name: "x".into() };
        assert!(matches!(check_dest(&fresh, &to_root), Err(Error::NoDevice)));
    }
}
