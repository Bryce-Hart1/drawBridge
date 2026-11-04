// Claude — Date 11/03/2026
import { describe, expect, it } from 'vitest';
import { bar, baseMime, fit, hasThumb, human, isText, percent, sizeLabel, splitName } from './format';
import type { Item } from './types';

const file = (over: Partial<Item>): Item => ({
  id: 'e',
  name: 'x',
  kind: 'file',
  size: 10,
  mime: 'application/octet-stream',
  blob: 'b',
  count: null,
  ...over,
});

describe('format', () => {
  it('sizes like dbx', () => {
    expect(human(0)).toBe('0 B');
    expect(human(1023)).toBe('1023 B');
    expect(human(1536)).toBe('1.5 KiB');
    expect(human(8 << 20)).toBe('8.0 MiB');
  });

  it('fits text to cells', () => {
    expect(fit('beach.jpg', 20)).toBe('beach.jpg');
    expect(fit('beach.jpg', 6)).toBe('beach…');
    expect(fit('ünïcødé', 4)).toBe('ünï…');
    expect(fit('abc', 0)).toBe('');
  });

  it('draws progress', () => {
    expect(bar(0, 100, 10)).toBe('░░░░░░░░░░');
    expect(bar(55, 100, 10)).toBe('█████░░░░░');
    expect(bar(100, 100, 10)).toBe('██████████');
    expect(bar(0, 0, 4)).toBe('████');
    expect(percent(999, 1000)).toBe('99%');
    expect(percent(0, 0)).toBe('100%');
  });

  it('splits names like Finder', () => {
    expect(splitName('beach.jpg')).toEqual(['beach', '.jpg']);
    expect(splitName('a.tar.gz')).toEqual(['a.tar', '.gz']);
    expect(splitName('.bashrc')).toEqual(['.bashrc', '']);
    expect(splitName('Makefile')).toEqual(['Makefile', '']);
  });

  it('knows what opens where', () => {
    expect(baseMime('text/markdown; charset=utf-8')).toBe('text/markdown');
    expect(isText(file({ mime: 'text/markdown; charset=utf-8' }))).toBe(true);
    expect(isText(file({ mime: 'application/json' }))).toBe(true);
    expect(isText(file({ mime: 'text/plain', size: 2 << 20 }))).toBe(false);
    expect(isText(file({ mime: 'image/png' }))).toBe(false);
    expect(hasThumb(file({ mime: 'image/jpeg' }))).toBe(true);
    expect(hasThumb(file({ mime: 'image/heic' }))).toBe(false);
    expect(hasThumb(file({ mime: 'image/png', blob: null }))).toBe(false);
    expect(sizeLabel(file({ kind: 'folder', count: 1 }))).toBe('1 item');
  });
});
