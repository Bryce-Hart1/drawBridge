// Claude — Date 11/03/2026
// Everything on screen, in one reactive object. Components read it directly;
// actions.ts changes it.

import type { Id, Item, Listing, Status, Transfer } from './types';

/** A question in the bottom line, in place of the key hints. */
export type Prompt =
  | {
      kind: 'input';
      label: string;
      value: string;
      /** The part of `value` selected to start with; all of it if unset. */
      select?: [number, number];
      submit: (value: string) => unknown;
    }
  | { kind: 'confirm'; text: string; yes: () => unknown }
  | { kind: 'choice'; text: string; choices: { key: string; label: string; pick: () => unknown }[] };

export type Overlay =
  | { kind: 'help' }
  | { kind: 'find' }
  | { kind: 'move'; item: Item }
  | { kind: 'note'; id: Id | null; parent: Id | null; name: string; size?: number }
  | { kind: 'view'; item: Item }
  | { kind: 'settings' };

export const ui = $state({
  status: null as Status | null,
  listing: { dir: null, path: [], items: [] } as Listing,
  /** Index of the selected item in `listing.items`. */
  sel: 0,
  mode: 'list' as 'list' | 'grid',
  prompt: null as Prompt | null,
  message: null as { text: string; error: boolean } | null,
  overlay: null as Overlay | null,
  transfers: {} as Record<number, Transfer>,
  /** Files are being dragged over the window. */
  dropping: false,
  /** Blob → when its thumbnail was drawn here, so views showing a miss load it again. */
  posters: {} as Record<string, number>,
});

export function current(): Item | undefined {
  return ui.listing.items[ui.sel];
}
