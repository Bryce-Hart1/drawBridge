// Claude — Date 11/03/2026
// `dbx`: drawbridge-core from the shell. It exercises the library end to end
// before any UI exists, and stays useful for poking at a store afterwards.
//
//   DBX_URL   the store: http://127.0.0.1:8080 (swiftSearch on loopback), or
//             https://vault.<domain> through castle+
//   DBX_KEY   a swiftSearch access key (`swiftSearch key add`)
//   DBX_HOME  local state, ~/.dbx by default: the upload queue lives here
//
// Ctrl-C during a transfer lets the chunks in flight finish, so an upload can
// pick up from there with the same `dbx put`. A second Ctrl-C quits at once.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::{Parser, Subcommand};
use drawbridge_core::blob::{self, UploadOptions};
use drawbridge_core::{ByteRange, Db, Error, Manifest, Progress, Remote};

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
    /// Upload files as blobs; an interrupted upload of the same file resumes.
    Put {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Chunk size: bytes, or with K or M (powers of 1024). At most 64M.
        #[arg(long, default_value = "8M", value_parser = parse_size)]
        chunk_size: u32,
        /// Chunk uploads in flight at once.
        #[arg(long, default_value_t = 3)]
        parallel: usize,
    },
    /// Download a blob to a file.
    Get {
        blob: String,
        out: PathBuf,
        /// Replace OUT if it exists.
        #[arg(long)]
        force: bool,
        /// Chunk downloads in flight at once.
        #[arg(long, default_value_t = 4)]
        parallel: usize,
    },
    /// Write a blob, or part of it, to stdout.
    Cat {
        blob: String,
        /// `first-last`, `first-` or `-count`, like an HTTP Range.
        // allow_hyphen_values: `-7` is a range here, not a flag.
        #[arg(long, value_parser = parse_range, allow_hyphen_values = true)]
        range: Option<ByteRange>,
    },
    /// Print a blob's manifest.
    Info { blob: String },
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
        let key = cli.key.as_deref().ok_or_else(|| {
            Error::Io(std::io::Error::other("no access key: set DBX_KEY or pass --key"))
        })?;
        Ok(Remote::new(&cli.url, key)?)
    };
    let meter = Meter::new();
    let progress = |p: Progress| meter.show(p);

    match cli.command {
        Command::Put { files, chunk_size, parallel } => {
            let (r, db) = (remote()?, db()?);
            let opts = UploadOptions { chunk_size, parallel };
            for file in files {
                let result = blob::upload(&r, &db, &file, opts, &progress, &STOP).await;
                meter.clear();
                match result {
                    Ok(up) => {
                        let resumed = if up.skipped > 0 {
                            format!(", resumed: {} of {} chunks already sent", up.skipped, up.skipped + up.sent)
                        } else {
                            String::new()
                        };
                        println!("{}  {}", up.manifest.blob, file.display());
                        eprintln!("  {} {}{resumed}", human(up.manifest.size), up.manifest.mime);
                    }
                    Err(Error::Stopped) => {
                        eprintln!("{}: stopped; run the same `dbx put` again to resume", file.display());
                        return Err(Error::Stopped);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Command::Get { blob: id, out, force, parallel } => {
            if out.exists() && !force {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("{} exists; pass --force to replace it", out.display()),
                )));
            }
            let r = remote()?;
            let m = blob::manifest(&r, &id).await?;
            let result = blob::download(&r, &m, &out, parallel, &progress, &STOP).await;
            meter.clear();
            result?;
            eprintln!("{}  {}", human(m.size), out.display());
        }
        Command::Cat { blob: id, range } => {
            let r = remote()?;
            let m = blob::manifest(&r, &id).await?;
            cat(&r, &m, range).await?;
        }
        Command::Info { blob: id } => {
            let m = blob::manifest(&remote()?, &id).await?;
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
        }
        Command::Cancel { blob: id } => {
            blob::cancel(&remote()?, &db()?, &id).await?;
            eprintln!("cancelled {id}");
        }
    }
    Ok(())
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
}
