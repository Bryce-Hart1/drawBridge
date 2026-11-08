// Claude — Date 11/05/2026
// Every change the editor can make to a diagram, as a function from one doc to
// the next. Docs are never changed in place, so undo is just keeping the old
// one. A command that can't be done (a shape too small for its text, a shape
// key pressed with a line selected) says why instead of doing nothing, and
// the editor shows that in its bottom line.

import { capacity, fits, labelFits, lineEnds, snapTarget, type Pt } from './geometry';
import { caretAtEnd, insertAt, removeAt } from './text';
import {
  BIG_STEP,
  COPY_OFFSET,
  FONT_PX,
  HEADS,
  MAX_SIZE,
  MIN_SIZE,
  PALETTE,
  PULSES,
  ROUTES,
  STEP,
  STROKES,
  find,
  isAttached,
  isLine,
  isShape,
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

/**
 * Move several parts at once (a drag, or an ⌥-arrow): shapes by (dx, dy), kept
 * together on the page, and the free ends of selected lines. Ends on shapes
 * follow their shapes, so a line between two moved shapes goes with them.
 */
export function moveBy(doc: Doc, ids: readonly number[], dx: number, dy: number): Doc {
  const set = new Set(ids);
  // However far the group is asked to go, every shape stays wholly on the page.
  for (const p of doc.items) {
    if (!set.has(p.id) || !isShape(p)) continue;
    if (p.w < doc.w) dx = clamp(dx, p.w / 2 - p.x, doc.w - p.w / 2 - p.x);
    if (p.h < doc.h) dy = clamp(dy, p.h / 2 - p.y, doc.h - p.h / 2 - p.y);
  }
  if (dx === 0 && dy === 0) return doc;
  const shift = (e: End): End => (isAttached(e) ? e : { x: clamp(e.x + dx, 0, doc.w), y: clamp(e.y + dy, 0, doc.h) });
  return {
    ...doc,
    items: doc.items.map((p) => {
      if (!set.has(p.id)) return p;
      return isShape(p) ? { ...p, x: p.x + dx, y: p.y + dy } : { ...p, a: shift(p.a), b: shift(p.b) };
    }),
  };
}

/** Do one command to every selected part: it succeeds if it worked on any, and says why if it worked on none. */
export function each(doc: Doc, ids: readonly number[], make: (doc: Doc, id: number) => Edited): Edited {
  let now = doc;
  let why: string | undefined;
  let any = false;
  for (const id of ids) {
    const r = make(now, id);
    if ('error' in r) why ??= r.error;
    else {
      now = r.doc;
      any = true;
    }
  }
  return any ? { doc: now } : { error: why ?? 'nothing is selected' };
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
  const p = find(doc, id);
  if (!p) return { error: 'select a shape or line to change its font' };
  if (p.f === f) return { doc };
  if (isLine(p)) {
    const next: Line = { ...p, f };
    if (!labelFits(doc, next) && labelFits(doc, p)) return { error: "the label wouldn't fit at that size" };
    return { doc: replace(doc, next) };
  }
  const next: Shape = { ...p, f };
  if (!fits(next) && fits(p)) return { error: "the text wouldn't fit at that size: make the shape bigger first" };
  return { doc: replace(doc, next) };
}

/** Add one character at the end of the text of a shape (or the label of a line), if it still fits. A newline starts a new line in a shape. */
export function typeChar(doc: Doc, id: number, ch: string): Edited {
  const p = find(doc, id);
  if (!p) return { error: 'select a shape or line to type in it' };
  const r = insertAt(doc, id, { ...caretAtEnd(p), pend: 0 }, ch);
  return 'error' in r ? r : { doc: r.doc };
}

export function backspace(doc: Doc, id: number): Edited {
  const p = find(doc, id);
  if (!p) return { error: 'select a shape or line to type in it' };
  const r = removeAt(doc, id, caretAtEnd(p));
  return 'error' in r ? r : { doc: r.doc };
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

/** Straight → curved → square-cornered. */
export function cycleRoute(doc: Doc, id: number): Edited {
  const l = lineById(doc, id);
  if (!l) return { error: 'select a line to bend it' };
  return { doc: replace(doc, { ...l, route: cycle(ROUTES, l.route) }) };
}

// ── Either ──────────────────────────────────────────────────────────────

/** Solid → dashed → dotted, for a shape's outline or a line. */
export function cycleStroke(doc: Doc, id: number): Edited {
  const p = find(doc, id);
  if (!p) return { error: 'select something to change its outline' };
  return { doc: replace(doc, { ...p, ln: cycle(STROKES, p.ln) }) };
}

/** Move parts in the stacking order: one step toward the front or back, or all the way. */
export function reorder(doc: Doc, ids: readonly number[], to: 'up' | 'down' | 'front' | 'back'): Edited {
  const set = new Set(ids);
  const chosen = doc.items.filter((p) => set.has(p.id));
  if (chosen.length === 0) return { error: 'nothing is selected' };
  let items = [...doc.items];
  if (to === 'front') items = [...items.filter((p) => !set.has(p.id)), ...chosen];
  else if (to === 'back') items = [...chosen, ...items.filter((p) => !set.has(p.id))];
  else if (to === 'up') {
    for (let i = items.length - 2; i >= 0; i--) if (set.has(items[i].id) && !set.has(items[i + 1].id)) [items[i], items[i + 1]] = [items[i + 1], items[i]];
  } else {
    for (let i = 1; i < items.length; i++) if (set.has(items[i].id) && !set.has(items[i - 1].id)) [items[i], items[i - 1]] = [items[i - 1], items[i]];
  }
  if (items.every((p, i) => p === doc.items[i])) return { error: to === 'up' || to === 'front' ? 'already at the front' : 'already at the back' };
  return { doc: { ...doc, items } };
}

/**
 * The parts to copy: `ids` in stacking order. A line whose end is on a shape
 * that isn't being copied keeps the point it ended at, since that shape
 * won't be where the copy goes.
 */
export function copyParts(doc: Doc, ids: readonly number[]): Part[] {
  const set = new Set(ids);
  const shapes = new Set(doc.items.filter((p) => set.has(p.id) && isShape(p)).map((p) => p.id));
  return doc.items
    .filter((p) => set.has(p.id))
    .map((p) => {
      if (!isLine(p)) return p;
      const ends = lineEnds(doc, p);
      const keep = (e: End, at: Pt): End => (isAttached(e) && !shapes.has(e.to) ? { x: at.x, y: at.y } : e);
      return { ...p, a: keep(p.a, ends.a), b: keep(p.b, ends.b) };
    });
}

/**
 * Put copies of `parts` on the page, moved by `delta`, at the front. Lines
 * between copied shapes join the copies; a line end on a shape that's still
 * in this doc but wasn't copied stays on it (duplicating a lone line makes a
 * second line between the same shapes, which runs beside the first).
 */
export function pasteParts(doc: Doc, parts: readonly Part[], delta: Pt): { doc: Doc; ids: number[] } {
  if (parts.length === 0) return { doc, ids: [] };
  let { x: dx, y: dy } = delta;
  for (const p of parts) {
    if (!isShape(p)) continue;
    if (p.w < doc.w) dx = clamp(dx, p.w / 2 - p.x, doc.w - p.w / 2 - p.x);
    if (p.h < doc.h) dy = clamp(dy, p.h / 2 - p.y, doc.h - p.h / 2 - p.y);
  }
  const fresh = new Map<number, number>();
  let next = doc.next;
  for (const p of parts) fresh.set(p.id, next++);
  const end = (e: End): End => {
    if (isAttached(e)) {
      const to = fresh.get(e.to);
      if (to !== undefined && parts.some((p) => p.id === e.to && isShape(p))) return { to };
      return shapeById(doc, e.to) ? e : { x: doc.w / 2, y: doc.h / 2 };
    }
    return { x: clamp(e.x + dx, 0, doc.w), y: clamp(e.y + dy, 0, doc.h) };
  };
  const made: Part[] = parts.map((p) =>
    isShape(p) ? { ...p, id: fresh.get(p.id)!, x: p.x + dx, y: p.y + dy } : { ...p, id: fresh.get(p.id)!, a: end(p.a), b: end(p.b) },
  );
  return { doc: { ...doc, next, items: [...doc.items, ...made] }, ids: made.map((p) => p.id) };
}

/** ⌘D: a copy of the selection a little way off, where it can be seen and dragged. */
export function duplicate(doc: Doc, ids: readonly number[]): { doc: Doc; ids: number[] } | { error: string } {
  const set = new Set(ids);
  const parts = doc.items.filter((p) => set.has(p.id));
  if (parts.length === 0) return { error: 'select something to duplicate it' };
  return pasteParts(doc, parts, { x: COPY_OFFSET, y: COPY_OFFSET });
}

/** Delete several parts. */
export function removeMany(doc: Doc, ids: readonly number[]): Doc {
  return ids.reduce((d, id) => remove(d, id), doc);
}

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
  const outline = part.ln === 'solid' ? '' : ` · ${part.ln === 'dash' ? 'dashed' : 'dotted'}`;
  if (isLine(part)) {
    const head = part.head === 'none' ? 'line' : part.head === 'end' ? 'arrow' : 'double arrow';
    const pulse =
      part.pulse === 'off' ? 'still' : part.pulse === 'mid' ? 'pulse from the middle' : part.pulse === 'fwd' ? 'pulse a→b' : 'pulse b→a';
    const route = part.route === 'straight' ? '' : ` · ${part.route === 'curve' ? 'curved' : 'square'}`;
    const label = part.text === '' ? '' : ` · label ${[...part.text].length} chars`;
    return `${head} · ${colour} · ${pulse}${part.pulse === 'off' ? '' : ` · ${part.glow}`}${route}${outline}${label}`;
  }
  const chars = [...part.text].length;
  return `${part.t} · ${colour} · ${FONT_NAME[part.f]} ${FONT_PX[part.f]}px · ${part.w}×${part.h} · ${chars}/${capacity(part)} chars${outline}`;
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
