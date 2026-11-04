// Claude — Date 11/03/2026
// Box-drawing pictures for entries without a thumbnail, in the grid and the
// inspector. Every one is 5 lines of 11 cells.

import { baseMime } from './format';
import type { Item } from './types';

const FOLDER = [
  '┌───┐      ',
  '│   └─────┐',
  '│         │',
  '│         │',
  '└─────────┘',
];

const FILE = [
  ' ┌──────┐  ',
  ' │      └┐ ',
  ' │       │ ',
  ' │       │ ',
  ' └───────┘ ',
];

const TEXT = [
  ' ┌──────┐  ',
  ' │ ──── └┐ ',
  ' │ ───── │ ',
  ' │ ───   │ ',
  ' └───────┘ ',
];

const IMAGE = [
  '┌─────────┐',
  '│      ●  │',
  '│  ╱╲     │',
  '│ ╱  ╲╱╲  │',
  '└─────────┘',
];

const VIDEO = [
  '┌─────────┐',
  '│ ▪ ▪ ▪ ▪ │',
  '│    ▶    │',
  '│ ▪ ▪ ▪ ▪ │',
  '└─────────┘',
];

const AUDIO = [
  '┌─────────┐',
  '│    ♪    │',
  '│ ▁▃▅▇▅▃▁ │',
  '│         │',
  '└─────────┘',
];

export function glyph(item: Item): string {
  const mime = baseMime(item.mime);
  const art =
    item.kind === 'folder' ? FOLDER
    : mime.startsWith('image/') ? IMAGE
    : mime.startsWith('video/') ? VIDEO
    : mime.startsWith('audio/') ? AUDIO
    : mime.startsWith('text/') || mime === 'application/json' ? TEXT
    : FILE;
  return art.join('\n');
}

/** The setup screen's drawing. */
export const LOGO = [
  '  ▟▙   ▟▙   ▟▙   ▟▙  ',
  '  ████████████████  ',
  '  ████▀▀▀▀▀▀▀▀████  ',
  '  ███ ┃ ┃ ┃ ┃ ┃███  ',
  '  ███ ┃ ┃ ┃ ┃ ┃███  ',
  '══╧══╧═╧═╧═╧═╧══╧══',
].join('\n');
