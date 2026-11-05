// Claude — Date 11/05/2026
import { describe, expect, it } from 'vitest';
import { parseLin, serializeLin } from './format';
import { blankDoc, newLine, newShape, type Doc } from './model';

function sample(): Doc {
  const doc = blankDoc();
  const a = { ...newShape(1, 'rect', 300, 200), c: 6, text: 'api "v2"' };
  const b = { ...newShape(2, 'circle', 700, 200), c: 4, f: 'i' as const, text: 'db' };
  const l = { ...newLine(3, { to: 1 }, { to: 2 }), head: 'both' as const, pulse: 'mid' as const, glow: 'light' as const };
  const free = newLine(4, { x: 10.5, y: 20 }, { to: 2 });
  return { ...doc, next: 5, items: [a, b, l, free] };
}

const shapeJson = (extra: string) => `{"lin":1,"items":[{"id":1,"t":"rect","x":50,"y":50,"w":100,"h":60${extra}}]}`;

describe('serializeLin', () => {
  it('writes one part per line', () => {
    const text = serializeLin(sample());
    const lines = text.trimEnd().split('\n');
    expect(lines[0]).toBe('{"lin":1,"w":1200,"h":800,"next":5,"items":[');
    expect(lines).toHaveLength(6);
    expect(lines[5]).toBe(']}');
    expect(lines[1]).toContain('"text":"api \\"v2\\""');
  });

  it('writes an empty page on one line', () => {
    expect(serializeLin(blankDoc())).toBe('{"lin":1,"w":1200,"h":800,"next":1,"items":[]}\n');
  });
});

describe('parseLin', () => {
  it('round-trips a diagram', () => {
    const doc = sample();
    expect(parseLin(serializeLin(doc))).toEqual(doc);
    expect(parseLin(serializeLin(blankDoc()))).toEqual(blankDoc());
  });

  it('fills in what a hand-written part leaves out', () => {
    const doc = parseLin(shapeJson(''));
    expect(doc.w).toBe(1200);
    expect(doc.items[0]).toMatchObject({ f: 'o', c: 0, text: '' });
    expect(doc.next).toBe(2);
  });

  it('clamps numbers instead of refusing the file', () => {
    const [tiny] = parseLin(shapeJson('').replace('"w":100', '"w":5')).items;
    expect(tiny).toMatchObject({ w: 24 });
    const [huge] = parseLin(shapeJson('').replace('"w":100', '"w":5000')).items;
    expect(huge).toMatchObject({ w: 800 });
    const [colour] = parseLin(shapeJson(',"c":42')).items;
    expect(colour).toMatchObject({ c: 0 });
  });

  it('keeps a circle round', () => {
    const [c] = parseLin('{"lin":1,"items":[{"id":1,"t":"circle","x":9,"y":9,"w":80,"h":30}]}').items;
    expect(c).toMatchObject({ w: 80, h: 80 });
  });

  it('cuts text to the character limit', () => {
    const [s] = parseLin(shapeJson(`,"text":"${'x'.repeat(500)}"`)).items;
    expect('text' in s && s.text.length).toBe(200);
  });

  it('numbers new parts past the highest id', () => {
    const doc = parseLin('{"lin":1,"next":1,"items":[{"id":7,"t":"rect","x":1,"y":1,"w":50,"h":50}]}');
    expect(doc.next).toBe(8);
  });

  it('refuses what is not a diagram', () => {
    expect(() => parseLin('hello')).toThrow(/not JSON/);
    expect(() => parseLin('[]')).toThrow(/not a .lin/);
    expect(() => parseLin('{"items":[]}')).toThrow(/not a .lin/);
    expect(() => parseLin('{"lin":1}')).toThrow(/no items/);
  });

  it('refuses a newer format by name', () => {
    expect(() => parseLin('{"lin":2,"items":[]}')).toThrow(/newer drawBridge \(format 2\)/);
  });

  it('says which part is wrong', () => {
    expect(() => parseLin(shapeJson('').replace('"rect"', '"star"'))).toThrow(/item 1: t should be one of/);
    expect(() => parseLin(shapeJson('').replace('"x":50', '"x":"left"'))).toThrow(/item 1: x should be a number/);
    expect(() => parseLin('{"lin":1,"items":[{"id":1,"x":1}]}')).toThrow(/item 1: has no type/);
  });

  it('refuses a reused id', () => {
    const two = '{"id":1,"t":"rect","x":1,"y":1,"w":50,"h":50}';
    expect(() => parseLin(`{"lin":1,"items":[${two},${two}]}`)).toThrow(/id 1 is used twice/);
  });

  it('refuses a line that ends on a shape that is not there', () => {
    const line = '{"id":2,"t":"line","a":{"to":9},"b":{"x":1,"y":1}}';
    const rect = '{"id":1,"t":"rect","x":1,"y":1,"w":50,"h":50}';
    expect(() => parseLin(`{"lin":1,"items":[${rect},${line}]}`)).toThrow(/ends on shape 9/);
  });

  it('refuses a line ending on another line', () => {
    const rect = '{"id":1,"t":"rect","x":1,"y":1,"w":50,"h":50}';
    const one = '{"id":2,"t":"line","a":{"to":1},"b":{"x":1,"y":1}}';
    const two = '{"id":3,"t":"line","a":{"to":2},"b":{"x":1,"y":1}}';
    expect(() => parseLin(`{"lin":1,"items":[${rect},${one},${two}]}`)).toThrow(/ends on shape 2/);
  });

  it('refuses an unknown pulse', () => {
    const rect = '{"id":1,"t":"rect","x":1,"y":1,"w":50,"h":50}';
    const line = '{"id":2,"t":"line","a":{"to":1},"b":{"x":1,"y":1},"pulse":"fast"}';
    expect(() => parseLin(`{"lin":1,"items":[${rect},${line}]}`)).toThrow(/pulse should be one of/);
  });
});
