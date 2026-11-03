// Claude — Date 11/03/2026
// `dbx`: drawbridge-core from the shell. It exercises the library end to end
// before any UI exists, and stays useful for poking at a store afterwards.
//
//   DBX_URL   the store: http://127.0.0.1:8080 (swiftSearch on loopback), or
//             https://vault.<domain> through castle+
//   DBX_KEY   a swiftSearch access key (`swiftSearch key add`)
//   DBX_HOME  local state, ~/.dbx by default: this device's id, its copy of
//             the index, and the upload queue
//
// Paths such as /Photos/beach.jpg name entries in the index as this device last
// synced it; an entry's id works anywhere a path does. Changes (put, mkdir, mv,
// rm) are made here first, then pushed with a sync. If the store can't be
// reached, they wait for the next `dbx sync`.
//
// Ctrl-C during a transfer lets the chunks in flight finish, so an upload can
// pick up from there with the same `dbx put`. A second Ctrl-C quits at once.

use std::collections::HashMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{Parser, Subcommand};
use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::oplog::check_name;
use drawbridge_core::sync::{self, SyncOptions};
use drawbridge_core::{ByteRange, Db, Dest, Device, Entry, Error, Lookup, Manifest, Progress, Remote, View, index};

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(name = "dbx", about = "drawBridge from the command line", version)]
struct Cli {
    /// Store address.
    #[arg(long, env = "DBX_URL", default_value = "http://127.0.0.1:8080", global = true)]
    url: String,
    /// swiftSearch access key.
    #[arg(long, env = "DBX_KEY", hide_env_values = true, global = true)]
    key: Option<String>,
    /// Folder for local state.
    #[arg(long, env = "DBX_HOME", global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Give this device its id. Other commands do this on first use, naming
    /// the device after this computer.
    Init {
        /// What other devices call this one.
        #[arg(long)]
        name: Option<String>,
    },
    /// Push this device's changes and pull everyone else's.
    Sync,
    /// List the devices sharing the store.
    Devices,
    /// List a folder.
    Ls {
        #[arg(default_value = "/")]
        path: String,
        /// Show each entry's id, size and type.
        #[arg(short, long)]
        long: bool,
        /// List folders inside it too.
        #[arg(short = 'R', long)]
        recursive: bool,
    },
    /// Make folders.
    Mkdir {
        #[arg(required = true)]
        paths: Vec<String>,
        /// Make missing parent folders too, and accept a folder that exists.
        #[arg(short, long)]
        parents: bool,
    },
    /// Move or rename a file or folder. Into DST if it's a folder, otherwise
    /// to the path DST.
    Mv { src: String, dst: String },
    /// Delete files and folders, at once and for every device.
    Rm {
        #[arg(required = true)]
        paths: Vec<String>,
        /// Delete folders and everything in them.
        #[arg(short, long)]
        recursive: bool,
    },
    /// Upload files into a folder, or as new content for a file. An
    /// interrupted upload of the same file resumes.
    Put {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// The folder to put them in.
        #[arg(long, default_value = "/")]
        to: String,
        /// Replace this file's content with FILE's.
        #[arg(long, value_name = "PATH", conflicts_with_all = ["to", "blob_only"])]
        replace: Option<String>,
        /// Store blobs only, outside the index, and print their ids.
        #[arg(long)]
        blob_only: bool,
        /// Chunk size: bytes, or with K or M (powers of 1024). At most 64M.
        #[arg(long, default_value = "8M", value_parser = parse_size)]
        chunk_size: u32,
        /// Chunk uploads in flight at once.
        #[arg(long, default_value_t = 3)]
        parallel: usize,
    },
    /// Download a file. OUT defaults to the file's name.
    Get {
        /// A path, an entry id, or a blob id.
        what: String,
        out: Option<PathBuf>,
        /// Replace OUT if it exists.
        #[arg(long)]
        force: bool,
        /// Chunk downloads in flight at once.
        #[arg(long, default_value_t = 4)]
        parallel: usize,
    },
    /// Write a file, or part of it, to stdout.
    Cat {
        /// A path, an entry id, or a blob id.
        what: String,
        /// `first-last`, `first-` or `-count`, like an HTTP Range.
        // allow_hyphen_values: `-7` is a range here, not a flag.
        #[arg(long, value_parser = parse_range, allow_hyphen_values = true)]
        range: Option<ByteRange>,
    },
    /// Show a file's entry and its blob's manifest.
    Info {
        /// A path, an entry id, or a blob id.
        what: String,
    },
    /// List unfinished uploads.
    Pending,
    /// Carry on with an unfinished upload.
    Resume {
        blob: String,
        #[arg(long, default_value_t = 3)]
        parallel: usize,
    },
    /// Abandon an unfinished upload and delete what it sent.
    Cancel { blob: String },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            STOP.store(true, Ordering::Relaxed);
            eprintln!("\nstopping once the chunks in flight land (Ctrl-C again to quit now)");
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        }
    });
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Stopped) => {
            eprintln!("dbx: stopped");
            ExitCode::from(130)
        }
        Err(e) => {
            eprintln!("dbx: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Error> {
    let home = cli.home.clone().unwrap_or_else(default_home);
    let db = || Db::open(&home.join("state.sqlite"));
    let remote = || -> Result<Remote, Error> {
        let key = cli
            .key
            .as_deref()
            .ok_or_else(|| Error::Invalid("no access key: set DBX_KEY or pass --key".into()))?;
        Ok(Remote::new(&cli.url, key)?)
    };
    let meter = Meter::new();
    let progress = |p: Progress| meter.show(p);

    match cli.command {
        Command::Init { name } => {
            let db = db()?;
            let fresh = db.device()?.is_none();
            let me = db.init_device(&name.unwrap_or_else(hostname))?;
            println!("{}  {}", me.id, me.name);
            if !fresh {
                eprintln!("  already set up; the name stays as it was");
            }
        }
        Command::Sync => {
            let (r, db) = (remote()?, db()?);
            device(&db)?;
            let report = sync::sync(&r, &db, &SyncOptions::default()).await?;
            eprintln!("{}", describe(&report));
        }
        Command::Devices => {
            let (r, db) = (remote()?, db()?);
            let me = db.device()?.map(|d| d.id);
            let applied: HashMap<String, u64> =
                db.peers()?.into_iter().map(|(id, p)| (id, p.applied)).collect();
            let pushed = db.published()?.seq;
            for d in sync::devices(&r).await? {
                let (log, mark) = if me.as_deref() == Some(&d.id) {
                    (pushed, "  (this device)")
                } else {
                    (applied.get(&d.id).copied().unwrap_or(0), "")
                };
                println!("{}  log {log:>5}  {}{mark}", d.id, d.name);
            }
        }
        Command::Ls { path, long, recursive } => {
            let view = db()?.view()?;
            match find(&view, &path)? {
                Some(e) if !e.is_folder() => line(e, &shown(&view, e), long),
                dir => list(&view, dir.map(|e| e.id.as_str()), "", long, recursive),
            }
        }
        Command::Mkdir { paths, parents } => {
            let db = db()?;
            device(&db)?;
            for path in &paths {
                if parents { mkdir_p(&db, path)? } else { mkdir(&db, path)? }
            }
            push(remote(), &db).await;
        }
        Command::Mv { src, dst } => {
            let db = db()?;
            device(&db)?;
            mv(&db, &src, &dst)?;
            push(remote(), &db).await;
        }
        Command::Rm { paths, recursive } => {
            let db = db()?;
            device(&db)?;
            for path in &paths {
                let view = db.view()?;
                let e = entry(&view, path)?;
                if e.is_folder() && !recursive {
                    return Err(Error::Invalid(format!("{path} is a folder; pass -r to delete it and its contents")));
                }
                let n = index::remove(&db, &e.id)?;
                eprintln!("removed {}{}", shown(&view, e), if n > 1 { format!(" and {} inside it", n - 1) } else { String::new() });
            }
            push(remote(), &db).await;
        }
        Command::Put { files, to, replace, blob_only, chunk_size, parallel } => {
            let (r, db) = (remote()?, db()?);
            if !blob_only {
                device(&db)?;
            }
            if replace.is_some() && files.len() != 1 {
                return Err(Error::Invalid("--replace takes exactly one file".into()));
            }
            let opts = UploadOptions { chunk_size, parallel };
            for file in files {
                let dest = destination(&db, &file, &to, replace.as_deref(), blob_only)?;
                let result = blob::upload(&r, &db, &file, dest, opts, &progress, &STOP).await;
                meter.clear();
                let up = match result {
                    Ok(up) => up,
                    Err(Error::Stopped) => {
                        eprintln!("{}: stopped; run the same `dbx put` again to resume", file.display());
                        return Err(Error::Stopped);
                    }
                    Err(e) => return Err(e),
                };
                let resumed = if up.skipped > 0 {
                    format!(", resumed: {} of {} chunks already sent", up.skipped, up.skipped + up.sent)
                } else {
                    String::new()
                };
                let m = &up.manifest;
                match &up.entry {
                    Some(id) => {
                        println!("{}", db.view()?.path(id).unwrap_or_else(|| id.clone()));
                        eprintln!("  {} {}, blob {}{resumed}", human(m.size), m.mime, m.blob);
                    }
                    None => {
                        println!("{}  {}", m.blob, file.display());
                        eprintln!("  {} {}{resumed}", human(m.size), m.mime);
                    }
                }
            }
            if !blob_only {
                push(Ok(r), &db).await;
            }
        }
        Command::Get { what, out, force, parallel } => {
            let (blob, name) = target(&db()?, &what)?;
            let out = match out {
                Some(out) => out,
                None => name.filter(|n| check_name(n).is_ok()).map(PathBuf::from).ok_or_else(|| {
                    Error::Invalid(format!("{what} has no name to save it under; give OUT"))
                })?,
            };
            if out.exists() && !force {
                return Err(Error::Invalid(format!("{} exists; pass --force to replace it", out.display())));
            }
            let r = remote()?;
            let m = blob::manifest(&r, &blob).await?;
            let result = blob::download(&r, &m, &out, parallel, &progress, &STOP).await;
            meter.clear();
            result?;
            eprintln!("{}  {}", human(m.size), out.display());
        }
        Command::Cat { what, range } => {
            let (blob, _) = target(&db()?, &what)?;
            let r = remote()?;
            let m = blob::manifest(&r, &blob).await?;
            cat(&r, &m, range).await?;
        }
        Command::Info { what } => {
            let db = db()?;
            let view = db.view()?;
            if let Ok(Some(e)) = find(&view, &what) {
                println!("path        {}", shown(&view, e));
                println!("entry       {}", e.id);
            }
            let (blob, _) = target(&db, &what)?;
            let m = blob::manifest(&remote()?, &blob).await?;
            println!("blob        {}", m.blob);
            println!("size        {} ({} bytes)", human(m.size), m.size);
            println!("type        {} ({})", m.mime, m.kind.as_str());
            println!("chunks      {} x {}", m.chunks.len(), human(m.chunk_size as u64));
            println!("sha256      {}", m.sha256);
            println!("encryption  {}", m.enc);
        }
        Command::Pending => {
            let pending = db()?.pending_uploads()?;
            if pending.is_empty() {
                eprintln!("no unfinished uploads");
            }
            for p in pending {
                let total = p.size.div_ceil(p.chunk_size as u64).max(1);
                println!(
                    "{}  {:>3}%  {:>10}  {}",
                    p.blob,
                    p.chunks_done as u64 * 100 / total,
                    human(p.size),
                    p.path.display()
                );
            }
        }
        Command::Resume { blob: id, parallel } => {
            let (r, db) = (remote()?, db()?);
            let result = blob::resume(&r, &db, &id, parallel, &progress, &STOP).await;
            meter.clear();
            let up = result?;
            println!("{}", up.manifest.blob);
            eprintln!("  {} {}", human(up.manifest.size), up.manifest.mime);
            if up.entry.is_some() {
                push(Ok(r), &db).await;
            }
        }
        Command::Cancel { blob: id } => {
            blob::cancel(&remote()?, &db()?, &id).await?;
            eprintln!("cancelled {id}");
        }
    }
    Ok(())
}

/// This device, set up under the computer's name if it isn't yet.
fn device(db: &Db) -> Result<Device, Error> {
    match db.device()? {
        Some(me) => Ok(me),
        None => {
            let me = db.init_device(&hostname())?;
            eprintln!("set up this device as {:?} ({})", me.name, me.id);
            Ok(me)
        }
    }
}

/// Push a change just made here. If the store can't be reached, it waits for
/// the next `dbx sync`; the change itself is done either way.
async fn push(remote: Result<Remote, Error>, db: &Db) {
    let result = match remote {
        Ok(r) => sync::sync(&r, db, &SyncOptions::default()).await,
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        eprintln!("dbx: saved on this device but not synced ({e}); `dbx sync` sends it");
    }
}

fn describe(r: &sync::Report) -> String {
    let others = if r.devices == 1 { "1 other device".to_owned() } else { format!("{} other devices", r.devices) };
    let mut s = format!("pushed {}, pulled {} from {others}", changes(r.pushed), changes(r.pulled));
    if r.registered {
        s.push_str("; registered this device");
    }
    if r.bootstrapped > 0 {
        s.push_str(&format!("; loaded {} checkpoint(s)", r.bootstrapped));
    }
    if let Some(n) = r.checkpoint {
        s.push_str(&format!("; wrote checkpoint {n}"));
    }
    if r.collected > 0 {
        s.push_str(&format!("; deleted {} old blob(s)", r.collected));
    }
    if r.read_only {
        s.push_str("; this key is read-only, so this device only follows");
    }
    s
}

fn changes(n: usize) -> String {
    if n == 1 { "1 change".into() } else { format!("{n} changes") }
}

// ---- paths ----

/// An entry by id or path. `Ok(None)` is the root.
fn find<'v>(view: &'v View, what: &str) -> Result<Option<&'v Entry>, Error> {
    if let Some(e) = view.get(what) {
        return Ok(Some(e));
    }
    view.resolve(what).map_err(|l| Error::Invalid(l.to_string()))
}

/// An entry by id or path, but not the root.
fn entry<'v>(view: &'v View, what: &str) -> Result<&'v Entry, Error> {
    find(view, what)?.ok_or_else(|| Error::Invalid("that's the root folder".into()))
}

/// A folder by id or path. `None` is the root.
fn folder<'v>(view: &'v View, what: &str) -> Result<Option<&'v Entry>, Error> {
    match find(view, what)? {
        Some(e) if !e.is_folder() => Err(Error::Invalid(format!("{} is a file, not a folder", shown(view, e)))),
        dir => Ok(dir),
    }
}

/// What a read means: a file in the index, by path or id, with its name; or,
/// failing that, a bare blob id.
fn target(db: &Db, what: &str) -> Result<(String, Option<String>), Error> {
    let view = db.view()?;
    if let Some(e) = view.get(what) {
        return content(&view, e);
    }
    match view.resolve(what) {
        Ok(Some(e)) => content(&view, e),
        Ok(None) => Err(Error::Invalid("that's the root folder".into())),
        Err(Lookup::NotFound(_)) if !what.contains('/') => Ok((what.to_owned(), None)),
        Err(l) => Err(Error::Invalid(l.to_string())),
    }
}

fn content(view: &View, e: &Entry) -> Result<(String, Option<String>), Error> {
    let c = e.content().ok_or_else(|| Error::Invalid(format!("{} is a folder", shown(view, e))))?;
    Ok((c.blob.clone(), Some(e.name().to_owned())))
}

/// `/a/b/c` → (`/a/b`, `c`).
fn split(path: &str) -> Result<(&str, &str), Error> {
    let trimmed = path.trim_end_matches('/');
    let (dir, name) = trimmed.rsplit_once('/').unwrap_or(("", trimmed));
    if name.is_empty() {
        return Err(Error::Invalid(format!("{path:?} doesn't name a file or folder")));
    }
    Ok((dir, name))
}

fn shown(view: &View, e: &Entry) -> String {
    view.path(&e.id).unwrap_or_else(|| e.id.clone())
}

fn destination(db: &Db, file: &std::path::Path, to: &str, replace: Option<&str>, blob_only: bool) -> Result<Dest, Error> {
    if blob_only {
        return Ok(Dest::Blob);
    }
    let view = db.view()?;
    if let Some(path) = replace {
        return Ok(Dest::Replace { id: entry(&view, path)?.id.clone() });
    }
    let name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Invalid(format!("{} has no file name to use", file.display())))?;
    let parent = folder(&view, to)?.map(|e| e.id.clone());
    Ok(Dest::New { parent, name: name.to_owned() })
}

// ---- commands ----

fn list(view: &View, dir: Option<&str>, prefix: &str, long: bool, recursive: bool) {
    for e in view.children(dir) {
        let name = format!("{prefix}{}", e.name());
        line(e, &name, long);
        if recursive && e.is_folder() {
            list(view, Some(&e.id), &format!("{name}/"), long, recursive);
        }
    }
}

fn line(e: &Entry, name: &str, long: bool) {
    let slash = if e.is_folder() { "/" } else { "" };
    if !long {
        println!("{name}{slash}");
        return;
    }
    let (size, kind) = match e.content() {
        // Without `; charset=…`, so the columns line up.
        Some(c) => (human(c.size), c.mime.split(';').next().unwrap_or_default()),
        None => ("-".to_owned(), "folder"),
    };
    println!("{}  {size:>10}  {kind:<24}  {name}{slash}", e.id);
}

fn mkdir(db: &Db, path: &str) -> Result<(), Error> {
    let (dir, name) = split(path)?;
    let view = db.view()?;
    let parent = folder(&view, dir)?.map(|e| e.id.clone());
    index::mkdir(db, parent.as_deref(), name)?;
    Ok(())
}

fn mkdir_p(db: &Db, path: &str) -> Result<(), Error> {
    let mut parent: Option<String> = None;
    for name in path.split('/').filter(|c| !c.is_empty()) {
        let view = db.view()?;
        let existing = view.children(parent.as_deref()).find(|e| e.name() == name).map(|e| (e.id.clone(), e.is_folder()));
        parent = Some(match existing {
            Some((id, true)) => id,
            Some((id, false)) => {
                return Err(Error::Invalid(format!("{} is a file, not a folder", view.path(&id).unwrap_or(id))));
            }
            None => index::mkdir(db, parent.as_deref(), name)?,
        });
    }
    Ok(())
}

fn mv(db: &Db, src: &str, dst: &str) -> Result<(), Error> {
    let view = db.view()?;
    let src = entry(&view, src)?.id.clone();
    let dest = match view.get(dst) {
        Some(e) => Ok(Some(e)),
        None => view.resolve(dst),
    };
    match dest {
        // Into the root, or into a folder, keeping the name.
        Ok(None) => index::move_to(db, &src, None, None),
        Ok(Some(e)) if e.is_folder() => index::move_to(db, &src, Some(&e.id), None),
        Ok(Some(e)) => Err(Error::Invalid(format!("{} exists", shown(&view, e)))),
        // A new path: move and rename.
        Err(Lookup::NotFound(_)) => {
            let (dir, name) = split(dst)?;
            let parent = folder(&view, dir)?.map(|e| e.id.clone());
            index::move_to(db, &src, parent.as_deref(), Some(name))
        }
        Err(l) => Err(Error::Invalid(l.to_string())),
    }
}

/// Stream a blob, or a range of it, to stdout a chunk at a time.
async fn cat(r: &Remote, m: &Manifest, range: Option<ByteRange>) -> Result<(), Error> {
    let (first, last) = match range {
        Some(range) => blob::resolve(range, m.size).ok_or(Error::Range { size: m.size })?,
        None if m.size == 0 => return Ok(()),
        None => (0, m.size - 1),
    };
    let mut out = std::io::stdout().lock();
    for span in m.spans(first, last) {
        if STOP.load(Ordering::Relaxed) {
            return Err(Error::Stopped);
        }
        let bytes = blob::read_span(r, m, span).await?;
        match out.write_all(&bytes) {
            // The reader went away (`dbx cat … | head`): that's the end.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
            other => other?,
        }
    }
    out.flush()?;
    Ok(())
}

/// A one-line progress meter on stderr, drawn only when stderr is a terminal.
struct Meter {
    on: bool,
}

impl Meter {
    fn new() -> Meter {
        Meter { on: std::io::stderr().is_terminal() }
    }

    fn show(&self, p: Progress) {
        if self.on {
            let pct = (p.done * 100).checked_div(p.total).unwrap_or(100);
            eprint!("\r\x1b[2K  {pct:>3}%  {} / {}", human(p.done), human(p.total));
        }
    }

    fn clear(&self) {
        if self.on {
            eprint!("\r\x1b[2K");
        }
    }
}

fn default_home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".dbx")
}

/// This computer's name, for naming the device: `hostname`, without macOS's
/// `.local`.
fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_end_matches(".local").to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dbx".into())
}

fn human(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn parse_size(s: &str) -> Result<u32, String> {
    let (digits, shift) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 10),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 20),
        _ => (s, 0),
    };
    let n: u64 = digits.parse().map_err(|_| format!("{s:?} is not a size"))?;
    let bytes = n.checked_shl(shift).filter(|b| (1..=64 << 20).contains(b));
    bytes.map(|b| b as u32).ok_or_else(|| format!("{s} is outside 1 byte to 64M"))
}

fn parse_range(s: &str) -> Result<ByteRange, String> {
    let bad = || format!("{s:?} is not a range: use first-last, first- or -count");
    let (a, b) = s.split_once('-').ok_or_else(bad)?;
    let num = |t: &str| t.parse::<u64>().map_err(|_| bad());
    match (a.is_empty(), b.is_empty()) {
        (false, false) => Ok(ByteRange::Span(num(a)?, num(b)?)),
        (false, true) => Ok(ByteRange::From(num(a)?)),
        (true, false) => Ok(ByteRange::Last(num(b)?)),
        (true, true) => Err(bad()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("8M"), Ok(8 << 20));
        assert_eq!(parse_size("64k"), Ok(64 << 10));
        assert_eq!(parse_size("1000"), Ok(1000));
        assert_eq!(parse_size("64M"), Ok(64 << 20));
        assert!(parse_size("65M").is_err());
        assert!(parse_size("0").is_err());
        assert!(parse_size("M").is_err());
        assert!(parse_size("1G").is_err());
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("0-99"), Ok(ByteRange::Span(0, 99)));
        assert_eq!(parse_range("100-"), Ok(ByteRange::From(100)));
        assert_eq!(parse_range("-5"), Ok(ByteRange::Last(5)));
        assert!(parse_range("-").is_err());
        assert!(parse_range("5").is_err());
        assert!(parse_range("a-b").is_err());
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(1023), "1023 B");
        assert_eq!(human(1536), "1.5 KiB");
        assert_eq!(human(8 << 20), "8.0 MiB");
    }

    #[test]
    fn paths_split() {
        assert_eq!(split("/a/b/c").unwrap(), ("/a/b", "c"));
        assert_eq!(split("/c/").unwrap(), ("", "c"));
        assert_eq!(split("c").unwrap(), ("", "c"));
        assert!(split("/").is_err());
    }

    #[test]
    fn mv_and_mkdir_p() {
        let db = Db::open_in_memory().unwrap();
        db.init_device("t").unwrap();
        mkdir_p(&db, "/a/b").unwrap();
        mkdir_p(&db, "/a/b").unwrap();
        mkdir(&db, "/c").unwrap();
        assert!(mkdir(&db, "/c").is_err());
        mv(&db, "/c", "/a").unwrap();
        mv(&db, "/a/c", "/a/b/d").unwrap();
        mv(&db, "/a/b", "/").unwrap();
        let view = db.view().unwrap();
        let mut paths: Vec<String> = view.entries().map(|e| view.path(&e.id).unwrap()).collect();
        paths.sort();
        assert_eq!(paths, ["/a", "/b", "/b/d"]);
        assert!(mv(&db, "/b", "/b/d").is_err(), "into itself");
        assert!(mv(&db, "/nope", "/a").is_err());
    }
}
