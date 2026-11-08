# drawBridge

Personal cloud storage on top of [swiftSearch](https://github.com/Bryce-Hart1/swiftSearch).
Upload, browse, stream or download photos, music, video, files and text notes
from any of your devices. drawBridge has no server of its own. The Rust core in the
app talks to swiftSearch through castle+ over HTTPS, and directly over loopback
in development.

**Status: M5.** Done so far:
- the swiftSearch client
- chunked, resumable blobs, with a local upload queue in SQLite
- the shared folder tree: each device writes its changes to its own op log in
  the store, and every device merges all the logs into the same tree (newest
  edit wins, deletes are final), with checkpoints for new devices and garbage
  collection of replaced content
- the `dbx` CLI
- the desktop app (Tauri): a monochrome, terminal-style window for browsing,
  uploading (picker or drag-and-drop, folders included), downloading, moving,
  renaming, deleting, notes, `.lin` diagrams (keyboard-drawn, animated, exported
  as JPEG, PNG or SVG), search across every folder, and photo thumbnails
- streaming and previews: photos full size, audio and video that play and seek
  without downloading the whole file, PDFs, code as text, video posters and
  HEIC thumbnails, through a capped on-disk cache
- running against the real server over HTTPS (swiftSearch behind castle+ and a
  Cloudflare Tunnel), and working offline: changes and uploads wait for the
  store and carry on by themselves when it's back

What drawBridge writes into the store is specified in [docs/FORMAT.md](docs/FORMAT.md).

## Layout

| Path | What it is |
|---|---|
| `crates/core` | `drawbridge-core`: the store format, all traffic to swiftSearch, the chunk cache and the stream server |
| `crates/cli` | `dbx`, the core from the shell |
| `app/src` | The app's UI: Svelte 5 and TypeScript, no UI library |
| `app/src-tauri` | The app's shell: commands, sync loop, transfer queue, `thumb://`, the cache and stream wiring |
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
| `dbx stream WHAT` | Serves the file on loopback the way the app's player reads it, and prints a URL for curl, a browser, IINA or VLC. Runs until Ctrl-C. |
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

To install it as an ordinary app, build a release and copy it to Applications:

```bash
npx tauri build --bundles app
```

```bash
cp -R ../target/release/bundle/macos/drawBridge.app /Applications/
```

The release build keeps the key in the Keychain rather than in `dev-keys.json`,
so the first launch asks to connect again; the folders and settings are the
same ones the dev build used. The app is signed ad hoc, not with a Developer
ID, so after each rebuild macOS may ask once whether it may read its Keychain
item (Always Allow). Picking straight from Photos in the upload panel needs
the bundled app: it's what can ask for access to the Photos library.

`npm run dev` on its own serves the UI to a plain browser at
http://127.0.0.1:1420 with a fake backend (`app/src/lib/mock.ts`), for working on
the look without a store. Add `?setup` for the setup screen.

Every action has a key, shown along the bottom; `?` lists them all. The app
syncs at launch, when its window comes to the front, after each change, and
every minute while it's on screen. Hidden, it makes no requests.

Enter opens a file where it can be read:
- **Diagrams** (`.lin`) open playing in the diagram editor; `e` edits. See below.
- **Text and code** open in the editor. Over 1 MiB, the first MiB shows read-only.
- **Photos, PDFs, songs and videos** open in the viewer. Songs and videos get a
  player line: space plays, ← → seek, ↑ ↓ set the volume.
- **Formats WebKit can't play** (mkv, avi) can be opened in IINA, VLC or
  QuickTime with `e`.
- **Anything else** saves a copy.

### Diagrams

A `.lin` file is a diagram: circles, triangles, rectangles and diamonds, joined
by lines. It opens playing, with its lines pulsing; `e` switches to editing.
`l` on the main screen starts a new one (so `l` no longer moves right; use `→`
or Enter), and so does `n` with a name ending in `.lin`. Everything is a key
except where things go, which is the mouse: a shape key arms a tool and the
next click places it, and dragging moves a shape or one end of a line. Drop an
end on a shape and it stays attached when the shape moves.

A new line is plain; arrowheads are something you add to it (`a`: none, then
one arrow, then two). Lines that join the same two shapes, such as one from A
to B and one from B to A, are drawn side by side rather than on top of each
other, far enough apart that a label on each can be read. A line can be straight, curved or square-cornered (`b`), solid, dashed
or dotted (`w`), and carry a short label (select it, then Enter).

| Keys | Does |
|---|---|
| `c` `t` `r` `d` | place a circle, triangle, rectangle or diamond (then click) |
| `l` | draw a line: click where it starts, then where it ends |
| `tab` `⇧tab` | select the next, previous part |
| click, `⇧` click | select; `⇧` adds a part to the selection or takes it out |
| drag on empty page | select everything the box touches |
| `⌘A` | select everything |
| `←` `→` `↑` `↓` | resize the shape (`⇧` for bigger steps) |
| `⌥` + arrows | move the selection 10 px (`⇧` as well for 1 px) |
| `0`–`9` | colour: a see-through wash with a solid outline (`0` is plain, in the page's colour) |
| `i` `o` `p` | small, medium, large text (for a line, its label) |
| `⏎` | type in the shape, or the label of a line. In a shape `⏎` starts a new line, so a list is one item per row (typing `\n` does the same, and the two characters become the break); `esc` finishes, and `⏎` finishes a line's one-row label. It stops when the text no longer fits, and a shape won't shrink, or the font grow, past its text |
| `←` `→` | while typing in a shape: move the caret through the letters (`⇧` selects them), so you can type or delete in the middle of the text |
| `⌥B` `⌥I` `⌥U` | while typing in a shape: bold, italic or underline the selected letters (off again if they all have it already). With nothing selected it switches the style for the letters you type next. Only letters that are there get a style, and they keep it when text is typed or deleted around them. A line's label can't be styled |
| `a` | line → arrow → double arrow |
| `f` | flip a line's direction |
| `b` | bend a line: straight → curved → square corners |
| `w` | outline: solid → dashed → dotted (shapes and lines; a dotted line stays dotted while it pulses) |
| `m` | pulse: off → start to end → end to start → out from the middle |
| `s` | pulse as a ball, or as a light inside the line |
| `[` `]` | send back, bring forward one step (`⇧` for all the way) |
| `⌘D` | duplicate, a little way off |
| `⌘C` `⌘V` | copy, and paste where the pointer is (into another diagram too) |
| `g` | snap to a 10 px grid; shapes also line up with each other, with a guide. Hold `⌥` while dragging to skip both |
| `+` `-` `z` | zoom in, out, fit the page. The wheel or a trackpad moves the page, and `⌘` or a pinch zooms. Dragging moves it while you're only viewing |
| `⌫` | delete (lines on a deleted shape stay where they ended) |
| `⌘Z` `⇧⌘Z` | undo, redo |
| `⌘S` | save, as a new version of the file, like a note |
| `x` | export. The file name you choose sets the format: `.jpg`, `.png` or `.svg` (cropped to the drawing, on white; images at twice the size) |
| `space` | pause or play the animation (while viewing; the space bar does nothing while editing) |
| `esc` | cancel the tool, deselect, close |

A `.lin` is JSON text with one part per line, so it syncs like any note. The
format is read and written in `app/src/lib/lin/format.ts`; a file from a newer
drawBridge is refused by name rather than half-read. Dashes, bends and labels
are only written when a part uses them, so a plain diagram's file is the same as
it always was. In the folder list a diagram's thumbnail is the diagram itself,
drawn the first time it's selected.

Files are read through a small HTTP server on `127.0.0.1`. It starts the first
time something is opened and stops after ten idle minutes, and its URLs carry a
random token. What it fetches is kept in a cache on disk, capped at 2 GiB; `,`
opens the settings to change the cap or clear the cache. A video or HEIC photo
gets its thumbnail the first time it's selected: the window draws one from the
file and stores it for every device.

## Deploying

In production the store is swiftSearch V2 on the home server, behind castle+,
reached through a Cloudflare Tunnel:

```
app ──HTTPS──▶ Cloudflare ──tunnel──▶ cloudflared ──▶ castle+ :8443 ──▶ swiftSearch 127.0.0.1:9201
```

**castle+ route.** No `auth`: castle+ would take the `Authorization` header
for itself, and swiftSearch checks each device's own key. Only blob reads and
writes and the health check get through:

```ini
[vault]
host     = vault.brycehart.dev
backend  = 127.0.0.1:9201
max_body = 64M
allow    = /v1/blobs/ /healthz
```

**Cloudflare.** The tunnel's public hostname points at `https://127.0.0.1:8443`.
It needs three settings:
- **No TLS Verify**, because castle+'s certificate is self-signed. That hop
  never leaves the server.
- **Disable Chunked Encoding**, or uploads reach castle+ chunked and get 501.
- **A Cache Rule that bypasses the cache** for the hostname.

**A device.** Each device gets its own key, made on the server (add `--access read`
for one that only follows):

```bash
doas su -s /bin/sh castle -c 'cd /var/lib/swiftsearch-logs && /opt/swiftsearch/swiftSearch key add --store /var/lib/swiftsearch --name <device>'
```

`key list` shows the keys by id and name, and `key remove <id>` revokes one
within a second. Then the device connects to `https://vault.brycehart.dev` with
its key.

**Checking it.** [scripts/check_vault.sh](scripts/check_vault.sh) runs the
whole path from outside. It checks:
- swiftSearch's own 401 when the key is missing, and 403 when a read key writes
- a byte-for-byte round trip of a chunk, Range, and conditional reads
- that nothing is cached on the way
- that admin, box, unbox and prepare are refused at the edge
- the 64 MiB cap

```bash
H=https://vault.brycehart.dev RW=<write key> RO=<read key> scripts/check_vault.sh
```

**Offline.** When the store can't be reached:
- The title bar shows `○ offline` and how many changes wait to be sent.
- Uploads and downloads pause at `⏸ waiting for the store` and pick up from
  their last chunk.
- While the window is on screen, the app retries after 5, 10, 20 and 40 s, then
  every minute.

## Tests

The integration tests start a real swiftSearch in a throwaway store. Build it
first, in your swiftSearch checkout:

```bash
cmake -S ~/Developer/swiftSearch -B ~/Developer/swiftSearch/build && cmake --build ~/Developer/swiftSearch/build -j
```

```bash
cargo test --workspace
```

The UI's checks run from `app/`:

```bash
npm run check && npm test
```

The tests look for the binary at
`~/Developer/swiftSearch/build/bin/swiftSearch`, then at the older sibling
checkout `../multithread_Search/swiftSearch`. Set `SWIFTSEARCH_BIN` to use one
somewhere else.
