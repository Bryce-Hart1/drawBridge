// Claude — Date 11/05/2026
// What a .lin diagram is: a page holding shapes (circle, triangle, rectangle,
// diamond) and lines (a line, a single arrow or a double arrow) in z-order,
// back to front. Lines end either at a point or on a shape, and follow the
// shape if it moves. Everything here is plain data, so the parser, the editor
// commands and the drawing all work on the same thing.

export const PAGE_W = 1200;
export const PAGE_H = 800;

export type ShapeKind = 'circle' | 'triangle' | 'rect' | 'diamond';
export const SHAPE_KINDS: readonly ShapeKind[] = ['circle', 'triangle', 'rect', 'diamond'];

/** The three font sizes, keyed `i`, `o` and `p`: small, medium, large. */
export type FontKey = 'i' | 'o' | 'p';
export const FONT_KEYS: readonly FontKey[] = ['i', 'o', 'p'];
export const FONT_PX: Record<FontKey, number> = { i: 12, o: 18, p: 28 };

/** A monospace character is this many ems wide, and a line this many ems tall. */
export const ADVANCE = 0.6;
export const LEADING = 1.3;

/** Room kept between the text and the edge of the text area. */
export const PAD = 6;

export const MIN_SIZE = 24;
export const MAX_SIZE = 800;
/** One arrow-key press; with ⇧, the big step. */
export const STEP = 10;
export const BIG_STEP = 40;
/** Most characters in one shape, however much room it has. */
export const MAX_TEXT = 200;
/** Most characters in a label on a line (one row, however long the line). */
export const MAX_LABEL = 40;
/** The snap grid, in page pixels, and how far a copy lands from what it copies. */
export const GRID = 10;
export const COPY_OFFSET = 20;

/**
 * Colours `0`–`9`. `0` is plain: a shape in the page's own colour with an ink
 * outline (so white in a JPEG), or a line in ink. It follows the theme; the
 * `fill` here is what it is on a white page.
 */
export const PALETTE = [
  { name: 'plain', fill: '#ffffff' },
  { name: 'red', fill: '#e5484d' },
  { name: 'orange', fill: '#f76b15' },
  { name: 'yellow', fill: '#f7c948' },
  { name: 'green', fill: '#46a758' },
  { name: 'teal', fill: '#12a594' },
  { name: 'blue', fill: '#3e63dd' },
  { name: 'purple', fill: '#8e4ec6' },
  { name: 'pink', fill: '#d6409f' },
  { name: 'charcoal', fill: '#30343b' },
] as const;

/** How much of its colour a shape's background shows: a wash, not a block. */
export const TINT = 0.22;

/** `#rrggbb` as a see-through `rgba(...)`. */
export function tint(hex: string, alpha = TINT): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** How an outline is drawn: unbroken, dashed or dotted. */
export type Stroke = 'solid' | 'dash' | 'dot';
export const STROKES: readonly Stroke[] = ['solid', 'dash', 'dot'];

/** The path a line takes: straight, a gentle curve, or square corners. */
export type Route = 'straight' | 'curve' | 'elbow';
export const ROUTES: readonly Route[] = ['straight', 'curve', 'elbow'];

export type Head = 'none' | 'end' | 'both';
export const HEADS: readonly Head[] = ['none', 'end', 'both'];

/** How a line pulses: `fwd` runs from its start to its end, `rev` back, `mid` out from the middle. */
export type Pulse = 'off' | 'fwd' | 'rev' | 'mid';
export const PULSES: readonly Pulse[] = ['off', 'fwd', 'rev', 'mid'];

/** What travels along a pulsing line: a ball, or a light inside the line. */
export type Glow = 'ball' | 'light';
export const GLOWS: readonly Glow[] = ['ball', 'light'];

export interface Shape {
  id: number;
  t: ShapeKind;
  /** The centre. */
  x: number;
  y: number;
  w: number;
  h: number;
  /** Colour, 0–9. */
  c: number;
  f: FontKey;
  text: string;
  ln: Stroke;
  /**
   * How each letter of `text` is styled: one digit 0–7 per character (see
   * BOLD, ITALIC, UNDERLINE), or '' when none is. Never any other length.
   */
  st: string;
}

/** Letter styles, added together in a shape's `st`. */
export const BOLD = 1;
export const ITALIC = 2;
export const UNDERLINE = 4;
export const STYLE_NAMES: readonly [number, string][] = [
  [BOLD, 'bold'],
  [ITALIC, 'italic'],
  [UNDERLINE, 'underline'],
];

export type End = { x: number; y: number } | { to: number };

export interface Line {
  id: number;
  t: 'line';
  a: End;
  b: End;
  head: Head;
  pulse: Pulse;
  glow: Glow;
  c: number;
  route: Route;
  ln: Stroke;
  /** A label, one row, drawn over the middle of the line; in font `f`. */
  text: string;
  f: FontKey;
}

export type Part = Shape | Line;

export interface Doc {
  lin: 1;
  w: number;
  h: number;
  /** The id the next new part gets. */
  next: number;
  items: Part[];
}

export const isLine = (p: Part): p is Line => p.t === 'line';
export const isShape = (p: Part): p is Shape => p.t !== 'line';
export const isAttached = (e: End): e is { to: number } => 'to' in e;

export function blankDoc(): Doc {
  return { lin: 1, w: PAGE_W, h: PAGE_H, next: 1, items: [] };
}

const SIZES: Record<ShapeKind, [w: number, h: number]> = {
  circle: [100, 100],
  triangle: [120, 100],
  rect: [160, 80],
  diamond: [140, 100],
};

/** A new shape centred at (x, y), medium font, no text. Doesn't touch the doc. */
export function newShape(id: number, t: ShapeKind, x: number, y: number): Shape {
  const [w, h] = SIZES[t];
  return { id, t, x, y, w, h, c: 0, f: 'o', text: '', ln: 'solid', st: '' };
}

/** A new line from `a` to `b`: plain, straight, not pulsing. Arrowheads are added on (`a`). */
export function newLine(id: number, a: End, b: End): Line {
  return { id, t: 'line', a, b, head: 'none', pulse: 'off', glow: 'ball', c: 0, route: 'straight', ln: 'solid', text: '', f: 'o' };
}

export function find(doc: Doc, id: number | null): Part | undefined {
  return id === null ? undefined : doc.items.find((p) => p.id === id);
}

export function shapeById(doc: Doc, id: number): Shape | undefined {
  const p = find(doc, id);
  return p && isShape(p) ? p : undefined;
}
