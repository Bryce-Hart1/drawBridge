// Claude — Date 11/06/2026
import { describe, expect, it } from 'vitest';
import { addShape } from './edit';
import { blankDoc, type Doc } from './model';
import { SNAP, snapPoint, snapShape, toGrid } from './snap';

/** 1 at (100,100), 2 at (400,300); both 160×80. */
function two(): Doc {
  let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
  doc = addShape(doc, 'rect', { x: 400, y: 300 }).doc;
  return doc;
}

describe('the grid', () => {
  it('rounds to the nearest 10', () => {
    expect(toGrid(14)).toBe(10);
    expect(toGrid(15)).toBe(20);
    expect(snapPoint({ x: 14, y: 26 }, true)).toEqual({ x: 10, y: 30 });
    expect(snapPoint({ x: 14, y: 26 }, false)).toEqual({ x: 14, y: 26 });
  });
});

describe('lining up with other shapes', () => {
  it('snaps a middle to another shape’s middle and says so', () => {
    const r = snapShape(two(), 2, { x: 403, y: 103 }, false);
    expect(r.at).toEqual({ x: 403, y: 100 });
    expect(r.guides).toHaveLength(1);
    // Same-size shapes line up by top, middle and bottom all at once; any of them is a fine guide.
    expect(r.guides[0].axis).toBe('y');
    expect([60, 100, 140]).toContain(r.guides[0].at);
  });

  it('snaps an edge to an edge', () => {
    // Shape 2's left edge at 182 is 2 px from shape 1's right edge at 180.
    const r = snapShape(two(), 2, { x: 262, y: 500 }, false);
    expect(r.at.x).toBe(260);
    expect(r.guides[0]).toMatchObject({ axis: 'x', at: 180 });
  });

  it('leaves it be when nothing is near', () => {
    const r = snapShape(two(), 2, { x: 700, y: 650 }, false);
    expect(r).toEqual({ at: { x: 700, y: 650 }, guides: [] });
    expect(SNAP).toBeGreaterThan(0);
  });

  it('falls back to the grid on the axis that did not line up', () => {
    const r = snapShape(two(), 2, { x: 413, y: 103 }, true);
    expect(r.at).toEqual({ x: 410, y: 100 });
  });

  it('ignores the shape being moved and anything that is not a shape', () => {
    expect(snapShape(two(), 1, { x: 103, y: 103 }, false).at).toEqual({ x: 103, y: 103 });
    expect(snapShape(two(), 99, { x: 5, y: 5 }, true)).toEqual({ at: { x: 5, y: 5 }, guides: [] });
  });
});
