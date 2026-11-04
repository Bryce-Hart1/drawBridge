// Claude — Date 11/03/2026
// The one place the UI talks to Rust. Inside the app, every call is an
// `invoke` of a command in src-tauri/src/commands.rs, and every event comes
// from there too. In a plain browser (`npm run dev`), an in-memory fake serves
// the same interface (mock.ts), so the UI can be worked on without the app.

import { convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { Clash, Hit, Id, Listing, NoteTarget, Status, SyncStatus, Transfer, UploadPlan } from './types';

export type Unlisten = () => void;

export type DropEvent = { type: 'enter' | 'drop'; paths: string[] } | { type: 'over' } | { type: 'leave' };

export interface Backend {
  status(): Promise<Status>;
  connect(url: string, key: string, deviceName: string): Promise<Status>;
  disconnect(): Promise<void>;
  list(dir: Id | null): Promise<Listing>;
  search(query: string): Promise<Hit[]>;
  mkdir(parent: Id | null, name: string): Promise<Id>;
  rename(id: Id, name: string): Promise<void>;
  moveTo(id: Id, parent: Id | null): Promise<void>;
  /** Returns how many entries were deleted: the entry and everything in it. */
  remove(id: Id): Promise<number>;
  upload(paths: string[], parent: Id | null, onClash: Clash): Promise<UploadPlan>;
  /** Returns the transfer's id. */
  download(id: Id, path: string): Promise<number>;
  cancel(transfer: number): Promise<void>;
  transfers(): Promise<Transfer[]>;
  noteRead(id: Id): Promise<string>;
  /** Returns the note's entry id, new or not. */
  noteSave(target: NoteTarget, text: string): Promise<Id>;
  syncNow(): Promise<void>;

  /** A URL an `<img>` can load the blob's thumbnail from. */
  thumbUrl(blob: string): string;
  pickFiles(): Promise<string[] | null>;
  pickSavePath(name: string): Promise<string | null>;
  onIndexChanged(cb: () => void): Promise<Unlisten>;
  onSyncStatus(cb: (s: SyncStatus) => void): Promise<Unlisten>;
  onTransfer(cb: (t: Transfer) => void): Promise<Unlisten>;
  onDragDrop(cb: (e: DropEvent) => void): Promise<Unlisten>;
}

function tauri(): Backend {
  return {
    status: () => invoke<Status>('status'),
    connect: (url, key, deviceName) => invoke<Status>('connect', { url, key, deviceName }),
    disconnect: () => invoke<void>('disconnect'),
    list: (dir) => invoke<Listing>('list', { dir }),
    search: (query) => invoke<Hit[]>('search', { query }),
    mkdir: (parent, name) => invoke<Id>('mkdir', { parent, name }),
    rename: (id, name) => invoke<void>('rename', { id, name }),
    moveTo: (id, parent) => invoke<void>('move_to', { id, parent }),
    remove: (id) => invoke<number>('remove', { id }),
    upload: (paths, parent, onClash) => invoke<UploadPlan>('upload', { paths, parent, onClash }),
    download: (id, path) => invoke<number>('download', { id, path }),
    cancel: (transfer) => invoke<void>('cancel', { transfer }),
    transfers: () => invoke<Transfer[]>('transfers'),
    noteRead: (id) => invoke<string>('note_read', { id }),
    noteSave: (target, text) => invoke<Id>('note_save', { target, text }),
    syncNow: () => invoke<void>('sync_now'),

    thumbUrl: (blob) => convertFileSrc(blob, 'thumb'),
    pickFiles: async () => (await open({ multiple: true, directory: false })) ?? null,
    pickSavePath: (name) => save({ defaultPath: name }),
    onIndexChanged: (cb) => listen('index-changed', () => cb()),
    onSyncStatus: (cb) => listen<SyncStatus>('sync-status', (e) => cb(e.payload)),
    onTransfer: (cb) => listen<Transfer>('transfer', (e) => cb(e.payload)),
    onDragDrop: (cb) =>
      getCurrentWebview().onDragDropEvent(({ payload: p }) =>
        cb(p.type === 'enter' || p.type === 'drop' ? { type: p.type, paths: p.paths } : { type: p.type }),
      ),
  };
}

export const api: Backend = isTauri() ? tauri() : (await import('./mock')).mockBackend();
