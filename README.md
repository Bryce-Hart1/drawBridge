# drawBridge

Personal cloud storage on top of [swiftSearch](../multithread_Search/swiftSearch).
Upload, browse, stream or download photos, music, video, files and text notes
from any of your devices. drawBridge has no server of its own. The Rust core in the
app talks to swiftSearch through castle+ over HTTPS, and directly over loopback
in development.

**Status: M1.** Done so far:
- the swiftSearch client
- chunked, resumable blobs, with a local upload queue in SQLite
- the `dbx` CLI

Still to come, in order: the shared index and sync between devices, the Tauri
app, streaming, and deployment. What drawBridge writes into the store is
specified in [docs/FORMAT.md](docs/FORMAT.md).

## Layout

| Path | What it is |
|---|---|
| `crates/core` | `drawbridge-core`: the store format and all traffic to swiftSearch |
| `crates/cli` | `dbx`, the core from the shell |
| `docs/FORMAT.md` | The on-store format: the contract any other client follows |

## Trying it with `dbx`

Start a swiftSearch, then point `dbx` at it:

```bash
export DBX_URL=http://127.0.0.1:8080 DBX_KEY=ssk_...   # a key from `swiftSearch key add`
```

```bash
cargo run -q -p drawbridge-cli -- put clip.mp4        # prints the blob id
```

```bash
cargo run -q -p drawbridge-cli -- get <blob> back.mp4
```

| Command | Does |
|---|---|
| `dbx put FILE…` | Uploads in 8 MiB chunks (`--chunk-size`, `--parallel`). Ctrl-C stops once the chunks in flight land; the same `put` resumes. |
| `dbx get BLOB OUT` | Downloads and checks every chunk's sha256. It won't replace `OUT` without `--force`. |
| `dbx cat BLOB [--range R]` | Writes the blob to stdout, or a range of it: `first-last`, `first-` or `-count` |
| `dbx info BLOB` | Prints the blob's manifest: size, type, chunks, sha256 |
| `dbx pending` | Lists unfinished uploads |
| `dbx resume BLOB` / `dbx cancel BLOB` | Finishes an unfinished upload, or deletes what it sent |

Local state goes in `~/.dbx` (set `DBX_HOME` to change that). Plain `http://` is
accepted only for loopback; anything else must be `https://` through castle+.

## Tests

The integration tests start a real swiftSearch in a throwaway store. Build it
first:

```bash
cmake -S ../multithread_Search/swiftSearch -B ../multithread_Search/swiftSearch/build && cmake --build ../multithread_Search/swiftSearch/build -j
```

```bash
cargo test --workspace
```

The tests look for the binary at
`../multithread_Search/swiftSearch/build/bin/swiftSearch`. Set `SWIFTSEARCH_BIN` to
use one somewhere else.
