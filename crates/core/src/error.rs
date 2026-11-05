// Claude — Date 11/03/2026
// The error every drawbridge-core operation above the raw swiftSearch client
// returns. The client keeps its own `remote::Error`, which lands here as
// `Error::Remote`, so a caller can still tell a 401 from a dropped connection.

use std::fmt;
use std::path::PathBuf;

use crate::keys::KeyError;
use crate::remote;

#[derive(Debug)]
pub enum Error {
    Remote(remote::Error),
    Io(std::io::Error),
    Db(rusqlite::Error),
    Key(KeyError),
    /// No such blob: its manifest isn't in the store.
    NotFound(String),
    /// Something in the store disagrees with its manifest: a chunk is missing,
    /// the wrong size, or fails its sha256. Never retried and never returned as
    /// data.
    Corrupt(String),
    /// The range starts past the end of a `size`-byte blob, or the blob is empty.
    Range { size: u64 },
    /// The local file changed while it was being uploaded. What was uploaded
    /// is discarded; upload it again.
    SourceChanged(PathBuf),
    /// The caller asked the transfer to stop. An upload keeps its progress and
    /// resumes from where it stopped.
    Stopped,
    /// A request that can't be carried out as asked: a bad name, a move into
    /// a file, an option out of range. The message says what to change.
    Invalid(String),
    /// The index needs a device id (every op is stamped with one), and this
    /// local state doesn't have one yet. `Db::init_device` makes one.
    NoDevice,
}

impl Error {
    /// The store couldn't be reached (see `remote::Error::unreachable`): worth
    /// waiting for, not reporting as a failure.
    pub fn unreachable(&self) -> bool {
        matches!(self, Error::Remote(e) if e.unreachable())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Remote(e) => e.fmt(f),
            Error::Io(e) => write!(f, "local file: {e}"),
            Error::Db(e) => write!(f, "local state: {e}"),
            Error::Key(e) => e.fmt(f),
            Error::NotFound(what) => write!(f, "{what} not found"),
            Error::Corrupt(why) => write!(f, "store data is damaged: {why}"),
            Error::Range { size } => write!(f, "range is outside the {size}-byte value"),
            Error::SourceChanged(path) => {
                write!(f, "{} changed while it was being uploaded", path.display())
            }
            Error::Stopped => f.write_str("stopped"),
            Error::Invalid(why) => f.write_str(why),
            Error::NoDevice => f.write_str("this device isn't set up yet: it has no device id"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Remote(e) => Some(e),
            Error::Io(e) => Some(e),
            Error::Db(e) => Some(e),
            Error::Key(e) => Some(e),
            _ => None,
        }
    }
}

impl From<remote::Error> for Error {
    fn from(e: remote::Error) -> Error {
        Error::Remote(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Db(e)
    }
}

impl From<KeyError> for Error {
    fn from(e: KeyError) -> Error {
        Error::Key(e)
    }
}
