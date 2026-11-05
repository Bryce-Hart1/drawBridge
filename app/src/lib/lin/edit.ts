// Claude — Date 11/05/2026
// Every change the editor can make to a diagram, as a function from one doc to
// the next. Docs are never changed in place, so undo is just keeping the old
// one. A command that can't be done (a shape too small for its text, a shape
// key pressed with a line selected) says why instead of doing nothing, and
// the editor shows that in its bottom line.

import { capacity, fits, lineEnds, snapTarget, type Pt } from './geometry';
import {
  BIG_STEP,
  FONT_PX,
  HEADS,
  MAX_SIZE,
  MIN_SIZE,
  PALETTE,
  PULSES,
  STEP,
  find,
  isAttached,
  isLine,
  newLine,
  newShape,
  shapeById,
  type Doc,
  type End,
  type FontKey,
  type Line,
  type Part,
  type Shape,
  type ShapeKind,
} from './model';

export type Edited = { doc: Doc; note?: string } | { error: string };

export type Dir = 'left' | 'right' | 'up' | 'down';

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function replace(doc: Doc, part: Part): Doc {
  return { ...doc, items: doc.items.map((p) => (p.id === part.id ? part : p)) };
}

function lineById(doc: Doc, id: number): Line | undefined {
  const p = find(doc, id);
  return p && isLine(p) ? p : undefined;
}

// ── Adding ──────────────────────────────────────────────────────────────

/** A shape's centre, kept so the whole shape stays on the page (centred if it's bigger than the page). */
function keepOn(centre: number, size: number, page: number): number {
  return size >= page ? page / 2 : clamp(centre, size / 2, page - size / 2);
}

export function addShape(doc: Doc, t: ShapeKind, at: Pt): { doc: Doc; id: number } {
  const blank = newShape(doc.next, t, 0, 0);
  const s = { ...blank, x: keepOn(at.x, blank.w, doc.w), y: keepOn(at.y, blank.h, doc.h) };
  return { doc: { ...doc, next: doc.next + 1, items: [...doc.items, s] }, id: s.id };
}

/** The end a click at `p` makes: on a shape if it's over one, else a bare point. */
export function endAt(doc: Doc, p: Pt): End {
  const s = snapTarget(doc, p);
  return s ? { to: s.id } : { x: clamp(p.x, 0, doc.w), y: clamp(p.y, 0, doc.h) };
}

const sameEnd = (a: End, b: End) =>
  isAttached(a) && isAttached(b) ? a.to === b.to : !isAttached(a) && !isAttached(b) && Math.hypot(a.x - b.x, a.y - b.y) < 6;

export function addLine(doc: Doc, a: End, b: End): Edited & { id?: number } {
  if (sameEnd(a, b)) return { error: 'a line needs two different ends' };
  const l = newLine(doc.next, a, b);
  return { doc: { ...doc, next: doc.next + 1, items: [...doc.items, l] }, id: l.id };
}

// ── Moving (the mouse) ──────────────────────────────────────────────────

export function moveShape(doc: Doc, id: number, to: Pt): Doc {
  const s = shapeById(doc, id);
  return s ? replace(doc, { ...s, x: keepOn(to.x, s.w, doc.w), y: keepOn(to.y, s.h, doc.h) }) : doc;
}

/** Drag one end of a line to `to`: onto a shape it attaches, anywhere else it's free. */
export function moveEnd(doc: Doc, id: number, which: 'a' | 'b', to: Pt): Doc {
  const l = lineById(doc, id);
  if (!l) return doc;
  const other = which === 'a' ? l.b : l.a;
  const end = endAt(doc, to);
  // Both ends on one shape would be a line of no length.
  const moved = isAttached(end) && isAttached(other) && end.to === other.to ? { x: to.x, y: to.y } : end;
  return replace(doc, { ...l, [which]: moved });
}

/** Drag a whole line: its free ends move, the ones on shapes stay on them. */
export function moveLineBy(doc: Doc, id: number, dx: number, dy: number): Doc {
  const l = lineById(doc, id);
  if (!l) return doc;
  const shift = (e: End): End =>
    isAttached(e) ? e : { x: clamp(e.x + dx, 0, doc.w), y: clamp(e.y + dy, 0, doc.h) };
  return replace(doc, { ...l, a: shift(l.a), b: shift(l.b) });
}

// ── Shapes ──────────────────────────────────────────────────────────────

/** An arrow key on the selected shape. Circles stay round: → and ↑ grow, ← and ↓ shrink. */
export function resize(doc: Doc, id: number, dir: Dir, big: boolean): Edited {
  const s = shapeById(doc, id);
  if (!s) return { error: 'select a shape to resize it' };
  const grow = dir === 'right' || dir === 'up';
  const d = (big ? BIG_STEP : STEP) * (grow ? 1 : -1);
  let { w, h } = s;
  if (s.t === 'circle') w = h = w + d;
  else if (dir === 'left' || dir === 'right') w += d;
  else h += d;
  w = clamp(w, MIN_SIZE, MAX_SIZE);
  h = clamp(h, MIN_SIZE, MAX_SIZE);
  if (w === s.w && h === s.h) return { error: grow ? 'as big as a shape goes' : 'as small as a shape goes' };
  const next: Shape = { ...s, w, h };
  // A shape that already overflows (a hand-edited file) may still be changed.
  if (!fits(next) && fits(s)) return { error: "the text wouldn't fit in a smaller shape" };
  return { doc: replace(doc, next) };
}

export function setFont(doc: Doc, id: number, f: FontKey): Edited {
  const s = shapeById(doc, id);
  if (!s) return { error: 'select a shape to change its font' };
  if (s.f === f) return { doc };
  const next: Shape = { ...s, f };
  if (!fits(next) && fits(s)) return { error: "the text wouldn't fit at that size: make the shape bigger first" };
  return { doc: replace(doc, next) };
}

/** Add one character to the shape's text, if it still fits. */
export function typeChar(doc: Doc, id: number, ch: string): Edited {
  const s = shapeById(doc, id);
  if (!s) return { error: 'select a shape to type in it' };
  const next: Shape = { ...s, text: s.text + ch };
  if (!fits(next)) return { error: `no room: it holds up to ${capacity(s)} at this size; a bigger shape or a smaller font (i) holds more` };
  return { doc: replace(doc, next) };
}

export function backspace(doc: Doc, id: number): Edited {
  const s = shapeById(doc, id);
  if (!s) return { error: 'select a shape to type in it' };
  if (s.text === '') return { doc };
  return { doc: replace(doc, { ...s, text: [...s.text].slice(0, -1).join('') }) };
}

// ── Lines ───────────────────────────────────────────────────────────────

function cycle<T>(all: readonly T[], now: T): T {
  return all[(all.indexOf(now) + 1) % all.length];
}

/** No arrowhead → one at the end → one at each end. */
export function cycleHead(doc: Doc, id: number): Edited {
  const l = lineById(doc, id);
  if (!l) return { error: 'select a line to change its arrowheads' };
  return { doc: replace(doc, { ...l, head: cycle(HEADS, l.head) }) };
}

/** Swap the ends, so the arrow points the other way and its pulse still follows it. */
export function flip(doc: Doc, id: number): Edited {
  const l = lineById(doc, id);
  if (!l) return { error: 'select a line to flip it' };
  return { doc: replace(doc, { ...l, a: l.b, b: l.a }) };
}

/** Still → start to end → end to start → out from the middle. */
export function cyclePulse(doc: Doc, id: number): Edited {
  const l = lineById(doc, id);
  if (!l) return { error: 'select a line to make it pulse' };
  return { doc: replace(doc, { ...l, pulse: cycle(PULSES, l.pulse) }) };
}

/** A ball, or a light inside the line. */
export function toggleGlow(doc: Doc, id: number): Edited {
  const l = lineById(doc, id);
  if (!l) return { error: 'select a line to change what travels along it' };
  return { doc: replace(doc, { ...l, glow: l.glow === 'ball' ? 'light' : 'ball' }) };
}

// ── Either ──────────────────────────────────────────────────────────────

export function recolor(doc: Doc, id: number, c: number): Edited {
  const p = find(doc, id);
  if (!p) return { error: 'select something to colour it' };
  if (p.c === c) return { doc };
  return { doc: replace(doc, { ...p, c }) };
}

/** Delete a part. Lines that ended on a deleted shape keep the point they ended at. */
export function remove(doc: Doc, id: number): Doc {
  const part = find(doc, id);
  if (!part) return doc;
  const items = doc.items
    .filter((p) => p.id !== id)
    .map((p) => {
      if (!isLine(p)) return p;
      const hit = (e: End) => isAttached(e) && e.to === id;
      if (!hit(p.a) && !hit(p.b)) return p;
      const ends = lineEnds(doc, p);
      return { ...p, a: hit(p.a) ? ends.a : p.a, b: hit(p.b) ? ends.b : p.b };
    });
  return { ...doc, items };
}

/** Tab: the next part in z-order (the previous with `-1`), wrapping round. */
export function stepSelection(doc: Doc, current: number | null, dir: 1 | -1): number | null {
  const ids = doc.items.map((p) => p.id);
  if (ids.length === 0) return null;
  const at = current === null ? -1 : ids.indexOf(current);
  if (at < 0) return dir === 1 ? ids[0] : ids[ids.length - 1];
  return ids[(at + dir + ids.length) % ids.length];
}

// ── Describing ──────────────────────────────────────────────────────────

const FONT_NAME: Record<FontKey, string> = { i: 'small', o: 'medium', p: 'large' };

/** One line about the selection for the editor's status row. */
export function describe(part: Part): string {
  const colour = PALETTE[part.c].name;
  if (isLine(part)) {
    const head = part.head === 'none' ? 'line' : part.head === 'end' ? 'arrow' : 'double arrow';
    const pulse =
      part.pulse === 'off' ? 'still' : part.pulse === 'mid' ? 'pulse from the middle' : part.pulse === 'fwd' ? 'pulse a→b' : 'pulse b→a';
    return `${head} · ${colour} · ${pulse}${part.pulse === 'off' ? '' : ` · ${part.glow}`}`;
  }
  const chars = [...part.text].length;
  return `${part.t} · ${colour} · ${FONT_NAME[part.f]} ${FONT_PX[part.f]}px · ${part.w}×${part.h} · ${chars}/${capacity(part)} chars`;
}

// ── Undo ────────────────────────────────────────────────────────────────

const LIMIT = 200;

/** Earlier docs to go back to, and the ones undone to go forward to. */
export class History {
  private past: Doc[] = [];
  private future: Doc[] = [];

  /** Call with the doc as it is *before* a change. */
  push(before: Doc) {
    this.past.push(before);
    if (this.past.length > LIMIT) this.past.shift();
    this.future = [];
  }

  undo(now: Doc): Doc | null {
    const back = this.past.pop();
    if (!back) return null;
    this.future.push(now);
    return back;
  }

  redo(now: Doc): Doc | null {
    const ahead = this.future.pop();
    if (!ahead) return null;
    this.past.push(now);
    return ahead;
  }

  get canUndo() {
    return this.past.length > 0;
  }

  get canRedo() {
    return this.future.length > 0;
  }
}
