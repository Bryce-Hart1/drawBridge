// Claude — Date 11/03/2026
// drawbridge-core: the client library behind every drawBridge front end. It
// owns the store format and all traffic to swiftSearch; the Tauri app and the
// `dbx` CLI are thin layers over it.
//
// Built so far: the store key layout and the swiftSearch client (M0); chunked,
// resumable blobs with the local upload queue (M1); and the shared index (M2):
// the op model and merge, the local Db, local changes, and sync between
// devices. The cache and the stream server come next.

pub mod blob;
pub mod db;
pub mod error;
pub mod index;
pub mod keys;
pub mod mime;
pub mod oplog;
pub mod remote;
pub mod sync;

pub use blob::{Dest, Manifest, Progress, Span, UploadOptions, Uploaded};
pub use db::{Db, Device, PendingUpload};
pub use error::Error;
pub use keys::{Key, KeyError};
pub use oplog::{Entry, EntryKind, Lookup, View};
pub use remote::{ByteRange, Conditional, Kind, Meta, Partial, Remote, Retry, Stored, Value};
pub use sync::{Report, SyncOptions};
