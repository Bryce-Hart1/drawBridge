// Claude — Date 11/06/2026
import { describe, expect, it } from 'vitest';
import { addLine, addShape, cycleRoute, typeChar, type Edited } from './edit';
import {
  FAN,
  hitTest,
  labelCapacity,
  labelFits,
  lineEnds,
  linePath,
  lineLength,
  partsIn,
  pathLength,
  pointAt,
  subPath,
  tipFrom,
  bounds,
} from './geometry';
import { blankDoc, find, newShape, type Doc, type Line } from './model';
import { fits, layout, wrap } from './geometry';

function ok(e: Edited): Doc {
  if ('error' in e) throw new Error(`refused: ${e.error}`);
  return e.doc;
}

/** Two 160×80 rectangles, 1 at (100,100) and 2 at (400,100). */
function two(): Doc {
  let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
  doc = addShape(doc, 'rect', { x: 400, y: 100 }).doc;
  return doc;
}

const joined = (doc: Doc, a: number, b: number) => {
  const r = addLine(doc, { to: a }, { to: b });
  if ('error' in r) throw new Error(r.error);
  return r.doc;
};
const line = (doc: Doc, id: number) => find(doc, id) as Line;

describe('lines between the same two shapes', () => {
  it('a lone line runs straight between the edges', () => {
    const doc = joined(two(), 1, 2);
    const { a, b } = lineEnds(doc, line(doc, 3));
    expect(a.x).toBeCloseTo(180);
    expect(a.y).toBeCloseTo(100);
    expect(b.x).toBeCloseTo(320);
    expect(b.y).toBeCloseTo(100);
  });

  it('shows one from x to y and one from y to x as two separate lines', () => {
    const doc = joined(joined(two(), 1, 2), 2, 1);
    const one = lineEnds(doc, line(doc, 3));
    const back = lineEnds(doc, line(doc, 4));
    // Side by side, FAN apart, each still meeting both shapes.
    expect(one.a.y).toBeCloseTo(100 - FAN / 2);
    expect(back.a.y).toBeCloseTo(100 + FAN / 2);
    expect(one.a.x).toBeCloseTo(180);
    expect(one.b.x).toBeCloseTo(320);
    // The second one starts on shape 2 and ends on shape 1.
    expect(back.a.x).toBeCloseTo(320);
    expect(back.b.x).toBeCloseTo(180);
    expect(back.b.y).toBeCloseTo(100 + FAN / 2);
  });

  it('spreads three the same way, the middle one on the centre line', () => {
    let doc = joined(two(), 1, 2);
    doc = joined(doc, 1, 2);
    doc = joined(doc, 2, 1);
    const ys = [3, 4, 5].map((id) => lineEnds(doc, line(doc, id)).a.y);
    expect(ys[0]).toBeCloseTo(100 - FAN);
    expect(ys[1]).toBeCloseTo(100);
    expect(ys[2]).toBeCloseTo(100 + FAN);
  });

  it('never spreads wider than the smaller shape', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = addShape(doc, 'rect', { x: 400, y: 100 }).doc;
    // Make shape 1 tiny: 24 px tall, so 0.7 of that is room for the spread.
    doc = { ...doc, items: doc.items.map((p) => (p.id === 1 ? { ...p, h: 24 } : p)) };
    doc = joined(joined(doc, 1, 2), 2, 1);
    const gap = Math.abs(lineEnds(doc, line(doc, 3)).a.y - lineEnds(doc, line(doc, 4)).b.y);
    expect(gap).toBeCloseTo(0.85 * 24);
  });

  it('spreads them further apart when they carry labels, so each can be read', () => {
    let doc = joined(joined(two(), 1, 2), 2, 1);
    const gap = () => Math.abs(lineEnds(doc, line(doc, 3)).a.y - lineEnds(doc, line(doc, 4)).b.y);
    expect(gap()).toBeCloseTo(FAN);
    // A medium label is 27 px tall: the lines go at least that far apart, plus a little air.
    doc = { ...doc, items: doc.items.map((p) => (p.id === 3 ? { ...p, text: 'login' } : p)) };
    expect(gap()).toBeCloseTo(27 + 6);
    // A small one needs less.
    doc = { ...doc, items: doc.items.map((p) => (p.id === 3 ? { ...p, f: 'i' as const } : p)) };
    expect(gap()).toBeCloseTo(FAN);
  });

  it('leaves lines to different shapes, and free-ended lines, alone', () => {
    let doc = addShape(two(), 'rect', { x: 700, y: 100 }).doc;
    doc = joined(joined(doc, 1, 2), 2, 3);
    expect(lineEnds(doc, line(doc, 4)).a.y).toBeCloseTo(100);
    expect(lineEnds(doc, line(doc, 5)).a.y).toBeCloseTo(100);
  });
});

describe('routes', () => {
  const routed = (route: number) => {
    let doc = joined(two(), 1, 2);
    for (let i = 0; i < route; i++) doc = ok(cycleRoute(doc, 3));
    return doc;
  };

  it('bows a curve away from the straight line without moving its ends', () => {
    const straight = linePath(routed(0), line(routed(0), 3));
    const curve = linePath(routed(1), line(routed(1), 3));
    expect(straight).toHaveLength(2);
    expect(curve.length).toBeGreaterThan(10);
    expect(curve[0].x).toBeCloseTo(straight[0].x);
    expect(curve[curve.length - 1].x).toBeCloseTo(straight[1].x);
    const mid = pointAt(curve, pathLength(curve) / 2);
    expect(Math.abs(mid.y - 100)).toBeGreaterThan(10);
    expect(pathLength(curve)).toBeGreaterThan(pathLength(straight));
  });

  it('routes an elbow with square corners, leaving the facing sides', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = addShape(doc, 'rect', { x: 400, y: 200 }).doc;
    doc = ok(cycleRoute(ok(cycleRoute(joined(doc, 1, 2), 3)), 3));
    const path = linePath(doc, line(doc, 3));
    expect(path).toHaveLength(4);
    expect(path[0]).toEqual({ x: 180, y: 100 });
    expect(path[1].x).toBeCloseTo(path[2].x);
    expect(path[1].y).toBeCloseTo(100);
    expect(path[2].y).toBeCloseTo(200);
    expect(path[3].x).toBeCloseTo(320);
    expect(path[3].y).toBeCloseTo(200);
  });

  it('runs an elbow up and down first when the shapes are more above than beside', () => {
    let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
    doc = addShape(doc, 'rect', { x: 150, y: 400 }).doc;
    doc = ok(cycleRoute(ok(cycleRoute(joined(doc, 1, 2), 3)), 3));
    const path = linePath(doc, line(doc, 3));
    expect(path[0]).toEqual({ x: 100, y: 140 });
    expect(path[path.length - 1]).toEqual({ x: 150, y: 360 });
    expect(path[1].x).toBeCloseTo(100);
  });

  it('keeps two elbows between the same shapes apart', () => {
    let doc = joined(joined(two(), 1, 2), 2, 1);
    for (const id of [3, 4]) doc = ok(cycleRoute(ok(cycleRoute(doc, id)), id));
    const ys = [3, 4].map((id) => lineEnds(doc, line(doc, id)).a.y);
    expect(Math.abs(ys[0] - ys[1])).toBeCloseTo(FAN);
  });
});

describe('walking a path', () => {
  const path = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }];

  it('measures it and finds points along it', () => {
    expect(pathLength(path)).toBe(20);
    expect(pointAt(path, 5)).toEqual({ x: 5, y: 0 });
    expect(pointAt(path, 15)).toEqual({ x: 10, y: 5 });
  });

  it('carries straight on past either end, for a light sliding in', () => {
    expect(pointAt(path, -5)).toEqual({ x: -5, y: 0 });
    expect(pointAt(path, 25)).toEqual({ x: 10, y: 15 });
  });

  it('cuts out a stretch, corners included', () => {
    expect(subPath(path, 5, 15)).toEqual([{ x: 5, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 5 }]);
  });

  it('says where it is heading at either tip', () => {
    expect(tipFrom(path, true)).toEqual({ x: 10, y: 0 });
    expect(tipFrom(path, false)).toEqual({ x: 10, y: 0 });
  });
});

describe('labels on lines', () => {
  it('fit within the length of the line', () => {
    let doc = joined(two(), 1, 2);
    // 140 px between the rectangles: (140 - 12) / 10.8 = 11 characters.
    expect(lineLength(doc, line(doc, 3))).toBeCloseTo(140);
    expect(labelCapacity(doc, line(doc, 3))).toBe(11);
    doc = ok(typeChar(doc, 3, 'a'));
    expect(labelFits(doc, line(doc, 3))).toBe(true);
    expect(labelFits(doc, { ...line(doc, 3), text: 'x'.repeat(12) })).toBe(false);
  });

  it('never exceed the cap, however long the line', () => {
    const doc = joined({ ...two(), items: two().items.map((p) => (p.id === 2 ? { ...p, x: 1100 } : p)) }, 1, 2);
    expect(labelCapacity(doc, line(doc, 3))).toBe(40);
  });
});

describe('clicking and boxing a curve or a group', () => {
  it('hits a curve on the curve, not on the chord it left', () => {
    const doc = ok(cycleRoute(joined(two(), 1, 2), 3));
    const path = linePath(doc, line(doc, 3));
    const on = pointAt(path, pathLength(path) / 2);
    expect(hitTest(doc, on, null)).toEqual({ kind: 'part', id: 3 });
    expect(hitTest(doc, { x: 250, y: 100 }, null)).toBeNull();
  });

  it('reaches further with a bigger slop', () => {
    const doc = joined(two(), 1, 2);
    expect(hitTest(doc, { x: 250, y: 120 }, null)).toBeNull();
    expect(hitTest(doc, { x: 250, y: 120 }, null, 3)).toEqual({ kind: 'part', id: 3 });
  });

  it('boxes what it touches', () => {
    const doc = joined(two(), 1, 2);
    expect(partsIn(doc, { x: 0, y: 0, w: 150, h: 200 })).toEqual([1]);
    expect(partsIn(doc, { x: 0, y: 0, w: 1200, h: 800 })).toEqual([1, 2, 3]);
    // A box across the middle of the line catches the line, not the shapes.
    expect(partsIn(doc, { x: 230, y: 90, w: 40, h: 20 })).toEqual([3]);
    expect(partsIn(doc, { x: 600, y: 500, w: 50, h: 50 })).toEqual([]);
  });

  it('bounds a chosen few', () => {
    const doc = joined(two(), 1, 2);
    expect(bounds(doc, [2])).toEqual({ x: 320, y: 60, w: 160, h: 80 });
    expect(bounds(doc, [])).toBeNull();
  });
});

describe('lists: a line break in a shape', () => {
  const rect = (text: string) => ({ ...newShape(1, 'rect', 300, 300), text });

  it('starts a new line at each break, even when the words would fit on one', () => {
    expect(wrap('one\ntwo\nthree', 20)).toEqual(['one', 'two', 'three']);
    expect(wrap('a b\nc d', 20)).toEqual(['a b', 'c d']);
  });

  it('keeps a blank line between items, and one after a trailing break', () => {
    expect(wrap('one\n\ntwo', 20)).toEqual(['one', '', 'two']);
    expect(wrap('one\n', 20)).toEqual(['one', '']);
    expect(wrap('\n', 20)).toEqual(['', '']);
  });

  it('still wraps a long item on its own', () => {
    expect(wrap('aaaa bbbb\ncc', 5)).toEqual(['aaaa', 'bbbb', 'cc']);
  });

  it('counts every row against the room in the shape', () => {
    // A 160×80 rectangle holds 2 rows at the medium font.
    expect(layout(rect('a\nb'))).toEqual(['a', 'b']);
    expect(fits(rect('a\nb'))).toBe(true);
    expect(fits(rect('a\nb\nc'))).toBe(false);
    // Breaks alone take up rows too.
    expect(fits(rect('\n\n'))).toBe(false);
  });
});
