// Claude — Date 11/03/2026
// A stand-in backend for working on the UI in a plain browser: an in-memory
// tree, transfers that tick along on a timer, and drawn thumbnails. The app
// itself never loads this (see api.ts).
//
// The viewer gets drawn photos, a generated tone for songs and a one-page PDF.
// Videos never load, so the player's can't-play panel shows; `tape.mkv` gets
// it straight away, as the app would.
//
// Add `?setup` to the URL to start at the setup screen. On it, the key `bad`
// is rejected and anything else connects. Add `?offline` to start with the
// store unreachable: changes waiting, and an upload waiting for it.

import type { Backend, DropEvent, Unlisten } from './api';
import { serializeLin } from './lin/format';
import type { Doc } from './lin/model';
import type { CacheInfo, CmdError, Hit, Id, Item, Listing, NoteTarget, Status, SyncStatus, Transfer } from './types';

/** Documents/flow.lin: every shape, every kind of line, and both ways a line can pulse. */
const SAMPLE_DIAGRAM: Doc = {
  lin: 1,
  w: 1200,
  h: 800,
  next: 16,
  items: [
    { id: 1, t: 'rect', x: 200, y: 200, w: 180, h: 80, c: 6, f: 'o', text: 'client', ln: 'solid', st: '' },
    { id: 2, t: 'diamond', x: 540, y: 200, w: 160, h: 110, c: 3, f: 'o', text: 'auth?', ln: 'solid', st: '' },
    { id: 3, t: 'circle', x: 880, y: 200, w: 110, h: 110, c: 4, f: 'o', text: 'db', ln: 'solid', st: '' },
    { id: 4, t: 'triangle', x: 540, y: 470, w: 140, h: 120, c: 1, f: 'i', text: 'err', ln: 'solid', st: '' },
    { id: 5, t: 'rect', x: 200, y: 470, w: 180, h: 80, c: 8, f: 'o', text: 'cache', ln: 'dash', st: '' },
    // client → auth and auth → client run side by side rather than on top of each other.
    { id: 6, t: 'line', a: { to: 1 }, b: { to: 2 }, head: 'end', pulse: 'fwd', glow: 'ball', c: 0, route: 'straight', ln: 'solid', text: 'login', f: 'i' },
    { id: 7, t: 'line', a: { to: 2 }, b: { to: 3 }, head: 'end', pulse: 'fwd', glow: 'light', c: 4, route: 'straight', ln: 'solid', text: '', f: 'o' },
    { id: 8, t: 'line', a: { to: 2 }, b: { to: 4 }, head: 'end', pulse: 'mid', glow: 'ball', c: 1, route: 'straight', ln: 'solid', text: '', f: 'o' },
    { id: 9, t: 'line', a: { to: 5 }, b: { to: 1 }, head: 'both', pulse: 'rev', glow: 'light', c: 8, route: 'straight', ln: 'solid', text: '', f: 'o' },
    { id: 10, t: 'line', a: { to: 4 }, b: { to: 5 }, head: 'none', pulse: 'off', glow: 'ball', c: 0, route: 'straight', ln: 'dot', text: '', f: 'o' },
    { id: 11, t: 'line', a: { x: 760, y: 420 }, b: { x: 1060, y: 420 }, head: 'end', pulse: 'fwd', glow: 'ball', c: 7, route: 'straight', ln: 'solid', text: '', f: 'o' },
    { id: 12, t: 'line', a: { to: 2 }, b: { to: 1 }, head: 'end', pulse: 'rev', glow: 'ball', c: 2, route: 'straight', ln: 'solid', text: 'token', f: 'i' },
    { id: 13, t: 'line', a: { to: 3 }, b: { x: 1000, y: 420 }, head: 'end', pulse: 'fwd', glow: 'ball', c: 5, route: 'elbow', ln: 'dash', text: '', f: 'o' },
    { id: 14, t: 'line', a: { to: 1 }, b: { to: 4 }, head: 'end', pulse: 'off', glow: 'ball', c: 6, route: 'curve', ln: 'solid', text: '', f: 'o' },
    { id: 15, t: 'line', a: { x: 760, y: 560 }, b: { x: 1060, y: 560 }, head: 'none', pulse: 'off', glow: 'ball', c: 0, route: 'straight', ln: 'solid', text: '', f: 'o' },
  ],
};

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
  mkv: 'video/x-matroska', heic: 'image/heic', c: 'text/plain; charset=utf-8',
  lin: 'text/x-lin; charset=utf-8',
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
add(y2026, 'IMG_0412.heic', 2_811_004);
add(y2026, 'tape.mkv', 1_204_003_118);
const trips = add(photos, 'trips');
for (let i = 1; i <= 14; i++) add(trips, `lisbon-${String(i).padStart(2, '0')}.jpg`, 1_500_000 + i * 91_337);
const music = add(null, 'Music');
add(music, 'side-a.mp3', 7_431_220);
add(music, 'side-b.flac', 31_902_117);
const docs = add(null, 'Documents');
add(docs, 'notes.md', undefined, '# notes\n\n- call the plumber\n- back up the photos\n- read about castle+ keep-alive\n');
add(docs, 'budget.csv', undefined, 'month,rent,food\njan,1200,410\nfeb,1200,388\n');
add(docs, 'lease.pdf', 402_118);
add(docs, 'finalMain.c', undefined, '#include <stdio.h>\n\nint main(void) {\n    puts("hello");\n    return 0;\n}\n');
add(docs, 'flow.lin', undefined, serializeLin(SAMPLE_DIAGRAM));
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

const offline = location.search.includes('offline');
let sync: SyncStatus = offline
  ? { state: 'offline', lastOkMs: Date.now() - 600_000, message: 'could not reach the store', readOnly: false, pending: 3 }
  : { state: 'idle', lastOkMs: Date.now(), message: null, readOnly: false, pending: 0 };
if (offline) {
  // An upload the store dropped part way, waiting for it to come back.
  const t: Transfer = { id: nextTransfer++, dir: 'up', name: 'holiday.mp4', done: 41_943_040, total: 143_000_000, state: 'waiting', error: null };
  transfers.set(t.id, t);
}
let connected = !location.search.includes('setup');
let device: string | null = connected ? 'laptop' : null;

function status(): Status {
  return { connected, url: connected ? 'http://127.0.0.1:8080' : null, device, suggestedName: 'Mock MacBook', sync };
}

/** A few seconds of a soft chord, as a WAV, for the mock player. */
function tone(): string {
  const rate = 8000;
  const secs = 12;
  const n = rate * secs;
  const buf = new DataView(new ArrayBuffer(44 + n));
  const str = (at: number, t: string) => [...t].forEach((c, i) => buf.setUint8(at + i, c.charCodeAt(0)));
  str(0, 'RIFF');
  buf.setUint32(4, 36 + n, true);
  str(8, 'WAVEfmt ');
  buf.setUint32(16, 16, true);
  buf.setUint16(20, 1, true);
  buf.setUint16(22, 1, true);
  buf.setUint32(24, rate, true);
  buf.setUint32(28, rate, true);
  buf.setUint16(32, 1, true);
  buf.setUint16(34, 8, true);
  str(36, 'data');
  buf.setUint32(40, n, true);
  for (let i = 0; i < n; i++) {
    const t = i / rate;
    const v = Math.sin(2 * Math.PI * 220 * t) + Math.sin(2 * Math.PI * 277 * t) + Math.sin(2 * Math.PI * 330 * t);
    buf.setUint8(44 + i, 128 + Math.round(v * 12 * Math.min(1, t, secs - t)));
  }
  return URL.createObjectURL(new Blob([buf], { type: 'audio/wav' }));
}

/** A one-page PDF that says what it is. */
function pdf(name: string): string {
  const text = `(drawBridge mock: ${name.replace(/[()\\]/g, '')}) Tj`;
  const stream = `BT /F1 18 Tf 72 720 Td ${text} ET`;
  const objs = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>',
  ];
  let out = '%PDF-1.4\n';
  const at: number[] = [];
  objs.forEach((o, i) => {
    at.push(out.length);
    out += `${i + 1} 0 obj\n${o}\nendobj\n`;
  });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n`;
  for (const a of at) out += `${String(a).padStart(10, '0')} 00000 n \n`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return URL.createObjectURL(new Blob([out], { type: 'application/pdf' }));
}

let song: string | null = null;
let cache: CacheInfo = { bytes: 734 * 2 ** 20, chunks: 92, cap: 2 * 2 ** 30, thumbs: 3_481_220 };

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
    async upload(paths, parent, onClash, trashOriginals) {
      const names = paths.map((p) => p.split('/').pop() ?? p);
      const taken = names.filter((name) => children(parent).some((n) => n.name === name));
      if (taken.length && onClash === 'ask') return { queued: 0, skipped: 0, clashes: taken };
      let queued = 0;
      let skipped = 0;
      // The real one trashes once the whole batch is up, and only if something
      // was queued; so does this, and keeps what was skipped.
      const keptBack: { name: string; why: string }[] = [];
      const sent: string[] = [];
      let left = 0;
      const settle = () => {
        if (trashOriginals && --left === 0) emit('originals', { trashed: sent, kept: keptBack });
      };
      for (let name of names) {
        const original = name;
        const existing = children(parent).find((n) => n.name === name);
        if (existing && onClash === 'skip') {
          skipped++;
          keptBack.push({ name: original, why: `${original} wasn't uploaded` });
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
          sent.push(original);
          settle();
        });
        queued++;
        left++;
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
      return { text: live(id).text ?? '', truncated: false };
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
      const was = sync.state;
      sync = { ...sync, state: 'syncing' };
      emit('sync-status', sync);
      await sleep(900);
      sync = offline && was === 'offline' ? { ...sync, state: 'offline' } : { ...sync, state: 'idle', lastOkMs: Date.now(), pending: 0 };
      emit('sync-status', sync);
    },

    async streamUrl(id) {
      await sleep(120);
      const n = live(id);
      const mime = n.mime ?? '';
      if (mime.startsWith('image/')) return thumb(n.blob ?? id);
      if (mime.startsWith('audio/')) return (song ??= tone());
      if (mime === 'application/pdf') return pdf(n.name);
      // Nothing a <video> can play.
      return `data:${mime},`;
    },
    async openExternal() {
      await sleep(200);
      return 'QuickTime Player';
    },
    async cacheInfo() {
      return cache;
    },
    async cacheSetCap(bytes) {
      cache = { ...cache, cap: bytes, bytes: Math.min(cache.bytes, Math.floor(bytes * 0.9)) };
      return cache;
    },
    async cacheClear() {
      cache = { ...cache, bytes: 0, chunks: 0, thumbs: 0 };
      return cache;
    },
    async thumbSave() {
      return true;
    },
    async exportFile(path, bytes) {
      // No disk to write to: the browser downloads it. The last export is also
      // kept on `window`, so a test can look at its bytes.
      (window as unknown as { __lastExport?: { path: string; jpeg: Uint8Array } }).__lastExport = { path, jpeg: bytes };
      const type = /\.png$/i.test(path) ? 'image/png' : /\.svg$/i.test(path) ? 'image/svg+xml' : 'image/jpeg';
      const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
      const a = document.createElement('a');
      a.href = url;
      a.download = path.split('/').pop() ?? 'export.jpg';
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
    },

    thumbUrl: (blob) => thumb(blob),
    async toggleFullscreen() {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await document.documentElement.requestFullscreen();
      return !!document.fullscreenElement;
    },
    async setFullscreen(on) {
      if (!on && document.fullscreenElement) await document.exitFullscreen();
      if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen();
    },
    async pickFiles() {
      return ['/Users/you/Desktop/new-photo.jpg', '/Users/you/Desktop/beach.jpg'];
    },
    async pickSavePath(name) {
      return `/Users/you/Downloads/${name}`;
    },
    onIndexChanged: (cb) => on('index-changed', cb),
    onSyncStatus: (cb) => on('sync-status', cb),
    onTransfer: (cb) => on('transfer', cb),
    onOriginals: (cb) => on('originals', cb),
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
