// Claude — Date 11/05/2026
// The maths of a diagram, with no drawing in it: how much text a shape holds,
// where a line meets a shape's edge, what a click landed on, and how big the
// drawing is. Text is monospace with fixed proportions (model.ts), so how much
// fits is the same on every machine and can be tested without a canvas.

import {
  ADVANCE,
  FONT_PX,
  LEADING,
  MAX_TEXT,
  PAD,
  isAttached,
  isLine,
  isShape,
  type Doc,
  type End,
  type Line,
  type Part,
  type Shape,
} from './model';

export interface Pt {
  x: number;
  y: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

// ── Text ────────────────────────────────────────────────────────────────

/** The rectangle text is set in, centred at (0, dy) from the shape's centre. */
export function textBox(s: Shape): { w: number; h: number; dy: number } {
  let w: number;
  let h: number;
  let dy = 0;
  switch (s.t) {
    case 'rect':
      w = s.w;
      h = s.h;
      break;
    case 'circle':
      // The biggest rectangle inside an ellipse has its corners on the edge.
      w = s.w * Math.SQRT1_2;
      h = s.h * Math.SQRT1_2;
      break;
    case 'diamond':
      w = s.w / 2;
      h = s.h / 2;
      break;
    case 'triangle':
      // Half the width and half the height, sitting on the base.
      w = s.w / 2;
      h = s.h / 2;
      dy = s.h / 4;
      break;
  }
  return { w: Math.max(0, w - 2 * PAD), h: Math.max(0, h - 2 * PAD), dy };
}

export function lineHeight(s: Shape): number {
  return FONT_PX[s.f] * LEADING;
}

/** Characters per line and lines that fit in the shape at its font size. */
export function grid(s: Shape): { cols: number; rows: number } {
  const box = textBox(s);
  return {
    cols: Math.floor(box.w / (FONT_PX[s.f] * ADVANCE)),
    rows: Math.floor(box.h / lineHeight(s)),
  };
}

/** The most characters the shape could hold (fewer if words wrap badly). */
export function capacity(s: Shape): number {
  const { cols, rows } = grid(s);
  return Math.max(0, Math.min(MAX_TEXT, cols * rows));
}

/**
 * Break `text` into lines of at most `cols` characters, at spaces where it
 * can and in the middle of a word where a word is longer than a line.
 * Runs of spaces count as one.
 */
export function wrap(text: string, cols: number): string[] {
  if (cols <= 0) return text.trim() === '' ? [] : [text];
  const lines: string[] = [];
  let line: string[] = [];
  for (const word of text.split(/ +/).filter((w) => w !== '')) {
    let chars = [...word];
    if (chars.length > cols) {
      if (line.length > 0) lines.push(line.join(''));
      while (chars.length > cols) {
        lines.push(chars.slice(0, cols).join(''));
        chars = chars.slice(cols);
      }
      line = chars;
    } else if (line.length === 0) {
      line = chars;
    } else if (line.length + 1 + chars.length <= cols) {
      line = [...line, ' ', ...chars];
    } else {
      lines.push(line.join(''));
      line = chars;
    }
  }
  if (line.length > 0) lines.push(line.join(''));
  return lines;
}

/** The lines the shape's text is drawn in. */
export function layout(s: Shape): string[] {
  return wrap(s.text, grid(s).cols);
}

/** Whether the shape's text fits in the shape, at its size and font. */
export function fits(s: Shape): boolean {
  if ([...s.text].length > MAX_TEXT) return false;
  if (s.text.trim() === '') return true;
  const { cols, rows } = grid(s);
  return cols > 0 && layout(s).length <= rows;
}

// ── Outlines ────────────────────────────────────────────────────────────

/** The corners of a polygon shape, relative to its centre; `null` for a circle. */
function corners(s: Shape): Pt[] | null {
  const hw = s.w / 2;
  const hh = s.h / 2;
  switch (s.t) {
    case 'rect':
      return [{ x: -hw, y: -hh }, { x: hw, y: -hh }, { x: hw, y: hh }, { x: -hw, y: hh }];
    case 'diamond':
      return [{ x: 0, y: -hh }, { x: hw, y: 0 }, { x: 0, y: hh }, { x: -hw, y: 0 }];
    case 'triangle':
      return [{ x: 0, y: -hh }, { x: hw, y: hh }, { x: -hw, y: hh }];
    case 'circle':
      return null;
  }
}

const cross = (a: Pt, b: Pt) => a.x * b.y - a.y * b.x;

/** Where a ray from the origin, in direction (dx, dy), leaves a convex polygon: how far along it, in units of (dx, dy). */
function rayExit(poly: Pt[], dx: number, dy: number): number | null {
  const d = { x: dx, y: dy };
  let best: number | null = null;
  for (let i = 0; i < poly.length; i++) {
    const p = poly[i];
    const q = poly[(i + 1) % poly.length];
    const e = { x: q.x - p.x, y: q.y - p.y };
    const denom = cross(d, e);
    if (Math.abs(denom) < 1e-12) continue;
    const t = cross(p, e) / denom;
    const u = cross(p, d) / denom;
    if (t > 1e-9 && u >= -1e-9 && u <= 1 + 1e-9 && (best === null || t < best)) best = t;
  }
  return best;
}

/** Where the line from the shape's centre toward `toward` crosses the shape's outline. */
export function outlinePoint(s: Shape, toward: Pt): Pt {
  const dx = toward.x - s.x;
  const dy = toward.y - s.y;
  if (dx === 0 && dy === 0) return { x: s.x, y: s.y };
  const poly = corners(s);
  let t: number | null;
  if (poly) {
    t = rayExit(poly, dx, dy);
  } else {
    const k = Math.hypot(dx / (s.w / 2), dy / (s.h / 2));
    t = k === 0 ? null : 1 / k;
  }
  return t === null ? { x: s.x, y: s.y } : { x: s.x + dx * t, y: s.y + dy * t };
}

/** Whether `p` is inside the shape, grown by `margin` on every side. */
export function inShape(s: Shape, p: Pt, margin = 0): boolean {
  const dx = p.x - s.x;
  const dy = p.y - s.y;
  const hw = s.w / 2 + margin;
  const hh = s.h / 2 + margin;
  switch (s.t) {
    case 'rect':
      return Math.abs(dx) <= hw && Math.abs(dy) <= hh;
    case 'circle':
      return (dx / hw) ** 2 + (dy / hh) ** 2 <= 1;
    case 'diamond':
      return Math.abs(dx) / hw + Math.abs(dy) / hh <= 1;
    case 'triangle': {
      if (dy < -hh || dy > hh) return false;
      // The width grows from nothing at the top to the full width at the base.
      return Math.abs(dx) <= hw * ((dy + hh) / (2 * hh));
    }
  }
}

// ── Lines ───────────────────────────────────────────────────────────────

function shapeOf(doc: Doc, id: number): Shape | undefined {
  const p = doc.items.find((q) => q.id === id);
  return p && isShape(p) ? p : undefined;
}

/** Where an end points *at*: its own point, or the centre of the shape it's on. */
function aim(doc: Doc, e: End): Pt {
  if (!isAttached(e)) return e;
  const s = shapeOf(doc, e.to);
  return s ? { x: s.x, y: s.y } : { x: 0, y: 0 };
}

/** The two points a line is drawn between: attached ends stop at the shape's edge. */
export function lineEnds(doc: Doc, line: Line): { a: Pt; b: Pt } {
  const place = (e: End, other: End): Pt => {
    if (!isAttached(e)) return { x: e.x, y: e.y };
    const s = shapeOf(doc, e.to);
    return s ? outlinePoint(s, aim(doc, other)) : { x: 0, y: 0 };
  };
  return { a: place(line.a, line.b), b: place(line.b, line.a) };
}

export function lineLength(doc: Doc, line: Line): number {
  const { a, b } = lineEnds(doc, line);
  return Math.hypot(b.x - a.x, b.y - a.y);
}

function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

// ── Hit testing ─────────────────────────────────────────────────────────

/** How close, in page pixels, a click has to be to a line or its end handle. */
export const LINE_SLOP = 7;
export const HANDLE_RADIUS = 10;

export type Hit = { kind: 'part'; id: number } | { kind: 'end'; id: number; which: 'a' | 'b' };

/**
 * What a click at `p` landed on: the selected line's end handles first, then
 * the front-most part. A shape counts anywhere inside it; a line within a few
 * pixels of it.
 */
export function hitTest(doc: Doc, p: Pt, selected: number | null): Hit | null {
  const sel = doc.items.find((q) => q.id === selected);
  if (sel && isLine(sel)) {
    const { a, b } = lineEnds(doc, sel);
    for (const which of ['b', 'a'] as const) {
      const at = which === 'a' ? a : b;
      if (Math.hypot(p.x - at.x, p.y - at.y) <= HANDLE_RADIUS) return { kind: 'end', id: sel.id, which };
    }
  }
  for (let i = doc.items.length - 1; i >= 0; i--) {
    const part = doc.items[i];
    if (isLine(part)) {
      const { a, b } = lineEnds(doc, part);
      if (distToSegment(p, a, b) <= LINE_SLOP) return { kind: 'part', id: part.id };
    } else if (inShape(part, p)) {
      return { kind: 'part', id: part.id };
    }
  }
  return null;
}

/** The front-most shape within a few pixels of `p`: where a dropped line end attaches. */
export function snapTarget(doc: Doc, p: Pt, margin = 8): Shape | null {
  for (let i = doc.items.length - 1; i >= 0; i--) {
    const part = doc.items[i];
    if (isShape(part) && inShape(part, p, margin)) return part;
  }
  return null;
}

// ── Extent ──────────────────────────────────────────────────────────────

/** The smallest box around everything drawn; `null` if there's nothing. */
export function bounds(doc: Doc): Box | null {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  const grow = (x: number, y: number) => {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  };
  for (const part of doc.items as Part[]) {
    if (isLine(part)) {
      const { a, b } = lineEnds(doc, part);
      grow(a.x, a.y);
      grow(b.x, b.y);
    } else {
      grow(part.x - part.w / 2, part.y - part.h / 2);
      grow(part.x + part.w / 2, part.y + part.h / 2);
    }
  }
  return x0 === Infinity ? null : { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}
