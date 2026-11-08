// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import {
  History,
  copyParts,
  cycleRoute,
  cycleStroke,
  duplicate,
  each,
  moveBy,
  pasteParts,
  removeMany,
  reorder,
  addLine,
  addShape,
  backspace,
  cycleHead,
  cyclePulse,
  describe as describePart,
  endAt,
  flip,
  moveEnd,
  moveLineBy,
  moveShape,
  recolor,
  remove,
  resize,
  setFont,
  stepSelection,
  toggleGlow,
  typeChar,
  type Edited,
} from './edit';
import { parseLin } from './format';
import { blankDoc, find, isLine, shapeById, type Doc, type Line } from './model';

/** The doc from a successful edit; fails the test if the edit was refused. */
function ok(e: Edited): Doc {
  if ('error' in e) throw new Error(`refused: ${e.error}`);
  return e.doc;
}

/** Two rectangles, 1 and 2, with a line, 3, from one to the other. */
function pair(): Doc {
  let doc = blankDoc();
  doc = addShape(doc, 'rect', { x: 100, y: 100 }).doc;
  doc = addShape(doc, 'rect', { x: 400, y: 100 }).doc;
  return ok(addLine(doc, { to: 1 }, { to: 2 }));
}

const line = (doc: Doc, id = 3) => find(doc, id) as Line;

describe('adding', () => {
  it('numbers parts in turn and keeps them on the page', () => {
    const one = addShape(blankDoc(), 'diamond', { x: 5000, y: -20 });
    expect(one.id).toBe(1);
    // A 140×100 diamond, kept wholly on the 1200×800 page.
    expect(shapeById(one.doc, 1)).toMatchObject({ t: 'diamond', x: 1130, y: 50 });
    expect(addShape(one.doc, 'circle', { x: 1, y: 1 }).id).toBe(2);
  });

  it('refuses a line from a place to itself', () => {
    const doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    expect(addLine(doc, { to: 1 }, { to: 1 })).toEqual({ error: 'a line needs two different ends' });
    expect('error' in addLine(doc, { x: 10, y: 10 }, { x: 12, y: 11 })).toBe(true);
    expect('error' in addLine(doc, { x: 10, y: 10 }, { x: 80, y: 11 })).toBe(false);
  });

  it('ends a click on a shape as attached', () => {
    const doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    expect(endAt(doc, { x: 110, y: 100 })).toEqual({ to: 1 });
    expect(endAt(doc, { x: 600, y: 100 })).toEqual({ x: 600, y: 100 });
  });
});

describe('resize', () => {
  const rect = () => addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;

  it('changes width with left and right, height with up and down', () => {
    expect(shapeById(ok(resize(rect(), 1, 'right', false)), 1)).toMatchObject({ w: 170, h: 80 });
    expect(shapeById(ok(resize(rect(), 1, 'left', false)), 1)).toMatchObject({ w: 150, h: 80 });
    expect(shapeById(ok(resize(rect(), 1, 'up', false)), 1)).toMatchObject({ w: 160, h: 90 });
    expect(shapeById(ok(resize(rect(), 1, 'down', false)), 1)).toMatchObject({ w: 160, h: 70 });
  });

  it('takes a bigger step with shift, and stays centred', () => {
    expect(shapeById(ok(resize(rect(), 1, 'right', true)), 1)).toMatchObject({ w: 200, x: 300, y: 300 });
  });

  it('keeps a circle round whichever arrow', () => {
    const doc = addShape(blankDoc(), 'circle', { x: 300, y: 300 }).doc;
    expect(shapeById(ok(resize(doc, 1, 'right', false)), 1)).toMatchObject({ w: 110, h: 110 });
    expect(shapeById(ok(resize(doc, 1, 'up', false)), 1)).toMatchObject({ w: 110, h: 110 });
    expect(shapeById(ok(resize(doc, 1, 'left', false)), 1)).toMatchObject({ w: 90, h: 90 });
    expect(shapeById(ok(resize(doc, 1, 'down', false)), 1)).toMatchObject({ w: 90, h: 90 });
  });

  it('stops at the smallest and the biggest', () => {
    /** Press an arrow key `n` times; a refused press leaves the doc as it was. */
    const press = (start: Doc, dir: 'left' | 'right', n: number) => {
      let doc = start;
      for (let i = 0; i < n; i++) {
        const next = resize(doc, 1, dir, true);
        if ('doc' in next) doc = next.doc;
      }
      return doc;
    };
    const small = press(rect(), 'left', 30);
    expect(shapeById(small, 1)!.w).toBe(24);
    expect(resize(small, 1, 'left', false)).toEqual({ error: 'as small as a shape goes' });
    const big = press(rect(), 'right', 30);
    expect(shapeById(big, 1)!.w).toBe(800);
    expect(resize(big, 1, 'right', false)).toEqual({ error: 'as big as a shape goes' });
  });

  it('refuses to shrink a shape below its text', () => {
    let doc = rect();
    for (const ch of 'hello world foo bar baz') doc = ok(typeChar(doc, 1, ch));
    expect(resize(doc, 1, 'left', true)).toEqual({ error: "the text wouldn't fit in a smaller shape" });
    // One small step still fits.
    expect(shapeById(ok(resize(doc, 1, 'left', false)), 1)!.w).toBe(150);
  });

  it('lets a shape that already overflows shrink', () => {
    const text = 'x'.repeat(40);
    const doc = parseLin(`{"lin":1,"items":[{"id":1,"t":"rect","x":300,"y":300,"w":160,"h":80,"text":"${text}"}]}`);
    expect(shapeById(ok(resize(doc, 1, 'left', true)), 1)!.w).toBe(120);
  });

  it('is for shapes only', () => {
    expect(resize(pair(), 3, 'left', false)).toEqual({ error: 'select a shape to resize it' });
  });
});

describe('text', () => {
  it('fills a shape to its capacity and no further', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    let typed = 0;
    for (let i = 0; i < 100; i++) {
      const next = typeChar(doc, 1, 'x');
      if ('error' in next) break;
      doc = next.doc;
      typed++;
    }
    // 13 characters a line, 2 lines, at the medium font.
    expect(typed).toBe(26);
    expect(typeChar(doc, 1, 'x')).toHaveProperty('error');
  });

  it('holds more in a smaller font and less in a bigger one', () => {
    const fill = (f: 'i' | 'o' | 'p') => {
      let doc = ok(setFont(addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc, 1, f));
      let n = 0;
      for (;;) {
        const next = typeChar(doc, 1, 'x');
        if ('error' in next) return n;
        doc = next.doc;
        n++;
      }
    };
    expect(fill('i')).toBe(80);
    expect(fill('o')).toBe(26);
    expect(fill('p')).toBe(8);
  });

  it('refuses a bigger font the text would overflow', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    for (const ch of 'hello world foo') doc = ok(typeChar(doc, 1, ch));
    expect(setFont(doc, 1, 'p')).toHaveProperty('error');
    expect(shapeById(ok(setFont(doc, 1, 'i')), 1)!.f).toBe('i');
  });

  it('backs over characters, including ones made of two code units', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    doc = ok(typeChar(ok(typeChar(doc, 1, 'a')), 1, '😀'));
    expect(shapeById(doc, 1)!.text).toBe('a😀');
    expect(shapeById(ok(backspace(doc, 1)), 1)!.text).toBe('a');
    expect(shapeById(ok(backspace(ok(backspace(doc, 1)), 1)), 1)!.text).toBe('');
    expect('doc' in backspace(ok(backspace(ok(backspace(doc, 1)), 1)), 1)).toBe(true);
  });

  it('makes a list: a newline starts a new row, and is refused when the rows run out', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    for (const ch of 'milk\neggs') doc = ok(typeChar(doc, 1, ch));
    expect(shapeById(doc, 1)!.text).toBe('milk\neggs');
    // Two rows fit in 160×80; a third doesn't.
    expect(typeChar(doc, 1, '\n')).toHaveProperty('error');
    // Four backspaces take off "eggs"; one more takes the break out, so the rows rejoin.
    let joined = doc;
    for (let i = 0; i < 4; i++) joined = ok(backspace(joined, 1));
    expect(shapeById(joined, 1)!.text).toBe('milk\n');
    expect(shapeById(ok(backspace(joined, 1)), 1)!.text).toBe('milk');
  });

  it('turns a typed backslash and n into one line break', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    for (const ch of 'milk\\neggs') doc = ok(typeChar(doc, 1, ch));
    expect(shapeById(doc, 1)!.text).toBe('milk\neggs');
    // A backslash on its own stays until an n follows it.
    doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    doc = ok(typeChar(ok(typeChar(doc, 1, 'a')), 1, '\\'));
    expect(shapeById(doc, 1)!.text).toBe('a\\');
  });

  it('keeps a label on a line to one row', () => {
    expect(typeChar(pair(), 3, '\n')).toEqual({ error: 'a label is one row' });
  });

  it('labels a line, up to what fits along it', () => {
    // The two rectangles are 140 px apart: 11 medium characters and some padding.
    let doc = pair();
    let typed = 0;
    for (;;) {
      const next = typeChar(doc, 3, 'x');
      if ('error' in next) break;
      doc = next.doc;
      typed++;
    }
    expect(typed).toBe(11);
    expect(line(doc).text).toBe('x'.repeat(11));
    expect(line(ok(backspace(doc, 3))).text).toBe('x'.repeat(10));
  });

  it('keeps a label from outgrowing its line with a bigger font', () => {
    let doc = pair();
    for (const ch of 'eight ch') doc = ok(typeChar(doc, 3, ch));
    expect(setFont(doc, 3, 'p')).toHaveProperty('error');
    expect(line(ok(setFont(doc, 3, 'i'))).f).toBe('i');
  });
});

describe('lines', () => {
  it('cycles the arrowheads: line, arrow, double arrow', () => {
    const heads: string[] = [];
    let doc = pair();
    for (let i = 0; i < 4; i++) {
      doc = ok(cycleHead(doc, 3));
      heads.push(line(doc).head);
    }
    // A new line is plain; the arrows are added on.
    expect(heads).toEqual(['end', 'both', 'none', 'end']);
  });

  it('cycles the pulse: off, start to end, end to start, from the middle', () => {
    const modes: string[] = [];
    let doc = pair();
    for (let i = 0; i < 5; i++) {
      doc = ok(cyclePulse(doc, 3));
      modes.push(line(doc).pulse);
    }
    expect(modes).toEqual(['fwd', 'rev', 'mid', 'off', 'fwd']);
  });

  it('switches between a ball and a light', () => {
    const light = ok(toggleGlow(pair(), 3));
    expect(line(light).glow).toBe('light');
    expect(line(ok(toggleGlow(light, 3))).glow).toBe('ball');
  });

  it('flips by swapping the ends', () => {
    const doc = ok(flip(pair(), 3));
    expect(line(doc).a).toEqual({ to: 2 });
    expect(line(doc).b).toEqual({ to: 1 });
  });

  it('is for lines only', () => {
    for (const f of [cycleHead, flip, cyclePulse, toggleGlow]) expect(f(pair(), 1)).toHaveProperty('error');
  });
});

describe('colour and deleting', () => {
  it('recolours shapes and lines, and says nothing changed by returning the same doc', () => {
    const doc = pair();
    expect(shapeById(ok(recolor(doc, 1, 6)), 1)!.c).toBe(6);
    expect(line(ok(recolor(doc, 3, 4))).c).toBe(4);
    expect(ok(recolor(doc, 1, 0))).toBe(doc);
    expect(recolor(doc, 99, 3)).toHaveProperty('error');
  });

  it('leaves a line where it ended when its shape is deleted', () => {
    const doc = remove(pair(), 1);
    expect(find(doc, 1)).toBeUndefined();
    const a = line(doc).a;
    expect('to' in a).toBe(false);
    // The edge of a 160-wide rectangle centred at x = 100, on the way to the other.
    expect((a as { x: number }).x).toBeCloseTo(180);
    expect((a as { y: number }).y).toBeCloseTo(100);
    expect(line(doc).b).toEqual({ to: 2 });
  });

  it('deletes a line without touching its shapes', () => {
    const doc = remove(pair(), 3);
    expect(doc.items.map((p) => p.id)).toEqual([1, 2]);
  });
});

describe('moving', () => {
  it('moves a shape and keeps it on the page', () => {
    // A 160×80 rectangle: its top edge can't go above the page.
    const doc = moveShape(pair(), 1, { x: 700, y: -5 });
    expect(shapeById(doc, 1)).toMatchObject({ x: 700, y: 40 });
    expect(shapeById(moveShape(pair(), 1, { x: 5000, y: 5000 }), 1)).toMatchObject({ x: 1120, y: 760 });
  });

  it('attaches a dragged end to the shape it is dropped on', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = addShape(doc, 'rect', { x: 400, y: 100 }).doc;
    doc = ok(addLine(doc, { x: 600, y: 600 }, { to: 1 }));
    expect(line(moveEnd(doc, 3, 'a', { x: 410, y: 100 })).a).toEqual({ to: 2 });
    expect(line(moveEnd(doc, 3, 'a', { x: 700, y: 650 })).a).toEqual({ x: 700, y: 650 });
  });

  it('will not put both ends on one shape', () => {
    const doc = moveEnd(pair(), 3, 'b', { x: 100, y: 100 });
    expect(line(doc).b).toEqual({ x: 100, y: 100 });
  });

  it('moves only the free ends of a dragged line', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = ok(addLine(doc, { to: 1 }, { x: 500, y: 500 }));
    const moved = line(moveLineBy(doc, 2, 30, -20), 2);
    expect(moved.a).toEqual({ to: 1 });
    expect(moved.b).toEqual({ x: 530, y: 480 });
  });
});

describe('selecting with tab', () => {
  it('walks the parts in order and wraps', () => {
    const doc = pair();
    expect(stepSelection(doc, null, 1)).toBe(1);
    expect(stepSelection(doc, null, -1)).toBe(3);
    expect(stepSelection(doc, 1, 1)).toBe(2);
    expect(stepSelection(doc, 3, 1)).toBe(1);
    expect(stepSelection(doc, 1, -1)).toBe(3);
    expect(stepSelection(blankDoc(), null, 1)).toBeNull();
  });
});

describe('describe', () => {
  it('summarises a shape with how full it is', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 300, y: 300 }).doc;
    doc = ok(typeChar(doc, 1, 'a'));
    expect(describePart(find(doc, 1)!)).toBe('rect · plain · medium 18px · 160×80 · 1/26 chars');
  });

  it('summarises a line with its pulse', () => {
    let doc = pair();
    expect(describePart(find(doc, 3)!)).toBe('line · plain · still');
    expect(describePart(find(ok(cycleHead(doc, 3)), 3)!)).toBe('arrow · plain · still');
    doc = ok(toggleGlow(ok(cyclePulse(doc, 3)), 3));
    expect(describePart(find(doc, 3)!)).toBe('line · plain · pulse a→b · light');
    expect(isLine(find(doc, 3)!)).toBe(true);
  });
});

describe('History', () => {
  it('goes back and forward', () => {
    const h = new History();
    const a = blankDoc();
    const b = addShape(a, 'rect', { x: 1, y: 1 }).doc;
    h.push(a);
    expect(h.canUndo).toBe(true);
    expect(h.undo(b)).toBe(a);
    expect(h.canUndo).toBe(false);
    expect(h.undo(a)).toBeNull();
    expect(h.redo(a)).toBe(b);
    expect(h.redo(b)).toBeNull();
  });

  it('forgets what was undone once something new is done', () => {
    const h = new History();
    const a = blankDoc();
    const b = addShape(a, 'rect', { x: 1, y: 1 }).doc;
    h.push(a);
    h.undo(b);
    expect(h.canRedo).toBe(true);
    h.push(a);
    expect(h.canRedo).toBe(false);
  });
});

describe('moving several at once', () => {
  it('moves shapes together and keeps them all on the page', () => {
    const moved = moveBy(pair(), [1, 2], 30, -10);
    expect(shapeById(moved, 1)).toMatchObject({ x: 130, y: 90 });
    expect(shapeById(moved, 2)).toMatchObject({ x: 430, y: 90 });
    // Shape 2 reaches the right edge first, so shape 1 stops the same distance along.
    const far = moveBy(pair(), [1, 2], 5000, 0);
    expect(shapeById(far, 2)!.x).toBe(1120);
    expect(shapeById(far, 1)!.x).toBe(820);
  });

  it('moves the free ends of a selected line, and leaves attached ends on their shapes', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = ok(addLine(doc, { to: 1 }, { x: 500, y: 500 }));
    const moved = moveBy(doc, [2], 25, 5);
    expect(line(moved, 2).a).toEqual({ to: 1 });
    expect(line(moved, 2).b).toEqual({ x: 525, y: 505 });
    expect(moveBy(doc, [2], 0, 0)).toBe(doc);
  });

  it('does a command to each part, and fails only if it worked on none', () => {
    const doc = pair();
    expect(shapeById(ok(each(doc, [1, 2], (d, i) => recolor(d, i, 4))), 2)!.c).toBe(4);
    // A resize is for shapes: the line in the selection is skipped.
    expect(shapeById(ok(each(doc, [1, 3], (d, i) => resize(d, i, 'right', false))), 1)!.w).toBe(170);
    expect(each(doc, [3], (d, i) => resize(d, i, 'right', false))).toEqual({ error: 'select a shape to resize it' });
    expect(each(doc, [], (d, i) => recolor(d, i, 1))).toHaveProperty('error');
  });
});

describe('outline and route', () => {
  it('cycles solid, dashed, dotted for shapes and lines', () => {
    let doc = pair();
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      doc = ok(cycleStroke(doc, 1));
      seen.push(shapeById(doc, 1)!.ln);
    }
    expect(seen).toEqual(['dash', 'dot', 'solid', 'dash']);
    expect(line(ok(cycleStroke(pair(), 3))).ln).toBe('dash');
    expect(cycleStroke(pair(), 99)).toHaveProperty('error');
  });

  it('cycles straight, curved, square for lines only', () => {
    let doc = pair();
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      doc = ok(cycleRoute(doc, 3));
      seen.push(line(doc).route);
    }
    expect(seen).toEqual(['curve', 'elbow', 'straight', 'curve']);
    expect(cycleRoute(pair(), 1)).toHaveProperty('error');
  });
});

describe('stacking order', () => {
  const order = (d: Doc) => d.items.map((p) => p.id);

  it('moves a step, or all the way', () => {
    expect(order(ok(reorder(pair(), [1], 'front')))).toEqual([2, 3, 1]);
    expect(order(ok(reorder(pair(), [3], 'back')))).toEqual([3, 1, 2]);
    expect(order(ok(reorder(pair(), [1], 'up')))).toEqual([2, 1, 3]);
    expect(order(ok(reorder(pair(), [2], 'down')))).toEqual([2, 1, 3]);
    expect(order(ok(reorder(pair(), [1, 2], 'front')))).toEqual([3, 1, 2]);
  });

  it('says when it is already there', () => {
    expect(reorder(pair(), [3], 'up')).toEqual({ error: 'already at the front' });
    expect(reorder(pair(), [1], 'back')).toEqual({ error: 'already at the back' });
    expect(reorder(pair(), [], 'front')).toHaveProperty('error');
  });
});

describe('copy, paste and duplicate', () => {
  it('copies parts, freeing a line end left on a shape that is not copied', () => {
    const parts = copyParts(pair(), [1, 3]);
    expect(parts.map((p) => p.id)).toEqual([1, 3]);
    const l = parts[1] as Line;
    expect(l.a).toEqual({ to: 1 });
    // The end on shape 2 stays where it was: the edge of the 160-wide rectangle at x = 400.
    expect((l.b as { x: number }).x).toBeCloseTo(320);
    expect((l.b as { y: number }).y).toBeCloseTo(100);
  });

  it('pastes new parts, joining the lines between pasted shapes', () => {
    const doc = pair();
    const r = pasteParts(doc, copyParts(doc, [1, 2, 3]), { x: 0, y: 200 });
    expect(r.ids).toEqual([4, 5, 6]);
    expect(shapeById(r.doc, 4)).toMatchObject({ x: 100, y: 300 });
    expect(line(r.doc, 6).a).toEqual({ to: 4 });
    expect(line(r.doc, 6).b).toEqual({ to: 5 });
    expect(r.doc.next).toBe(7);
    // The original is untouched.
    expect(line(r.doc, 3).a).toEqual({ to: 1 });
  });

  it('keeps pasted shapes on the page', () => {
    const doc = pair();
    const r = pasteParts(doc, copyParts(doc, [2]), { x: 5000, y: 0 });
    expect(shapeById(r.doc, r.ids[0])!.x).toBe(1120);
  });

  it('duplicates a little way off and selects the copy', () => {
    const r = duplicate(pair(), [1]) as { doc: Doc; ids: number[] };
    expect(r.ids).toEqual([4]);
    expect(shapeById(r.doc, 4)).toMatchObject({ x: 120, y: 120 });
    expect(duplicate(pair(), [])).toHaveProperty('error');
  });

  it('duplicates a lone line as a second line between the same shapes', () => {
    const r = duplicate(pair(), [3]) as { doc: Doc; ids: number[] };
    expect(line(r.doc, 4).a).toEqual({ to: 1 });
    expect(line(r.doc, 4).b).toEqual({ to: 2 });
  });

  it('deletes several parts', () => {
    const doc = removeMany(pair(), [1, 3]);
    expect(doc.items.map((p) => p.id)).toEqual([2]);
  });
});
