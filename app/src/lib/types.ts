// Claude — Date 11/03/2026
// What crosses between the UI and Rust: the shapes `invoke` returns and events
// carry. Each mirrors a struct in app/src-tauri/src (serde, camelCase); change
// both sides together.

/** An entry id: a file or a folder in the index. */
export type Id = string;

export interface Item {
  id: Id;
  name: string;
  kind: 'folder' | 'file';
  /** Files: bytes of the current content. */
  size: number | null;
  /** Files: MIME type, from the name at upload. */
  mime: string | null;
  /** Files: the blob holding the current content. */
  blob: string | null;
  /** Folders: entries shown directly inside. */
  count: number | null;
}

export interface Crumb {
  id: Id;
  name: string;
}

export interface Listing {
  /** The folder listed; `null` is the root. */
  dir: Id | null;
  /** The folders from the root down to `dir`, root itself left out. */
  path: Crumb[];
  /** Folders first, then files, each by name. */
  items: Item[];
}

export interface Hit {
  item: Item;
  /** The folder it's in, `null` for the root. */
  parent: Id | null;
  /** That folder's path: `/Photos/2026`, or `/`. */
  path: string;
}

export type SyncState = 'idle' | 'syncing' | 'offline' | 'error';

export interface SyncStatus {
  state: SyncState;
  /** When the last sync finished cleanly. */
  lastOkMs: number | null;
  /** What went wrong, for `offline` and `error`. */
  message: string | null;
  /** The key can read but not write. */
  readOnly: boolean;
  /** Changes made here that the store doesn't have yet. */
  pending: number;
}

export interface Status {
  connected: boolean;
  url: string | null;
  device: string | null;
  /** A name for this device, for the setup screen: the computer's name. */
  suggestedName: string;
  sync: SyncStatus;
}

/** What to do with an upload whose name is taken. `ask` returns the clashes instead. */
export type Clash = 'ask' | 'replace' | 'keep' | 'skip';

export interface UploadPlan {
  queued: number;
  skipped: number;
  /** With `ask`: the names that are taken. Nothing was queued. */
  clashes: string[];
}

/** `waiting`: the store couldn't be reached; it goes again once it can. */
export type TransferState = 'queued' | 'running' | 'waiting' | 'done' | 'failed' | 'stopped';

export interface Transfer {
  id: number;
  dir: 'up' | 'down';
  name: string;
  done: number;
  total: number;
  state: TransferState;
  error: string | null;
}

/** A text file as the editor gets it. */
export interface NoteText {
  text: string;
  /** Only the first 1 MiB is in `text`: shown read-only. */
  truncated: boolean;
}

/** The chunk cache, for the settings. */
export interface CacheInfo {
  bytes: number;
  chunks: number;
  cap: number;
  /** Bytes of cached thumbnails, counted apart from the cap. */
  thumbs: number;
}

/** Where a note is saved: over an existing file, or as a new one. */
export type NoteTarget = { id: Id } | { parent: Id | null; name: string };

/** How a command fails. `kind` is one of: invalid, not_found, not_connected, unauthorized, forbidden, offline, remote, other. */
export interface CmdError {
  kind: string;
  message: string;
}
