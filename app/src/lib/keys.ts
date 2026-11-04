// Claude — Date 11/03/2026
// The key map: one key per action, as the key-hint bar shows them. Arrows and
// hjkl both move. What left and right do depends on the view: in the list they
// go up a folder and into one, in the grid they move between tiles.

export type Action =
  | 'up' | 'down' | 'left' | 'right' | 'open' | 'back' | 'top' | 'bottom' | 'pageUp' | 'pageDown'
  | 'upload' | 'mkdir' | 'note' | 'rename' | 'move' | 'delete' | 'save'
  | 'find' | 'grid' | 'sync' | 'cancel' | 'help';

export interface KeyLike {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

const PLAIN: Record<string, Action> = {
  ArrowUp: 'up', k: 'up',
  ArrowDown: 'down', j: 'down',
  ArrowLeft: 'left', h: 'left',
  ArrowRight: 'right', l: 'right',
  Enter: 'open',
  Backspace: 'back',
  Home: 'top',
  End: 'bottom', G: 'bottom',
  PageUp: 'pageUp',
  PageDown: 'pageDown',
  u: 'upload',
  '+': 'mkdir', '=': 'mkdir',
  n: 'note',
  r: 'rename',
  m: 'move',
  d: 'delete', Delete: 'delete',
  o: 'save',
  '/': 'find',
  g: 'grid',
  s: 'sync',
  x: 'cancel',
  '?': 'help',
};

export function actionFor(e: KeyLike): Action | null {
  // ⌘F finds; every other ⌘, Ctrl or ⌥ chord is left to the system (⌘Q, ⌘W…).
  if (e.metaKey || e.ctrlKey) return e.key === 'f' ? 'find' : null;
  if (e.altKey) return null;
  return PLAIN[e.key] ?? null;
}

export interface Hint {
  key: string;
  word: string;
  action: Action;
}

/** The key-hint bar, left to right. */
export const HINTS: Hint[] = [
  { key: 'u', word: 'upload', action: 'upload' },
  { key: '+', word: 'folder', action: 'mkdir' },
  { key: 'n', word: 'note', action: 'note' },
  { key: 'r', word: 'rename', action: 'rename' },
  { key: 'm', word: 'move', action: 'move' },
  { key: 'd', word: 'delete', action: 'delete' },
  { key: 'o', word: 'save', action: 'save' },
  { key: '/', word: 'find', action: 'find' },
  { key: 'g', word: 'grid', action: 'grid' },
  { key: 's', word: 'sync', action: 'sync' },
  { key: '?', word: 'help', action: 'help' },
];

/** `[u]pload` when the word starts with its key, `[o]save` otherwise: the text after the brackets. */
export function hintRest(h: Hint): string {
  return h.word.startsWith(h.key) ? h.word.slice(h.key.length) : h.word;
}
