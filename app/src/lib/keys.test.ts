// Claude — Date 11/03/2026
import { describe, expect, it } from 'vitest';
import { HINTS, actionFor, hintRest } from './keys';

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
  actionFor({ key: k, metaKey: false, ctrlKey: false, altKey: false, ...mods });

describe('keys', () => {
  it('maps arrows and hjkl alike', () => {
    expect(key('ArrowDown')).toBe('down');
    expect(key('j')).toBe('down');
    expect(key('h')).toBe('left');
    expect(key('Enter')).toBe('open');
    expect(key('Backspace')).toBe('back');
  });

  it('leaves system chords alone', () => {
    expect(key('q', { metaKey: true })).toBeNull();
    expect(key('w', { metaKey: true })).toBeNull();
    expect(key('f', { metaKey: true })).toBe('find');
    expect(key(',', { metaKey: true })).toBe('settings');
    expect(key(',')).toBe('settings');
    expect(key('e')).toBe('external');
    expect(key('d', { altKey: true })).toBeNull();
    expect(key('z')).toBeNull();
  });

  it('has a key for every hint', () => {
    for (const h of HINTS) expect(key(h.key)).toBe(h.action);
    expect(hintRest(HINTS[0])).toBe('pload');
    expect(hintRest({ key: 'o', word: 'save', action: 'save' })).toBe('save');
  });
});
