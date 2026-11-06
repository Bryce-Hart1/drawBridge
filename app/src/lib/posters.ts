// Claude — Date 11/04/2026
// Thumbnails the app can't make at upload, drawn here instead: a .lin diagram
// (drawn from its own file), a frame from a video, or a HEIC, AVIF, TIFF or BMP photo, which WebKit decodes and the
// image crate doesn't. The file is read from the stream server into an
// element nobody sees, drawn onto a canvas at most 256 px across, and handed
// to Rust as a JPEG, which keeps it and puts it in the store for every other
// device.
//
// Only for the selected file, one at a time, and once per file per run: a
// folder of videos never streams them all.

import { api } from './api';
import { isLin } from './format';
import { thumbnail } from './lin/draw';
import { parseLin } from './lin/format';
import type { Item } from './types';

const SIDE = 256;
const TIMEOUT_MS = 20_000;

const tried = new Set<string>();
let running = false;
let next: { item: Item; done: (ok: boolean) => void } | null = null;

/** Make `item`'s thumbnail. Resolves `true` once it's saved; `false` if it
 *  was tried before, couldn't be drawn, or a newer request took its turn. */
export function makePoster(item: Item): Promise<boolean> {
  if (!item.blob || tried.has(item.blob)) return Promise.resolve(false);
  return new Promise((done) => {
    // Only the newest waits: the selection has moved past the rest.
    next?.done(false);
    next = { item, done };
    if (!running) void pump();
  });
}

async function pump() {
  running = true;
  while (next) {
    const { item, done } = next;
    next = null;
    tried.add(item.blob!);
    done(await draw(item).catch(() => false));
  }
  running = false;
}

/** A .lin diagram's thumbnail: the diagram itself, drawn small. An empty page has none. */
async function drawDiagram(item: Item): Promise<boolean> {
  const got = await api.noteRead(item.id);
  if (got.truncated) return false;
  await api.thumbSave(item.blob!, await thumbnail(parseLin(got.text), SIDE));
  return true;
}

async function draw(item: Item): Promise<boolean> {
  if (isLin(item)) return drawDiagram(item);
  const url = await api.streamUrl(item.id);
  const video = (item.mime ?? '').startsWith('video/');
  const source = video ? await frame(url) : await picture(url);
  try {
    const w = source instanceof HTMLVideoElement ? source.videoWidth : source.naturalWidth;
    const h = source instanceof HTMLVideoElement ? source.videoHeight : source.naturalHeight;
    if (!w || !h) return false;
    const scale = Math.min(1, SIDE / Math.max(w, h));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(w * scale));
    canvas.height = Math.max(1, Math.round(h * scale));
    const ctx = canvas.getContext('2d');
    if (!ctx) return false;
    // Transparent PNG-likes over white, as core's thumb.rs does.
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(source, 0, 0, canvas.width, canvas.height);
    const jpeg = await new Promise<Blob | null>((r) => canvas.toBlob(r, 'image/jpeg', 0.8));
    if (!jpeg) return false;
    await api.thumbSave(item.blob!, new Uint8Array(await jpeg.arrayBuffer()));
    return true;
  } finally {
    if (source instanceof HTMLVideoElement) {
      // Stop it fetching.
      source.removeAttribute('src');
      source.load();
    }
  }
}

/** A frame a second in (or a tenth of the way, for a short clip). */
function frame(url: string): Promise<HTMLVideoElement> {
  const v = document.createElement('video');
  // CORS, so the canvas isn't tainted: the stream server allows this page.
  v.crossOrigin = 'anonymous';
  v.muted = true;
  v.playsInline = true;
  v.preload = 'metadata';
  return within(
    new Promise((ok, no) => {
      v.onloadedmetadata = () => {
        v.currentTime = Math.min(1, (v.duration || 0) * 0.1);
      };
      v.onseeked = () => ok(v);
      v.onerror = () => no(new Error('video'));
      v.src = url;
    }),
  );
}

function picture(url: string): Promise<HTMLImageElement> {
  const img = new Image();
  img.crossOrigin = 'anonymous';
  img.decoding = 'async';
  img.src = url;
  return within(img.decode().then(() => img));
}

function within<T>(p: Promise<T>): Promise<T> {
  return Promise.race([p, new Promise<T>((_, no) => setTimeout(() => no(new Error('timeout')), TIMEOUT_MS))]);
}
