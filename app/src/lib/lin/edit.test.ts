// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import {
  History,
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

  it('is for shapes only', () => {
    expect(typeChar(pair(), 3, 'a')).toHaveProperty('error');
    expect(setFont(pair(), 3, 'p')).toHaveProperty('error');
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
    // A new line is an arrow already.
    expect(heads).toEqual(['both', 'none', 'end', 'both']);
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
    expect(describePart(find(doc, 3)!)).toBe('arrow · plain · still');
    doc = ok(toggleGlow(ok(cyclePulse(doc, 3)), 3));
    expect(describePart(find(doc, 3)!)).toBe('arrow · plain · pulse a→b · light');
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
