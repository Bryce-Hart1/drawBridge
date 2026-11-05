// Claude — Date 11/03/2026
// The key map: one key per action, as the key-hint bar shows them. Arrows and
// hjk both move (`l` is taken by new diagrams, so → and Enter are what go in).
// What left and right do depends on the view: in the list they go up a folder
// and into one, in the grid they move between tiles. The viewer and player
// have keys of their own (Viewer.svelte, Player.svelte).

export type Action =
  | 'up' | 'down' | 'left' | 'right' | 'open' | 'back' | 'top' | 'bottom' | 'pageUp' | 'pageDown'
  | 'upload' | 'mkdir' | 'note' | 'diagram' | 'rename' | 'move' | 'delete' | 'save'
  | 'find' | 'grid' | 'sync' | 'cancel' | 'help' | 'external' | 'settings';

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
  ArrowRight: 'right',
  Enter: 'open',
  Backspace: 'back',
  Home: 'top',
  End: 'bottom', G: 'bottom',
  PageUp: 'pageUp',
  PageDown: 'pageDown',
  u: 'upload',
  '+': 'mkdir', '=': 'mkdir',
  n: 'note',
  l: 'diagram',
  r: 'rename',
  m: 'move',
  d: 'delete', Delete: 'delete',
  o: 'save',
  e: 'external',
  ',': 'settings',
  '/': 'find',
  g: 'grid',
  s: 'sync',
  x: 'cancel',
  '?': 'help',
};

export function actionFor(e: KeyLike): Action | null {
  // ⌘F finds and ⌘, opens the settings; every other ⌘, Ctrl or ⌥ chord is
  // left to the system (⌘Q, ⌘W…).
  if (e.metaKey || e.ctrlKey) return e.key === 'f' ? 'find' : e.key === ',' ? 'settings' : null;
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
  { key: 'l', word: 'line', action: 'diagram' },
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
