// Claude — Date 11/05/2026
// Drawing a diagram onto a canvas. The editor, the animated viewer and the
// JPEG export all use `drawDoc`, so what you see is what you export (minus the
// selection and the pulses). Coordinates are the page's; the caller sets the
// canvas transform to scale and place the page.

import { bounds, layout, lineEnds, lineHeight, snapTarget, textBox, type Pt } from './geometry';
import {
  FONT_PX,
  PALETTE,
  isLine,
  newShape,
  tint,
  type Doc,
  type Line,
  type Shape,
  type ShapeKind,
} from './model';
import { LIGHT_LEN, pulseAt } from './pulse';

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
  selected: number | null;
  /** The shape being typed into, which shows a caret. */
  typing?: number | null;
  theme: Theme;
  /** A shape or line tool is waiting for a click: where it would land. */
  ghost?: { tool: ShapeKind; at: Pt } | { tool: 'line'; from: Pt | null; to: Pt };
}

const FONT = 'ui-monospace, "SF Mono", Menlo, Monaco, Consolas, monospace';
const STROKE = 2;
const LINE_W = 2.5;
const TUBE_W = 5;
const HEAD = 14;
const BALL = 6;

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

function drawShape(ctx: CanvasRenderingContext2D, s: Shape, theme: Theme, typing = false) {
  const plain = s.c === 0;
  const colour = PALETTE[s.c].fill;
  shapePath(ctx, s);
  // A colour is a see-through wash with a full-strength outline; plain is the page's own colour.
  ctx.fillStyle = plain ? theme.paper : tint(colour);
  ctx.fill();
  ctx.lineWidth = STROKE;
  ctx.lineJoin = 'round';
  ctx.strokeStyle = plain ? theme.ink : colour;
  ctx.stroke();

  if (s.text === '' && !typing) return;
  const px = FONT_PX[s.f];
  const lh = lineHeight(s);
  const lines = layout(s);
  const { dy } = textBox(s);
  ctx.font = `${px}px ${FONT}`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillStyle = theme.ink;
  const top = s.y + dy - (lines.length * lh) / 2 + lh / 2;
  lines.forEach((line, i) => ctx.fillText(line, s.x, top + i * lh));
  if (typing) {
    // A caret after the last character.
    const last = lines.length > 0 ? lines[lines.length - 1] : '';
    const x = s.x + ctx.measureText(last).width / 2 + 1;
    ctx.fillRect(x, top + Math.max(lines.length - 1, 0) * lh - px * 0.55, 1.5, px * 1.1);
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

function drawLine(ctx: CanvasRenderingContext2D, doc: Doc, l: Line, view: View) {
  const { a, b } = lineEnds(doc, l);
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1) return;
  const { theme } = view;
  const plain = l.c === 0;
  const color = plain ? theme.ink : PALETTE[l.c].fill;
  const pulsing = view.pulses && l.pulse !== 'off';
  // A light needs a line thick enough to have an inside.
  const tube = pulsing && l.glow === 'light';

  if (view.selected === l.id) {
    ctx.lineCap = 'round';
    ctx.strokeStyle = theme.accent;
    ctx.globalAlpha = 0.35;
    ctx.lineWidth = TUBE_W + 6;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }

  ctx.lineCap = 'round';
  ctx.lineWidth = tube ? TUBE_W : LINE_W;
  ctx.strokeStyle = color;
  ctx.beginPath();
  ctx.moveTo(a.x, a.y);
  ctx.lineTo(b.x, b.y);
  ctx.stroke();

  if (l.head !== 'none') head(ctx, b, a, color);
  if (l.head === 'both') head(ctx, a, b, color);

  if (!pulsing) return;
  const at = (u: number): Pt => ({ x: a.x + (b.x - a.x) * u, y: a.y + (b.y - a.y) * u });
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
    // The part of the light that's on the line (it starts and ends off it).
    const on0 = at(Math.max(0, centre - half) / len);
    const on1 = at(Math.min(len, centre + half) / len);
    if (Math.max(0, centre - half) >= Math.min(len, centre + half)) continue;
    // The gradient spans the whole light, so a half that's still off the line shows as a fade-in.
    const lo = at((centre - half) / len);
    const hi = at((centre + half) / len);
    const g = ctx.createLinearGradient(lo.x, lo.y, hi.x, hi.y);
    g.addColorStop(0, 'rgba(255,255,255,0)');
    g.addColorStop(0.5, bright);
    g.addColorStop(1, 'rgba(255,255,255,0)');
    ctx.strokeStyle = g;
    ctx.beginPath();
    ctx.moveTo(on0.x, on0.y);
    ctx.lineTo(on1.x, on1.y);
    ctx.stroke();
  }
}

function drawSelection(ctx: CanvasRenderingContext2D, doc: Doc, view: View) {
  const part = doc.items.find((p) => p.id === view.selected);
  if (!part) return;
  const { accent } = view.theme;
  if (isLine(part)) {
    const { a, b } = lineEnds(doc, part);
    ctx.fillStyle = accent;
    ctx.strokeStyle = view.theme.paper;
    ctx.lineWidth = 2;
    for (const p of [a, b]) {
      ctx.beginPath();
      ctx.rect(p.x - 5, p.y - 5, 10, 10);
      ctx.fill();
      ctx.stroke();
    }
    return;
  }
  ctx.strokeStyle = accent;
  ctx.lineWidth = 1.5;
  ctx.setLineDash([6, 4]);
  ctx.strokeRect(part.x - part.w / 2 - 6, part.y - part.h / 2 - 6, part.w + 12, part.h + 12);
  ctx.setLineDash([]);
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
  for (const part of doc.items) {
    if (isLine(part)) drawLine(ctx, doc, part, view);
    else drawShape(ctx, part, view.theme, view.typing === part.id);
  }
  drawSelection(ctx, doc, view);
  drawGhost(ctx, doc, view);
}

const EXPORT_SCALE = 2;
const EXPORT_MARGIN = 32;
const EXPORT_MIN = 120;

/**
 * The diagram as JPEG bytes: cropped to what's on the page plus a margin, at
 * twice the size, on white, with the plain light colours. No selection, no
 * pulses. Goes through a blob rather than a data URL because the app's
 * content-security policy doesn't let the page fetch either.
 */
export async function exportJpeg(doc: Doc): Promise<Uint8Array> {
  const box = bounds(doc);
  if (!box) throw new Error('nothing to export: the page is empty');
  const w = Math.max(EXPORT_MIN, box.w + 2 * EXPORT_MARGIN);
  const h = Math.max(EXPORT_MIN, box.h + 2 * EXPORT_MARGIN);
  const canvas = document.createElement('canvas');
  canvas.width = Math.ceil(w * EXPORT_SCALE);
  canvas.height = Math.ceil(h * EXPORT_SCALE);
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error("can't draw here");
  ctx.fillStyle = LIGHT.paper;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.scale(EXPORT_SCALE, EXPORT_SCALE);
  // Centre what's drawn in the crop.
  ctx.translate((w - box.w) / 2 - box.x, (h - box.h) / 2 - box.y);
  drawDoc(ctx, doc, { t: 0, pulses: false, selected: null, theme: LIGHT });
  const jpeg = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/jpeg', 0.92));
  if (!jpeg) throw new Error("couldn't make the JPEG");
  return new Uint8Array(await jpeg.arrayBuffer());
}
