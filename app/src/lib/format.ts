// Claude — Date 11/03/2026
// Text for the screen: sizes, times, types and progress bars, all measured in
// character cells.

import type { Item } from './types';

const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];

/** `1536` → `1.5 KiB`, the same as `dbx`. */
export function human(n: number): string {
  let v = n;
  let unit = 0;
  while (v >= 1024 && unit < UNITS.length - 1) {
    v /= 1024;
    unit++;
  }
  return unit === 0 ? `${n} B` : `${v.toFixed(1)} ${UNITS[unit]}`;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** `12:04`, local time. */
export function clock(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

/** `text/markdown; charset=utf-8` → `text/markdown`. */
export function baseMime(mime: string | null): string {
  return (mime ?? '').split(';')[0].trim();
}

export function typeLabel(item: Item): string {
  return item.kind === 'folder' ? 'folder' : baseMime(item.mime) || 'file';
}

export function sizeLabel(item: Item): string {
  return item.kind === 'folder' ? plural(item.count ?? 0, 'item') : human(item.size ?? 0);
}

/** Cut to `cells` characters, ending in `…` if anything was cut. */
export function fit(text: string, cells: number): string {
  const chars = [...text];
  if (chars.length <= cells) return text;
  if (cells <= 0) return '';
  return `${chars.slice(0, cells - 1).join('')}…`;
}

/** A progress bar `cells` wide: `██████░░░░`. */
export function bar(done: number, total: number, cells: number): string {
  const filled = total > 0 ? Math.min(cells, Math.floor((done / total) * cells)) : cells;
  return '█'.repeat(filled) + '░'.repeat(cells - filled);
}

export function percent(done: number, total: number): string {
  return `${total > 0 ? Math.floor((done / total) * 100) : 100}%`;
}

/** `beach.tar.gz` → `beach.tar` + `.gz`. A leading dot isn't an extension. */
export function splitName(name: string): [string, string] {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
}

/** Notes opened in the editor stay this small: the whole text is held in memory. */
export const NOTE_LIMIT = 1 << 20;

/** Opens in the note editor: text, and small enough to edit in place. */
export function isText(item: Item): boolean {
  const mime = baseMime(item.mime);
  const text = mime.startsWith('text/') || mime === 'application/json';
  return item.kind === 'file' && text && (item.size ?? 0) <= NOTE_LIMIT;
}

// What core/src/thumb.rs makes thumbnails of at upload.
const THUMBNAILED = ['image/jpeg', 'image/png', 'image/webp', 'image/gif'];

export function hasThumb(item: Item): boolean {
  return item.kind === 'file' && item.blob !== null && THUMBNAILED.includes(baseMime(item.mime));
}

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
