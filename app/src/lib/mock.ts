// Claude — Date 11/03/2026
// A stand-in backend for working on the UI in a plain browser: an in-memory
// tree, transfers that tick along on a timer, and drawn thumbnails. The app
// itself never loads this (see api.ts).
//
// Add `?setup` to the URL to start at the setup screen. On it, the key `bad`
// is rejected and anything else connects.

import type { Backend, DropEvent, Unlisten } from './api';
import type { CmdError, Hit, Id, Item, Listing, NoteTarget, Status, SyncStatus, Transfer } from './types';

interface Node {
  id: Id;
  name: string;
  kind: 'folder' | 'file';
  parent: Id | null;
  size: number | null;
  mime: string | null;
  blob: string | null;
  text?: string;
}

const MIME: Record<string, string> = {
  jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', webp: 'image/webp', gif: 'image/gif',
  mp4: 'video/mp4', mov: 'video/quicktime', mp3: 'audio/mpeg', flac: 'audio/flac',
  md: 'text/markdown; charset=utf-8', txt: 'text/plain; charset=utf-8', log: 'text/plain; charset=utf-8',
  csv: 'text/csv; charset=utf-8', json: 'application/json', pdf: 'application/pdf',
};

function mimeOf(name: string): string {
  return MIME[name.split('.').pop()?.toLowerCase() ?? ''] ?? 'application/octet-stream';
}

let seq = 0;
const nodes = new Map<Id, Node>();

function add(parent: Id | null, name: string, size?: number, text?: string): Id {
  seq += 1;
  const id = `0192f0a5-${seq.toString(16).padStart(4, '0')}-7000-8000-000000000000`;
  const kind = size === undefined && text === undefined ? 'folder' : 'file';
  const file = kind === 'file';
  nodes.set(id, {
    id,
    name,
    kind,
    parent,
    size: file ? (text !== undefined ? new TextEncoder().encode(text).length : size!) : null,
    mime: file ? mimeOf(name) : null,
    blob: file ? `0192f0b0-${seq.toString(16).padStart(4, '0')}-7000-8000-000000000000` : null,
    text,
  });
  return id;
}

const photos = add(null, 'Photos');
const y2026 = add(photos, '2026');
for (const [name, size] of [['beach.jpg', 2_048_311], ['sunset.jpg', 3_401_772], ['harbor.png', 5_120_003], ['dunes.webp', 812_440], ['fog.jpg', 1_733_090], ['pier.jpg', 2_903_118]] as const) {
  add(y2026, name, size);
}
add(y2026, 'clip.mp4', 85_337_100);
const trips = add(photos, 'trips');
for (let i = 1; i <= 14; i++) add(trips, `lisbon-${String(i).padStart(2, '0')}.jpg`, 1_500_000 + i * 91_337);
const music = add(null, 'Music');
add(music, 'side-a.mp3', 7_431_220);
add(music, 'side-b.flac', 31_902_117);
const docs = add(null, 'Documents');
add(docs, 'notes.md', undefined, '# notes\n\n- call the plumber\n- back up the photos\n- read about castle+ keep-alive\n');
add(docs, 'budget.csv', undefined, 'month,rent,food\njan,1200,410\nfeb,1200,388\n');
add(docs, 'lease.pdf', 402_118);
const logs = add(null, 'logs');
for (let i = 0; i < 400; i++) add(logs, `run-${String(i).padStart(3, '0')}.log`, undefined, `run ${i}: ok\n`);
add(null, 'readme.md', undefined, 'drawBridge mock store.\n\nEverything here is fake.\n');
add(null, 'render-final.mov', 2_400_118_337);

function fail(kind: string, message: string): never {
  throw { kind, message } satisfies CmdError;
}

function live(id: Id): Node {
  return nodes.get(id) ?? fail('not_found', `entry ${id} not found`);
}

function children(dir: Id | null): Node[] {
  return [...nodes.values()].filter((n) => n.parent === dir);
}

function item(n: Node): Item {
  return {
    id: n.id,
    name: n.name,
    kind: n.kind,
    size: n.size,
    mime: n.mime,
    blob: n.blob,
    count: n.kind === 'folder' ? children(n.id).length : null,
  };
}

function pathOf(dir: Id | null): string {
  const names: string[] = [];
  for (let d = dir; d !== null; d = live(d).parent) names.unshift(live(d).name);
  return `/${names.join('/')}`;
}

function checkNew(parent: Id | null, name: string, except?: Id) {
  if (!name || name === '.' || name === '..' || name.includes('/')) fail('invalid', `${JSON.stringify(name)} can't be a name`);
  if (children(parent).some((n) => n.name === name && n.id !== except)) {
    fail('invalid', `${pathOf(parent).replace(/\/$/, '')}/${name} already exists`);
  }
}

function subtree(id: Id): Id[] {
  const out = [id];
  for (let i = 0; i < out.length; i++) for (const c of children(out[i])) out.push(c.id);
  return out;
}

const bus = new Map<string, Set<(payload: never) => void>>();

function on<T>(name: string, cb: (payload: T) => void): Promise<Unlisten> {
  const set = bus.get(name) ?? new Set();
  bus.set(name, set);
  set.add(cb as (payload: never) => void);
  return Promise.resolve(() => set.delete(cb as (payload: never) => void));
}

function emit(name: string, payload?: unknown) {
  for (const cb of bus.get(name) ?? []) (cb as (p: unknown) => void)(payload);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// Transfers run one at a time, like the real queue.
let nextTransfer = 1;
const transfers = new Map<number, Transfer>();
const finish = new Map<number, () => void>();

setInterval(() => {
  const running = [...transfers.values()].find((t) => t.state === 'running');
  const t = running ?? [...transfers.values()].find((t) => t.state === 'queued');
  if (!t) return;
  t.state = 'running';
  t.done = Math.min(t.total, t.done + Math.max(1, Math.round(t.total / 25)));
  if (t.done >= t.total) {
    t.state = 'done';
    finish.get(t.id)?.();
    finish.delete(t.id);
  }
  emit('transfer', { ...t });
}, 120);

function queue(dir: 'up' | 'down', name: string, total: number, done: () => void): number {
  const t: Transfer = { id: nextTransfer++, dir, name, done: 0, total, state: 'queued', error: null };
  transfers.set(t.id, t);
  finish.set(t.id, done);
  emit('transfer', { ...t });
  return t.id;
}

let sync: SyncStatus = { state: 'idle', lastOkMs: Date.now(), message: null, readOnly: false };
let connected = !location.search.includes('setup');
let device: string | null = connected ? 'laptop' : null;

function status(): Status {
  return { connected, url: connected ? 'http://127.0.0.1:8080' : null, device, suggestedName: 'Mock MacBook', sync };
}

function thumb(blob: string): string {
  // A little landscape, different for every blob. The UI shows it in gray.
  let h = 0;
  for (const c of blob) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const r = () => ((h = (h * 1103515245 + 12345) >>> 0) / 2 ** 32);
  const sky = `hsl(${Math.floor(r() * 360)} 60% ${70 + Math.floor(r() * 20)}%)`;
  const sun = { x: 30 + r() * 200, y: 30 + r() * 60, r: 12 + r() * 22 };
  const hill = (base: number, shade: number) => {
    let d = `M0 ${base}`;
    for (let x = 0; x <= 256; x += 32) d += ` L${x} ${base - r() * 50}`;
    return `<path d="${d} L256 192 L0 192 Z" fill="hsl(${Math.floor(r() * 360)} 30% ${shade}%)"/>`;
  };
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 192">` +
    `<rect width="256" height="192" fill="${sky}"/>` +
    `<circle cx="${sun.x}" cy="${sun.y}" r="${sun.r}" fill="#fff8e0"/>` +
    hill(150, 45) + hill(175, 25) + `</svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

export function mockBackend(): Backend {
  return {
    async status() {
      return status();
    },
    async connect(url, key, deviceName) {
      await sleep(700);
      if (!/^https?:\/\//.test(url)) fail('invalid', `${url} isn't an http:// or https:// address`);
      if (key === 'bad') fail('unauthorized', 'the server rejected this key');
      connected = true;
      device = deviceName || 'Mock MacBook';
      return status();
    },
    async disconnect() {
      connected = false;
    },
    async list(dir) {
      if (dir !== null && live(dir).kind !== 'folder') fail('not_found', 'not a folder');
      const items = children(dir)
        .sort((a, b) => (a.kind === b.kind ? a.name.toLowerCase().localeCompare(b.name.toLowerCase()) : a.kind === 'folder' ? -1 : 1))
        .map(item);
      const path = [];
      for (let d = dir; d !== null; d = live(d).parent) path.unshift({ id: d, name: live(d).name });
      return { dir, path, items } satisfies Listing;
    },
    async search(query) {
      const q = query.toLowerCase();
      return [...nodes.values()]
        .filter((n) => n.name.toLowerCase().includes(q))
        .sort((a, b) => Number(!a.name.toLowerCase().startsWith(q)) - Number(!b.name.toLowerCase().startsWith(q)))
        .slice(0, 200)
        .map((n): Hit => ({ item: item(n), parent: n.parent, path: pathOf(n.parent) }));
    },
    async mkdir(parent, name) {
      checkNew(parent, name);
      const id = add(parent, name);
      emit('index-changed');
      return id;
    },
    async rename(id, name) {
      const n = live(id);
      checkNew(n.parent, name, id);
      n.name = name;
      emit('index-changed');
    },
    async moveTo(id, parent) {
      const n = live(id);
      if (parent !== null && subtree(id).includes(parent)) fail('invalid', `can't move ${pathOf(id)} into itself`);
      checkNew(parent, n.name, id);
      n.parent = parent;
      emit('index-changed');
    },
    async remove(id) {
      const ids = subtree(live(id).id);
      for (const i of ids) nodes.delete(i);
      emit('index-changed');
      return ids.length;
    },
    async upload(paths, parent, onClash) {
      const names = paths.map((p) => p.split('/').pop() ?? p);
      const taken = names.filter((name) => children(parent).some((n) => n.name === name));
      if (taken.length && onClash === 'ask') return { queued: 0, skipped: 0, clashes: taken };
      let queued = 0;
      let skipped = 0;
      for (let name of names) {
        const existing = children(parent).find((n) => n.name === name);
        if (existing && onClash === 'skip') {
          skipped++;
          continue;
        }
        if (existing && onClash === 'keep') {
          const dot = name.lastIndexOf('.');
          const [stem, ext] = dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
          for (let i = 2; children(parent).some((n) => n.name === name); i++) name = `${stem} (${i})${ext}`;
        }
        const size = 400_000 + Math.floor(Math.random() * 9_000_000);
        const target = name;
        queue('up', target, size, () => {
          if (existing && onClash === 'replace') existing.size = size;
          else add(parent, target, size);
          emit('index-changed');
        });
        queued++;
      }
      return { queued, skipped, clashes: [] };
    },
    async download(id) {
      const n = live(id);
      return queue('down', n.name, n.size ?? 0, () => {});
    },
    async cancel(id) {
      const t = transfers.get(id);
      if (t && (t.state === 'running' || t.state === 'queued')) {
        t.state = 'stopped';
        finish.delete(id);
        emit('transfer', { ...t });
      }
    },
    async transfers() {
      return [...transfers.values()];
    },
    async noteRead(id) {
      await sleep(150);
      return live(id).text ?? '';
    },
    async noteSave(target: NoteTarget, text) {
      await sleep(250);
      let id: Id;
      if ('id' in target) {
        id = target.id;
        const n = live(id);
        n.text = text;
        n.size = new TextEncoder().encode(text).length;
        n.blob = `${n.blob}-x`;
      } else {
        checkNew(target.parent, target.name);
        id = add(target.parent, target.name, undefined, text);
      }
      emit('index-changed');
      return id;
    },
    async syncNow() {
      sync = { ...sync, state: 'syncing' };
      emit('sync-status', sync);
      await sleep(900);
      sync = { ...sync, state: 'idle', lastOkMs: Date.now() };
      emit('sync-status', sync);
    },

    thumbUrl: thumb,
    async pickFiles() {
      return ['/Users/you/Desktop/new-photo.jpg', '/Users/you/Desktop/beach.jpg'];
    },
    async pickSavePath(name) {
      return `/Users/you/Downloads/${name}`;
    },
    onIndexChanged: (cb) => on('index-changed', cb),
    onSyncStatus: (cb) => on('sync-status', cb),
    onTransfer: (cb) => on('transfer', cb),
    onDragDrop(cb: (e: DropEvent) => void) {
      // The browser gives names, not paths; good enough for a fake upload.
      let depth = 0;
      const enter = (e: DragEvent) => {
        e.preventDefault();
        if (depth++ === 0) cb({ type: 'enter', paths: [] });
      };
      const over = (e: DragEvent) => e.preventDefault();
      const leave = () => {
        if (--depth === 0) cb({ type: 'leave' });
      };
      const drop = (e: DragEvent) => {
        e.preventDefault();
        depth = 0;
        const files = [...(e.dataTransfer?.files ?? [])];
        cb({ type: 'drop', paths: files.map((f) => `/dropped/${f.name}`) });
      };
      window.addEventListener('dragenter', enter);
      window.addEventListener('dragover', over);
      window.addEventListener('dragleave', leave);
      window.addEventListener('drop', drop);
      return Promise.resolve(() => {
        window.removeEventListener('dragenter', enter);
        window.removeEventListener('dragover', over);
        window.removeEventListener('dragleave', leave);
        window.removeEventListener('drop', drop);
      });
    },
  };
}
