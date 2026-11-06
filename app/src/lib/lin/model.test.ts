// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import { FONT_PX, PALETTE, TINT, newLine, newShape, tint } from './model';

describe('palette', () => {
  it('has ten colours for the keys 0 to 9', () => {
    expect(PALETTE).toHaveLength(10);
    for (const c of PALETTE) expect(c.fill).toMatch(/^#[0-9a-f]{6}$/);
    expect(new Set(PALETTE.map((c) => c.fill)).size).toBe(10);
    expect(new Set(PALETTE.map((c) => c.name)).size).toBe(10);
  });

  it('tints a colour to a see-through wash', () => {
    expect(tint('#e5484d', 0.5)).toBe('rgba(229, 72, 77, 0.5)');
    expect(tint('#000000')).toBe(`rgba(0, 0, 0, ${TINT})`);
    // A wash: well under half strength, so what's behind shows through.
    expect(TINT).toBeGreaterThan(0);
    expect(TINT).toBeLessThan(0.5);
  });
});

describe('fonts', () => {
  it('has three sizes, small to large', () => {
    expect(FONT_PX.i).toBeLessThan(FONT_PX.o);
    expect(FONT_PX.o).toBeLessThan(FONT_PX.p);
  });
});

describe('new parts', () => {
  it('start plain, medium and empty', () => {
    expect(newShape(1, 'rect', 10, 20)).toMatchObject({ id: 1, x: 10, y: 20, c: 0, f: 'o', text: '' });
    expect(newShape(1, 'circle', 0, 0)).toMatchObject({ w: 100, h: 100 });
  });

  it('start a line plain and still, with the arrowheads to be added', () => {
    expect(newLine(2, { to: 1 }, { x: 5, y: 5 })).toMatchObject({ head: 'none', pulse: 'off', glow: 'ball', c: 0, route: 'straight', ln: 'solid', text: '' });
    expect(newShape(1, 'rect', 0, 0).ln).toBe('solid');
  });
});
