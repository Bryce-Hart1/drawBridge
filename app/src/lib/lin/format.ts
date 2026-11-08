// Claude — Date 11/05/2026
// Reading and writing .lin files. A .lin is JSON text, one part per line so a
// diff shows what changed:
//
//   {"lin":1,"w":1200,"h":800,"next":3,"items":[
//    {"id":1,"t":"rect","x":300,"y":200,"w":160,"h":80,"c":6,"f":"o","text":"api"},
//    {"id":2,"t":"line","a":{"to":1},"b":{"x":700,"y":200},"head":"end","pulse":"fwd","glow":"ball","c":0}
//   ]}
//
// A shape may also have `"ln":"dash"|"dot"`, and `"st"`, one digit 0-7 per letter of its
// text (1 bold, 2 italic, 4 underline, added together); a line may have `"route":"curve"|"elbow"`,
// `"ln"`, and a label `"text"` in font `"f"`. They're left out when they're the
// default, so a file that doesn't use them reads and writes as it always did.
//
// Parsing is strict about structure (a part it can't make sense of is an
// error, never silently dropped, since the editor would then save the loss)
// and forgiving about numbers, which are clamped into range.

import {
  FONT_KEYS,
  GLOWS,
  HEADS,
  MAX_LABEL,
  MAX_SIZE,
  MAX_TEXT,
  MIN_SIZE,
  PULSES,
  ROUTES,
  SHAPE_KINDS,
  STROKES,
  isLine,
  type Doc,
  type End,
  type FontKey,
  type Glow,
  type Head,
  type Line,
  type Part,
  type Pulse,
  type Route,
  type Shape,
  type ShapeKind,
  type Stroke,
} from './model';

const PAGE_MIN = 200;
const PAGE_MAX = 4000;

type Raw = Record<string, unknown>;

const isObject = (v: unknown): v is Raw => typeof v === 'object' && v !== null && !Array.isArray(v);

function num(v: unknown, what: string): number {
  if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`${what} should be a number`);
  return v;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function oneOf<T extends string>(v: unknown, allowed: readonly T[], fallback: T, what: string): T {
  if (v === undefined) return fallback;
  if (typeof v === 'string' && (allowed as readonly string[]).includes(v)) return v as T;
  throw new Error(`${what} should be one of ${allowed.join(', ')}`);
}

/** Round to a tenth: drags make long fractions nobody wants in a file. */
const r1 = (v: number) => Math.round(v * 10) / 10;

export function parseLin(text: string): Doc {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    throw new Error('not a .lin diagram (the file is not JSON)');
  }
  if (!isObject(raw) || typeof raw.lin !== 'number') throw new Error('not a .lin diagram');
  if (raw.lin > 1) throw new Error(`this diagram was made by a newer drawBridge (format ${raw.lin})`);
  if (raw.lin !== 1) throw new Error('not a .lin diagram');
  if (!Array.isArray(raw.items)) throw new Error('not a .lin diagram (no items)');

  const w = clamp(raw.w === undefined ? 1200 : num(raw.w, 'w'), PAGE_MIN, PAGE_MAX);
  const h = clamp(raw.h === undefined ? 800 : num(raw.h, 'h'), PAGE_MIN, PAGE_MAX);

  const items: Part[] = [];
  const ids = new Set<number>();
  raw.items.forEach((entry, i) => {
    const where = `item ${i + 1}`;
    try {
      if (!isObject(entry)) throw new Error('should be an object');
      const id = num(entry.id, 'id');
      if (!Number.isInteger(id) || id < 1) throw new Error('id should be a whole number from 1');
      if (ids.has(id)) throw new Error(`id ${id} is used twice`);
      ids.add(id);
      items.push(entry.t === 'line' ? parseLine(entry, id) : parseShape(entry, id, w, h));
    } catch (e) {
      throw new Error(`${where}: ${(e as Error).message}`);
    }
  });

  // Lines can only end on shapes that exist.
  const shapeIds = new Set(items.filter((p) => !isLine(p)).map((p) => p.id));
  for (const p of items) {
    if (!isLine(p)) continue;
    for (const e of [p.a, p.b]) {
      if ('to' in e && !shapeIds.has(e.to)) throw new Error(`line ${p.id}: ends on shape ${e.to}, which isn't there`);
    }
  }

  const top = items.reduce((m, p) => Math.max(m, p.id), 0);
  const next = typeof raw.next === 'number' && Number.isInteger(raw.next) && raw.next > top ? raw.next : top + 1;
  return { lin: 1, w, h, next, items };
}

function parseShape(raw: Raw, id: number, pageW: number, pageH: number): Shape {
  if (raw.t === undefined) throw new Error('has no type (t)');
  const t = oneOf<ShapeKind>(raw.t, SHAPE_KINDS, 'rect', 't');
  const size = (v: unknown, what: string) => clamp(num(v, what), MIN_SIZE, MAX_SIZE);
  const w = size(raw.w, 'w');
  // A circle is round: the width wins.
  const h = t === 'circle' ? w : size(raw.h, 'h');
  const c = typeof raw.c === 'number' && Number.isInteger(raw.c) && raw.c >= 0 && raw.c <= 9 ? raw.c : 0;
  const text = typeof raw.text === 'string' ? [...raw.text].slice(0, MAX_TEXT).join('') : '';
  return {
    id,
    t,
    x: clamp(num(raw.x, 'x'), 0, pageW),
    y: clamp(num(raw.y, 'y'), 0, pageH),
    w,
    h,
    c,
    f: oneOf<FontKey>(raw.f, FONT_KEYS, 'o', 'f'),
    text,
    ln: oneOf<Stroke>(raw.ln, STROKES, 'solid', 'ln'),
    st: parseStyles(raw.st, text),
  };
}

/** Letter styles are kept only if there's one digit 0–7 per character of the text; anything else is plain. */
function parseStyles(v: unknown, text: string): string {
  if (typeof v !== 'string' || !/^[0-7]+$/.test(v) || v.length !== [...text].length) return '';
  return /^0+$/.test(v) ? '' : v;
}

function parseEnd(v: unknown, what: string): End {
  if (!isObject(v)) throw new Error(`${what} should be an object`);
  if ('to' in v) {
    const to = num(v.to, `${what}.to`);
    if (!Number.isInteger(to) || to < 1) throw new Error(`${what}.to should be a shape id`);
    return { to };
  }
  return { x: num(v.x, `${what}.x`), y: num(v.y, `${what}.y`) };
}

function parseLine(raw: Raw, id: number): Line {
  const c = typeof raw.c === 'number' && Number.isInteger(raw.c) && raw.c >= 0 && raw.c <= 9 ? raw.c : 0;
  return {
    id,
    t: 'line',
    a: parseEnd(raw.a, 'a'),
    b: parseEnd(raw.b, 'b'),
    head: oneOf<Head>(raw.head, HEADS, 'none', 'head'),
    pulse: oneOf<Pulse>(raw.pulse, PULSES, 'off', 'pulse'),
    glow: oneOf<Glow>(raw.glow, GLOWS, 'ball', 'glow'),
    c,
    route: oneOf<Route>(raw.route, ROUTES, 'straight', 'route'),
    ln: oneOf<Stroke>(raw.ln, STROKES, 'solid', 'ln'),
    text: typeof raw.text === 'string' ? [...raw.text.replace(/[\r\n]+/g, ' ')].slice(0, MAX_LABEL).join('') : '',
    f: oneOf<FontKey>(raw.f, FONT_KEYS, 'o', 'f'),
  };
}

function endJson(e: End): string {
  return 'to' in e ? `{"to":${e.to}}` : `{"x":${r1(e.x)},"y":${r1(e.y)}}`;
}

function partJson(p: Part): string {
  if (isLine(p)) {
    // What a plain line leaves out stays out, so older files read and write the same.
    const more =
      (p.route !== 'straight' ? `,"route":"${p.route}"` : '') +
      (p.ln !== 'solid' ? `,"ln":"${p.ln}"` : '') +
      (p.text !== '' ? `,"text":${JSON.stringify(p.text)}` : '') +
      (p.text !== '' || p.f !== 'o' ? `,"f":"${p.f}"` : '');
    return (
      `{"id":${p.id},"t":"line","a":${endJson(p.a)},"b":${endJson(p.b)},` +
      `"head":"${p.head}","pulse":"${p.pulse}","glow":"${p.glow}","c":${p.c}${more}}`
    );
  }
  return (
    `{"id":${p.id},"t":"${p.t}","x":${r1(p.x)},"y":${r1(p.y)},"w":${r1(p.w)},"h":${r1(p.h)},` +
    `"c":${p.c},"f":"${p.f}","text":${JSON.stringify(p.text)}${p.ln !== 'solid' ? `,"ln":"${p.ln}"` : ''}${p.st !== '' ? `,"st":"${p.st}"` : ''}}`
  );
}

export function serializeLin(doc: Doc): string {
  const head = `{"lin":1,"w":${doc.w},"h":${doc.h},"next":${doc.next},"items":[`;
  if (doc.items.length === 0) return `${head}]}\n`;
  return `${head}\n${doc.items.map((p) => ` ${partJson(p)}`).join(',\n')}\n]}\n`;
}
