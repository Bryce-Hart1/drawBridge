<script lang="ts">
  // Claude — Date 11/05/2026
  // A .lin diagram, filling the window. It opens playing: lines pulse. `e`
  // switches to editing, where the keyboard does everything except say where
  // things go: a shape key (c t r d) or `l` arms a tool and the next click
  // places it; a click selects, and dragging moves a shape or a line's end.
  // Arrow keys resize, 0–9 colour, i/o/p pick the font, Enter types into a
  // shape. ⌘S saves the diagram as a new version of the file, like a note;
  // x exports a JPEG. lib/lin/keys.ts has the whole key map.
  //
  // The drawing is lib/lin/draw.ts on a canvas; this component is the glue:
  // load, key and mouse input, the animation clock, save and export.
  import { onDestroy, onMount } from 'svelte';
  import { fail, reload, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { human, plural, splitName } from '../lib/format';
  import { drawDoc, exportJpeg, themeFromCss, type View } from '../lib/lin/draw';
  import {
    History,
    addLine,
    addShape,
    backspace,
    cycleHead,
    cyclePulse,
    describe,
    endAt,
    flip,
    moveEnd,
    moveLineBy,
    moveShape,
    recolor,
    remove,
    resize,
    setFont,
    stepSelection,
    toggleGlow,
    typeChar,
    type Edited,
  } from '../lib/lin/edit';
  import { parseLin, serializeLin } from '../lib/lin/format';
  import { capacity, hitTest, type Pt } from '../lib/lin/geometry';
  import { commandFor, hintsFor, type Command } from '../lib/lin/keys';
  import { blankDoc, find, isLine, type Doc, type End, type ShapeKind } from '../lib/lin/model';
  import { ui } from '../lib/state.svelte';
  import type { Id } from '../lib/types';

  let props: { lin: { id: Id | null; parent: Id | null; name: string; size?: number; edit?: boolean } } = $props();
  // Read once: the prop reads ui.overlay, which turns null as the editor closes.
  // svelte-ignore state_referenced_locally
  const lin = { ...props.lin };

  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)');

  let id = $state(lin.id);
  /** The diagram. Replaced, never changed in place (lib/lin/edit.ts). */
  let doc = $state.raw<Doc>(blankDoc());
  /** The file as last loaded or saved, to tell whether anything changed. */
  let saved = $state(serializeLin(blankDoc()));
  let loading = $state(lin.id !== null);
  let saving = $state(false);
  let exporting = $state(false);
  let edit = $state(lin.edit ?? lin.id === null);
  let typing = $state(false);
  let playing = $state(!reduceMotion.matches);
  let sel = $state<number | null>(null);
  /** A tool waiting for a click. */
  let armed = $state<ShapeKind | 'line' | null>(null);
  /** The line tool's first end, once clicked. */
  let lineFrom = $state<{ end: End; at: Pt } | null>(null);
  let pointer = $state<Pt | null>(null);

  const history = new History();
  const dirty = $derived(serializeLin(doc) !== saved);
  const selPart = $derived(find(doc, sel));
  const selKind = $derived(selPart ? (isLine(selPart) ? 'line' : 'shape') : null);
  const hints = $derived(hintsFor({ edit, typing, selected: selKind, armed: armed !== null, playing }));

  const status = $derived.by(() => {
    if (loading) return 'loading…';
    const shape = selPart && !isLine(selPart) ? selPart : null;
    if (typing && shape) return `typing · ${[...shape.text].length}/${capacity(shape)} characters fit`;
    if (armed === 'line') return lineFrom ? 'click where the line ends' : 'click where the line starts';
    if (armed) return `click to place the ${armed}`;
    if (selPart) return describe(selPart);
    const parts = plural(doc.items.length, 'part');
    if (!edit) return parts;
    return doc.items.length === 0 ? 'an empty page: press c, t, r or d to place a shape, or l for a line' : `${parts} · click one, or press tab, to select it`;
  });

  // ── Canvas ────────────────────────────────────────────────────────────

  let canvas: HTMLCanvasElement | undefined = $state();
  let theme = themeFromCss();
  let width = 0;
  let height = 0;
  /** Page → screen: scale and offset, so the page fits and is centred. */
  let fit = { scale: 1, x: 0, y: 0 };
  let clock = 0;
  let lastFrame = 0;
  let raf = 0;
  let redraw = true;

  function resizeCanvas() {
    if (!canvas) return;
    const box = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    width = box.width;
    height = box.height;
    canvas.width = Math.max(1, Math.round(width * dpr));
    canvas.height = Math.max(1, Math.round(height * dpr));
    redraw = true;
  }

  function paint() {
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || width === 0) return;
    const dpr = window.devicePixelRatio || 1;
    const scale = Math.min(2, width / doc.w, height / doc.h);
    fit = { scale, x: (width - doc.w * scale) / 2, y: (height - doc.h * scale) / 2 };
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = theme.paper;
    ctx.fillRect(0, 0, width, height);
    ctx.setTransform(dpr * scale, 0, 0, dpr * scale, dpr * fit.x, dpr * fit.y);
    // The page.
    ctx.strokeStyle = theme.faint;
    ctx.lineWidth = 1 / scale;
    ctx.strokeRect(0, 0, doc.w, doc.h);
    const view: View = {
      t: clock,
      pulses: true,
      selected: edit ? sel : null,
      theme,
      typing: typing ? sel : null,
    };
    if (edit && pointer && armed) {
      view.ghost = armed === 'line' ? { tool: 'line', from: lineFrom?.at ?? null, to: pointer } : { tool: armed, at: pointer };
    }
    drawDoc(ctx, doc, view);
  }

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    const dt = lastFrame ? (now - lastFrame) / 1000 : 0;
    lastFrame = now;
    const moving = playing && doc.items.some((p) => isLine(p) && p.pulse !== 'off');
    // The clock stands still when paused, and so does the drawing (pulses frozen where they were).
    if (playing) clock += Math.min(dt, 0.1);
    if (moving || redraw) {
      redraw = false;
      paint();
    }
  }

  // Anything on screen changing means a redraw on the next frame.
  $effect(() => {
    void [doc, sel, armed, lineFrom, pointer, edit, typing];
    redraw = true;
  });

  const dark = window.matchMedia('(prefers-color-scheme: dark)');
  function retheme() {
    theme = themeFromCss();
    redraw = true;
  }

  onMount(async () => {
    resizeCanvas();
    raf = requestAnimationFrame(frame);
    dark.addEventListener('change', retheme);
    if (!id) return;
    try {
      const got = await api.noteRead(id);
      if (got.truncated) throw new Error(`${lin.name} is too big to open as a diagram`);
      doc = parseLin(got.text);
      saved = serializeLin(doc);
    } catch (e) {
      fail(e);
      ui.overlay = null;
      return;
    } finally {
      loading = false;
    }
  });

  onDestroy(() => {
    cancelAnimationFrame(raf);
    dark.removeEventListener('change', retheme);
  });

  // ── Doing things ──────────────────────────────────────────────────────

  /** Do `make` to the selection. A refusal says why in the bottom line. */
  function apply(make: (id: number) => Edited) {
    if (sel === null) {
      say('nothing is selected: click a shape or press tab', true);
      return;
    }
    const r = make(sel);
    if ('error' in r) {
      say(r.error, true);
      return;
    }
    if (r.doc !== doc) {
      history.push(doc);
      doc = r.doc;
    }
  }

  /** Where the doc was when typing started, so the whole text is one undo step. */
  let typingBase: Doc | null = null;

  function endTyping() {
    if (!typing) return;
    typing = false;
    if (typingBase && typingBase !== doc) history.push(typingBase);
    typingBase = null;
  }

  function setEdit(on: boolean) {
    if (loading) return;
    endTyping();
    edit = on;
    armed = null;
    lineFrom = null;
    if (!on) sel = null;
  }

  function run(c: Command) {
    switch (c.do) {
      case 'arm':
        armed = c.tool;
        lineFrom = null;
        sel = null;
        break;
      case 'select':
        sel = stepSelection(doc, sel, c.dir);
        break;
      case 'resize':
        apply((i) => resize(doc, i, c.dir, c.big));
        break;
      case 'color':
        apply((i) => recolor(doc, i, c.c));
        break;
      case 'font':
        apply((i) => setFont(doc, i, c.f));
        break;
      case 'type':
        if (selKind !== 'shape') {
          say('select a shape first, then press enter to type in it', true);
          break;
        }
        typing = true;
        typingBase = doc;
        break;
      case 'char':
        if (sel !== null) {
          const r = typeChar(doc, sel, c.ch);
          if ('error' in r) say(r.error, true);
          else doc = r.doc;
        }
        break;
      case 'backspace':
        if (sel !== null) {
          const r = backspace(doc, sel);
          if (!('error' in r)) doc = r.doc;
        }
        break;
      case 'done':
        endTyping();
        break;
      case 'head':
        apply((i) => cycleHead(doc, i));
        break;
      case 'flip':
        apply((i) => flip(doc, i));
        break;
      case 'pulse':
        apply((i) => cyclePulse(doc, i));
        break;
      case 'glow':
        apply((i) => toggleGlow(doc, i));
        break;
      case 'delete':
        if (sel === null) {
          say('nothing is selected', true);
          break;
        }
        history.push(doc);
        doc = remove(doc, sel);
        sel = null;
        break;
      case 'undo':
      case 'redo': {
        const back = c.do === 'undo' ? history.undo(doc) : history.redo(doc);
        if (!back) {
          say(c.do === 'undo' ? 'nothing to undo' : 'nothing to redo');
          break;
        }
        doc = back;
        if (sel !== null && !find(doc, sel)) sel = null;
        break;
      }
      case 'save':
        endTyping();
        void save();
        break;
      case 'export':
        void exportIt();
        break;
      case 'play':
        playing = !playing;
        break;
      case 'edit':
        setEdit(!edit);
        break;
      case 'escape':
        if (armed) {
          armed = null;
          lineFrom = null;
        } else if (sel !== null) sel = null;
        else close();
        break;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // A prompt (discard changes?) has the keys; so does an input it just closed on.
    if (ui.prompt || e.defaultPrevented) return;
    const c = commandFor(e, { edit, typing });
    // Typing eats every key, so Tab or an arrow doesn't wander off.
    if (c || (typing && !e.metaKey && !e.ctrlKey)) e.preventDefault();
    if (c) run(c);
  }

  async function save() {
    if (saving || loading) return;
    if (!dirty && id) {
      say('no changes to save');
      return;
    }
    saving = true;
    const body = serializeLin(doc);
    try {
      id = await api.noteSave(id ? { id } : { parent: lin.parent, name: lin.name }, body);
      saved = body;
      say(`saved ${lin.name}`);
      await reload();
    } catch (e) {
      // Offline, the diagram stays here; like notes, saves aren't queued.
      if ((e as { kind?: string }).kind === 'offline') say(`can't reach the store: ${lin.name} isn't saved yet; ⌘S again once it's back`, true);
      else fail(e);
    } finally {
      saving = false;
    }
  }

  async function exportIt() {
    if (exporting || loading) return;
    exporting = true;
    try {
      // Drawn first: an empty page says so before any dialog.
      const jpeg = await exportJpeg(doc);
      const [stem] = splitName(lin.name);
      let path = await api.pickSavePath(`${stem}.jpg`);
      if (!path) return;
      if (!/\.jpe?g$/i.test(path)) path += '.jpg';
      await api.exportFile(path, jpeg);
      say(`exported ${human(jpeg.length)} to ${path}`);
    } catch (e) {
      fail(e);
    } finally {
      exporting = false;
    }
  }

  function close() {
    if (!dirty) {
      ui.overlay = null;
      return;
    }
    ui.prompt = {
      kind: 'confirm',
      text: `discard your changes to ${lin.name}?`,
      yes: () => (ui.overlay = null),
    };
  }

  // ── Mouse ─────────────────────────────────────────────────────────────

  /** A press on something: where it started, the doc then, and what's being moved. */
  type Drag = { start: Pt; base: Doc; moved: boolean } & (
    | { kind: 'shape'; id: number; dx: number; dy: number }
    | { kind: 'end'; id: number; which: 'a' | 'b' }
    | { kind: 'line'; id: number }
  );
  let drag: Drag | null = null;

  function toPage(e: PointerEvent): Pt {
    const box = canvas!.getBoundingClientRect();
    return { x: (e.clientX - box.left - fit.x) / fit.scale, y: (e.clientY - box.top - fit.y) / fit.scale };
  }

  function onpointerdown(e: PointerEvent) {
    if (!edit || loading || e.button !== 0) return;
    const p = toPage(e);
    endTyping();
    if (armed && armed !== 'line') {
      history.push(doc);
      const added = addShape(doc, armed, p);
      doc = added.doc;
      sel = added.id;
      armed = null;
      return;
    }
    if (armed === 'line') {
      const end = endAt(doc, p);
      if (!lineFrom) {
        lineFrom = { end, at: p };
        return;
      }
      const added = addLine(doc, lineFrom.end, end);
      if ('error' in added) {
        say(added.error, true);
        return;
      }
      history.push(doc);
      doc = added.doc;
      sel = added.id ?? null;
      armed = null;
      lineFrom = null;
      return;
    }
    const hit = hitTest(doc, p, sel);
    sel = hit ? hit.id : null;
    if (!hit) return;
    const part = find(doc, hit.id)!;
    const base = { start: p, base: doc, moved: false };
    if (hit.kind === 'end') drag = { ...base, kind: 'end', id: hit.id, which: hit.which };
    else if (isLine(part)) drag = { ...base, kind: 'line', id: hit.id };
    else drag = { ...base, kind: 'shape', id: hit.id, dx: part.x - p.x, dy: part.y - p.y };
    canvas?.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    const p = toPage(e);
    pointer = p;
    const d = drag;
    if (!d) return;
    // A click that wobbles a pixel isn't a drag. The first real movement is
    // one undo step, from where the doc was when the button went down.
    if (!d.moved) {
      if (Math.hypot(p.x - d.start.x, p.y - d.start.y) < 3) return;
      d.moved = true;
      history.push(d.base);
    }
    if (d.kind === 'shape') doc = moveShape(d.base, d.id, { x: p.x + d.dx, y: p.y + d.dy });
    else if (d.kind === 'end') doc = moveEnd(d.base, d.id, d.which, p);
    else doc = moveLineBy(d.base, d.id, p.x - d.start.x, p.y - d.start.y);
  }

  function onpointerup() {
    drag = null;
  }

  function onpointerleave() {
    pointer = null;
  }

  // The status row's right side.
  const fileState = $derived(saving ? 'saving…' : exporting ? 'exporting…' : dirty ? 'modified' : id ? 'saved' : 'new');
</script>

<svelte:window {onkeydown} onresize={resizeCanvas} />

<div class="overlay full">
  <div class="frame double lin">
    <span class="legend">{lin.name}{dirty ? ' ●' : ''} · {edit ? 'editing' : 'viewing'}</span>
    <div class="stage" class:armed={armed !== null}>
      <!-- The keyboard is handled on the window (the whole editor is keys); the mouse only says where. -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <canvas
        bind:this={canvas}
        aria-label="diagram {lin.name}"
        {onpointerdown}
        {onpointermove}
        {onpointerup}
        {onpointerleave}
      ></canvas>
    </div>
    <div class="row rule-top dim">
      <span class="ellipsis">{status}</span>
      <span class="spacer"></span>
      <span>{fileState}</span>
    </div>
    <div class="row hints dim">
      {#each hints as h (h.label + h.word)}
        <span><span class="key">{h.label}</span> {h.word}</span>
      {/each}
    </div>
  </div>
</div>

<style>
  .stage {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }

  canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    touch-action: none;
  }

  .armed canvas {
    cursor: crosshair;
  }

  /* Many keys: wrap onto more lines, each still a whole number of cells tall. */
  .hints {
    height: auto;
    min-height: var(--cell-h);
    flex-wrap: wrap;
    gap: 0 2ch;
    white-space: normal;
    overflow: visible;
  }

  .hints > span {
    white-space: nowrap;
  }
</style>
