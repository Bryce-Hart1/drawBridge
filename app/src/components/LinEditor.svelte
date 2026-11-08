<script lang="ts">
  // Claude — Date 11/06/2026
  // A .lin diagram, filling the window. It opens playing: lines pulse. `e`
  // switches to editing, where the keyboard does everything except say where
  // things go: a shape key (c t r d) or `l` arms a tool and the next click
  // places it; a click selects (⇧ adds to the selection, dragging out a box
  // selects what it touches), and dragging moves shapes or a line's end.
  // Arrow keys resize, ⌥ arrows move, 0–9 colour, i/o/p pick the font, Enter
  // types into a shape or a line's label. ⌘S saves the diagram as a new
  // version of the file, like a note; x exports a JPEG, PNG or SVG.
  // lib/lin/keys.ts has the whole key map.
  //
  // The drawing is lib/lin/draw.ts on a canvas; this component is the glue:
  // load, key and mouse input, the animation clock, the view (zoom and pan),
  // save and export.
  import { onDestroy, onMount } from 'svelte';
  import { fail, reload, say } from '../lib/actions';
  import { api } from '../lib/api';
  import { human, plural, splitName } from '../lib/format';
  import { drawDoc, exportImage, themeFromCss, type View } from '../lib/lin/draw';
  import { getClip, setClip } from '../lib/lin/clip';
  import {
    History,
    addLine,
    addShape,
    backspace,
    copyParts,
    cycleHead,
    cyclePulse,
    cycleRoute,
    cycleStroke,
    describe,
    duplicate,
    each,
    endAt,
    flip,
    moveBy,
    moveEnd,
    moveLineBy,
    moveShape,
    pasteParts,
    recolor,
    removeMany,
    reorder,
    resize,
    setFont,
    stepSelection,
    toggleGlow,
    typeChar,
    type Edited,
  } from '../lib/lin/edit';
  import { parseLin, serializeLin } from '../lib/lin/format';
  import { bounds, capacity, hitTest, labelCapacity, partsIn, snapTarget, type Box, type Pt } from '../lib/lin/geometry';
  import { commandFor, hintsFor, type Command } from '../lib/lin/keys';
  import { COPY_OFFSET, STEP, blankDoc, find, isLine, type Doc, type End, type ShapeKind } from '../lib/lin/model';
  import { snapPoint, snapShape, toGrid, type Guide } from '../lib/lin/snap';
  import { exportSvg } from '../lib/lin/svg';
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
  /** The selected parts; the last is the one the status row talks about. */
  let sel = $state.raw<number[]>([]);
  /** A tool waiting for a click. */
  let armed = $state<ShapeKind | 'line' | null>(null);
  /** The line tool's first end, once clicked. */
  let lineFrom = $state<{ end: End; at: Pt } | null>(null);
  let pointer = $state<Pt | null>(null);
  let grid = $state(false);
  let guides = $state.raw<Guide[]>([]);
  let marquee = $state.raw<Box | null>(null);
  /** How far in the page is zoomed (1 fits the window), and how far it's been moved. */
  let zoom = $state(1);
  let pan = $state.raw<Pt>({ x: 0, y: 0 });

  const history = new History();
  const dirty = $derived(serializeLin(doc) !== saved);
  const primary = $derived(sel.length > 0 ? sel[sel.length - 1] : null);
  const selPart = $derived(find(doc, primary));
  const selKind = $derived(sel.length > 1 ? 'many' : selPart ? (isLine(selPart) ? 'line' : 'shape') : null);
  /** Typing in a shape, where Enter starts a new line; a line's label is one row. */
  const multiline = $derived(typing && !!selPart && !isLine(selPart));
  const hints = $derived(hintsFor({ edit, typing, multiline, selected: selKind, armed: armed !== null, playing }));

  const status = $derived.by(() => {
    if (loading) return 'loading…';
    if (typing && selPart) {
      const n = [...selPart.text].length;
      return isLine(selPart)
        ? `typing a label · ${n}/${labelCapacity(doc, selPart)} characters fit`
        : `typing · ${n}/${capacity(selPart)} characters fit · enter starts a new line, esc finishes`;
    }
    if (armed === 'line') return lineFrom ? 'click where the line ends' : 'click where the line starts';
    if (armed) return `click to place the ${armed}`;
    if (sel.length > 1) return `${sel.length} selected · drag to move them together`;
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
  /** Page → screen: scale and offset, so the page fits (or is zoomed) and is centred. */
  let fit = { scale: 1, x: 0, y: 0 };
  let clock = 0;
  let lastFrame = 0;
  let raf = 0;
  let redraw = true;

  const MIN_ZOOM = 0.25;
  const MAX_ZOOM = 8;

  function currentFit() {
    const base = width > 0 ? Math.min(2, width / doc.w, height / doc.h) : 1;
    const scale = base * zoom;
    return { scale, x: (width - doc.w * scale) / 2 + pan.x, y: (height - doc.h * scale) / 2 + pan.y };
  }

  /** Zoom by `factor`, keeping the page point under the screen point (sx, sy) where it is. */
  function zoomAt(factor: number, sx: number, sy: number) {
    const z = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * factor));
    if (z === zoom) return;
    const was = currentFit();
    const under = { x: (sx - was.x) / was.scale, y: (sy - was.y) / was.scale };
    zoom = z;
    const now = currentFit();
    pan = { x: pan.x + (sx - (now.x + under.x * now.scale)), y: pan.y + (sy - (now.y + under.y * now.scale)) };
  }

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
    fit = currentFit();
    const { scale } = fit;
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
      selected: edit ? sel : [],
      theme,
      typing: typing ? primary : null,
      px: 1 / scale,
      guides: edit ? guides : [],
      marquee: edit ? marquee : null,
      grid: edit && grid,
    };
    if (edit && pointer && armed) {
      const at = armed === 'line' ? pointer : snapPoint(pointer, grid);
      view.ghost = armed === 'line' ? { tool: 'line', from: lineFrom?.at ?? null, to: at } : { tool: armed, at };
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
    void [doc, sel, armed, lineFrom, pointer, edit, typing, zoom, pan, grid, guides, marquee];
    redraw = true;
  });

  const dark = window.matchMedia('(prefers-color-scheme: dark)');
  function retheme() {
    theme = themeFromCss();
    redraw = true;
  }

  /** The wheel (or a trackpad) moves the page; with ⌘ or Ctrl, or a pinch, it zooms. */
  function onwheel(e: WheelEvent) {
    e.preventDefault();
    const box = canvas!.getBoundingClientRect();
    if (e.ctrlKey || e.metaKey) zoomAt(Math.exp(-e.deltaY * 0.01), e.clientX - box.left, e.clientY - box.top);
    else pan = { x: pan.x - e.deltaX, y: pan.y - e.deltaY };
  }

  onMount(async () => {
    resizeCanvas();
    raf = requestAnimationFrame(frame);
    dark.addEventListener('change', retheme);
    // Not passive: the wheel is ours, and the page behind must not scroll.
    canvas?.addEventListener('wheel', onwheel, { passive: false });
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
    canvas?.removeEventListener('wheel', onwheel);
  });

  // ── Doing things ──────────────────────────────────────────────────────

  /** Do `make` to every selected part. A refusal says why in the bottom line. */
  function apply(make: (doc: Doc, id: number) => Edited) {
    if (sel.length === 0) {
      say('nothing is selected: click a part or press tab', true);
      return;
    }
    take(each(doc, sel, make));
  }

  /** Make a result the new doc (one undo step), or say why not. */
  function take(r: Edited) {
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
    marquee = null;
    guides = [];
    if (!on) sel = [];
  }

  const ARROW: Record<string, Pt> = { left: { x: -1, y: 0 }, right: { x: 1, y: 0 }, up: { x: 0, y: -1 }, down: { x: 0, y: 1 } };

  function run(c: Command) {
    switch (c.do) {
      case 'arm':
        armed = c.tool;
        lineFrom = null;
        sel = [];
        break;
      case 'select': {
        const next = stepSelection(doc, primary, c.dir);
        sel = next === null ? [] : [next];
        break;
      }
      case 'all':
        sel = doc.items.map((p) => p.id);
        if (sel.length === 0) say('an empty page: nothing to select');
        break;
      case 'resize':
        apply((d, i) => resize(d, i, c.dir, c.big));
        break;
      case 'nudge': {
        if (sel.length === 0) {
          say('nothing is selected: click a part or press tab', true);
          break;
        }
        const step = c.fine ? 1 : STEP;
        const moved = moveBy(doc, sel, ARROW[c.dir].x * step, ARROW[c.dir].y * step);
        if (moved === doc) say('at the edge of the page', true);
        else {
          history.push(doc);
          doc = moved;
        }
        break;
      }
      case 'color':
        apply((d, i) => recolor(d, i, c.c));
        break;
      case 'font':
        apply((d, i) => setFont(d, i, c.f));
        break;
      case 'type':
        if (sel.length !== 1 || !selPart) {
          say('select one shape or line first, then press enter to type in it', true);
          break;
        }
        typing = true;
        typingBase = doc;
        break;
      case 'char':
        if (primary !== null) {
          const r = typeChar(doc, primary, c.ch);
          if ('error' in r) say(r.error, true);
          else doc = r.doc;
        }
        break;
      case 'backspace':
        if (primary !== null) {
          const r = backspace(doc, primary);
          if (!('error' in r)) doc = r.doc;
        }
        break;
      case 'done':
        endTyping();
        break;
      case 'head':
        apply(cycleHead);
        break;
      case 'flip':
        apply(flip);
        break;
      case 'pulse':
        apply(cyclePulse);
        break;
      case 'glow':
        apply(toggleGlow);
        break;
      case 'route':
        apply(cycleRoute);
        break;
      case 'style':
        apply(cycleStroke);
        break;
      case 'order':
        if (sel.length === 0) say('nothing is selected', true);
        else take(reorder(doc, sel, c.to));
        break;
      case 'grid':
        grid = !grid;
        say(grid ? 'snapping to a 10 px grid (g turns it off); hold ⌥ while dragging to skip it' : 'grid off');
        break;
      case 'zoom':
        if (c.by === 'fit') {
          zoom = 1;
          pan = { x: 0, y: 0 };
        } else zoomAt(c.by === 'in' ? 1.25 : 0.8, width / 2, height / 2);
        break;
      case 'copy':
        if (sel.length === 0) {
          say('nothing is selected', true);
          break;
        }
        setClip(copyParts(doc, sel));
        say(`copied ${plural(sel.length, 'part')}; ⌘V pastes where the pointer is`);
        break;
      case 'paste': {
        const clip = getClip();
        if (clip.length === 0) {
          say('nothing copied yet: select something and press ⌘C', true);
          break;
        }
        // Where the pointer is, if it's on the page; else a little off from the original.
        const box = bounds({ ...doc, items: [...clip] });
        let delta: Pt = { x: COPY_OFFSET, y: COPY_OFFSET };
        if (box && pointer && pointer.x >= 0 && pointer.y >= 0 && pointer.x <= doc.w && pointer.y <= doc.h) {
          const to = snapPoint(pointer, grid);
          delta = { x: to.x - (box.x + box.w / 2), y: to.y - (box.y + box.h / 2) };
        }
        const r = pasteParts(doc, clip, delta);
        history.push(doc);
        doc = r.doc;
        sel = r.ids;
        break;
      }
      case 'duplicate': {
        const r = duplicate(doc, sel);
        if ('error' in r) {
          say(r.error, true);
          break;
        }
        history.push(doc);
        doc = r.doc;
        sel = r.ids;
        break;
      }
      case 'delete':
        if (sel.length === 0) {
          say('nothing is selected', true);
          break;
        }
        history.push(doc);
        doc = removeMany(doc, sel);
        sel = [];
        break;
      case 'undo':
      case 'redo': {
        const back = c.do === 'undo' ? history.undo(doc) : history.redo(doc);
        if (!back) {
          say(c.do === 'undo' ? 'nothing to undo' : 'nothing to redo');
          break;
        }
        doc = back;
        sel = sel.filter((i) => find(doc, i));
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
        } else if (sel.length > 0) sel = [];
        else close();
        break;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // A prompt (discard changes?) has the keys; so does an input it just closed on.
    if (ui.prompt || e.defaultPrevented) return;
    const c = commandFor(e, { edit, typing, multiline });
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

  /** x: choose where to save, and the file's extension chooses the format: .jpg, .png or .svg. */
  async function exportIt() {
    if (exporting || loading) return;
    exporting = true;
    try {
      // An empty page says so before any dialog.
      if (!bounds(doc)) throw new Error('nothing to export: the page is empty');
      const [stem] = splitName(lin.name);
      let path = await api.pickSavePath(`${stem}.jpg`);
      if (!path) return;
      if (!/\.(jpe?g|png|svg)$/i.test(path)) path += '.jpg';
      const bytes = /\.svg$/i.test(path)
        ? new TextEncoder().encode(exportSvg(doc))
        : await exportImage(doc, /\.png$/i.test(path) ? 'png' : 'jpeg');
      await api.exportFile(path, bytes);
      say(`exported ${human(bytes.length)} to ${path}`);
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
    | { kind: 'many'; ids: number[] }
  );
  let drag: Drag | null = null;
  /** Dragging out a selection box: the selection it started from. */
  let boxing: { start: Pt; from: number[] } | null = null;
  /** Dragging the page itself: the pan it started from and the screen point. */
  let panning: { sx: number; sy: number; from: Pt } | null = null;

  function toPage(e: PointerEvent): Pt {
    const box = canvas!.getBoundingClientRect();
    const f = currentFit();
    return { x: (e.clientX - box.left - f.x) / f.scale, y: (e.clientY - box.top - f.y) / f.scale };
  }

  function onpointerdown(e: PointerEvent) {
    if (loading) return;
    // Middle button anywhere, or any drag while just viewing, moves the page.
    if (e.button === 1 || (!edit && e.button === 0)) {
      panning = { sx: e.clientX, sy: e.clientY, from: pan };
      canvas?.setPointerCapture(e.pointerId);
      e.preventDefault();
      return;
    }
    if (!edit || e.button !== 0) return;
    const p = toPage(e);
    endTyping();
    if (armed && armed !== 'line') {
      history.push(doc);
      const added = addShape(doc, armed, snapPoint(p, grid));
      doc = added.doc;
      sel = [added.id];
      armed = null;
      return;
    }
    if (armed === 'line') {
      const end = endAt(doc, snapTarget(doc, p) ? p : snapPoint(p, grid));
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
      sel = added.id ? [added.id] : [];
      armed = null;
      lineFrom = null;
      return;
    }
    const hit = hitTest(doc, p, sel.length === 1 ? sel[0] : null, Math.max(1, 1 / currentFit().scale));
    if (e.shiftKey) {
      // ⇧ adds a part to the selection, or takes it out; it never starts a drag.
      if (hit) sel = sel.includes(hit.id) ? sel.filter((i) => i !== hit.id) : [...sel, hit.id];
      else startBox(e, p, sel);
      return;
    }
    if (!hit) {
      sel = [];
      startBox(e, p, []);
      return;
    }
    const part = find(doc, hit.id)!;
    const base = { start: p, base: doc, moved: false };
    if (hit.kind === 'end') {
      sel = [hit.id];
      drag = { ...base, kind: 'end', id: hit.id, which: hit.which };
    } else if (sel.length > 1 && sel.includes(hit.id)) {
      // Pressing on one of several selected parts moves them all.
      drag = { ...base, kind: 'many', ids: sel };
    } else {
      sel = [hit.id];
      drag = isLine(part) ? { ...base, kind: 'line', id: hit.id } : { ...base, kind: 'shape', id: hit.id, dx: part.x - p.x, dy: part.y - p.y };
    }
    canvas?.setPointerCapture(e.pointerId);
  }

  function startBox(e: PointerEvent, p: Pt, from: number[]) {
    boxing = { start: p, from };
    canvas?.setPointerCapture(e.pointerId);
  }

  function onpointermove(e: PointerEvent) {
    if (panning) {
      pan = { x: panning.from.x + e.clientX - panning.sx, y: panning.from.y + e.clientY - panning.sy };
      return;
    }
    const p = toPage(e);
    pointer = p;
    if (boxing) {
      const x = Math.min(boxing.start.x, p.x);
      const y = Math.min(boxing.start.y, p.y);
      marquee = { x, y, w: Math.abs(p.x - boxing.start.x), h: Math.abs(p.y - boxing.start.y) };
      const touched = partsIn(doc, marquee);
      sel = [...boxing.from, ...touched.filter((i) => !boxing!.from.includes(i))];
      return;
    }
    const d = drag;
    if (!d) return;
    // A click that wobbles a pixel isn't a drag. The first real movement is
    // one undo step, from where the doc was when the button went down.
    if (!d.moved) {
      if (Math.hypot(p.x - d.start.x, p.y - d.start.y) * fit.scale < 3) return;
      d.moved = true;
      history.push(d.base);
    }
    // ⌥ while dragging turns snapping off.
    const snap = !e.altKey;
    if (d.kind === 'shape') {
      const target = { x: p.x + d.dx, y: p.y + d.dy };
      const r = snap ? snapShape(d.base, d.id, target, grid) : { at: target, guides: [] };
      guides = r.guides;
      doc = moveShape(d.base, d.id, r.at);
    } else if (d.kind === 'end') {
      doc = moveEnd(d.base, d.id, d.which, snap && !snapTarget(d.base, p) ? snapPoint(p, grid) : p);
    } else if (d.kind === 'many') {
      const g = snap && grid;
      doc = moveBy(d.base, d.ids, g ? toGrid(p.x - d.start.x) : p.x - d.start.x, g ? toGrid(p.y - d.start.y) : p.y - d.start.y);
    } else {
      doc = moveLineBy(d.base, d.id, p.x - d.start.x, p.y - d.start.y);
    }
  }

  function onpointerup() {
    drag = null;
    boxing = null;
    panning = null;
    marquee = null;
    guides = [];
  }

  function onpointerleave() {
    pointer = null;
  }

  // The status row's right side.
  const fileState = $derived(saving ? 'saving…' : exporting ? 'exporting…' : dirty ? 'modified' : id ? 'saved' : 'new');
  const viewState = $derived(
    [grid && edit ? 'grid' : '', zoom !== 1 ? `${Math.round(zoom * 100)}%` : ''].filter(Boolean).join(' · '),
  );
</script>

<svelte:window {onkeydown} onresize={resizeCanvas} />

<div class="overlay full">
  <div class="frame double lin">
    <span class="legend">{lin.name}{dirty ? ' ●' : ''} · {edit ? 'editing' : 'viewing'}</span>
    <div class="stage" class:armed={armed !== null} class:viewing={!edit}>
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
      {#if viewState}<span>{viewState} ·&nbsp;</span>{/if}
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

  .viewing canvas {
    cursor: grab;
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
