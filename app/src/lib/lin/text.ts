// Claude — Date 11/08/2026
// Typing into a shape or a line's label, with a caret: where letters go, what
// the arrows select, and which letters are bold, italic or underlined. Pure
// functions from one doc to the next, like edit.ts; the editor keeps the
// caret between key presses.
//
// A shape's styles live beside its text, one digit per character (`st`, see
// model.ts). Styles only ever belong to letters that are there: they move and
// go with the letters when you type or delete in the middle of the text, and a
// style can't be set on nothing. A line's label has a caret to type at the
// end of, but no styles.

import { capacity, fits, labelCapacity, labelFits } from './geometry';
import { STYLE_NAMES, find, isLine, type Doc, type Line, type Part, type Shape } from './model';

/** Where typing is. `at` is how many characters come before the caret. */
export interface Caret {
  at: number;
  /** The other end of a selection; the same as `at` when nothing is selected. */
  anchor: number;
  /**
   * The style the next letter typed gets, once ⌥B, ⌥I or ⌥U has been pressed with
   * nothing selected. `null` means the same as the letter before the caret.
   */
  pend: number | null;
}

export type Typed = { doc: Doc; caret: Caret; note?: string } | { error: string };

export const caretAt = (at: number, pend: number | null = null): Caret => ({ at, anchor: at, pend });

/** The caret at the end of what's there, which is where typing starts. */
export function caretAtEnd(p: Part): Caret {
  return caretAt([...p.text].length);
}

/** The selected stretch, as [from, to). */
export function span(c: Caret): [number, number] {
  return [Math.min(c.at, c.anchor), Math.max(c.at, c.anchor)];
}

export const hasSelection = (c: Caret) => c.at !== c.anchor;

/** A style for each character of a shape's text. */
export function stylesOf(s: Pick<Shape, 'text' | 'st'>): number[] {
  const n = [...s.text].length;
  return Array.from({ length: n }, (_, i) => {
    const d = Number(s.st[i]);
    return Number.isInteger(d) && d >= 0 && d <= 7 ? d : 0;
  });
}

function pack(styles: readonly number[]): string {
  return styles.every((d) => d === 0) ? '' : styles.join('');
}

function replace(doc: Doc, part: Part): Doc {
  return { ...doc, items: doc.items.map((p) => (p.id === part.id ? part : p)) };
}

/** What a newly typed letter looks like at `c`: the pending style, else the letter before it (or the one after, at the start). */
function styleHere(styles: readonly number[], c: Caret): number {
  if (c.pend !== null) return c.pend;
  const [from] = span(c);
  return from > 0 ? styles[from - 1] ?? 0 : styles[from] ?? 0;
}

/** The style the caret would type in, or that all of the selected letters share (a style they only partly have doesn't count). */
export function activeStyle(p: Part, c: Caret): number {
  if (isLine(p)) return 0;
  const styles = stylesOf(p);
  if (!hasSelection(c)) return styleHere(styles, c);
  const [from, to] = span(c);
  const chars = [...p.text];
  const letters = styles.map((d, i) => (chars[i] === '\n' ? 7 : d)).slice(from, to);
  return chars.slice(from, to).every((c) => c === '\n') ? 0 : letters.reduce((all, d) => all & d, 7);
}

/** The words for a style: "bold, italic". */
export function describeStyle(style: number): string {
  return STYLE_NAMES.filter(([flag]) => (style & flag) !== 0)
    .map(([, name]) => name)
    .join(', ');
}

// ── Typing ──────────────────────────────────────────────────────────────

/**
 * Type one character at the caret, replacing what's selected. A newline starts
 * a new line in a shape (a label is one row), and so does a backslash followed
 * by n: the two become one break.
 */
export function insertAt(doc: Doc, id: number, caret: Caret, ch: string): Typed {
  const p = find(doc, id);
  if (!p) return { error: 'select a shape or line to type in it' };
  const chars = [...p.text];
  const styles = isLine(p) ? [] : stylesOf(p);
  const [from, to] = span(caret);
  const here = isLine(p) ? 0 : styleHere(styles, caret);

  if (isLine(p) && ch === '\n') return { error: 'a label is one row' };

  let put = ch;
  let start = from;
  let style = ch === '\n' ? 0 : here;
  if (ch === 'n' && !isLine(p) && from === to && chars[from - 1] === '\\') {
    // The backslash becomes the break.
    put = '\n';
    start = from - 1;
    style = 0;
  }
  chars.splice(start, to - start, put);
  if (!isLine(p)) styles.splice(start, to - start, style);

  const text = chars.join('');
  const next = { at: start + 1, anchor: start + 1, pend: caret.pend };
  if (isLine(p)) {
    const label: Line = { ...p, text };
    if (!labelFits(doc, label)) return { error: `no room: a label holds up to ${labelCapacity(doc, p)} here at this size (i is smaller)` };
    return { doc: replace(doc, label), caret: next };
  }
  const shape: Shape = { ...p, text, st: pack(styles) };
  if (!fits(shape)) return { error: `no room: it holds up to ${capacity(p)} at this size; a bigger shape or a smaller font (i) holds more` };
  return { doc: replace(doc, shape), caret: next };
}

/** Backspace: the selected letters, or the one before the caret. */
export function removeAt(doc: Doc, id: number, caret: Caret): Typed {
  const p = find(doc, id);
  if (!p) return { error: 'select a shape or line to type in it' };
  const [lo, hi] = span(caret);
  const from = lo === hi ? Math.max(0, lo - 1) : lo;
  const to = hi;
  if (from === to) return { doc, caret: caretAt(from, caret.pend) };
  const chars = [...p.text];
  chars.splice(from, to - from);
  const gone = { at: from, anchor: from, pend: caret.pend };
  if (isLine(p)) return { doc: replace(doc, { ...p, text: chars.join('') }), caret: gone };
  const styles = stylesOf(p);
  styles.splice(from, to - from);
  return { doc: replace(doc, { ...p, text: chars.join(''), st: pack(styles) }), caret: gone };
}

/** An arrow key: one letter left or right, or with ⇧ keeping the other end of the selection where it is. */
export function moveCaret(length: number, caret: Caret, dir: 'left' | 'right', select: boolean): Caret {
  const [from, to] = span(caret);
  if (!select && from !== to) {
    // With something selected, an arrow drops the selection at the edge it points to.
    const at = dir === 'left' ? from : to;
    return caretAt(at);
  }
  const at = Math.min(length, Math.max(0, caret.at + (dir === 'left' ? -1 : 1)));
  return { at, anchor: select ? caret.anchor : at, pend: null };
}

// ── Styles ──────────────────────────────────────────────────────────────

/**
 * ⌥B, ⌥I or ⌥U. With letters selected, it turns the style on for all of them
 * (or off, if they all have it already); with nothing selected it switches the
 * style for the letters typed next. Only letters that are there get one: a
 * line break has no look, so it's left alone.
 */
export function toggleStyle(doc: Doc, id: number, caret: Caret, flag: number): Typed {
  const p = find(doc, id);
  if (!p) return { error: 'select a shape to style its text' };
  if (isLine(p)) return { error: "a line's label can't be bold, italic or underlined: put the words in a shape" };
  const styles = stylesOf(p);
  const [from, to] = span(caret);
  if (from === to) {
    const now = styleHere(styles, caret);
    return { doc, caret: { ...caret, pend: now ^ flag } };
  }
  const chars = [...p.text];
  const letters: number[] = [];
  for (let i = from; i < to; i++) if (chars[i] !== '\n') letters.push(i);
  if (letters.length === 0) return { error: 'select some letters first' };
  const all = letters.every((i) => (styles[i] & flag) !== 0);
  for (const i of letters) styles[i] = all ? styles[i] & ~flag : styles[i] | flag;
  return { doc: replace(doc, { ...p, st: pack(styles) }), caret };
}
