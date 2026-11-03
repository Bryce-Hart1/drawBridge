# drawBridge store format

This is what drawBridge writes into swiftSearch, and the contract any other
client must follow to read or write the same store (the Swift app, a recovery
tool, `curl`). The Rust implementation is `crates/core`. When the two disagree,
this file is wrong.

**Status:** blobs (v1) are settled. The index (devices, op logs, checkpoints)
arrives with M2.

---

## Keys

Everything lives under the root `dbx/v1/`:

| Key | Written by | Contents |
|---|---|---|
| `dbx/v1/obj/<blob>/<i>` | the uploader, once | Chunk `i` of a blob's content: raw bytes |
| `dbx/v1/obj/<blob>/manifest.json` | the uploader, once, after every chunk | How to put the chunks back together |
| `dbx/v1/obj/<blob>/thumb` | the uploader | Thumbnail or video poster (M4) |
| `dbx/v1/devices.json`, `dbx/v1/dev/<device>/…` | | The index (M2) |

- **A key is made only from fixed words, ids and decimal integers.** File names
  never appear in a key: they live in the index. Every key fits in
  `[A-Za-z0-9._-]` segments joined by `/`, which needs no percent-encoding and is
  always valid for swiftSearch.
- **`<blob>` is a UUID** (v7 in practice: lowercase, hyphenated). One blob is one
  immutable version of a file's content. Editing a file makes a new blob.
- **`<i>` is the chunk index** in decimal, from `0`, with no padding.

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
