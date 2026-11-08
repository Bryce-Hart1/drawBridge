// Claude — Date 11/05/2026
// The diagram editor's keys. Everything is a key except where things go, which
// is the mouse. Pressing Enter on a shape types into it; until Esc (or Enter,
// on a line's one-row label, since Enter in a shape starts a new line),
// every key is a letter, so `i`, `o`, `p` and the digits are free to be both
// text and commands without clashing.
//
// This turns a key press into a command; LinEditor.svelte does the command.

import { BOLD, FONT_KEYS, ITALIC, UNDERLINE, type FontKey, type ShapeKind } from './model';
import type { Dir } from './edit';

export type Command =
  | { do: 'arm'; tool: ShapeKind | 'line' }
  | { do: 'select'; dir: 1 | -1 }
  | { do: 'resize'; dir: Dir; big: boolean }
  | { do: 'nudge'; dir: Dir; fine: boolean }
  | { do: 'color'; c: number }
  | { do: 'font'; f: FontKey }
  | { do: 'type' }
  | { do: 'head' }
  | { do: 'flip' }
  | { do: 'pulse' }
  | { do: 'glow' }
  | { do: 'route' }
  | { do: 'style' }
  | { do: 'order'; to: 'up' | 'down' | 'front' | 'back' }
  | { do: 'grid' }
  | { do: 'zoom'; by: 'in' | 'out' | 'fit' }
  | { do: 'copy' }
  | { do: 'paste' }
  | { do: 'duplicate' }
  | { do: 'all' }
  | { do: 'delete' }
  | { do: 'undo' }
  | { do: 'redo' }
  | { do: 'save' }
  | { do: 'export' }
  | { do: 'play' }
  | { do: 'edit' }
  | { do: 'escape' }
  // While typing:
  | { do: 'char'; ch: string }
  | { do: 'backspace' }
  | { do: 'caret'; dir: 'left' | 'right'; select: boolean }
  | { do: 'format'; flag: number }
  | { do: 'done' };

export interface KeyEvent {
  key: string;
  /** The physical key ('KeyB'). With ⌥ held a Mac's `key` is a symbol, not the letter. */
  code?: string;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

export interface Mode {
  /** Editing, not just watching. */
  edit: boolean;
  /** Typing into a shape. */
  typing: boolean;
  /**
   * Typing into a shape, where Enter starts a new line (a label on a line is one
   * row, so Enter ends it), the arrows move a caret, and ⌥B, ⌥I and ⌥U style letters.
   */
  multiline?: boolean;
}

const ARM: Record<string, ShapeKind | 'line'> = { c: 'circle', t: 'triangle', r: 'rect', d: 'diamond', l: 'line' };
/** ⌥ with these letters, while typing in a shape. */
const FORMATS: Record<string, number> = { b: BOLD, i: ITALIC, u: UNDERLINE };
const ARROWS: Record<string, Dir> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };

/** One typed character: a single, printable one. */
export function isTypable(key: string): boolean {
  return [...key].length === 1 && key >= ' ' && key !== '\u007f';
}

/** The letter a key is, even with ⌥ held. */
function letterOf(e: KeyEvent): string {
  if (e.code?.startsWith('Key')) return e.code.slice(3).toLowerCase();
  return e.key.length === 1 ? e.key.toLowerCase() : '';
}

export function commandFor(e: KeyEvent, mode: Mode): Command | null {
  const chord = e.metaKey || e.ctrlKey;
  if (e.altKey) {
    // Typing is all letters, so ⌥ is what tells a style from a letter.
    if (!chord && mode.typing && mode.multiline) {
      const flag = FORMATS[letterOf(e)];
      return flag ? { do: 'format', flag } : null;
    }
    // ⌥ with an arrow moves the selection: 10 px, or 1 with ⇧. Nothing else uses ⌥.
    if (!chord && mode.edit && !mode.typing && e.key in ARROWS) return { do: 'nudge', dir: ARROWS[e.key], fine: e.shiftKey };
    return null;
  }

  if (chord) {
    const k = e.key.toLowerCase();
    if (k === 's') return { do: 'save' };
    if (!mode.edit || mode.typing) return null;
    // Typing has no undo of its own; the text is one step back.
    if (k === 'z') return { do: e.shiftKey ? 'redo' : 'undo' };
    if (k === 'c') return { do: 'copy' };
    if (k === 'v') return { do: 'paste' };
    if (k === 'd') return { do: 'duplicate' };
    if (k === 'a') return { do: 'all' };
    return null;
  }

  if (mode.typing) {
    if (e.key === 'Enter') return mode.multiline ? { do: 'char', ch: '\n' } : { do: 'done' };
    if (e.key === 'Escape') return { do: 'done' };
    if (e.key === 'Backspace') return { do: 'backspace' };
    if (mode.multiline && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      return { do: 'caret', dir: e.key === 'ArrowLeft' ? 'left' : 'right', select: e.shiftKey };
    }
    return isTypable(e.key) ? { do: 'char', ch: e.key } : null;
  }

  // Letters mean the same with Caps Lock on.
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;

  if (k === 'Escape') return { do: 'escape' };
  // Play and pause are for watching; while editing, the space bar does nothing.
  if (k === ' ') return mode.edit ? null : { do: 'play' };
  if (k === 'x') return { do: 'export' };
  if (k === 'e') return { do: 'edit' };
  // Looking around works while viewing too.
  if (k === '=' || k === '+') return { do: 'zoom', by: 'in' };
  if (k === '-' || k === '_') return { do: 'zoom', by: 'out' };
  if (k === 'z') return { do: 'zoom', by: 'fit' };
  if (!mode.edit) return null;

  if (k in ARM) return { do: 'arm', tool: ARM[k] };
  if (k in ARROWS) return { do: 'resize', dir: ARROWS[k], big: e.shiftKey };
  if (k === 'Tab') return { do: 'select', dir: e.shiftKey ? -1 : 1 };
  if (/^[0-9]$/.test(k)) return { do: 'color', c: Number(k) };
  if ((FONT_KEYS as readonly string[]).includes(k)) return { do: 'font', f: k as FontKey };
  if (k === 'Enter') return { do: 'type' };
  if (k === 'a') return { do: 'head' };
  if (k === 'f') return { do: 'flip' };
  if (k === 'm') return { do: 'pulse' };
  if (k === 's') return { do: 'glow' };
  if (k === 'b') return { do: 'route' };
  if (k === 'w') return { do: 'style' };
  if (k === 'g') return { do: 'grid' };
  if (k === ']') return { do: 'order', to: 'up' };
  if (k === '[') return { do: 'order', to: 'down' };
  if (k === '}') return { do: 'order', to: 'front' };
  if (k === '{') return { do: 'order', to: 'back' };
  if (k === 'Backspace' || k === 'Delete') return { do: 'delete' };
  return null;
}

// ── The hint row ────────────────────────────────────────────────────────

export interface Hint {
  /** What the key looks like on screen. */
  label: string;
  word: string;
  /** A key that does it, so a test can check the hint isn't lying. `null` for a hint that isn't a key. */
  press: string | null;
  /** ⌘ with it. */
  chord?: boolean;
  /** ⌥ with it. */
  alt?: boolean;
  does: Command['do'] | null;
}

export interface Context extends Mode {
  /** What is selected: one shape, one line, or several parts. */
  selected: 'shape' | 'line' | 'many' | null;
  /** A shape or line tool is waiting for a click. */
  armed: boolean;
  playing: boolean;
}

const h = (label: string, word: string, press: string | null, does: Command['do'] | null, chord = false, alt = false): Hint => ({
  label,
  word,
  press,
  does,
  ...(chord ? { chord } : {}),
  ...(alt ? { alt } : {}),
});

/** The keys worth showing right now: what the selection can do, and the always-there ones. */
export function hintsFor(c: Context): Hint[] {
  if (c.typing && c.multiline) {
    return [
      h('⌥B', 'bold', 'b', 'format', false, true),
      h('⌥I', 'italic', 'i', 'format', false, true),
      h('⌥U', 'underline', 'u', 'format', false, true),
      h('← →', 'move · ⇧ selects', 'ArrowLeft', 'caret'),
      h('⏎', 'new line', 'Enter', 'char'),
      h('⌫', 'delete', 'Backspace', 'backspace'),
      h('esc', 'done', 'Escape', 'done'),
    ];
  }
  if (c.typing) return [h('⏎', 'done', 'Enter', 'done'), h('esc', 'done', 'Escape', 'done'), h('⌫', 'delete', 'Backspace', 'backspace')];
  const play = h('space', c.playing ? 'pause' : 'play', ' ', 'play');
  const zoom = h('+ -', 'zoom', '=', 'zoom');
  const fit = h('z', 'fit', 'z', 'zoom');
  if (!c.edit) return [play, h('e', 'edit', 'e', 'edit'), zoom, fit, h('x', 'export', 'x', 'export'), h('esc', 'close', 'Escape', 'escape')];
  if (c.armed) return [h('click', 'to place', null, null), h('esc', 'cancel', 'Escape', 'escape')];

  const tail = [
    h('⌘S', 'save', 's', 'save', true),
    h('x', 'export', 'x', 'export'),
    zoom,
    fit,
    h('g', 'grid', 'g', 'grid'),
    h('e', 'view', 'e', 'edit'),
    h('esc', c.selected ? 'deselect' : 'close', 'Escape', 'escape'),
  ];
  const common = [
    h('⌥ arrows', 'move', 'ArrowLeft', 'resize'),
    h('⌘D', 'duplicate', 'd', 'duplicate', true),
    h('⌘C ⌘V', 'copy', 'c', 'copy', true),
    h('[ ]', 'order', ']', 'order'),
    h('w', 'dash/dot', 'w', 'style'),
    h('⌫', 'delete', 'Backspace', 'delete'),
  ];
  // The ⌥ hint is a description, not a key a test can press: it has no `press`.
  common[0] = { label: '⌥ arrows', word: 'move', press: null, does: null };
  if (c.selected === 'shape') {
    return [
      h('← → ↑ ↓', 'size', 'ArrowLeft', 'resize'),
      h('0-9', 'colour', '0', 'color'),
      h('i o p', 'font', 'i', 'font'),
      h('⏎', 'text', 'Enter', 'type'),
      ...common,
      h('tab', 'next', 'Tab', 'select'),
      ...tail,
    ];
  }
  if (c.selected === 'line') {
    return [
      h('a', 'arrowheads', 'a', 'head'),
      h('f', 'flip', 'f', 'flip'),
      h('b', 'bend', 'b', 'route'),
      h('m', 'pulse', 'm', 'pulse'),
      h('s', 'ball/light', 's', 'glow'),
      h('⏎', 'label', 'Enter', 'type'),
      h('0-9', 'colour', '0', 'color'),
      h('i o p', 'font', 'i', 'font'),
      ...common,
      h('tab', 'next', 'Tab', 'select'),
      ...tail,
    ];
  }
  if (c.selected === 'many') {
    return [
      h('← → ↑ ↓', 'size', 'ArrowLeft', 'resize'),
      h('0-9', 'colour', '0', 'color'),
      h('i o p', 'font', 'i', 'font'),
      h('a', 'arrowheads', 'a', 'head'),
      h('m', 'pulse', 'm', 'pulse'),
      ...common,
      ...tail,
    ];
  }
  return [
    h('c t r d', 'shape', 'c', 'arm'),
    h('l', 'line', 'l', 'arm'),
    h('tab', 'select', 'Tab', 'select'),
    h('⌘A', 'all', 'a', 'all', true),
    h('⌘V', 'paste', 'v', 'paste', true),
    h('⌘Z', 'undo', 'z', 'undo', true),
    ...tail,
  ];
}
