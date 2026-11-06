// Claude — Date 11/03/2026
import { describe, expect, it } from 'vitest';
import { bar, baseMime, duration, fit, hasThumb, human, isLin, isLinName, isText, percent, preview, scrubBar, sizeLabel, splitName, typeLabel, volumeBar, wantsPoster } from './format';
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
    // Big text opens too, read-only.
    expect(isText(file({ mime: 'text/plain', size: 2 << 20 }))).toBe(true);
    expect(isText(file({ mime: 'image/png' }))).toBe(false);
    expect(hasThumb(file({ mime: 'image/jpeg' }))).toBe(true);
    expect(hasThumb(file({ mime: 'image/heic' }))).toBe(false);
    expect(hasThumb(file({ mime: 'image/png', blob: null }))).toBe(false);
    expect(sizeLabel(file({ kind: 'folder', count: 1 }))).toBe('1 item');
    expect(wantsPoster(file({ mime: 'image/heic' }))).toBe(true);
    expect(wantsPoster(file({ mime: 'video/quicktime' }))).toBe(true);
    expect(wantsPoster(file({ mime: 'image/jpeg' }))).toBe(false);
    // A diagram draws its own thumbnail.
    expect(wantsPoster(file({ name: 'flow.lin', mime: 'text/x-lin; charset=utf-8' }))).toBe(true);
    expect(wantsPoster(file({ name: 'flow.lin', blob: null }))).toBe(false);
  });

  it('picks a viewer', () => {
    expect(preview(file({ mime: 'image/heic' }))).toBe('image');
    expect(preview(file({ mime: 'audio/flac' }))).toBe('audio');
    expect(preview(file({ mime: 'video/x-matroska' }))).toBe('video');
    expect(preview(file({ mime: 'application/pdf' }))).toBe('pdf');
    expect(preview(file({ mime: 'application/zip' }))).toBeNull();
    expect(preview(file({ kind: 'folder', mime: null }))).toBeNull();
  });

  it('draws the player line', () => {
    expect(duration(0)).toBe('0:00');
    expect(duration(92.7)).toBe('1:32');
    expect(duration(3723)).toBe('1:02:03');
    expect(duration(NaN)).toBe('-:--');
    expect(duration(Infinity)).toBe('-:--');
    expect(scrubBar(25, 50, 100, 8)).toBe('██▒▒░░░░');
    expect(scrubBar(0, 0, 0, 4)).toBe('░░░░');
    expect(scrubBar(100, 80, 100, 4)).toBe('████');
    expect(volumeBar(1, false)).toBe('▮▮▮▮');
    expect(volumeBar(0.3, false)).toBe('▮▯▯▯');
    expect(volumeBar(0.01, false)).toBe('▮▯▯▯');
    expect(volumeBar(1, true)).toBe('muted');
  });

  it('knows a .lin diagram by its name, whatever the store recorded', () => {
    // Uploaded before the type existed: plain bytes. Uploaded since: text/x-lin.
    for (const mime of ['application/octet-stream', 'text/x-lin; charset=utf-8']) {
      expect(isLin(file({ name: 'flow.lin', mime }))).toBe(true);
    }
    expect(isLin(file({ name: 'FLOW.LIN' }))).toBe(true);
    expect(isLin(file({ name: 'flow.lin.bak' }))).toBe(false);
    expect(isLin(file({ name: 'lin' }))).toBe(false);
    expect(isLin(file({ name: '.lin' }))).toBe(false);
    expect(isLin(file({ name: 'flow.lin', kind: 'folder', mime: null }))).toBe(false);
    expect(isLinName('a.b.lin')).toBe(true);
    expect(typeLabel(file({ name: 'flow.lin' }))).toBe('lin diagram');
    expect(typeLabel(file({ name: 'a.png', mime: 'image/png' }))).toBe('image/png');
  });
});
