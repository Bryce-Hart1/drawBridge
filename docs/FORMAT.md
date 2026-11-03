# drawBridge store format

This is what drawBridge writes into swiftSearch, and the contract any other
client must follow to read or write the same store (the Swift app, a recovery
tool, `curl`). The Rust implementation is `crates/core`. When the two disagree,
this file is wrong.

**Status:** v1. Blobs (M1) and the index (M2: devices, op logs, checkpoints)
are settled.

---

## Keys

Everything lives under the root `dbx/v1/`:

| Key | Written by | Contents |
|---|---|---|
| `dbx/v1/obj/<blob>/<i>` | the uploader, once | Chunk `i` of a blob's content: raw bytes |
| `dbx/v1/obj/<blob>/manifest.json` | the uploader, once, after every chunk | How to put the chunks back together |
| `dbx/v1/obj/<blob>/thumb` | the uploader | Thumbnail or video poster (M4) |
| `dbx/v1/devices.json` | every device, read-merge-write | The devices sharing the store |
| `dbx/v1/dev/<device>/log/<seq>.json` | that device, once | A batch of index changes |
| `dbx/v1/dev/<device>/head.json` | that device | How far its log goes |
| `dbx/v1/dev/<device>/ckpt/<n>.json` | that device | A snapshot of the whole index |

- **A key is made only from fixed words, ids and decimal integers.** File names
  never appear in a key: they live in the index. Every key fits in
  `[A-Za-z0-9._-]` segments joined by `/`, which needs no percent-encoding and is
  always valid for swiftSearch.
- **`<blob>` and `<device>` are UUIDs** (v7 in practice: lowercase, hyphenated).
  One blob is one immutable version of a file's content. Editing a file makes a
  new blob.
- **`<i>`, `<seq>` and `<n>` are decimal integers** with no padding. Chunks count
  from `0`; log seqs and checkpoints count from `1`.
- **JSON values** (manifests, logs, heads, checkpoints, `devices.json`) are PUT
  with `X-SwiftSearch-Type: str`.

## Blobs

### Chunks

- The content is cut into consecutive pieces of `chunk_size` bytes. The last
  piece holds whatever is left, so it may be shorter. An empty file has no chunks.
- Chunk `i` covers bytes `i × chunk_size` up to (not including)
  `min((i + 1) × chunk_size, size)`.
- `chunk_size` is at most 64 MiB, swiftSearch's largest value. drawBridge
  defaults to 8 MiB (8388608).
- Each chunk is PUT with `X-SwiftSearch-Type` set to the blob's `kind`, so the
  store stays readable on its own.

### Manifest

`manifest.json` is a JSON object, PUT with `X-SwiftSearch-Type: str`:

```json
{
  "v": 1,
  "blob": "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f",
  "size": 20971523,
  "chunk_size": 8388608,
  "chunks": ["9f86d0…", "60303a…", "2c26b4…"],
  "sha256": "e3b0c4…",
  "mime": "video/mp4",
  "kind": "video",
  "enc": "none"
}
```

| Field | Meaning |
|---|---|
| `v` | Manifest format version, `1`. A reader refuses any version it doesn't know. |
| `blob` | The blob's id, which must match the key it was read from |
| `size` | Bytes of content |
| `chunk_size` | Bytes per chunk, `1` to `67108864` |
| `chunks` | sha256 of each chunk, in order, as 64 lowercase hex digits. Its length must be `ceil(size / chunk_size)`. |
| `sha256` | sha256 of the whole content, 64 lowercase hex digits |
| `mime` | The content's MIME type, from the file's extension when uploaded |
| `kind` | swiftSearch type of the chunks: `img`, `video`, `str` or `bin` (audio is `bin`) |
| `enc` | How the chunks are encrypted. Always `"none"` in v1. A reader refuses any other value rather than return ciphertext as content. |

The manifest contains nothing that varies between two uploads of the same
bytes under the same blob id (no timestamps). Rewriting it after a crash is
therefore idempotent.

### Writing a blob

1. PUT every chunk, in any order, with any number in flight.
2. Then PUT the manifest.

**A manifest exists only once all of its chunks are in the store.** Nothing in
the index may refer to a blob before its manifest is written. A reader that finds
no manifest treats the blob as absent, even if some chunks exist.

An abandoned upload (cancelled, or its file changed) deletes its chunks and its
manifest. Chunks orphaned by a crash are harmless dead space. Clients delete what
they can find, and swiftSearch compaction reclaims it.

### Reading a blob

- **A whole chunk** must be exactly its expected length, and its sha256 must
  match `chunks[i]`. Anything else means the store is damaged. The reader stops
  and never returns the bytes.
- **Part of a chunk** (a `Range` request, as a media player makes) can't be
  hashed against the manifest. It relies on TLS in transit and on swiftSearch's
  per-record checksums, which never let a damaged value out (500).
- **A whole download** that matches every chunk hash is the uploaded content.
  The whole-file `sha256` is for cross-checking against a local file.

### Serving content

A client that serves blob content to a web view (the desktop app's stream
server) must never serve `text/html` or `image/svg+xml` as such. Both can run
script in the app's origin. Serve them as `text/plain` or
`application/octet-stream`. swiftSearch applies the same rule to its own responses.


## The index

swiftSearch can't list keys and has no compare-and-swap, so the folder tree
isn't stored as one value that devices take turns rewriting. Each device
appends its own changes (ops) to its own log, and every device merges all the
logs into the same tree. The merge depends only on the ops, so devices that have
read the same logs show the same tree, in whatever order they read them.

### Devices

`devices.json`:

```json
{
  "v": 1,
  "devices": [
    { "id": "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f", "name": "laptop", "added_ms": 1762200000000 }
  ]
}
```

- A device adds itself by GET, append, PUT. It never removes or changes another
  device's line. No `devices.json` yet means an empty list.
- Two devices adding themselves at the same moment can each overwrite the other,
  so one line is lost. **Every sync checks that the device's own line is there
  and adds it back if not**, so a lost line returns on that device's next sync.
- A reader must cope with a listed device that has no `head.json` yet
  (registered, nothing pushed) and should skip ids that aren't valid key
  segments.
- A device with a read-only key can't write, so it never appears here.

### Ops

A batch, `dev/<device>/log/<seq>.json`:

```json
{
  "v": 1,
  "device": "0192f0a4-7c1e-7b3a-9f00-5d2c8e1a4b6f",
  "seq": 3,
  "ops": [
    { "t": [1762200000000, 0], "id": "0192f0a5-…", "op": "folder", "name": "Photos", "parent": null },
    { "t": [1762200000000, 1], "id": "0192f0a5-…", "op": "file", "name": "beach.jpg",
      "parent": "0192f0a5-…", "blob": "0192f0a5-…", "size": 2048311, "mime": "image/jpeg", "sha256": "…" }
  ]
}
```

- A device's seqs start at `1` with no gaps. A log is never changed once
  written; a retried PUT sends identical bytes.
- `device` and `seq` must match the key the batch was read from.
- drawBridge puts at most 5000 ops in a batch. Readers accept any number.

Every op has `t`, its clock reading (below), and `id`, the entry (file or
folder) it changes, a UUID chosen by the device that created the entry.

| `op` | Other fields | Does |
|---|---|---|
| `folder` | `name`, `parent` | Creates a folder |
| `file` | `name`, `parent`, `blob`, `size`, `mime`, `sha256` | Creates a file whose content is that blob |
| `content` | `blob`, `size`, `mime`, `sha256` | Gives a file new content |
| `rename` | `name` | Renames |
| `move` | `parent` | Moves |
| `delete` | | Deletes the entry, for good |

- `parent` is a folder's entry id, or `null` for the root.
- `size`, `mime` and `sha256` repeat the blob's manifest, so a listing needs no
  manifest fetches.
- **A blob is named by exactly one op**: the `file` or `content` op its uploader
  made after writing its manifest. No op copies a blob. Garbage collection
  depends on this.
- **Names** are 1 to 255 bytes of UTF-8, not `.` or `..`, with no `/` and no
  control characters. A reader should still cope with any string from a careless
  writer: show it, but never use it as a local path without checking it.

### Heads

`dev/<device>/head.json`:

```json
{ "v": 1, "seq": 7, "ckpt": 1 }
```

- `seq` is the device's last log written. Every log `1` to `seq` exists.
- `ckpt` is its latest checkpoint, `0` for none.
- **A device writes its logs first, then its head**, so a reader that sees a
  head finds every log it counts. After a crash between the two, the device
  writes both again.
- Readers poll heads with `If-None-Match`, so an unchanged head costs a 304.

### Clock

`t` is a hybrid logical clock reading `[ms, counter]`: wall-clock milliseconds,
and a counter for events within one millisecond or after the clock steps back.
Both are JSON integers; one 64-bit number would lose precision in JavaScript.

- Stamping a local op: if `now_ms > last.ms`, the stamp is `[now_ms, 0]`;
  otherwise it is `[last.ms, last.counter + 1]`.
- Reading another device's op (or a checkpoint) moves `last` up to the greatest
  stamp in it. So a change made after seeing another is always stamped later.
- **Ops are ordered by `(t, device)`**: `ms`, then `counter`, then the device id
  as a string. Two different devices never tie.

### Merge

Each entry is a set of registers. Applying a set of ops in any order, any
number of times, gives the same registers.

| Register | Set by | Keeps |
|---|---|---|
| created (folder or file) | `folder`, `file` | The op with the smallest `(t, device)` |
| name | `folder`, `file`, `rename` | The op with the greatest `(t, device)` |
| parent | `folder`, `file`, `move` | The op with the greatest `(t, device)` |
| content | `file`, `content` | The op with the greatest `(t, device)` |
| deleted | `delete` | Set once and never cleared, whatever the stamps |

- **The newest edit wins.** There are no conflict copies.
- **A delete is final.** A rename, move or new content arriving later, from any
  device, doesn't bring the entry back.
- An op can arrive before the op that created its entry (it was in a log read
  first). It is kept, unseen, and takes effect when the create arrives.
- A folder's content register, if any op set one, is ignored.

### The tree

What a device shows is computed from the registers alone:

- **An entry is shown** if it was created and isn't deleted.
- **It is shown in the folder its parent register names**, if that folder is
  shown. Otherwise it is shown at the root. That covers a folder deleted on one
  device while something was moved into it on another, a parent that is a file,
  and a parent not seen yet.
- **Loops are broken at the newest move.** Two devices can each move one folder
  into the other (`p` into `q` on one, `q` into `p` on the other). Of the entries
  in a loop, the one whose parent register has the greatest `(t, device)` is
  shown at the root. Loops never share an entry, so it doesn't matter in which
  order they are broken.
- **Names can repeat in one folder**, when two devices add the same name at
  once. Folders list by `(name, id)`, and a path that matches more than one entry
  needs an id to pick one. drawBridge refuses to make a duplicate on its own.
- **Deleting a folder** writes `delete` for it and for everything shown inside
  it, in one batch. Anything moved into it on another device at the same moment
  ends up at the root.

### Checkpoints

A new device would otherwise read every log from `1`. Instead it can start from
a checkpoint, `dev/<device>/ckpt/<n>.json`:

```json
{
  "v": 1,
  "device": "0192f0a4-…",
  "n": 2,
  "hlc": [1762200000000, 4],
  "vector": { "0192f0a4-…": 41, "0192f0b9-…": 17 },
  "entries": [
    {
      "id": "0192f0a5-…",
      "created": { "at": { "t": [1762200000000, 0], "device": "0192f0a4-…" }, "value": "file" },
      "name":    { "at": { "t": [1762200000300, 0], "device": "0192f0b9-…" }, "value": "beach.jpg" },
      "parent":  { "at": { "t": [1762200000000, 0], "device": "0192f0a4-…" }, "value": null },
      "content": { "at": { "t": [1762200000000, 0], "device": "0192f0a4-…" },
                   "value": { "blob": "0192f0a5-…", "size": 2048311, "mime": "image/jpeg", "sha256": "…" } },
      "deleted": { "t": [1762200000900, 0], "device": "0192f0b9-…" }
    }
  ]
}
```

- `entries` holds every entry's registers as the writer had them. A register
  never written is left out.
- `vector[d] = s` means the entries include every op in logs `1` to `s` of
  device `d`. They may include some later ops too; applying those again changes
  nothing.
- **Loading one:** merge each entry into the local state with the rules above,
  then treat each device's log as read up to `vector[d]` (or further, if it
  already was), and move the clock past `hlc`.
- drawBridge writes one after every 500 ops applied (its own and others'),
  updates `head.ckpt`, then deletes the previous checkpoint. A reader that finds
  a checkpoint gone reads that device's logs instead. A checkpoint that would
  be over 64 MiB isn't written.

### Sync

One sync runs in this order:

1. **Push.** Write any new logs, then the head.
2. **Register.** Make sure `devices.json` lists this device.
3. **Pull.** Do a conditional GET of each other device's head. A device read for
   the first time whose head names a checkpoint is loaded from that checkpoint.
   Then read each device's logs, from just after the last one read to its head's
   `seq`, and apply them in seq order.
4. **Checkpoint**, when one is due.
5. **Collect garbage.**

### Garbage collection

A blob is garbage, and any device may delete it, when the entry its op names:

- has been deleted, or
- was created and its content register now holds a different blob.

A blob named by an op whose entry hasn't been created yet isn't garbage: that
entry's create may still make it current.

This is safe because a blob is named by exactly one op and registers only move
forward: once a blob is garbage it can never become current again. Blobs
uploaded but not yet named by any op are invisible to it.

drawBridge waits 10 minutes after it notices, so a player still streaming the
old content isn't cut off, then deletes the chunks, the thumbnail and finally
the manifest. A blob whose manifest is gone has nothing else left.
