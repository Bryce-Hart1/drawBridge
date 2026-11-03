# drawBridge

Personal cloud storage on top of [swiftSearch](../multithread_Search/swiftSearch).
Upload, browse, stream or download photos, music, video, files and text notes
from any of your devices. drawBridge has no server of its own. The Rust core in the
app talks to swiftSearch through castle+ over HTTPS, and directly over loopback
in development.

**Status: M0.** Done so far: the store key layout (`crates/core/src/keys.rs`) and
the swiftSearch client (`crates/core/src/remote.rs`). Still to come, in order:
chunked blobs and the `dbx` CLI, the op log and sync, the Tauri app, streaming,
and deployment.

## Tests

The integration tests start a real swiftSearch in a throwaway store. Build it
first:

```bash
cmake -S ../multithread_Search/swiftSearch -B ../multithread_Search/swiftSearch/build && cmake --build ../multithread_Search/swiftSearch/build -j
```

```bash
cargo test -p drawbridge-core
```

The tests look for the binary at
`../multithread_Search/swiftSearch/build/bin/swiftSearch`. Set `SWIFTSEARCH_BIN` to
use one somewhere else.
