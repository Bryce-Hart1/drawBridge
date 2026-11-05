// Claude — Date 11/04/2026
import { describe, expect, it } from 'vitest';
import { highlight, langFor, tokenize, type Lang, type Token } from './syntax';

/** The coloured tokens only, as `token:text`. */
function tokens(text: string, lang: Lang): string[] {
  return tokenize(text, lang)
    .filter((s) => s.token)
    .map((s) => `${s.token}:${s.text}`);
}

const lang = (name: string): Lang => langFor(name)!;

describe('syntax', () => {
  it('picks a language by name', () => {
    expect(lang('finalMain.c').name).toBe('c');
    expect(lang('a.HPP').name).toBe('cpp');
    expect(lang('main.rs').name).toBe('rust');
    expect(lang('api.ts').name).toBe('ts');
    expect(lang('Makefile').name).toBe('shell');
    expect(langFor('notes.md')).toBeNull();
    expect(langFor('README')).toBeNull();
    expect(langFor('.bashrc')).toBeNull();
  });

  it('colours C', () => {
    const src = '#include <stdio.h>\n\nint main(void) {\n    puts("hello"); // say it\n    return 0x1F;\n}\n';
    expect(tokens(src, lang('a.c'))).toEqual([
      'pre:#include',
      'type:int', 'fn:main', 'type:void',
      'fn:puts', 'str:"hello"', 'com:// say it',
      'kw:return', 'num:0x1F',
    ]);
  });

  it('colours Python, triple quotes included', () => {
    const src = 'def f(x):\n    """doc\n    more"""\n    return x + 1.5  # done\n';
    expect(tokens(src, lang('a.py'))).toEqual([
      'kw:def', 'fn:f', 'str:"""doc\n    more"""', 'kw:return', 'num:1.5', 'com:# done',
    ]);
  });

  it("colours Rust without mistaking lifetimes or != for anything", () => {
    const src = "fn get<'a>(v: &'a Vec<u8>) -> Option<char> { if v.len() != 0 { println!(\"x\"); } let c = 'z'; None }";
    expect(tokens(src, lang('a.rs'))).toEqual([
      'kw:fn', 'fn:get', 'type:Vec', 'type:u8', 'type:Option', 'type:char',
      'kw:if', 'fn:len', 'num:0', 'fn:println!', 'str:"x"', 'kw:let', "str:'z'", 'type:None',
    ]);
  });

  it('runs an unclosed string or comment to its end, and nothing further', () => {
    expect(tokens('x = "open\ny = 1', lang('a.js'))).toEqual(['str:"open', 'num:1']);
    expect(tokens('/* open', lang('a.c'))).toEqual(['com:/* open']);
  });

  it('keeps every character, and escapes HTML', () => {
    const src = 'if (a < b && c > "<script>") { x = 1; } // <b>';
    const spans = tokenize(src, lang('a.js'));
    expect(spans.map((s) => s.text).join('')).toBe(src);
    const html = highlight(src, lang('a.js'));
    expect(html).not.toContain('<script>');
    expect(html).not.toContain('<b>');
    expect(html).toContain('<span class="t-str">&quot;&lt;script&gt;&quot;</span>');
    expect(html.replace(/<[^>]+>/g, '').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&')).toBe(src);
  });

  it('handles every kind of token', () => {
    const seen = new Set<Token>();
    const src = '#define N 3\n// c\nconst char *s = "x"; size_t n = strlen(s);';
    for (const s of tokenize(src, lang('a.c'))) if (s.token) seen.add(s.token);
    expect([...seen].sort()).toEqual(['com', 'fn', 'kw', 'num', 'pre', 'str', 'type']);
  });
});
