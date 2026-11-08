// Claude — Date 11/08/2026
import { describe, expect, it } from 'vitest';
import { addLine, addShape, resize, type Edited } from './edit';
import { caretCell, layoutIndexed, wrap, wrapIndexed } from './geometry';
import { parseLin, serializeLin } from './format';
import { BOLD, ITALIC, UNDERLINE, blankDoc, shapeById, find, isLine, type Doc, type Line, type Shape } from './model';
import { activeStyle, caretAt, caretAtEnd, describeStyle, hasSelection, insertAt, moveCaret, removeAt, span, stylesOf, toggleStyle, type Caret, type Typed } from './text';
import { runsOf } from './draw';

function done(t: Typed): { doc: Doc; caret: Caret } {
  if ('error' in t) throw new Error(t.error);
  return t;
}

/** A big rectangle (id 1) so the text has room. */
function start(): Doc {
  let doc = addShape(blankDoc(), 'rect', { x: 400, y: 300 }).doc;
  for (let i = 0; i < 8; i++) doc = (resize(doc, 1, 'right', true) as { doc: Doc }).doc;
  for (let i = 0; i < 5; i++) doc = (resize(doc, 1, 'up', true) as { doc: Doc }).doc;
  return doc;
}

const shape = (doc: Doc): Shape => shapeById(doc, 1)!;

/** Type a string at the caret, one key at a time. */
function typed(doc: Doc, caret: Caret, text: string): { doc: Doc; caret: Caret } {
  let now = { doc, caret };
  for (const ch of text) now = done(insertAt(now.doc, 1, now.caret, ch));
  return now;
}

describe('a caret in the text', () => {
  it('types where the caret is, not only at the end', () => {
    let now = typed(start(), caretAt(0), 'helo');
    now = { ...now, caret: caretAt(3) };
    now = typed(now.doc, now.caret, 'l');
    expect(shape(now.doc).text).toBe('hello');
    expect(now.caret.at).toBe(4);
  });

  it('backspaces the letter before the caret, and the whole selection', () => {
    let now = typed(start(), caretAt(0), 'hello');
    now = done(removeAt(now.doc, 1, caretAt(3)));
    expect(shape(now.doc).text).toBe('helo');
    expect(now.caret.at).toBe(2);
    now = done(removeAt(now.doc, 1, { at: 1, anchor: 3, pend: null }));
    expect(shape(now.doc).text).toBe('ho');
    expect(now.caret).toEqual(caretAt(1));
    // Nothing before the caret: nothing happens.
    expect(done(removeAt(now.doc, 1, caretAt(0))).doc).toBe(now.doc);
  });

  it('types over a selection', () => {
    let now = typed(start(), caretAt(0), 'cat');
    now = done(insertAt(now.doc, 1, { at: 3, anchor: 1, pend: null }, 'u'));
    expect(shape(now.doc).text).toBe('cu');
  });

  it('moves with the arrows, selects with shift, and stops at the ends', () => {
    expect(moveCaret(5, caretAt(2), 'left', false)).toEqual(caretAt(1));
    expect(moveCaret(5, caretAt(0), 'left', false)).toEqual(caretAt(0));
    expect(moveCaret(5, caretAt(5), 'right', false)).toEqual(caretAt(5));
    const sel = moveCaret(5, moveCaret(5, caretAt(2), 'left', true), 'left', true);
    expect(sel).toEqual({ at: 0, anchor: 2, pend: null });
    expect(hasSelection(sel)).toBe(true);
    expect(span(sel)).toEqual([0, 2]);
    // An arrow with a selection drops it at the edge it points to.
    expect(moveCaret(5, sel, 'right', false)).toEqual(caretAt(2));
    expect(moveCaret(5, sel, 'left', false)).toEqual(caretAt(0));
  });

  it('still turns a backslash and n into a line break, wherever the caret is', () => {
    let now = typed(start(), caretAt(0), 'ab');
    now = { ...now, caret: caretAt(1) };
    now = typed(now.doc, now.caret, '\\n');
    expect(shape(now.doc).text).toBe('a\nb');
    expect(now.caret.at).toBe(2);
  });

  it('keeps a label to the end of its row, with the same limits', () => {
    let doc = start();
    doc = addShape(doc, 'circle', { x: 1100, y: 300 }).doc;
    const line = (doc: Doc) => find(doc, 3) as Line;
    const added = addLine(doc, { to: 1 }, { to: 2 }) as { doc: Doc };
    doc = added.doc;
    expect(isLine(line(doc))).toBe(true);
    const r = done(insertAt(doc, 3, caretAtEnd(line(doc)), 'x'));
    expect(line(r.doc).text).toBe('x');
    expect(insertAt(doc, 3, caretAt(0), '\n')).toEqual({ error: 'a label is one row' });
  });
});

describe('bold, italic and underline', () => {
  it('styles the selected letters, and only those', () => {
    let now = typed(start(), caretAt(0), 'make it so');
    now = done(toggleStyle(now.doc, 1, { at: 7, anchor: 5, pend: null }, BOLD));
    expect(stylesOf(shape(now.doc))).toEqual([0, 0, 0, 0, 0, 1, 1, 0, 0, 0]);
    expect(shape(now.doc).st).toBe('0000011000');
  });

  it('adds styles together, and takes one off when every letter has it', () => {
    let now = typed(start(), caretAt(0), 'abcd');
    const all = { at: 4, anchor: 0, pend: null };
    now = done(toggleStyle(now.doc, 1, all, BOLD));
    now = done(toggleStyle(now.doc, 1, all, UNDERLINE));
    expect(shape(now.doc).st).toBe('5555');
    now = done(toggleStyle(now.doc, 1, all, BOLD));
    expect(shape(now.doc).st).toBe('4444');
    // Part of the selection already has it: it goes on for all, rather than off.
    now = done(toggleStyle(now.doc, 1, { at: 2, anchor: 0, pend: null }, ITALIC));
    now = done(toggleStyle(now.doc, 1, all, ITALIC));
    expect(shape(now.doc).st).toBe('6666');
    now = done(toggleStyle(now.doc, 1, all, ITALIC));
    now = done(toggleStyle(now.doc, 1, all, UNDERLINE));
    expect(shape(now.doc).st).toBe('');
  });

  it('with nothing selected, switches the style for the next letters', () => {
    let now = typed(start(), caretAt(0), 'a');
    now = done(toggleStyle(now.doc, 1, now.caret, BOLD));
    expect(shape(now.doc).st).toBe('');
    expect(now.caret.pend).toBe(BOLD);
    now = typed(now.doc, now.caret, 'bc');
    now = done(toggleStyle(now.doc, 1, now.caret, BOLD));
    expect(now.caret.pend).toBe(0);
    now = typed(now.doc, now.caret, 'd');
    expect(stylesOf(shape(now.doc))).toEqual([0, 1, 1, 0]);
  });

  it('has a new letter take the style of the one before it, until a style is switched', () => {
    let now = typed(start(), caretAt(0), 'ab');
    now = done(toggleStyle(now.doc, 1, { at: 2, anchor: 0, pend: null }, ITALIC));
    now = typed(now.doc, caretAt(2), 'c');
    expect(shape(now.doc).st).toBe('222');
    // Moving the caret forgets the switch.
    expect(moveCaret(3, { at: 3, anchor: 3, pend: ITALIC }, 'left', false).pend).toBeNull();
  });

  it('moves the styles with the letters when typing or deleting in the middle', () => {
    let now = typed(start(), caretAt(0), 'abcd');
    now = done(toggleStyle(now.doc, 1, { at: 3, anchor: 2, pend: null }, BOLD));
    expect(shape(now.doc).st).toBe('0010');
    // Type a plain letter before the bold one (the caret's pending style is plain).
    now = done(insertAt(now.doc, 1, { at: 1, anchor: 1, pend: 0 }, 'X'));
    expect(shape(now.doc).text).toBe('aXbcd');
    expect(shape(now.doc).st).toBe('00010');
    now = done(removeAt(now.doc, 1, { at: 2, anchor: 0, pend: null }));
    expect(shape(now.doc).text).toBe('bcd');
    expect(shape(now.doc).st).toBe('010');
    // Deleting the last styled letter leaves no styles at all.
    now = done(removeAt(now.doc, 1, { at: 2, anchor: 1, pend: null }));
    expect(shape(now.doc).st).toBe('');
  });

  it("can't style nothing: no letters selected, or a line break", () => {
    let now = typed(start(), caretAt(0), 'a\nb');
    expect(toggleStyle(now.doc, 1, { at: 2, anchor: 1, pend: null }, BOLD)).toEqual({ error: 'select some letters first' });
    now = done(toggleStyle(now.doc, 1, { at: 3, anchor: 0, pend: null }, BOLD));
    // The break keeps no style.
    expect(shape(now.doc).st).toBe('101');
  });

  it("is for shapes: a line's label can't have it", () => {
    let doc = addShape(start(), 'circle', { x: 700, y: 300 }).doc;
    doc = (addLine(doc, { to: 1 }, { to: 2 }) as { doc: Doc }).doc;
    expect(toggleStyle(doc, 3, caretAt(0), BOLD)).toHaveProperty('error');
  });

  it('says what the caret would type in', () => {
    let now = typed(start(), caretAt(0), 'ab');
    now = done(toggleStyle(now.doc, 1, { at: 1, anchor: 0, pend: null }, BOLD));
    const s = shape(now.doc);
    expect(activeStyle(s, { at: 1, anchor: 0, pend: null })).toBe(BOLD);
    expect(activeStyle(s, { at: 2, anchor: 0, pend: null })).toBe(0);
    expect(activeStyle(s, caretAt(1))).toBe(BOLD);
    expect(activeStyle(s, { at: 2, anchor: 2, pend: ITALIC | UNDERLINE })).toBe(6);
    expect(describeStyle(BOLD | UNDERLINE)).toBe('bold, underline');
    expect(describeStyle(0)).toBe('');
  });

  it('refuses a letter that no longer fits, styled or not', () => {
    let doc = start();
    doc = (resize(doc, 1, 'left', true) as { doc: Doc }).doc;
    let now = { doc, caret: caretAt(0) };
    let error = '';
    for (let i = 0; i < 400 && !error; i++) {
      const r = insertAt(now.doc, 1, now.caret, 'w');
      if ('error' in r) error = r.error;
      else now = r;
    }
    expect(error).toMatch(/no room/);
  });
});

describe('where letters are on the shape', () => {
  it('gives the same lines as wrap, with the index of every letter shown', () => {
    for (const [text, cols] of [
      ['', 10],
      ['hello', 10],
      ['one two  three four', 8],
      ['abcdefghijkl', 5],
      ['milk\neggs\n\nbread', 10],
      ['   ', 4],
      ['a\n   \nb', 4],
      ['trailing\n', 10],
      ['x y', 0],
    ] as const) {
      const cs = [...text];
      expect(wrapIndexed(text, cols).map((row) => row.map((i) => cs[i]).join('')), `${JSON.stringify(text)} in ${cols}`).toEqual(wrap(text, cols));
    }
  });

  it('shows the space just before the next word when spaces are folded together', () => {
    expect(wrapIndexed('a  b', 10)).toEqual([[0, 2, 3]]);
  });

  it('finds the row and column of a caret', () => {
    expect(caretCell('', 10, 0)).toEqual({ row: 0, col: 0 });
    expect(caretCell('hello', 10, 5)).toEqual({ row: 0, col: 5 });
    expect(caretCell('hello', 10, 2)).toEqual({ row: 0, col: 2 });
    expect(caretCell('abcdefgh', 5, 6)).toEqual({ row: 1, col: 1 });
    // Words that wrap: a caret on the space that was folded away goes to the start of the next row.
    expect(caretCell('one two', 4, 3)).toEqual({ row: 1, col: 0 });
    expect(caretCell('one two', 4, 7)).toEqual({ row: 1, col: 3 });
  });

  it('finds the caret around line breaks, blank lines included', () => {
    expect(caretCell('a\nb', 10, 1)).toEqual({ row: 0, col: 1 });
    expect(caretCell('a\nb', 10, 2)).toEqual({ row: 1, col: 0 });
    expect(caretCell('a\n\nb', 10, 2)).toEqual({ row: 1, col: 0 });
    expect(caretCell('a\n\nb', 10, 3)).toEqual({ row: 2, col: 0 });
    expect(caretCell('a\n', 10, 2)).toEqual({ row: 1, col: 0 });
    expect(caretCell('abc', 10, 99)).toEqual({ row: 0, col: 3 });
  });

  it("indexes a shape's own layout", () => {
    const doc = typed(start(), caretAt(0), 'a b').doc;
    expect(layoutIndexed(shape(doc))).toEqual([[0, 1, 2]]);
  });

  it('splits a row into runs of one style', () => {
    expect(runsOf([0, 1, 2, 3, 4], [0, 0, 1, 1, 0])).toEqual([
      [0, 2, 0],
      [2, 4, 1],
      [4, 5, 0],
    ]);
    expect(runsOf([], [])).toEqual([]);
    // A row that skips a folded space still reads its own letters' styles.
    expect(runsOf([0, 2], [1, 0, 1])).toEqual([[0, 2, 1]]);
  });
});

describe('styles in the file', () => {
  const withStyles = () => {
    let now = typed(start(), caretAt(0), 'hi there');
    now = done(toggleStyle(now.doc, 1, { at: 8, anchor: 3, pend: null }, BOLD | ITALIC));
    return now.doc;
  };

  it('are saved and read back', () => {
    const doc = withStyles();
    expect(shape(doc).st).toBe('00033333');
    const back = parseLin(serializeLin(doc));
    expect(shapeById(back, 1)!.st).toBe(shape(doc).st);
    expect(serializeLin(back)).toBe(serializeLin(doc));
  });

  it('are left out of a shape without any, so older files read and write the same', () => {
    expect(serializeLin(typed(start(), caretAt(0), 'plain').doc)).not.toContain('"st"');
  });

  it('are dropped, not guessed at, when they do not match the text', () => {
    const text = serializeLin(withStyles());
    for (const bad of ['"st":"333"', '"st":"0003333x"', '"st":"00088888"', '"st":3', '"st":""', '"st":"00000000"']) {
      const parsed = parseLin(text.replace(/"st":"[^"]*"/, bad));
      expect(shapeById(parsed, 1)!.st, bad).toBe('');
    }
  });
});
