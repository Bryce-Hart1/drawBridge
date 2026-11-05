// Claude — Date 11/05/2026
// The diagram editor's keys. Everything is a key except where things go, which
// is the mouse. Pressing Enter on a shape types into it; until Enter or Esc,
// every key is a letter, so `i`, `o`, `p` and the digits are free to be both
// text and commands without clashing.
//
// This turns a key press into a command; LinEditor.svelte does the command.

import { FONT_KEYS, type FontKey, type ShapeKind } from './model';
import type { Dir } from './edit';

export type Command =
  | { do: 'arm'; tool: ShapeKind | 'line' }
  | { do: 'select'; dir: 1 | -1 }
  | { do: 'resize'; dir: Dir; big: boolean }
  | { do: 'color'; c: number }
  | { do: 'font'; f: FontKey }
  | { do: 'type' }
  | { do: 'head' }
  | { do: 'flip' }
  | { do: 'pulse' }
  | { do: 'glow' }
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
  | { do: 'done' };

export interface KeyEvent {
  key: string;
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
}

const ARM: Record<string, ShapeKind | 'line'> = { c: 'circle', t: 'triangle', r: 'rect', d: 'diamond', l: 'line' };
const ARROWS: Record<string, Dir> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };

/** One typed character: a single, printable one. */
export function isTypable(key: string): boolean {
  return [...key].length === 1 && key >= ' ' && key !== '\u007f';
}

export function commandFor(e: KeyEvent, mode: Mode): Command | null {
  const chord = e.metaKey || e.ctrlKey;
  if (e.altKey) return null;

  if (chord) {
    const k = e.key.toLowerCase();
    if (k === 's') return { do: 'save' };
    // Typing has no undo of its own; the text is one step back.
    if (mode.edit && !mode.typing && k === 'z') return { do: e.shiftKey ? 'redo' : 'undo' };
    return null;
  }

  if (mode.typing) {
    if (e.key === 'Enter' || e.key === 'Escape') return { do: 'done' };
    if (e.key === 'Backspace') return { do: 'backspace' };
    return isTypable(e.key) ? { do: 'char', ch: e.key } : null;
  }

  // Letters mean the same with Caps Lock on.
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;

  if (k === 'Escape') return { do: 'escape' };
  if (k === ' ') return { do: 'play' };
  if (k === 'x') return { do: 'export' };
  if (k === 'e') return { do: 'edit' };
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
  does: Command['do'] | null;
}

export interface Context extends Mode {
  /** What is selected. */
  selected: 'shape' | 'line' | null;
  /** A shape or line tool is waiting for a click. */
  armed: boolean;
  playing: boolean;
}

const h = (label: string, word: string, press: string | null, does: Command['do'] | null, chord = false): Hint => ({
  label,
  word,
  press,
  does,
  ...(chord ? { chord } : {}),
});

/** The keys worth showing right now: what the selection can do, and the always-there ones. */
export function hintsFor(c: Context): Hint[] {
  if (c.typing) return [h('⏎', 'done', 'Enter', 'done'), h('esc', 'done', 'Escape', 'done'), h('⌫', 'delete', 'Backspace', 'backspace')];
  const play = h('space', c.playing ? 'pause' : 'play', ' ', 'play');
  if (!c.edit) return [play, h('e', 'edit', 'e', 'edit'), h('x', 'export jpeg', 'x', 'export'), h('esc', 'close', 'Escape', 'escape')];
  if (c.armed) return [h('click', 'to place', null, null), h('esc', 'cancel', 'Escape', 'escape')];

  const tail = [play, h('⌘S', 'save', 's', 'save', true), h('x', 'jpeg', 'x', 'export'), h('e', 'view', 'e', 'edit'), h('esc', c.selected ? 'deselect' : 'close', 'Escape', 'escape')];
  if (c.selected === 'shape') {
    return [
      h('← → ↑ ↓', 'size', 'ArrowLeft', 'resize'),
      h('0-9', 'colour', '0', 'color'),
      h('i o p', 'font', 'i', 'font'),
      h('⏎', 'text', 'Enter', 'type'),
      h('⌫', 'delete', 'Backspace', 'delete'),
      h('tab', 'next', 'Tab', 'select'),
      ...tail,
    ];
  }
  if (c.selected === 'line') {
    return [
      h('a', 'arrowheads', 'a', 'head'),
      h('f', 'flip', 'f', 'flip'),
      h('m', 'pulse', 'm', 'pulse'),
      h('s', 'ball/light', 's', 'glow'),
      h('0-9', 'colour', '0', 'color'),
      h('⌫', 'delete', 'Backspace', 'delete'),
      h('tab', 'next', 'Tab', 'select'),
      ...tail,
    ];
  }
  return [
    h('c t r d', 'shape', 'c', 'arm'),
    h('l', 'line', 'l', 'arm'),
    h('tab', 'select', 'Tab', 'select'),
    h('⌘Z', 'undo', 'z', 'undo', true),
    ...tail,
  ];
}
