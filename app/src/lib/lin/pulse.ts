// Claude — Date 11/05/2026
// Where the pulses on a line are at a moment in time. A line pulses at a
// steady speed, so a long line takes longer than a short one, then rests a
// moment before it goes again.
//
// Positions are fractions along the line: 0 at its start (a), 1 at its end
// (b). A ball travels exactly from 0 to 1. A light is longer than a point, so
// it starts and ends *off* the line, by `overshoot` pixels, to slide in and
// out of it rather than pop.

import type { Pulse } from './model';

/** Pixels per second. */
export const SPEED = 180;
/** Seconds of rest between one run and the next. */
export const GAP = 0.5;
/** Length of the light, in pixels. */
export const LIGHT_LEN = 56;

/** The positions of the pulses on a line `len` pixels long at time `t` seconds. */
export function pulseAt(mode: Pulse, t: number, len: number, overshoot = 0): number[] {
  if (mode === 'off' || !(len > 0) || !(t >= 0)) return [];
  const o = overshoot / len;
  if (mode === 'mid') {
    // From the middle to each end.
    const run = (len / 2 + overshoot) / SPEED;
    const phase = t % (run + GAP);
    if (phase >= run) return [];
    const reach = (phase / run) * (0.5 + o);
    return [0.5 - reach, 0.5 + reach];
  }
  const run = (len + 2 * overshoot) / SPEED;
  const phase = t % (run + GAP);
  if (phase >= run) return [];
  const u = -o + (phase / run) * (1 + 2 * o);
  return [mode === 'fwd' ? u : 1 - u];
}
