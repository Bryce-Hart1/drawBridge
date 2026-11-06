// Claude — Date 11/06/2026
// Helping a drag land where it's meant to: a shape's edges and middle line up
// with another shape's (and a guide shows it), and, with the grid on (`g`),
// whatever isn't lined up falls to the nearest 10 px. Pure, so it's tested
// without a canvas.

import type { Pt } from './geometry';
import { GRID, isShape, type Doc } from './model';

/** How close, in page pixels, an edge has to be to another's to snap to it. */
export const SNAP = 6;

/** A line to draw while something is lined up: at `at` on `axis`, from `from` to `to` along the other. */
export interface Guide {
  axis: 'x' | 'y';
  at: number;
  from: number;
  to: number;
}

export const toGrid = (v: number) => Math.round(v / GRID) * GRID;

export function snapPoint(p: Pt, grid: boolean): Pt {
  return grid ? { x: toGrid(p.x), y: toGrid(p.y) } : p;
}

/**
 * Where shape `id` lands if its centre is dropped at `centre`: nudged so an
 * edge or the middle meets another shape's, else (with the grid on) onto the
 * grid. `guides` are the alignments found.
 */
export function snapShape(doc: Doc, id: number, centre: Pt, grid: boolean): { at: Pt; guides: Guide[] } {
  const me = doc.items.find((p) => p.id === id);
  if (!me || !isShape(me)) return { at: centre, guides: [] };
  const others = doc.items.filter((p) => p.id !== id && isShape(p));
  const guides: Guide[] = [];

  const fit = (axis: 'x' | 'y'): number => {
    const c = centre[axis];
    const half = (axis === 'x' ? me.w : me.h) / 2;
    const mine = [c - half, c, c + half];
    let best: { d: number; at: number; other: (typeof others)[number] } | null = null;
    for (const o of others) {
      if (!isShape(o)) continue;
      const oc = o[axis];
      const oh = (axis === 'x' ? o.w : o.h) / 2;
      for (const theirs of [oc - oh, oc, oc + oh]) {
        for (const m of mine) {
          const d = theirs - m;
          if (Math.abs(d) <= SNAP && (best === null || Math.abs(d) < Math.abs(best.d))) best = { d, at: theirs, other: o };
        }
      }
    }
    if (!best || !isShape(best.other)) return grid ? toGrid(c) : c;
    const o = best.other;
    const moved = c + best.d;
    const cross = axis === 'x' ? 'y' : 'x';
    const size = (s: { w: number; h: number }) => (cross === 'y' ? s.h : s.w) / 2;
    const from = Math.min(o[cross] - size(o), centre[cross] - size(me));
    const to = Math.max(o[cross] + size(o), centre[cross] + size(me));
    guides.push({ axis, at: best.at, from, to });
    return moved;
  };

  return { at: { x: fit('x'), y: fit('y') }, guides };
}
