# drawBridge

Personal cloud storage on top of [swiftSearch](https://github.com/Bryce-Hart1/swiftSearch).
Upload, browse, stream or download photos, music, video, files and text notes
from any of your devices. drawBridge has no server of its own. The Rust core in the
app talks to swiftSearch through castle+ over HTTPS, and directly over loopback
in development.

**Status: M3.** Done so far:
- the swiftSearch client
- chunked, resumable blobs, with a local upload queue in SQLite
- the shared folder tree: each device writes its changes to its own op log in
  the store, and every device merges all the logs into the same tree (newest
  edit wins, deletes are final), with checkpoints for new devices and garbage
  collection of replaced content
- the `dbx` CLI
- the desktop app (Tauri): a monochrome, terminal-style window for browsing,
  uploading (picker or drag-and-drop, folders included), downloading, moving,
  renaming, deleting, notes, search across every folder, and photo thumbnails

Still to come, in order: streaming and previews (photos full size, audio and
video), and deployment. What
drawBridge writes into the store is specified in [docs/FORMAT.md](docs/FORMAT.md).

## Layout

| Path | What it is |
|---|---|
| `crates/core` | `drawbridge-core`: the store format and all traffic to swiftSearch |
| `crates/cli` | `dbx`, the core from the shell |
| `app/src` | The app's UI: Svelte 5 and TypeScript, no UI library |
| `app/src-tauri` | The app's shell: commands, sync loop, transfer queue, `thumb://` |
| `docs/FORMAT.md` | The on-store format: the contract any other client follows |

## Trying it with `dbx`

Start a swiftSearch, then point `dbx` at it:

```bash
export DBX_URL=http://127.0.0.1:8080 DBX_KEY=ssk_...   # a key from `swiftSearch key add`
```

```bash
cargo run -q -p drawbridge-cli -- mkdir /Videos
```

```bash
cargo run -q -p drawbridge-cli -- put clip.mp4 --to /Videos     # prints /Videos/clip.mp4
```

On another device (another `DBX_HOME`, same URL, its own key):

```bash
cargo run -q -p drawbridge-cli -- sync && cargo run -q -p drawbridge-cli -- get /Videos/clip.mp4
```

| Command | Does |
|---|---|
| `dbx init [--name N]` | Gives this device its id. Other commands do this on first use, named after the computer. |
| `dbx sync` | Pushes this device's changes and pulls everyone else's |
| `dbx devices` | Lists the devices sharing the store |
| `dbx ls [PATH] [-l] [-R]` | Lists a folder; `-l` adds ids, sizes and types |
| `dbx mkdir PATH… [-p]` | Makes folders |
| `dbx mv SRC DST` | Moves into DST if it's a folder, otherwise moves and renames to DST |
| `dbx rm PATH… [-r]` | Deletes, at once and for every device; folders need `-r` |
| `dbx put FILE… [--to DIR]` | Uploads in 8 MiB chunks (`--chunk-size`, `--parallel`). Ctrl-C stops once the chunks in flight land; the same `put` resumes. `--replace PATH` gives an existing file new content; `--blob-only` stores a bare blob outside the tree. |
| `dbx get WHAT [OUT]` | Downloads and checks every chunk's sha256. It won't replace `OUT` without `--force`. |
| `dbx cat WHAT [--range R]` | Writes a file to stdout, or a range of it: `first-last`, `first-` or `-count` |
| `dbx info WHAT` | Prints the file's entry and its blob's manifest: size, type, chunks, sha256 |
| `dbx pending` | Lists unfinished uploads |
| `dbx resume BLOB` / `dbx cancel BLOB` | Finishes an unfinished upload, or deletes what it sent |

`WHAT` is a path like `/Videos/clip.mp4`, an entry id (from `ls -l`), or a bare
blob id. Paths read this device's copy of the tree, so run `dbx sync` to see
other devices' changes. Changes are made here first and then synced at once. If
the store can't be reached, they wait for the next `dbx sync`.

Local state goes in `~/.dbx` (set `DBX_HOME` to change that): this device's id,
its copy of the tree, and the upload queue. Plain `http://` is accepted only for
loopback; anything else must be `https://` through castle+.

## The desktop app

Needs Node (with npm) and Xcode's command line tools. From `app/`:

```bash
npm install
```

```bash
npm run tauri dev
```

The first run shows a setup screen: the store's address, this device's key, and
a name for the device. Give each device its own key (`swiftSearch key add
--name <device>`); a read-only key makes a device that follows but can't change
anything. Release
builds keep the key in the macOS Keychain; debug builds keep it in
`dev-keys.json` in the app's data folder, so rebuilding doesn't ask for
Keychain access each time.

`npm run dev` on its own serves the UI to a plain browser at
http://127.0.0.1:1420 with a fake backend (`app/src/lib/mock.ts`), for working on
the look without a store. Add `?setup` for the setup screen.

Every action has a key, shown along the bottom; `?` lists them all. The app
syncs at launch, when its window comes to the front, after each change, and
every minute while it's on screen. Hidden, it makes no requests.

## Tests

The integration tests start a real swiftSearch in a throwaway store. Build it
first:

```bash
cmake -S ../multithread_Search/swiftSearch -B ../multithread_Search/swiftSearch/build && cmake --build ../multithread_Search/swiftSearch/build -j
```

```bash
cargo test --workspace
```

The UI's checks run from `app/`:

```bash
npm run check && npm test
```

The tests look for the binary at
`../multithread_Search/swiftSearch/build/bin/swiftSearch`. Set `SWIFTSEARCH_BIN` to
use one somewhere else.
