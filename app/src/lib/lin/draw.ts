// Claude — Date 11/05/2026
// Drawing a diagram onto a canvas. The editor, the animated viewer and the
// JPEG export all use `drawDoc`, so what you see is what you export (minus the
// selection and the pulses). Coordinates are the page's; the caller sets the
// canvas transform to scale and place the page.

import {
  bounds,
  caretCell,
  grid,
  layoutIndexed,
  LABEL_EM,
  labelWidth,
  lineEnds,
  linePath,
  lineHeight,
  midpoint,
  pathLength,
  pointAt,
  snapTarget,
  subPath,
  textBox,
  tipFrom,
  type Box,
  type Pt,
} from './geometry';
import {
  ADVANCE,
  BOLD,
  FONT_PX,
  GRID,
  ITALIC,
  UNDERLINE,
  PALETTE,
  isLine,
  newShape,
  tint,
  type Doc,
  type Line,
  type Shape,
  type ShapeKind,
  type Stroke,
} from './model';
import { LIGHT_LEN, pulseAt } from './pulse';
import { span, stylesOf } from './text';
import type { Guide } from './snap';

export interface Theme {
  paper: string;
  ink: string;
  dim: string;
  faint: string;
  /** Selection, and the ball on an ink line. */
  accent: string;
}

/** A white page, for the JPEG: it looks the same whatever the app's theme is. */
export const LIGHT: Theme = { paper: '#ffffff', ink: '#111111', dim: '#6f6f6f', faint: '#d6d6d2', accent: '#1f5fbf' };

/** The app's current colours (tui.css), so the canvas follows light and dark mode. */
export function themeFromCss(): Theme {
  const css = getComputedStyle(document.documentElement);
  const get = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    paper: get('--paper', LIGHT.paper),
    ink: get('--ink', LIGHT.ink),
    dim: get('--dim', LIGHT.dim),
    faint: get('--faint', LIGHT.faint),
    accent: get('--syn-fn', LIGHT.accent),
  };
}

export interface View {
  /** Seconds into the animation. */
  t: number;
  /** Draw the pulses, at time `t`. Paused is `t` standing still. */
  pulses: boolean;
  /** The selected parts. One selected line also shows its end handles. */
  selected: readonly number[];
  /** The shape or line being typed into, which shows a caret. */
  typing?: number | null;
  /** Where the caret is in that text, and the other end of what's selected there. Shapes only: a label's caret is always at its end. */
  caret?: { at: number; anchor: number } | null;
  theme: Theme;
  /** Page pixels in one screen pixel (1 over the zoom), so handles stay a steady size. */
  px?: number;
  /** Alignment guides while a shape is dragged. */
  guides?: readonly Guide[];
  /** The box being dragged out to select. */
  marquee?: Box | null;
  /** Show the snap grid. */
  grid?: boolean;
  /** A shape or line tool is waiting for a click: where it would land. */
  ghost?: { tool: ShapeKind; at: Pt } | { tool: 'line'; from: Pt | null; to: Pt };
}

const FONT = 'ui-monospace, "SF Mono", Menlo, Monaco, Consolas, monospace';
const STROKE = 2;
const LINE_W = 2.5;
const TUBE_W = 5;
const HEAD = 14;
const BALL = 6;

/**
 * The dash pattern for an outline `w` px thick; dots are zero-length dashes
 * with round caps, so they come out `w` across. The gaps grow with the
 * thickness so a thick dotted line is still dots and not a solid one.
 */
function dashes(ln: Stroke, w = 2): number[] {
  const gap = Math.max(6, w * 2.4);
  return ln === 'dash' ? [Math.max(10, w * 4), gap] : ln === 'dot' ? [0.1, gap] : [];
}

function strokePath(ctx: CanvasRenderingContext2D, pts: readonly Pt[]) {
  ctx.beginPath();
  pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
  ctx.stroke();
}

function shapePath(ctx: CanvasRenderingContext2D, s: Shape) {
  const hw = s.w / 2;
  const hh = s.h / 2;
  ctx.beginPath();
  switch (s.t) {
    case 'rect':
      ctx.rect(s.x - hw, s.y - hh, s.w, s.h);
      break;
    case 'circle':
      ctx.ellipse(s.x, s.y, hw, hh, 0, 0, Math.PI * 2);
      break;
    case 'diamond':
      ctx.moveTo(s.x, s.y - hh);
      ctx.lineTo(s.x + hw, s.y);
      ctx.lineTo(s.x, s.y + hh);
      ctx.lineTo(s.x - hw, s.y);
      ctx.closePath();
      break;
    case 'triangle':
      ctx.moveTo(s.x, s.y - hh);
      ctx.lineTo(s.x + hw, s.y + hh);
      ctx.lineTo(s.x - hw, s.y + hh);
      ctx.closePath();
      break;
  }
}

/** The font for letters with this style: bold, italic, both, or neither. */
export function fontFor(px: number, style: number): string {
  return `${style & ITALIC ? 'italic ' : ''}${style & BOLD ? 'bold ' : ''}${px}px ${FONT}`;
}

/** Stretches of a row (by position in the row) that share one style: [from, to, style]. */
export function runsOf(row: readonly number[], styles: readonly number[]): [number, number, number][] {
  const runs: [number, number, number][] = [];
  for (let k = 0; k < row.length; ) {
    const st = styles[row[k]] ?? 0;
    let m = k + 1;
    while (m < row.length && (styles[row[m]] ?? 0) === st) m++;
    runs.push([k, m, st]);
    k = m;
  }
  return runs;
}

function drawShape(ctx: CanvasRenderingContext2D, s: Shape, theme: Theme, typing = false, caret: View['caret'] = null) {
  const plain = s.c === 0;
  const colour = PALETTE[s.c].fill;
  shapePath(ctx, s);
  // A colour is a see-through wash with a full-strength outline; plain is the page's own colour.
  ctx.fillStyle = plain ? theme.paper : tint(colour);
  ctx.fill();
  ctx.lineWidth = STROKE;
  ctx.lineJoin = 'round';
  ctx.strokeStyle = plain ? theme.ink : colour;
  ctx.setLineDash(dashes(s.ln));
  ctx.lineCap = s.ln === 'dot' ? 'round' : 'butt';
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.lineCap = 'butt';

  if (s.text === '' && !typing) return;
  const px = FONT_PX[s.f];
  const adv = px * ADVANCE;
  const lh = lineHeight(s);
  const rows = layoutIndexed(s);
  const chars = [...s.text];
  const styles = stylesOf(s);
  const { dy } = textBox(s);
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  // An empty shape being typed in still has one row, for the caret.
  const count = Math.max(rows.length, 1);
  const top = s.y + dy - (count * lh) / 2 + lh / 2;
  const left = (row: number) => s.x - ((rows[row]?.length ?? 0) * adv) / 2;

  // What's selected, behind the letters.
  const [from, to] = caret ? span({ ...caret, pend: null }) : [0, 0];
  if (typing && from !== to) {
    ctx.fillStyle = theme.accent;
    ctx.globalAlpha = 0.3;
    rows.forEach((row, r) => {
      const first = row.findIndex((i) => i >= from && i < to);
      if (first < 0) return;
      let last = first;
      while (last + 1 < row.length && row[last + 1] < to) last++;
      ctx.fillRect(left(r) + first * adv, top + r * lh - lh / 2, (last - first + 1) * adv, lh);
    });
    ctx.globalAlpha = 1;
  }

  // The letters, a run at a time: each run is centred over the columns it covers, so a bold word lines up with its neighbours.
  ctx.fillStyle = theme.ink;
  ctx.strokeStyle = theme.ink;
  rows.forEach((row, r) => {
    const y = top + r * lh;
    for (const [a, b, st] of runsOf(row, styles)) {
      ctx.font = fontFor(px, st);
      ctx.fillText(row.slice(a, b).map((i) => chars[i]).join(''), left(r) + ((a + b) / 2) * adv, y);
      if (st & UNDERLINE) {
        ctx.lineWidth = Math.max(1, px / 14);
        ctx.beginPath();
        ctx.moveTo(left(r) + a * adv, y + px * 0.5);
        ctx.lineTo(left(r) + b * adv, y + px * 0.5);
        ctx.stroke();
      }
    }
  });

  if (typing) {
    const cell = caretCell(s.text, grid(s).cols, caret ? caret.at : chars.length);
    const x = left(cell.row) + cell.col * adv;
    ctx.fillRect(x - 0.75, top + cell.row * lh - px * 0.55, 1.5, px * 1.1);
  }
}

function head(ctx: CanvasRenderingContext2D, tip: Pt, from: Pt, color: string) {
  const angle = Math.atan2(tip.y - from.y, tip.x - from.x);
  ctx.beginPath();
  ctx.moveTo(tip.x, tip.y);
  ctx.lineTo(tip.x - HEAD * Math.cos(angle - 0.4), tip.y - HEAD * Math.sin(angle - 0.4));
  ctx.lineTo(tip.x - HEAD * Math.cos(angle + 0.4), tip.y - HEAD * Math.sin(angle + 0.4));
  ctx.closePath();
  ctx.fillStyle = color;
  ctx.fill();
}

function drawLabel(ctx: CanvasRenderingContext2D, l: Line, path: readonly Pt[], theme: Theme, typing: boolean) {
  if (l.text === '' && !typing) return;
  const px = FONT_PX[l.f];
  const at = midpoint(path);
  const w = Math.max(labelWidth(l), typing ? px * 0.6 : 0);
  // The label sits on a patch of page so the line doesn't run through the words.
  ctx.fillStyle = theme.paper;
  ctx.fillRect(at.x - w / 2 - 4, at.y - (px * LABEL_EM) / 2, w + 8, px * LABEL_EM);
  ctx.font = `${px}px ${FONT}`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillStyle = theme.ink;
  ctx.fillText(l.text, at.x, at.y);
  if (typing) ctx.fillRect(at.x + labelWidth(l) / 2 + 1, at.y - px * 0.55, 1.5, px * 1.1);
}

function drawLine(ctx: CanvasRenderingContext2D, doc: Doc, l: Line, view: View) {
  const path = linePath(doc, l);
  const len = pathLength(path);
  if (len < 1) return;
  const a = path[0];
  const b = path[path.length - 1];
  const { theme } = view;
  const plain = l.c === 0;
  const color = plain ? theme.ink : PALETTE[l.c].fill;
  const pulsing = view.pulses && l.pulse !== 'off';
  // A light needs a line thick enough to have an inside.
  const tube = pulsing && l.glow === 'light';

  if (view.selected.includes(l.id)) {
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.strokeStyle = theme.accent;
    ctx.globalAlpha = 0.35;
    ctx.lineWidth = TUBE_W + 6;
    strokePath(ctx, path);
    ctx.globalAlpha = 1;
  }

  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  ctx.lineWidth = tube ? TUBE_W : LINE_W;
  ctx.strokeStyle = color;
  ctx.setLineDash(dashes(l.ln, ctx.lineWidth));
  strokePath(ctx, path);
  ctx.setLineDash([]);

  if (l.head !== 'none') head(ctx, b, tipFrom(path, true), color);
  if (l.head === 'both') head(ctx, a, tipFrom(path, false), color);

  if (pulsing) drawPulses(ctx, l, path, len, view, color, plain);
  drawLabel(ctx, l, path, theme, view.typing === l.id);
}

function drawPulses(ctx: CanvasRenderingContext2D, l: Line, path: readonly Pt[], len: number, view: View, color: string, plain: boolean) {
  const { theme } = view;
  const at = (u: number): Pt => pointAt(path, u * len);
  if (l.glow === 'ball') {
    for (const u of pulseAt(l.pulse, view.t, len)) {
      const p = at(u);
      ctx.beginPath();
      ctx.arc(p.x, p.y, BALL, 0, Math.PI * 2);
      ctx.fillStyle = plain ? theme.accent : color;
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = theme.paper;
      ctx.stroke();
    }
    return;
  }
  // A light: a short bright stretch of the line, fading out at both ends.
  const half = LIGHT_LEN / 2;
  const bright = plain ? theme.paper : '#ffffff';
  ctx.lineCap = 'butt';
  ctx.lineWidth = TUBE_W - 2;
  for (const u of pulseAt(l.pulse, view.t, len, half)) {
    const centre = u * len;
    const from = Math.max(0, centre - half);
    const to = Math.min(len, centre + half);
    // The part of the light that's on the line (it starts and ends off it).
    if (from >= to) continue;
    // The gradient spans the whole light, so a half that's still off the line shows as a fade-in.
    const lo = pointAt(path, centre - half);
    const hi = pointAt(path, centre + half);
    const g = ctx.createLinearGradient(lo.x, lo.y, hi.x, hi.y);
    g.addColorStop(0, 'rgba(255,255,255,0)');
    g.addColorStop(0.5, bright);
    g.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.strokeStyle = g;
    strokePath(ctx, subPath(path, from, to));
  }
}

function drawSelection(ctx: CanvasRenderingContext2D, doc: Doc, view: View) {
  const { accent } = view.theme;
  const px = view.px ?? 1;
  for (const id of view.selected) {
    const part = doc.items.find((p) => p.id === id);
    if (!part) continue;
    if (isLine(part)) {
      // End handles only when the line is the whole selection: they're what you grab.
      if (view.selected.length !== 1) continue;
      const { a, b } = lineEnds(doc, part);
      ctx.fillStyle = accent;
      ctx.strokeStyle = view.theme.paper;
      ctx.lineWidth = 2 * px;
      const r = 5 * px;
      for (const p of [a, b]) {
        ctx.beginPath();
        ctx.rect(p.x - r, p.y - r, 2 * r, 2 * r);
        ctx.fill();
        ctx.stroke();
      }
      continue;
    }
    ctx.strokeStyle = accent;
    ctx.lineWidth = 1.5 * px;
    ctx.setLineDash([6 * px, 4 * px]);
    ctx.strokeRect(part.x - part.w / 2 - 6, part.y - part.h / 2 - 6, part.w + 12, part.h + 12);
    ctx.setLineDash([]);
  }
}

function drawGuides(ctx: CanvasRenderingContext2D, view: View) {
  if (!view.guides || view.guides.length === 0) return;
  const px = view.px ?? 1;
  ctx.strokeStyle = view.theme.accent;
  ctx.lineWidth = px;
  ctx.setLineDash([4 * px, 3 * px]);
  for (const g of view.guides) {
    ctx.beginPath();
    if (g.axis === 'x') {
      ctx.moveTo(g.at, g.from - 12);
      ctx.lineTo(g.at, g.to + 12);
    } else {
      ctx.moveTo(g.from - 12, g.at);
      ctx.lineTo(g.to + 12, g.at);
    }
    ctx.stroke();
  }
  ctx.setLineDash([]);
}

function drawGrid(ctx: CanvasRenderingContext2D, doc: Doc, view: View) {
  const px = view.px ?? 1;
  // Dots closer than a few screen pixels are just a haze.
  if (GRID / px < 5) return;
  ctx.fillStyle = view.theme.faint;
  ctx.beginPath();
  for (let x = GRID; x < doc.w; x += GRID) for (let y = GRID; y < doc.h; y += GRID) ctx.rect(x - px / 2, y - px / 2, px, px);
  ctx.fill();
}

function drawGhost(ctx: CanvasRenderingContext2D, doc: Doc, view: View) {
  const ghost = view.ghost;
  if (!ghost) return;
  const { theme } = view;
  if (ghost.tool === 'line') {
    // A dashed rubber band, and the shape an end would attach to lit up.
    if (ghost.from) {
      ctx.strokeStyle = theme.accent;
      ctx.lineWidth = 2;
      ctx.setLineDash([6, 4]);
      ctx.beginPath();
      ctx.moveTo(ghost.from.x, ghost.from.y);
      ctx.lineTo(ghost.to.x, ghost.to.y);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    const target = snapTarget(doc, ghost.to);
    if (target) {
      ctx.strokeStyle = theme.accent;
      ctx.lineWidth = 3;
      shapePath(ctx, target);
      ctx.stroke();
    } else {
      ctx.fillStyle = theme.accent;
      ctx.beginPath();
      ctx.arc(ghost.to.x, ghost.to.y, 4, 0, Math.PI * 2);
      ctx.fill();
    }
    return;
  }
  ctx.globalAlpha = 0.45;
  drawShape(ctx, newShape(0, ghost.tool, ghost.at.x, ghost.at.y), theme);
  ctx.globalAlpha = 1;
}

/** Everything on the page, back to front, then the selection and any ghost on top. */
export function drawDoc(ctx: CanvasRenderingContext2D, doc: Doc, view: View) {
  if (view.grid) drawGrid(ctx, doc, view);
  for (const part of doc.items) {
    if (isLine(part)) drawLine(ctx, doc, part, view);
    else drawShape(ctx, part, view.theme, view.typing === part.id, view.typing === part.id ? view.caret : null);
  }
  drawSelection(ctx, doc, view);
  drawGuides(ctx, view);
  drawGhost(ctx, doc, view);
  if (view.marquee) {
    const m = view.marquee;
    ctx.fillStyle = view.theme.accent;
    ctx.globalAlpha = 0.08;
    ctx.fillRect(m.x, m.y, m.w, m.h);
    ctx.globalAlpha = 1;
    ctx.strokeStyle = view.theme.accent;
    ctx.lineWidth = view.px ?? 1;
    ctx.strokeRect(m.x, m.y, m.w, m.h);
  }
}

const EXPORT_SCALE = 2;
const EXPORT_MARGIN = 32;
const EXPORT_MIN = 120;

/** The diagram drawn on white, cropped to what's on the page plus a margin, at `scale`. */
function render(doc: Doc, scale: number, flatten = true): HTMLCanvasElement {
  const box = bounds(doc);
  if (!box) throw new Error('nothing to export: the page is empty');
  const w = Math.max(EXPORT_MIN, box.w + 2 * EXPORT_MARGIN);
  const h = Math.max(EXPORT_MIN, box.h + 2 * EXPORT_MARGIN);
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.ceil(w * scale));
  canvas.height = Math.max(1, Math.ceil(h * scale));
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error("can't draw here");
  if (flatten) {
    ctx.fillStyle = LIGHT.paper;
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  }
  ctx.scale(scale, scale);
  // Centre what's drawn in the crop.
  ctx.translate((w - box.w) / 2 - box.x, (h - box.h) / 2 - box.y);
  drawDoc(ctx, doc, { t: 0, pulses: false, selected: [], theme: LIGHT });
  return canvas;
}

async function blobBytes(canvas: HTMLCanvasElement, type: string, quality?: number): Promise<Uint8Array> {
  // Through a blob rather than a data URL: the app's content-security policy doesn't let the page fetch either.
  const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, type, quality));
  if (!blob) throw new Error(`couldn't make the ${type.slice(6).toUpperCase()}`);
  return new Uint8Array(await blob.arrayBuffer());
}

/**
 * The diagram as an image: cropped to what's on the page plus a margin, at
 * twice the size, on white, in the plain light colours. No selection, no
 * pulses. A JPEG, or a PNG with a white background.
 */
export async function exportImage(doc: Doc, kind: 'jpeg' | 'png'): Promise<Uint8Array> {
  const canvas = render(doc, EXPORT_SCALE);
  return kind === 'png' ? blobBytes(canvas, 'image/png') : blobBytes(canvas, 'image/jpeg', 0.92);
}

export const exportJpeg = (doc: Doc) => exportImage(doc, 'jpeg');

/** A small JPEG of the diagram, at most `side` pixels across, for a folder's thumbnail. */
export async function thumbnail(doc: Doc, side: number): Promise<Uint8Array> {
  const box = bounds(doc);
  if (!box) throw new Error('empty');
  const w = Math.max(EXPORT_MIN, box.w + 2 * EXPORT_MARGIN);
  const h = Math.max(EXPORT_MIN, box.h + 2 * EXPORT_MARGIN);
  return blobBytes(render(doc, Math.min(1, side / Math.max(w, h))), 'image/jpeg', 0.8);
}
