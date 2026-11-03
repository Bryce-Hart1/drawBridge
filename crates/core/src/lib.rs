// Claude — Date 11/03/2026
// drawbridge-core: the client library behind every drawBridge front end. It
// owns the store format and all traffic to swiftSearch; the Tauri app and the
// `dbx` CLI are thin layers over it.
//
// Built so far: the store key layout and the swiftSearch client (M0), and
// chunked, resumable blobs with the local upload queue (M1). The op log and
// sync, the cache and the stream server come next.

pub mod blob;
pub mod db;
pub mod error;
pub mod keys;
pub mod mime;
pub mod remote;

pub use blob::{Manifest, Progress, Span, UploadOptions, Uploaded};
pub use db::{Db, PendingUpload};
pub use error::Error;
pub use keys::{Key, KeyError};
pub use remote::{ByteRange, Conditional, Kind, Meta, Partial, Remote, Retry, Stored, Value};
