// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import {
  bounds,
  capacity,
  fits,
  grid,
  hitTest,
  inShape,
  lineEnds,
  outlinePoint,
  snapTarget,
  textBox,
  wrap,
} from './geometry';
import { blankDoc, newLine, newShape, type Doc, type FontKey, type Shape, type ShapeKind } from './model';

const shape = (t: ShapeKind, x: number, y: number, w: number, h: number, f: FontKey = 'o', text = ''): Shape => ({
  ...newShape(1, t, x, y),
  w,
  h,
  f,
  text,
});

const docOf = (...items: Doc['items']): Doc => ({ ...blankDoc(), next: 100, items });

describe('how much text fits', () => {
  it('counts characters per line and lines in a rectangle', () => {
    // 160×80 less 6 px padding all round is 148×68; 18 px type is 10.8 wide, 23.4 tall.
    expect(grid(shape('rect', 0, 0, 160, 80, 'o'))).toEqual({ cols: 13, rows: 2 });
    expect(capacity(shape('rect', 0, 0, 160, 80, 'o'))).toBe(26);
    expect(capacity(shape('rect', 0, 0, 160, 80, 'i'))).toBe(80);
    expect(capacity(shape('rect', 0, 0, 160, 80, 'p'))).toBe(8);
  });

  it('holds less in a bigger font and more in a bigger shape', () => {
    const at = (w: number, h: number, f: FontKey) => capacity(shape('rect', 0, 0, w, h, f));
    expect(at(160, 80, 'i')).toBeGreaterThan(at(160, 80, 'o'));
    expect(at(160, 80, 'o')).toBeGreaterThan(at(160, 80, 'p'));
    expect(at(320, 160, 'o')).toBeGreaterThan(at(160, 80, 'o'));
  });

  it('holds less in the curved shapes than a rectangle of the same size', () => {
    const cap = (t: ShapeKind) => capacity(shape(t, 0, 0, 120, 100, 'i'));
    expect(cap('circle')).toBeLessThan(cap('rect'));
    expect(cap('diamond')).toBeLessThan(cap('circle'));
    expect(cap('triangle')).toBe(cap('diamond'));
  });

  it('sets a triangle\'s text low, on its base', () => {
    expect(textBox(shape('triangle', 0, 0, 120, 100)).dy).toBe(25);
    expect(textBox(shape('rect', 0, 0, 120, 100)).dy).toBe(0);
  });

  it('never goes below nothing', () => {
    expect(capacity(shape('diamond', 0, 0, 24, 24, 'p'))).toBe(0);
  });

  it('stops at 200 characters however big the shape', () => {
    expect(capacity(shape('rect', 0, 0, 800, 800, 'i'))).toBe(200);
  });
});

describe('wrap', () => {
  it('breaks at spaces', () => {
    expect(wrap('hello world', 5)).toEqual(['hello', 'world']);
    expect(wrap('hello world foo', 11)).toEqual(['hello world', 'foo']);
  });

  it('breaks inside a word longer than a line', () => {
    expect(wrap('abcdefghij', 4)).toEqual(['abcd', 'efgh', 'ij']);
    expect(wrap('ab abcdefgh', 4)).toEqual(['ab', 'abcd', 'efgh']);
  });

  it('counts a run of spaces as one', () => {
    expect(wrap('a   b', 10)).toEqual(['a b']);
    expect(wrap('', 5)).toEqual([]);
    expect(wrap('   ', 5)).toEqual([]);
  });
});

describe('fits', () => {
  const rect = (text: string, f: FontKey = 'o') => shape('rect', 0, 0, 160, 80, f, text);

  it('is true while the wrapped text is within the rows', () => {
    expect(fits(rect(''))).toBe(true);
    expect(fits(rect('hello world foo bar baz'))).toBe(true);
    expect(fits(rect('hello world foo bar baz qux'))).toBe(false);
  });

  it('depends on the font', () => {
    expect(fits(rect('hello world foo', 'i'))).toBe(true);
    expect(fits(rect('hello world foo', 'p'))).toBe(false);
  });

  it('is false for text in a shape with no room', () => {
    expect(fits(shape('diamond', 0, 0, 24, 24, 'o', 'x'))).toBe(false);
    expect(fits(shape('diamond', 0, 0, 24, 24, 'o', ''))).toBe(true);
  });
});

describe('outlines', () => {
  const near = (p: { x: number; y: number }, x: number, y: number) => {
    expect(p.x).toBeCloseTo(x, 6);
    expect(p.y).toBeCloseTo(y, 6);
  };

  it('finds where a ray leaves a rectangle', () => {
    const r = shape('rect', 0, 0, 100, 50);
    near(outlinePoint(r, { x: 200, y: 0 }), 50, 0);
    near(outlinePoint(r, { x: 0, y: 100 }), 0, 25);
    near(outlinePoint(r, { x: 100, y: 50 }), 50, 25);
  });

  it('finds it for a circle, a diamond and a triangle', () => {
    near(outlinePoint(shape('circle', 0, 0, 100, 100), { x: 100, y: 0 }), 50, 0);
    near(outlinePoint(shape('circle', 0, 0, 100, 100), { x: 100, y: 100 }), 35.355339, 35.355339);
    near(outlinePoint(shape('diamond', 0, 0, 100, 100), { x: 100, y: 100 }), 25, 25);
    const tri = shape('triangle', 0, 0, 100, 100);
    near(outlinePoint(tri, { x: 0, y: 100 }), 0, 50);
    near(outlinePoint(tri, { x: 0, y: -100 }), 0, -50);
    near(outlinePoint(tri, { x: 100, y: 0 }), 25, 0);
  });

  it('works from wherever the shape is, and stays put when pointed at itself', () => {
    const r = shape('rect', 100, 100, 100, 50);
    near(outlinePoint(r, { x: 300, y: 100 }), 150, 100);
    near(outlinePoint(r, { x: 100, y: 100 }), 100, 100);
  });

  it('knows what is inside', () => {
    const tri = shape('triangle', 0, 0, 100, 100);
    expect(inShape(tri, { x: 0, y: 40 })).toBe(true);
    expect(inShape(tri, { x: 0, y: -49 })).toBe(true);
    expect(inShape(tri, { x: 45, y: -40 })).toBe(false);
    expect(inShape(shape('circle', 0, 0, 100, 100), { x: 40, y: 40 })).toBe(false);
    expect(inShape(shape('circle', 0, 0, 100, 100), { x: 40, y: 40 }, 10)).toBe(true);
    expect(inShape(shape('diamond', 0, 0, 100, 100), { x: 40, y: 40 })).toBe(false);
  });
});

describe('lines', () => {
  it('stops attached ends at the shape\'s edge and follows it', () => {
    const a = { ...shape('rect', 0, 0, 100, 100), id: 1 };
    const b = { ...shape('rect', 300, 0, 100, 100), id: 2 };
    const line = newLine(3, { to: 1 }, { to: 2 });
    const ends = lineEnds(docOf(a, b, line), line);
    expect(ends.a.x).toBeCloseTo(50);
    expect(ends.a.y).toBeCloseTo(0);
    expect(ends.b.x).toBeCloseTo(250);
    expect(ends.b.y).toBeCloseTo(0);

    const moved = { ...b, y: 300, x: 0 };
    const after = lineEnds(docOf(a, moved, line), line);
    expect(after.a.y).toBeCloseTo(50);
    expect(after.b.y).toBeCloseTo(250);
  });

  it('keeps a free end where it is', () => {
    const a = { ...shape('circle', 0, 0, 100, 100), id: 1 };
    const line = newLine(2, { to: 1 }, { x: 200, y: 0 });
    const ends = lineEnds(docOf(a, line), line);
    expect(ends.a.x).toBeCloseTo(50);
    expect(ends.b).toEqual({ x: 200, y: 0 });
  });
});

describe('what a click lands on', () => {
  const back = { ...shape('rect', 100, 100, 200, 200), id: 1 };
  const front = { ...shape('rect', 150, 100, 100, 100), id: 2 };

  it('picks the front-most shape', () => {
    expect(hitTest(docOf(back, front), { x: 150, y: 100 }, null)).toEqual({ kind: 'part', id: 2 });
    expect(hitTest(docOf(back, front), { x: 40, y: 100 }, null)).toEqual({ kind: 'part', id: 1 });
    expect(hitTest(docOf(back, front), { x: 900, y: 900 }, null)).toBeNull();
  });

  it('picks a line within a few pixels of it', () => {
    const line = newLine(3, { x: 400, y: 400 }, { x: 600, y: 400 });
    const doc = docOf(line);
    expect(hitTest(doc, { x: 500, y: 405 }, null)).toEqual({ kind: 'part', id: 3 });
    expect(hitTest(doc, { x: 500, y: 420 }, null)).toBeNull();
  });

  it('prefers the end handles of the selected line', () => {
    const line = newLine(3, { x: 400, y: 400 }, { x: 600, y: 400 });
    const doc = docOf(line);
    expect(hitTest(doc, { x: 598, y: 402 }, 3)).toEqual({ kind: 'end', id: 3, which: 'b' });
    expect(hitTest(doc, { x: 402, y: 398 }, 3)).toEqual({ kind: 'end', id: 3, which: 'a' });
    // Not selected: it's just the line.
    expect(hitTest(doc, { x: 598, y: 402 }, null)).toEqual({ kind: 'part', id: 3 });
  });

  it('finds the shape a dropped line end attaches to', () => {
    // Just outside both, within the margin of both: the front one wins.
    expect(snapTarget(docOf(back, front), { x: 205, y: 100 })?.id).toBe(2);
    expect(snapTarget(docOf(back, front), { x: 215, y: 100 })).toBeNull();
    expect(snapTarget(docOf(back, front), { x: 600, y: 600 })).toBeNull();
  });
});

describe('bounds', () => {
  it('is nothing for an empty page', () => {
    expect(bounds(blankDoc())).toBeNull();
  });

  it('surrounds shapes and line ends', () => {
    const a = { ...shape('rect', 100, 100, 100, 50), id: 1 };
    const line = newLine(2, { to: 1 }, { x: 400, y: 300 });
    expect(bounds(docOf(a, line))).toEqual({ x: 50, y: 75, w: 350, h: 225 });
  });
});
