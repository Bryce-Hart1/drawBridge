// Claude — Date 11/03/2026
// drawbridge-core: the client library behind every drawBridge front end. It
// owns the store format and all traffic to swiftSearch; the Tauri app and the
// `dbx` CLI are thin layers over it.
//
// Built so far (M0): the store key layout and the swiftSearch client. Chunked
// blobs, the op log and sync, the cache and the stream server come next.

pub mod keys;
pub mod remote;

pub use keys::{Key, KeyError};
pub use remote::{ByteRange, Conditional, Error, Kind, Meta, Partial, Remote, Retry, Stored, Value};
