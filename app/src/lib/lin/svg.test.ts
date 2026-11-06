// Claude — Date 11/06/2026
import { describe, expect, it } from 'vitest';
import { addLine, addShape, cycleHead, cycleRoute, cycleStroke, recolor, typeChar, type Edited } from './edit';
import { blankDoc, type Doc } from './model';
import { exportSvg, xmlEscape } from './svg';

function ok(e: Edited): Doc {
  if ('error' in e) throw new Error(e.error);
  return e.doc;
}

function sample(): Doc {
  let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
  doc = addShape(doc, 'circle', { x: 400, y: 100 }).doc;
  for (const ch of 'a & b') doc = ok(typeChar(doc, 1, ch));
  doc = ok(recolor(doc, 2, 6));
  const added = addLine(doc, { to: 1 }, { to: 2 });
  if ('error' in added) throw new Error(added.error);
  return ok(cycleHead(added.doc, 3));
}

describe('SVG export', () => {
  it('writes a complete, cropped document on white', () => {
    const svg = exportSvg(sample());
    expect(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg" viewBox="')).toBe(true);
    expect(svg.trimEnd().endsWith('</svg>')).toBe(true);
    expect(svg).toContain('fill="#ffffff"');
    // The picture is bounds + a 32 px margin: the shapes span 20..450 across, 50..150 down... and 3 parts in all.
    expect(svg).toContain('<rect');
    expect(svg).toContain('<ellipse');
    expect(svg).toContain('<polyline');
  });

  it('gives a coloured shape a see-through wash and an outline', () => {
    const svg = exportSvg(sample());
    expect(svg).toContain('fill="#3e63dd" fill-opacity="0.22"');
    expect(svg).toContain('stroke="#3e63dd"');
  });

  it('escapes the text', () => {
    expect(xmlEscape(`<a href="x">&'`)).toBe('&lt;a href=&quot;x&quot;&gt;&amp;&apos;');
    const svg = exportSvg(sample());
    expect(svg).toContain('a &amp; b');
    expect(svg).not.toContain('a & b');
  });

  it('draws an arrowhead only where one was added', () => {
    const plainDoc = (() => {
      let doc = addShape(blankDoc(), 'rect', { x: 100, y: 100 }).doc;
      doc = addShape(doc, 'rect', { x: 400, y: 100 }).doc;
      const r = addLine(doc, { to: 1 }, { to: 2 });
      if ('error' in r) throw new Error(r.error);
      return r.doc;
    })();
    expect(exportSvg(plainDoc)).not.toContain('<polygon');
    expect(exportSvg(ok(cycleHead(plainDoc, 3)))).toContain('<polygon');
  });

  it('writes dashes and curves', () => {
    const doc = ok(cycleRoute(ok(cycleStroke(sample(), 3)), 3));
    const svg = exportSvg(doc);
    expect(svg).toContain('stroke-dasharray="10 6"');
    const points = svg.match(/<polyline points="([^"]+)"/)![1].split(' ');
    expect(points.length).toBeGreaterThan(10);
  });

  it('refuses an empty page', () => {
    expect(() => exportSvg(blankDoc())).toThrow('nothing to export');
  });
});
