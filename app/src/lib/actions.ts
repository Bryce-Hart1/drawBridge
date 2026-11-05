// Claude — Date 11/03/2026
// What the keys and clicks do. Each action calls the backend and then shows
// what changed; anything that fails is reported in the bottom line.

import { api } from './api';
import { errorText, isText, plural, preview, splitName } from './format';
import { current, ui } from './state.svelte';
import type { Clash, Id, Item } from './types';

let messageTimer: ReturnType<typeof setTimeout> | undefined;

/** A message in the bottom line, in place of the key hints for a few seconds. */
export function say(text: string, error = false) {
  ui.message = { text, error };
  clearTimeout(messageTimer);
  messageTimer = setTimeout(() => (ui.message = null), error ? 8000 : 4000);
}

export function fail(e: unknown) {
  say(errorText(e), true);
}

/**
 * Show folder `dir` (`null` is the root). With `select`, that entry is
 * selected if it's still there; without it, the first one is.
 */
export async function load(dir: Id | null, select?: Id) {
  try {
    const listing = await api.list(dir);
    const at = select === undefined ? 0 : listing.items.findIndex((i) => i.id === select);
    ui.listing = listing;
    ui.sel = at >= 0 ? at : Math.min(ui.sel, Math.max(0, listing.items.length - 1));
  } catch (e) {
    // The folder is gone, most likely deleted on another device.
    if (dir !== null) {
      await load(null);
      say('that folder was deleted');
    } else {
      fail(e);
    }
  }
}

/** Show the current folder again, keeping the selection. */
export function reload() {
  return load(ui.listing.dir, current()?.id ?? '');
}

/** Open a folder, a note, or a file in the viewer; anything else is saved as a copy. */
export function open(item: Item) {
  if (item.kind === 'folder') return load(item.id);
  if (isText(item)) {
    ui.overlay = { kind: 'note', id: item.id, parent: ui.listing.dir, name: item.name, size: item.size ?? 0 };
    return;
  }
  if (preview(item)) {
    ui.overlay = { kind: 'view', item };
    return;
  }
  return saveAs(item);
}

/** Play a file in IINA, VLC or QuickTime. */
export async function openExternal(item: Item) {
  if (item.kind !== 'file') return;
  try {
    say(`opening ${item.name} in ${await api.openExternal(item.id)}`);
  } catch (e) {
    fail(e);
  }
}

/** Up one folder, with the one just left selected. */
export function back() {
  const path = ui.listing.path;
  if (path.length === 0) return;
  const parent = path.length > 1 ? path[path.length - 2].id : null;
  return load(parent, path[path.length - 1].id);
}

export function askMkdir() {
  ui.prompt = {
    kind: 'input',
    label: 'new folder',
    value: '',
    submit: async (name) => {
      const id = await api.mkdir(ui.listing.dir, name);
      await load(ui.listing.dir, id);
    },
  };
}

export function askRename(item: Item) {
  const [stem] = splitName(item.name);
  ui.prompt = {
    kind: 'input',
    label: `rename ${item.name} →`,
    value: item.name,
    // Like Finder: the name is selected, the extension isn't.
    select: [0, item.kind === 'folder' ? item.name.length : stem.length],
    submit: async (name) => {
      if (name === item.name) return;
      await api.rename(item.id, name);
      await load(ui.listing.dir, item.id);
    },
  };
}

export function askDelete(item: Item) {
  const what = item.kind === 'folder' ? `${item.name}/${item.count ? ' and everything in it' : ''}` : item.name;
  ui.prompt = {
    kind: 'confirm',
    text: `delete ${what} for every device?`,
    yes: async () => {
      await api.remove(item.id);
      await reload();
      say(`deleted ${what}`);
    },
  };
}

export function askNote() {
  ui.prompt = {
    kind: 'input',
    label: 'new note',
    value: 'note.md',
    select: [0, 4],
    submit: (name) => {
      if (ui.listing.items.some((i) => i.name === name)) throw new Error(`${name} already exists`);
      ui.overlay = { kind: 'note', id: null, parent: ui.listing.dir, name };
    },
  };
}

/** Upload files or folders into the current folder, asking once about names already taken. */
export async function uploadPaths(paths: string[], onClash: Clash = 'ask') {
  if (paths.length === 0) return;
  try {
    const plan = await api.upload(paths, ui.listing.dir, onClash);
    if (plan.clashes.length > 0) {
      const one = plan.clashes.length === 1;
      ui.prompt = {
        kind: 'choice',
        text: `${one ? plan.clashes[0] : plural(plan.clashes.length, 'name')} already ${one ? 'exists' : 'exist'}:`,
        choices: [
          { key: 'r', label: 'replace', pick: () => uploadPaths(paths, 'replace') },
          { key: 'k', label: 'keep both', pick: () => uploadPaths(paths, 'keep') },
          { key: 's', label: 'skip', pick: () => uploadPaths(paths, 'skip') },
        ],
      };
      return;
    }
    const skipped = plan.skipped ? `, ${plan.skipped} skipped` : '';
    say(plan.queued ? `uploading ${plural(plan.queued, 'file')}${skipped}` : `nothing to upload${skipped}`);
  } catch (e) {
    fail(e);
  }
}

export async function pickAndUpload() {
  try {
    const paths = await api.pickFiles();
    if (paths) await uploadPaths(paths);
  } catch (e) {
    fail(e);
  }
}

export async function saveAs(item: Pick<Item, 'id' | 'name'>) {
  try {
    const path = await api.pickSavePath(item.name);
    if (path) await api.download(item.id, path);
  } catch (e) {
    fail(e);
  }
}

export function askDisconnect() {
  ui.prompt = {
    kind: 'confirm',
    text: 'disconnect this device? its key is forgotten; everything stays in the store',
    yes: async () => {
      await api.disconnect();
      ui.status = await api.status();
    },
  };
}

export async function syncNow() {
  try {
    await api.syncNow();
  } catch (e) {
    fail(e);
  }
}

export async function cancelTransfer() {
  // The one running, or else the one waiting for the store.
  const all = Object.values(ui.transfers);
  const running = all.find((t) => t.state === 'running') ?? all.find((t) => t.state === 'waiting');
  if (!running) return;
  try {
    await api.cancel(running.id);
  } catch (e) {
    fail(e);
  }
}
