// Claude — Date 11/06/2026
// A diagram as an SVG file: the same drawing as draw.ts, written as markup
// instead of painted on a canvas, so it stays sharp at any size and opens in
// a browser or a vector editor. Light colours on white, no pulses, cropped to
// the drawing plus a margin, like the image exports. Pure text out, so it's
// tested without a canvas.

import { LABEL_EM, bounds, labelWidth, layoutIndexed, lineHeight, linePath, midpoint, textBox, tipFrom, type Pt } from './geometry';
import { ADVANCE, BOLD, FONT_PX, ITALIC, PALETTE, TINT, UNDERLINE, isLine, type Doc, type Line, type Shape, type Stroke } from './model';
import { LIGHT, runsOf } from './draw';
import { stylesOf } from './text';

const MARGIN = 32;
const MIN = 120;
const FONT = 'ui-monospace, SF Mono, Menlo, Monaco, Consolas, monospace';
const HEAD = 14;

const n = (v: number) => String(Math.round(v * 10) / 10);

export function xmlEscape(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' })[c]!);
}

function dashAttr(ln: Stroke): string {
  if (ln === 'dash') return ' stroke-dasharray="10 6"';
  if (ln === 'dot') return ' stroke-dasharray="0.1 6" stroke-linecap="round"';
  return '';
}

function shapeSvg(s: Shape): string {
  const colour = PALETTE[s.c].fill;
  const plain = s.c === 0;
  const fill = plain ? `fill="${LIGHT.paper}"` : `fill="${colour}" fill-opacity="${TINT}"`;
  const stroke = `stroke="${plain ? LIGHT.ink : colour}" stroke-width="2" stroke-linejoin="round"${dashAttr(s.ln)}`;
  const hw = s.w / 2;
  const hh = s.h / 2;
  let body: string;
  switch (s.t) {
    case 'rect':
      body = `<rect x="${n(s.x - hw)}" y="${n(s.y - hh)}" width="${n(s.w)}" height="${n(s.h)}" ${fill} ${stroke}/>`;
      break;
    case 'circle':
      body = `<ellipse cx="${n(s.x)}" cy="${n(s.y)}" rx="${n(hw)}" ry="${n(hh)}" ${fill} ${stroke}/>`;
      break;
    case 'diamond':
      body = `<polygon points="${n(s.x)},${n(s.y - hh)} ${n(s.x + hw)},${n(s.y)} ${n(s.x)},${n(s.y + hh)} ${n(s.x - hw)},${n(s.y)}" ${fill} ${stroke}/>`;
      break;
    case 'triangle':
      body = `<polygon points="${n(s.x)},${n(s.y - hh)} ${n(s.x + hw)},${n(s.y + hh)} ${n(s.x - hw)},${n(s.y + hh)}" ${fill} ${stroke}/>`;
      break;
  }
  if (s.text === '') return body;
  const rows = layoutIndexed(s);
  const chars = [...s.text];
  const styles = stylesOf(s);
  const lh = lineHeight(s);
  const adv = FONT_PX[s.f] * ADVANCE;
  const top = s.y + textBox(s).dy - (rows.length * lh) / 2 + lh / 2;
  // A row is drawn a run at a time, each over the columns it covers, so bold and plain letters keep the editor's spacing.
  const text = rows
    .flatMap((row, r) => {
      const runs = runsOf(row, styles);
      return runs.map(([a, b, st]) => {
        const x = runs.length === 1 ? s.x : s.x - (row.length * adv) / 2 + ((a + b) / 2) * adv;
        const look =
          (st & BOLD ? ' font-weight="bold"' : '') + (st & ITALIC ? ' font-style="italic"' : '') + (st & UNDERLINE ? ' text-decoration="underline"' : '');
        const words = xmlEscape(row.slice(a, b).map((i) => chars[i]).join(''));
        return `<text x="${n(x)}" y="${n(top + r * lh)}"${look}${runs.length > 1 ? ' xml:space="preserve"' : ''}>${words}</text>`;
      });
    })
    .join('');
  return `${body}<g font-family="${FONT}" font-size="${FONT_PX[s.f]}" text-anchor="middle" dominant-baseline="central" fill="${LIGHT.ink}">${text}</g>`;
}

function headSvg(tip: Pt, from: Pt, colour: string): string {
  const angle = Math.atan2(tip.y - from.y, tip.x - from.x);
  const p = (d: number) => `${n(tip.x - HEAD * Math.cos(angle + d))},${n(tip.y - HEAD * Math.sin(angle + d))}`;
  return `<polygon points="${n(tip.x)},${n(tip.y)} ${p(-0.4)} ${p(0.4)}" fill="${colour}"/>`;
}

function lineSvg(doc: Doc, l: Line): string {
  const path = linePath(doc, l);
  if (path.length < 2) return '';
  const colour = l.c === 0 ? LIGHT.ink : PALETTE[l.c].fill;
  const points = path.map((p) => `${n(p.x)},${n(p.y)}`).join(' ');
  let out = `<polyline points="${points}" fill="none" stroke="${colour}" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"${dashAttr(l.ln)}/>`;
  if (l.head !== 'none') out += headSvg(path[path.length - 1], tipFrom(path, true), colour);
  if (l.head === 'both') out += headSvg(path[0], tipFrom(path, false), colour);
  if (l.text !== '') {
    const px = FONT_PX[l.f];
    const at = midpoint(path);
    const w = labelWidth(l);
    out +=
      `<rect x="${n(at.x - w / 2 - 4)}" y="${n(at.y - (px * LABEL_EM) / 2)}" width="${n(w + 8)}" height="${n(px * LABEL_EM)}" fill="${LIGHT.paper}"/>` +
      `<text x="${n(at.x)}" y="${n(at.y)}" font-family="${FONT}" font-size="${px}" text-anchor="middle" dominant-baseline="central" fill="${LIGHT.ink}">${xmlEscape(l.text)}</text>`;
  }
  return out;
}

/** The diagram as an SVG document. Throws if the page is empty. */
export function exportSvg(doc: Doc): string {
  const box = bounds(doc);
  if (!box) throw new Error('nothing to export: the page is empty');
  const w = Math.max(MIN, box.w + 2 * MARGIN);
  const h = Math.max(MIN, box.h + 2 * MARGIN);
  const x = box.x - (w - box.w) / 2;
  const y = box.y - (h - box.h) / 2;
  const parts = doc.items.map((p) => (isLine(p) ? lineSvg(doc, p) : shapeSvg(p))).join('\n');
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${n(x)} ${n(y)} ${n(w)} ${n(h)}" width="${n(w)}" height="${n(h)}">\n` +
    `<rect x="${n(x)}" y="${n(y)}" width="${n(w)}" height="${n(h)}" fill="${LIGHT.paper}"/>\n${parts}\n</svg>\n`
  );
}
