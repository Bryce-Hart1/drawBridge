// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import { GAP, LIGHT_LEN, SPEED, pulseAt } from './pulse';

// A line 180 px long at 180 px/s takes exactly a second to cross.
const LEN = SPEED;

describe('pulseAt', () => {
  it('does nothing when off or when there is no line', () => {
    expect(pulseAt('off', 0.3, LEN)).toEqual([]);
    expect(pulseAt('fwd', 0.3, 0)).toEqual([]);
  });

  it('runs from the start to the end', () => {
    expect(pulseAt('fwd', 0, LEN)).toEqual([0]);
    expect(pulseAt('fwd', 0.5, LEN)).toEqual([0.5]);
    expect(pulseAt('fwd', 0.75, LEN)[0]).toBeCloseTo(0.75);
  });

  it('runs the other way', () => {
    expect(pulseAt('rev', 0, LEN)).toEqual([1]);
    expect(pulseAt('rev', 0.25, LEN)[0]).toBeCloseTo(0.75);
  });

  it('rests between runs, then goes again', () => {
    expect(pulseAt('fwd', 1, LEN)).toEqual([]);
    expect(pulseAt('fwd', 1 + GAP - 0.01, LEN)).toEqual([]);
    expect(pulseAt('fwd', 1 + GAP, LEN)).toEqual([0]);
  });

  it('goes out from the middle both ways', () => {
    // Half the line takes half a second.
    expect(pulseAt('mid', 0, LEN)).toEqual([0.5, 0.5]);
    const [a, b] = pulseAt('mid', 0.25, LEN);
    expect(a).toBeCloseTo(0.25);
    expect(b).toBeCloseTo(0.75);
    expect(pulseAt('mid', 0.5, LEN)).toEqual([]);
  });

  it('takes longer on a longer line', () => {
    expect(pulseAt('fwd', 1.5, LEN * 2)[0]).toBeCloseTo(0.75);
  });

  it('lets a light start and finish off the line', () => {
    const before = pulseAt('fwd', 0, LEN, LIGHT_LEN)[0];
    expect(before).toBeCloseTo(-LIGHT_LEN / LEN);
    const run = (LEN + 2 * LIGHT_LEN) / SPEED;
    expect(pulseAt('fwd', run / 2, LEN, LIGHT_LEN)[0]).toBeCloseTo(0.5);
    expect(pulseAt('fwd', run - 0.001, LEN, LIGHT_LEN)[0]).toBeGreaterThan(1);
  });
});
