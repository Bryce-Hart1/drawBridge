// Claude — Date 11/05/2026
// The maths of a diagram, with no drawing in it: how much text a shape holds,
// where a line meets a shape's edge, what a click landed on, and how big the
// drawing is. Text is monospace with fixed proportions (model.ts), so how much
// fits is the same on every machine and can be tested without a canvas.

import {
  ADVANCE,
  FONT_PX,
  LEADING,
  MAX_LABEL,
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
 * Runs of spaces count as one. A newline always starts a new line (so a list
 * is one item to a line), and an empty line between two is kept.
 */
export function wrap(text: string, cols: number): string[] {
  const cs = [...text];
  return wrapIndexed(text, cols).map((row) => row.map((i) => cs[i]).join(''));
}

/** One paragraph of text (the characters from `start` up to `end`) and the rows it breaks into. */
interface Paragraph {
  start: number;
  end: number;
  rows: number[][];
}

const range = (from: number, to: number): number[] => Array.from({ length: Math.max(0, to - from) }, (_, k) => from + k);

function paragraphs(cs: string[], cols: number): Paragraph[] {
  const lists = cs.includes('\n');
  const out: Paragraph[] = [];
  let start = 0;
  for (let i = 0; i <= cs.length; i++) {
    if (i < cs.length && cs[i] !== '\n') continue;
    const blank = cs.slice(start, i).join('').trim() === '';
    // With breaks in the text an empty paragraph is an empty line; on its own it's nothing.
    out.push({ start, end: i, rows: blank ? (lists ? [[]] : []) : paragraphRows(cs, start, i, cols) });
    start = i + 1;
  }
  return out;
}

/** The rows `wrap` makes, as the index of each character shown (a space where words were run together is the one just before the next word). */
export function wrapIndexed(text: string, cols: number): number[][] {
  const cs = [...text];
  if (cs.length === 0) return [];
  return paragraphs(cs, cols).flatMap((p) => p.rows);
}

function paragraphRows(cs: string[], from: number, to: number, cols: number): number[][] {
  if (cols <= 0) return [range(from, to)];
  const rows: number[][] = [];
  let line: number[] = [];
  let i = from;
  while (i < to) {
    if (cs[i] === ' ') {
      i++;
      continue;
    }
    const s = i;
    while (i < to && cs[i] !== ' ') i++;
    let word = range(s, i);
    if (word.length > cols) {
      if (line.length > 0) rows.push(line);
      while (word.length > cols) {
        rows.push(word.slice(0, cols));
        word = word.slice(cols);
      }
      line = word;
    } else if (line.length === 0) {
      line = word;
    } else if (line.length + 1 + word.length <= cols) {
      line = [...line, s - 1, ...word];
    } else {
      rows.push(line);
      line = word;
    }
  }
  if (line.length > 0) rows.push(line);
  return rows;
}

/**
 * Which row and column a caret is at, as `at` characters in. A caret on a space
 * that was folded away (at the end of a row) is at the start of the next one.
 */
export function caretCell(text: string, cols: number, at: number): { row: number; col: number } {
  const cs = [...text];
  at = Math.min(Math.max(at, 0), cs.length);
  let base = 0;
  for (const p of paragraphs(cs, cols)) {
    if (at <= p.end) {
      for (let r = 0; r < p.rows.length; r++) {
        const c = p.rows[r].findIndex((i) => i >= at);
        if (c >= 0) return { row: base + r, col: c };
      }
      const last = p.rows.length - 1;
      return last < 0 ? { row: base, col: 0 } : { row: base + last, col: p.rows[last].length };
    }
    base += p.rows.length;
  }
  return { row: base, col: 0 };
}

/** The lines the shape's text is drawn in. */
export function layout(s: Shape): string[] {
  return wrap(s.text, grid(s).cols);
}

/** The same lines, as which characters of the text are in each. */
export function layoutIndexed(s: Shape): number[][] {
  return wrapIndexed(s.text, grid(s).cols);
}

/** Whether the shape's text fits in the shape, at its size and font. */
export function fits(s: Shape): boolean {
  if ([...s.text].length > MAX_TEXT) return false;
  if (s.text.trim() === '' && !s.text.includes('\n')) return true;
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

/**
 * Where a ray from `q` (a point inside the shape, relative to its centre)
 * heading (dx, dy) leaves the outline. `outlinePoint` is this with `q` at the centre.
 */
function exitFrom(s: Shape, q: Pt, dx: number, dy: number): Pt | null {
  const poly = corners(s);
  let t: number | null;
  if (poly) {
    t = rayExit(poly.map((p) => ({ x: p.x - q.x, y: p.y - q.y })), dx, dy);
  } else {
    const hw = s.w / 2;
    const hh = s.h / 2;
    const a = (dx / hw) ** 2 + (dy / hh) ** 2;
    const b = 2 * ((q.x * dx) / (hw * hw) + (q.y * dy) / (hh * hh));
    const c = (q.x / hw) ** 2 + (q.y / hh) ** 2 - 1;
    const disc = b * b - 4 * a * c;
    t = a === 0 || disc < 0 ? null : (-b + Math.sqrt(disc)) / (2 * a);
  }
  return t === null || t < 0 ? null : { x: s.x + q.x + dx * t, y: s.y + q.y + dy * t };
}

const unit = (v: Pt): Pt => {
  const n = Math.hypot(v.x, v.y);
  return n === 0 ? { x: 0, y: 0 } : { x: v.x / n, y: v.y / n };
};

/** How far apart lines joining the same two shapes are drawn, at least when there's room. */
export const FAN = 24;

/** The patch of page a label sits on is this many ems tall (draw.ts uses the same). */
export const LABEL_EM = 1.5;

/** The two shapes a line joins, lowest id first, if both its ends are on shapes. */
function pairOf(l: Line): [number, number] | null {
  return isAttached(l.a) && isAttached(l.b) ? [Math.min(l.a.to, l.b.to), Math.max(l.a.to, l.b.to)] : null;
}

/**
 * Lines that join the same two shapes (either way round) don't pile up on one
 * another: they run side by side. Where `l` sits among them, as a distance
 * from the middle in the direction of `fanNormal`; 0 for a line on its own.
 */
function fanOffset(doc: Doc, l: Line): number {
  const key = pairOf(l);
  if (!key) return 0;
  const mates = doc.items.filter((p): p is Line => {
    if (!isLine(p)) return false;
    const k = pairOf(p);
    return k !== null && k[0] === key[0] && k[1] === key[1];
  });
  if (mates.length < 2) return 0;
  const lo = shapeOf(doc, key[0]);
  const hi = shapeOf(doc, key[1]);
  if (!lo || !hi) return 0;
  // Far enough apart that the tallest label among them has a row to itself, so each can be read.
  const label = Math.max(...mates.map((m) => (m.text === '' ? 0 : FONT_PX[m.f] * LABEL_EM)));
  const want = Math.max(FAN, label + 6);
  // Never fan wider than the smaller shape, or the ends would miss it.
  const room = 0.85 * Math.min(lo.w, lo.h, hi.w, hi.h);
  const step = Math.min(want, room / (mates.length - 1));
  return (mates.findIndex((m) => m.id === l.id) - (mates.length - 1) / 2) * step;
}

interface Resolved {
  a: Pt;
  b: Pt;
  /** For an elbow: the first run goes sideways rather than up and down. */
  horizontal: boolean;
  /** The fan offset, and the direction it's measured in. */
  fan: number;
  normal: Pt;
}

function resolve(doc: Doc, l: Line): Resolved {
  const aimA = aim(doc, l.a);
  const aimB = aim(doc, l.b);
  const dx = aimB.x - aimA.x;
  const dy = aimB.y - aimA.y;
  const horizontal = Math.abs(dx) >= Math.abs(dy);
  const elbow = l.route === 'elbow';
  const fan = fanOffset(doc, l);

  // The direction the fan is measured across: perpendicular to the way from the
  // lower-numbered shape to the higher, whichever way round this line is drawn.
  let along = unit({ x: dx, y: dy });
  const key = pairOf(l);
  if (key && key[0] !== (l.a as { to: number }).to) along = { x: -along.x, y: -along.y };
  if (elbow) along = horizontal ? { x: Math.sign(along.x || 1), y: 0 } : { x: 0, y: Math.sign(along.y || 1) };
  const normal = { x: -along.y, y: along.x };
  const ov = { x: normal.x * fan, y: normal.y * fan };

  const place = (e: End, other: Pt, own: Pt, sign: 1 | -1): Pt => {
    if (!isAttached(e)) return { x: e.x, y: e.y };
    const s = shapeOf(doc, e.to);
    if (!s) return { x: 0, y: 0 };
    const out = elbow ? (horizontal ? { x: Math.sign(dx || 1) * sign, y: 0 } : { x: 0, y: Math.sign(dy || 1) * sign }) : unit({ x: other.x - own.x, y: other.y - own.y });
    if ((ov.x !== 0 || ov.y !== 0) && inShape(s, { x: s.x + ov.x, y: s.y + ov.y })) {
      const p = exitFrom(s, ov, out.x, out.y);
      if (p) return p;
    }
    return elbow ? (exitFrom(s, { x: 0, y: 0 }, out.x, out.y) ?? { x: s.x, y: s.y }) : outlinePoint(s, other);
  };
  return { a: place(l.a, aimB, aimA, 1), b: place(l.b, aimA, aimB, -1), horizontal, fan, normal };
}

/** How far a curve bows out at its middle, as a fraction of its length. */
const BOW = 0.2;
const BOW_MAX = 70;
const SAMPLES = 24;

/**
 * The points a line is drawn through, from its start (a) to its end (b): two
 * for a straight line, a few for an elbow, and a fine run of them for a curve.
 * Attached ends stop at the shape's edge.
 */
export function linePath(doc: Doc, l: Line): Pt[] {
  const r = resolve(doc, l);
  const { a, b } = r;
  if (l.route === 'elbow') {
    const bend = r.fan;
    const pts: Pt[] = r.horizontal
      ? [a, { x: (a.x + b.x) / 2 + bend, y: a.y }, { x: (a.x + b.x) / 2 + bend, y: b.y }, b]
      : [a, { x: a.x, y: (a.y + b.y) / 2 + bend }, { x: b.x, y: (a.y + b.y) / 2 + bend }, b];
    // Drop the corners of a run that doesn't turn.
    return pts.filter((p, i) => i === 0 || Math.hypot(p.x - pts[i - 1].x, p.y - pts[i - 1].y) > 0.5);
  }
  if (l.route === 'curve') {
    const len = Math.hypot(b.x - a.x, b.y - a.y);
    if (len < 1) return [a, b];
    // Parallel lines bow apart, each to its own side, so they never cross.
    const d = unit({ x: b.x - a.x, y: b.y - a.y });
    const bow =
      r.fan !== 0
        ? { x: r.normal.x * r.fan * 4, y: r.normal.y * r.fan * 4 }
        : { x: -d.y * Math.min(BOW * len, BOW_MAX), y: d.x * Math.min(BOW * len, BOW_MAX) };
    // A quadratic curve's middle is halfway to its control point.
    const c = { x: (a.x + b.x) / 2 + bow.x * 2, y: (a.y + b.y) / 2 + bow.y * 2 };
    const pts: Pt[] = [];
    for (let i = 0; i <= SAMPLES; i++) {
      const u = i / SAMPLES;
      const k = 1 - u;
      pts.push({ x: k * k * a.x + 2 * k * u * c.x + u * u * b.x, y: k * k * a.y + 2 * k * u * c.y + u * u * b.y });
    }
    return pts;
  }
  return [a, b];
}

/** The two points a line starts and ends at. */
export function lineEnds(doc: Doc, line: Line): { a: Pt; b: Pt } {
  const path = linePath(doc, line);
  return { a: path[0], b: path[path.length - 1] };
}

export function pathLength(path: readonly Pt[]): number {
  let n = 0;
  for (let i = 1; i < path.length; i++) n += Math.hypot(path[i].x - path[i - 1].x, path[i].y - path[i - 1].y);
  return n;
}

export function lineLength(doc: Doc, line: Line): number {
  return pathLength(linePath(doc, line));
}

/**
 * The point `d` pixels along the path. Before the start or past the end it
 * carries straight on in the direction the path was going, so a light can
 * slide in from off the line.
 */
export function pointAt(path: readonly Pt[], d: number): Pt {
  if (path.length === 0) return { x: 0, y: 0 };
  if (path.length === 1) return path[0];
  const first = path[0];
  const last = path[path.length - 1];
  if (d <= 0) {
    const u = unit({ x: path[1].x - first.x, y: path[1].y - first.y });
    return { x: first.x + u.x * d, y: first.y + u.y * d };
  }
  let left = d;
  for (let i = 1; i < path.length; i++) {
    const seg = Math.hypot(path[i].x - path[i - 1].x, path[i].y - path[i - 1].y);
    if (left <= seg && seg > 0) {
      const u = left / seg;
      return { x: path[i - 1].x + (path[i].x - path[i - 1].x) * u, y: path[i - 1].y + (path[i].y - path[i - 1].y) * u };
    }
    left -= seg;
  }
  const prev = path[path.length - 2];
  const u = unit({ x: last.x - prev.x, y: last.y - prev.y });
  return { x: last.x + u.x * left, y: last.y + u.y * left };
}

/** The stretch of the path from `d0` to `d1` pixels along, as points. */
export function subPath(path: readonly Pt[], d0: number, d1: number): Pt[] {
  const out: Pt[] = [pointAt(path, d0)];
  let at = 0;
  for (let i = 1; i < path.length; i++) {
    at += Math.hypot(path[i].x - path[i - 1].x, path[i].y - path[i - 1].y);
    if (at > d0 && at < d1) out.push(path[i]);
  }
  out.push(pointAt(path, d1));
  return out;
}

/** Which way the path is heading where it ends (`end` true) or starts, as the point just before the tip. */
export function tipFrom(path: readonly Pt[], end: boolean): Pt {
  if (path.length < 2) return path[0] ?? { x: 0, y: 0 };
  const tip = end ? path[path.length - 1] : path[0];
  // Skip back to a point that isn't on top of the tip (a curve's last step is tiny).
  for (let i = 1; i < path.length; i++) {
    const p = end ? path[path.length - 1 - i] : path[i];
    if (Math.hypot(p.x - tip.x, p.y - tip.y) >= 3) return p;
  }
  return end ? path[0] : path[path.length - 1];
}

/** The middle of the path, by length: where a label goes. */
export function midpoint(path: readonly Pt[]): Pt {
  return pointAt(path, pathLength(path) / 2);
}

function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

function distToPath(p: Pt, path: readonly Pt[]): number {
  let best = Infinity;
  for (let i = 1; i < path.length; i++) best = Math.min(best, distToSegment(p, path[i - 1], path[i]));
  return best;
}

// ── Labels on lines ─────────────────────────────────────────────────────

/** How wide a label is, in pixels. */
export function labelWidth(l: Line): number {
  return [...l.text].length * FONT_PX[l.f] * ADVANCE;
}

/** Whether the label fits its line: one row, within the length of the line (and a fixed most). */
export function labelFits(doc: Doc, l: Line): boolean {
  const n = [...l.text].length;
  if (n > MAX_LABEL) return false;
  if (n === 0) return true;
  return labelWidth(l) + 2 * PAD <= lineLength(doc, l);
}

/** The most characters a label on this line holds at its font size. */
export function labelCapacity(doc: Doc, l: Line): number {
  const room = Math.floor((lineLength(doc, l) - 2 * PAD) / (FONT_PX[l.f] * ADVANCE));
  return Math.max(0, Math.min(MAX_LABEL, room));
}

// ── Hit testing ─────────────────────────────────────────────────────────

/** How close, in page pixels, a click has to be to a line or its end handle. */
export const LINE_SLOP = 7;
export const HANDLE_RADIUS = 10;

export type Hit = { kind: 'part'; id: number } | { kind: 'end'; id: number; which: 'a' | 'b' };

/**
 * What a click at `p` landed on: the selected line's end handles first, then
 * the front-most part. A shape counts anywhere inside it; a line within a few
 * pixels of it. `slop` is the click's reach in page pixels (a zoomed-out page
 * needs more of it).
 */
export function hitTest(doc: Doc, p: Pt, selected: number | null, slop = 1): Hit | null {
  const sel = doc.items.find((q) => q.id === selected);
  if (sel && isLine(sel)) {
    const { a, b } = lineEnds(doc, sel);
    for (const which of ['b', 'a'] as const) {
      const at = which === 'a' ? a : b;
      if (Math.hypot(p.x - at.x, p.y - at.y) <= HANDLE_RADIUS * slop) return { kind: 'end', id: sel.id, which };
    }
  }
  for (let i = doc.items.length - 1; i >= 0; i--) {
    const part = doc.items[i];
    if (isLine(part)) {
      if (distToPath(p, linePath(doc, part)) <= LINE_SLOP * slop) return { kind: 'part', id: part.id };
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

/** The smallest box around the given parts (all of them if no ids are given); `null` if there's nothing. */
export function bounds(doc: Doc, ids?: readonly number[]): Box | null {
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
    if (ids && !ids.includes(part.id)) continue;
    if (isLine(part)) {
      for (const p of linePath(doc, part)) grow(p.x, p.y);
    } else {
      grow(part.x - part.w / 2, part.y - part.h / 2);
      grow(part.x + part.w / 2, part.y + part.h / 2);
    }
  }
  return x0 === Infinity ? null : { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** The parts a dragged-out box touches: a shape if it overlaps the box, a line if any of it is inside. */
export function partsIn(doc: Doc, box: Box): number[] {
  const inside = (p: Pt) => p.x >= box.x && p.x <= box.x + box.w && p.y >= box.y && p.y <= box.y + box.h;
  const ids: number[] = [];
  for (const part of doc.items) {
    if (isLine(part)) {
      const path = linePath(doc, part);
      const len = pathLength(path);
      let hit = false;
      for (let d = 0; d <= len && !hit; d += 6) hit = inside(pointAt(path, d));
      if (hit || inside(path[path.length - 1])) ids.push(part.id);
    } else if (
      part.x + part.w / 2 >= box.x &&
      part.x - part.w / 2 <= box.x + box.w &&
      part.y + part.h / 2 >= box.y &&
      part.y - part.h / 2 <= box.y + box.h
    ) {
      ids.push(part.id);
    }
  }
  return ids;
}
