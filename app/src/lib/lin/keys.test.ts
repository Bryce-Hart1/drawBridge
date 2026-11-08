// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import { commandFor, hintsFor, isTypable, type Command, type Context, type KeyEvent } from './keys';
import { BOLD, ITALIC, UNDERLINE } from './model';

const press = (key: string, mods: Partial<KeyEvent> = {}): KeyEvent => ({
  key,
  shiftKey: false,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

const EDIT = { edit: true, typing: false };
const VIEW = { edit: false, typing: false };
const TYPING = { edit: true, typing: true };

const cmd = (key: string, mode = EDIT, mods: Partial<KeyEvent> = {}) => commandFor(press(key, mods), mode);

describe('editing keys', () => {
  it('arms a shape or a line to place', () => {
    expect(cmd('c')).toEqual({ do: 'arm', tool: 'circle' });
    expect(cmd('t')).toEqual({ do: 'arm', tool: 'triangle' });
    expect(cmd('r')).toEqual({ do: 'arm', tool: 'rect' });
    expect(cmd('d')).toEqual({ do: 'arm', tool: 'diamond' });
    expect(cmd('l')).toEqual({ do: 'arm', tool: 'line' });
  });

  it('means the same with caps lock on', () => {
    expect(cmd('C')).toEqual({ do: 'arm', tool: 'circle' });
    expect(cmd('P')).toEqual({ do: 'font', f: 'p' });
  });

  it('resizes with the arrows, and more with shift', () => {
    expect(cmd('ArrowLeft')).toEqual({ do: 'resize', dir: 'left', big: false });
    expect(cmd('ArrowDown')).toEqual({ do: 'resize', dir: 'down', big: false });
    expect(cmd('ArrowUp', EDIT, { shiftKey: true })).toEqual({ do: 'resize', dir: 'up', big: true });
  });

  it('colours with 0 to 9', () => {
    for (let c = 0; c <= 9; c++) expect(cmd(String(c))).toEqual({ do: 'color', c });
    expect(cmd('!', EDIT, { shiftKey: true })).toBeNull();
  });

  it('sets the font with i, o and p', () => {
    expect(cmd('i')).toEqual({ do: 'font', f: 'i' });
    expect(cmd('o')).toEqual({ do: 'font', f: 'o' });
    expect(cmd('p')).toEqual({ do: 'font', f: 'p' });
  });

  it('has a key for each thing a line can do', () => {
    expect(cmd('a')).toEqual({ do: 'head' });
    expect(cmd('f')).toEqual({ do: 'flip' });
    expect(cmd('m')).toEqual({ do: 'pulse' });
    expect(cmd('s')).toEqual({ do: 'glow' });
  });

  it('selects, types, deletes and leaves', () => {
    expect(cmd('Tab')).toEqual({ do: 'select', dir: 1 });
    expect(cmd('Tab', EDIT, { shiftKey: true })).toEqual({ do: 'select', dir: -1 });
    expect(cmd('Enter')).toEqual({ do: 'type' });
    expect(cmd('Backspace')).toEqual({ do: 'delete' });
    expect(cmd('Delete')).toEqual({ do: 'delete' });
    expect(cmd('Escape')).toEqual({ do: 'escape' });
  });

  it('undoes, redoes and saves with ⌘ or Ctrl', () => {
    expect(cmd('z', EDIT, { metaKey: true })).toEqual({ do: 'undo' });
    expect(cmd('z', EDIT, { ctrlKey: true })).toEqual({ do: 'undo' });
    expect(cmd('z', EDIT, { metaKey: true, shiftKey: true })).toEqual({ do: 'redo' });
    expect(cmd('Z', EDIT, { metaKey: true, shiftKey: true })).toEqual({ do: 'redo' });
    expect(cmd('s', EDIT, { metaKey: true })).toEqual({ do: 'save' });
  });

  it('leaves other chords to the system', () => {
    expect(cmd('q', EDIT, { metaKey: true })).toBeNull();
    expect(cmd('w', EDIT, { metaKey: true })).toBeNull();
    expect(cmd('c', EDIT, { altKey: true })).toBeNull();
  });

  it('has no two keys for different things', () => {
    // Every plain key in the edit map, run through the map: each gives one command.
    const keys = ['c', 't', 'r', 'd', 'l', 'i', 'o', 'p', 'a', 'f', 'm', 's', 'x', 'e', 'Tab', 'Enter', 'Backspace', 'Delete', 'Escape'];
    const seen = new Map<string, string>();
    for (const k of keys) {
      const c = cmd(k) as Command;
      expect(c).not.toBeNull();
      const tag = JSON.stringify(c);
      if (c.do === 'arm' || c.do === 'font' || c.do === 'delete') continue;
      expect(seen.get(tag)).toBeUndefined();
      seen.set(tag, k);
    }
  });
});

describe('the rest of the editing keys', () => {
  it('bends, restyles, snaps and reorders', () => {
    expect(cmd('b')).toEqual({ do: 'route' });
    expect(cmd('w')).toEqual({ do: 'style' });
    expect(cmd('g')).toEqual({ do: 'grid' });
    expect(cmd(']')).toEqual({ do: 'order', to: 'up' });
    expect(cmd('[')).toEqual({ do: 'order', to: 'down' });
    expect(cmd('}', EDIT, { shiftKey: true })).toEqual({ do: 'order', to: 'front' });
    expect(cmd('{', EDIT, { shiftKey: true })).toEqual({ do: 'order', to: 'back' });
  });

  it('moves with ⌥ and the arrows: 10 px, or 1 with shift', () => {
    expect(cmd('ArrowLeft', EDIT, { altKey: true })).toEqual({ do: 'nudge', dir: 'left', fine: false });
    expect(cmd('ArrowDown', EDIT, { altKey: true, shiftKey: true })).toEqual({ do: 'nudge', dir: 'down', fine: true });
    expect(cmd('ArrowLeft', VIEW, { altKey: true })).toBeNull();
    expect(cmd('ArrowLeft', TYPING, { altKey: true })).toBeNull();
  });

  it('copies, pastes, duplicates and selects all with ⌘', () => {
    expect(cmd('c', EDIT, { metaKey: true })).toEqual({ do: 'copy' });
    expect(cmd('v', EDIT, { metaKey: true })).toEqual({ do: 'paste' });
    expect(cmd('d', EDIT, { ctrlKey: true })).toEqual({ do: 'duplicate' });
    expect(cmd('a', EDIT, { metaKey: true })).toEqual({ do: 'all' });
    for (const k of ['c', 'v', 'd', 'a']) {
      expect(cmd(k, VIEW, { metaKey: true })).toBeNull();
      expect(cmd(k, TYPING, { metaKey: true })).toBeNull();
    }
  });

  it('zooms with + - and z, viewing or editing, but not while typing', () => {
    for (const mode of [EDIT, VIEW]) {
      expect(cmd('=', mode)).toEqual({ do: 'zoom', by: 'in' });
      expect(cmd('+', mode, { shiftKey: true })).toEqual({ do: 'zoom', by: 'in' });
      expect(cmd('-', mode)).toEqual({ do: 'zoom', by: 'out' });
      expect(cmd('z', mode)).toEqual({ do: 'zoom', by: 'fit' });
    }
    expect(cmd('-', TYPING)).toEqual({ do: 'char', ch: '-' });
  });
});

describe('viewing', () => {
  it('only plays, exports, edits and closes', () => {
    expect(cmd(' ', VIEW)).toEqual({ do: 'play' });
    expect(cmd('x', VIEW)).toEqual({ do: 'export' });
    expect(cmd('e', VIEW)).toEqual({ do: 'edit' });
    expect(cmd('Escape', VIEW)).toEqual({ do: 'escape' });
    for (const k of ['c', 'l', '3', 'i', 'ArrowLeft', 'Tab', 'Enter', 'Backspace', 'a', 'm']) expect(cmd(k, VIEW)).toBeNull();
    expect(cmd('z', VIEW, { metaKey: true })).toBeNull();
  });
});

describe('typing', () => {
  it('takes every printable key as text, even the ones that are commands elsewhere', () => {
    for (const k of ['i', 'o', 'p', '0', '9', 'c', 'x', 'e', ' ', 'A', '!', 'é']) expect(cmd(k, TYPING)).toEqual({ do: 'char', ch: k });
  });

  it('ends on Enter or Esc and deletes on Backspace', () => {
    expect(cmd('Enter', TYPING)).toEqual({ do: 'done' });
    expect(cmd('Escape', TYPING)).toEqual({ do: 'done' });
    expect(cmd('Backspace', TYPING)).toEqual({ do: 'backspace' });
  });

  it('starts a new line on Enter in a shape, where a one-row label ends instead', () => {
    const list = { edit: true, typing: true, multiline: true };
    expect(cmd('Enter', list)).toEqual({ do: 'char', ch: '\n' });
    expect(cmd('Escape', list)).toEqual({ do: 'done' });
    expect(cmd('Enter', TYPING)).toEqual({ do: 'done' });
  });

  it('moves a caret with the arrows in a shape, and selects with shift', () => {
    const list = { edit: true, typing: true, multiline: true };
    expect(cmd('ArrowLeft', list)).toEqual({ do: 'caret', dir: 'left', select: false });
    expect(cmd('ArrowRight', list)).toEqual({ do: 'caret', dir: 'right', select: false });
    expect(cmd('ArrowLeft', list, { shiftKey: true })).toEqual({ do: 'caret', dir: 'left', select: true });
    expect(cmd('ArrowUp', list)).toBeNull();
    // A label has no caret to move.
    expect(cmd('ArrowLeft', TYPING)).toBeNull();
  });

  it('styles with ⌥B, ⌥I and ⌥U, by the key and not the symbol a Mac makes of it', () => {
    const list = { edit: true, typing: true, multiline: true };
    expect(cmd('b', list, { altKey: true })).toEqual({ do: 'format', flag: BOLD });
    expect(cmd('i', list, { altKey: true })).toEqual({ do: 'format', flag: ITALIC });
    expect(cmd('U', list, { altKey: true })).toEqual({ do: 'format', flag: UNDERLINE });
    expect(commandFor(press('∫', { altKey: true, code: 'KeyB' }), list)).toEqual({ do: 'format', flag: BOLD });
    expect(commandFor(press('ˆ', { altKey: true, code: 'KeyI' }), list)).toEqual({ do: 'format', flag: ITALIC });
    expect(commandFor(press('¨', { altKey: true, code: 'KeyU' }), list)).toEqual({ do: 'format', flag: UNDERLINE });
    // Without ⌥ they are letters; other ⌥ keys do nothing; and a label has no styles.
    expect(cmd('b', list)).toEqual({ do: 'char', ch: 'b' });
    expect(cmd('x', list, { altKey: true })).toBeNull();
    expect(cmd('b', TYPING, { altKey: true })).toBeNull();
    expect(cmd('b', EDIT, { altKey: true })).toBeNull();
  });

  it('ignores keys that are not text, but still saves', () => {
    expect(cmd('ArrowLeft', TYPING)).toBeNull();
    expect(cmd('Tab', TYPING)).toBeNull();
    expect(cmd('Shift', TYPING)).toBeNull();
    expect(cmd('z', TYPING, { metaKey: true })).toBeNull();
    expect(cmd('s', TYPING, { metaKey: true })).toEqual({ do: 'save' });
  });

  it('knows what is typable', () => {
    expect(isTypable('a')).toBe(true);
    expect(isTypable(' ')).toBe(true);
    expect(isTypable('😀')).toBe(true);
    expect(isTypable('Enter')).toBe(false);
    expect(isTypable('\u007f')).toBe(false);
    expect(isTypable('')).toBe(false);
  });
});

describe('hints', () => {
  const base = { edit: true, typing: false, selected: null, armed: false, playing: true } satisfies Context;
  const contexts: Record<string, Context> = {
    viewing: { ...base, edit: false },
    'nothing selected': base,
    'a shape': { ...base, selected: 'shape' },
    'a line': { ...base, selected: 'line' },
    'several parts': { ...base, selected: 'many' },
    'a tool armed': { ...base, armed: true },
    typing: { ...base, typing: true },
    'typing in a shape': { ...base, typing: true, multiline: true },
  };

  for (const [name, ctx] of Object.entries(contexts)) {
    it(`only shows keys that work: ${name}`, () => {
      const hints = hintsFor(ctx);
      expect(hints.length).toBeGreaterThan(0);
      for (const h of hints) {
        if (h.press === null) continue;
        const got = commandFor(press(h.press, { ...(h.chord ? { metaKey: true } : {}), ...(h.alt ? { altKey: true } : {}) }), ctx);
        expect(got?.do, `${h.label} ${h.word}`).toBe(h.does);
      }
    });
  }

  it('shows only what typing in a shape can do', () => {
    const words = hintsFor({ ...base, typing: true, multiline: true }).map((h) => `${h.label} ${h.word}`);
    expect(words).toEqual(['⌥B bold', '⌥I italic', '⌥U underline', '← → move · ⇧ selects', '⏎ new line', '⌫ delete', 'esc done']);
    // A label can't be styled or moved through, so it doesn't say so.
    const label = hintsFor({ ...base, typing: true }).map((h) => h.label);
    expect(label).toEqual(['⏎', 'esc', '⌫']);
  });

  it('says what Enter does while typing', () => {
    const enter = (multiline: boolean) => hintsFor({ ...base, typing: true, multiline }).find((h) => h.label === '⏎')?.word;
    expect(enter(true)).toBe('new line');
    expect(enter(false)).toBe('done');
  });

  it('says pause while playing and play while paused, when viewing', () => {
    const view = { ...base, edit: false };
    expect(hintsFor(view).find((h) => h.label === 'space')?.word).toBe('pause');
    expect(hintsFor({ ...view, playing: false }).find((h) => h.label === 'space')?.word).toBe('play');
  });

  it('has no play or pause while editing', () => {
    for (const selected of [null, 'shape', 'line', 'many'] as const) {
      expect(hintsFor({ ...base, selected }).some((h) => h.label === 'space')).toBe(false);
    }
    expect(cmd(' ', EDIT)).toBeNull();
  });

  it('offers only what the selection can do', () => {
    const words = (ctx: Context) => hintsFor(ctx).map((h) => h.word);
    expect(words({ ...base, selected: 'shape' })).toContain('font');
    expect(words({ ...base, selected: 'shape' })).not.toContain('arrowheads');
    expect(words({ ...base, selected: 'line' })).toContain('arrowheads');
    expect(words({ ...base, selected: 'line' })).toContain('bend');
    expect(words({ ...base, selected: 'line' })).not.toContain('size');
    expect(words({ ...base, selected: 'many' })).toContain('duplicate');
  });
});
