// Claude — Date 11/04/2026
// Just enough syntax colouring for the note editor to make code readable:
// comments, strings, numbers, keywords, type names, function calls and C's
// preprocessor lines. One regex pass per change, no parsing: a string or
// comment the regex can't see the end of simply runs to the end of the line.
//
// The language comes from the file's extension (`langFor`). `highlight` turns
// text into HTML with every character escaped and each token wrapped in
// `<span class="t-…">`; tui.css gives the classes their colours.

export type Token = 'kw' | 'type' | 'str' | 'num' | 'com' | 'pre' | 'fn';

export interface Lang {
  name: string;
  /** Line comment openers: `//`, `#`, `--`. */
  line: string[];
  /** Block comment `[open, close]`. */
  block?: [string, string];
  /** Regex sources for string literals, tried in order (longest first). */
  strings: string[];
  keywords: Set<string>;
  types: Set<string>;
  /** Capitalised words are type names (Rust, Java, Swift…). */
  capsTypes?: boolean;
  /** `#include` and friends at the start of a line. */
  preproc?: boolean;
  /** `name!(` is a macro call, coloured like a function (Rust). */
  macros?: boolean;
}

const words = (s: string) => new Set(s.split(/\s+/).filter(Boolean));

const DQ = String.raw`"(?:\\.|[^"\\\n])*"?`;
const SQ = String.raw`'(?:\\.|[^'\\\n])*'?`;
/** One character in single quotes: C and Rust chars, but not Rust's `'a` lifetimes. */
const CHAR = String.raw`'(?:\\.[^'\n]{0,8}|[^'\\\n])'`;
const BT = String.raw`\`(?:\\[\s\S]|[^\`\\])*\`?`;
const TRIPLE = String.raw`"""[\s\S]*?(?:"""|$(?![\s\S]))|'''[\s\S]*?(?:'''|$(?![\s\S]))`;

const C_KW = `auto break case const continue default do else enum extern for goto if inline register restrict
  return sizeof static struct switch typedef union volatile while true false NULL`;
const C_TYPES = `void char short int long float double signed unsigned bool size_t ssize_t ptrdiff_t
  int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t uintptr_t FILE`;

const C: Lang = {
  name: 'c',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [DQ, CHAR],
  keywords: words(C_KW),
  types: words(C_TYPES),
  preproc: true,
};

const CPP: Lang = {
  ...C,
  name: 'cpp',
  keywords: words(`${C_KW} class namespace template typename public private protected virtual override final
    new delete this nullptr using try catch throw operator friend constexpr consteval noexcept mutable explicit
    static_cast dynamic_cast reinterpret_cast const_cast co_await co_return co_yield concept requires`),
  types: words(`${C_TYPES} auto std string vector map set unordered_map unique_ptr shared_ptr optional`),
};

const PYTHON: Lang = {
  name: 'python',
  line: ['#'],
  strings: [TRIPLE, DQ, SQ],
  keywords: words(`and as assert async await break class continue def del elif else except finally for from
    global if import in is lambda match case nonlocal not or pass raise return try while with yield True False
    None self cls`),
  types: words('int str float bool list dict set tuple bytes bytearray object type complex frozenset'),
  capsTypes: true,
};

const RUST: Lang = {
  name: 'rust',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [String.raw`b?r#*"[\s\S]*?"#*`, String.raw`b?"(?:\\[\s\S]|[^"\\])*"?`, CHAR],
  keywords: words(`as async await break const continue crate dyn else enum extern false fn for if impl in let
    loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while`),
  types: words('i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64 bool char str'),
  capsTypes: true,
  macros: true,
};

const JS_KW = `async await break case catch class const continue debugger default delete do else export extends
  finally for from function if import in instanceof let new of return static super switch this throw try typeof
  var void while with yield true false null undefined`;

const JS: Lang = {
  name: 'js',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [BT, DQ, SQ],
  keywords: words(JS_KW),
  types: words('Array Object String Number Boolean Map Set Promise Date RegExp Error JSON Math'),
  capsTypes: true,
};

const TS: Lang = {
  ...JS,
  name: 'ts',
  keywords: words(`${JS_KW} as type interface enum implements private protected public readonly abstract
    declare namespace keyof satisfies is infer`),
  types: words('string number boolean any unknown never void object bigint symbol Record Partial Readonly'),
};

const GO: Lang = {
  name: 'go',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [BT, DQ, CHAR],
  keywords: words(`break case chan const continue default defer else fallthrough for func go goto if import
    interface map package range return select struct switch type var true false nil iota`),
  types: words(`bool byte rune string error any int int8 int16 int32 int64 uint uint8 uint16 uint32 uint64
    uintptr float32 float64 complex64 complex128`),
};

const JAVA: Lang = {
  name: 'java',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [String.raw`"""[\s\S]*?(?:"""|$(?![\s\S]))`, DQ, CHAR],
  keywords: words(`abstract assert break case catch class const continue default do else enum extends final
    finally for goto if implements import instanceof interface native new package private protected public
    return static strictfp super switch synchronized this throw throws transient try volatile while var record
    sealed permits yield true false null fun val when object companion data override open internal lateinit`),
  types: words('void boolean byte char short int long float double'),
  capsTypes: true,
};

const SWIFT: Lang = {
  name: 'swift',
  line: ['//'],
  block: ['/*', '*/'],
  strings: [String.raw`"""[\s\S]*?(?:"""|$(?![\s\S]))`, DQ],
  keywords: words(`associatedtype class deinit enum extension fileprivate func import init inout internal let
    open operator private protocol public rethrows static struct subscript typealias var break case continue
    default defer do else fallthrough for guard if in repeat return switch where while as catch is throw throws
    try async await self Self super true false nil some any`),
  types: words(''),
  capsTypes: true,
};

const SHELL: Lang = {
  name: 'shell',
  line: ['#'],
  strings: [DQ, String.raw`'[^'\n]*'?`],
  keywords: words(`if then else elif fi case esac for while until do done in function return local export
    readonly set unset shift exit source alias echo printf cd test true false`),
  types: words(''),
};

const SQL: Lang = {
  name: 'sql',
  line: ['--'],
  block: ['/*', '*/'],
  strings: [SQ, DQ],
  keywords: words(`select from where insert into values update set delete create table index view drop alter
    add primary key foreign references join left right inner outer on group by order having limit offset as
    and or not null is in like between distinct union all case when then else end begin commit rollback
    SELECT FROM WHERE INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE INDEX VIEW DROP ALTER ADD PRIMARY KEY
    FOREIGN REFERENCES JOIN LEFT RIGHT INNER OUTER ON GROUP BY ORDER HAVING LIMIT OFFSET AS AND OR NOT NULL IS
    IN LIKE BETWEEN DISTINCT UNION ALL CASE WHEN THEN ELSE END BEGIN COMMIT ROLLBACK`),
  types: words('integer int text real blob varchar boolean INTEGER INT TEXT REAL BLOB VARCHAR BOOLEAN'),
};

/** Config files: comments, strings, numbers and booleans. */
const CONFIG: Lang = {
  name: 'config',
  line: ['#'],
  strings: [DQ, SQ],
  keywords: words('true false null yes no on off'),
  types: words(''),
};

const JSON_LANG: Lang = { ...CONFIG, name: 'json', line: [], strings: [DQ] };

const CSS: Lang = {
  name: 'css',
  line: [],
  block: ['/*', '*/'],
  strings: [DQ, SQ],
  keywords: words('important media import supports keyframes font-face from to'),
  types: words(''),
};

const BY_EXT: Record<string, Lang> = {
  c: C, h: C,
  cc: CPP, cpp: CPP, cxx: CPP, hpp: CPP, hh: CPP, hxx: CPP, ino: CPP,
  py: PYTHON, pyw: PYTHON,
  rs: RUST,
  js: JS, mjs: JS, cjs: JS, jsx: JS,
  ts: TS, tsx: TS, mts: TS, cts: TS,
  go: GO,
  java: JAVA, kt: JAVA, kts: JAVA, scala: JAVA,
  swift: SWIFT,
  sh: SHELL, bash: SHELL, zsh: SHELL, fish: SHELL,
  sql: SQL,
  toml: CONFIG, yaml: CONFIG, yml: CONFIG, ini: CONFIG, cfg: CONFIG, conf: CONFIG, env: CONFIG,
  json: JSON_LANG,
  css: CSS, scss: CSS, less: CSS,
};

const BY_NAME: Record<string, Lang> = {
  Makefile: SHELL, makefile: SHELL, GNUmakefile: SHELL, Dockerfile: SHELL, Containerfile: SHELL,
  Gemfile: CONFIG, Rakefile: CONFIG, Procfile: CONFIG,
};

/** The language a file is written in, by its name; `null` for plain text. */
export function langFor(name: string): Lang | null {
  if (BY_NAME[name]) return BY_NAME[name];
  const dot = name.lastIndexOf('.');
  return dot > 0 ? (BY_EXT[name.slice(dot + 1).toLowerCase()] ?? null) : null;
}

const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const compiled = new WeakMap<Lang, RegExp>();

/** One regex for a language: each alternative is a named group per token kind. */
function pattern(lang: Lang): RegExp {
  const hit = compiled.get(lang);
  if (hit) return hit;
  const com = [
    ...lang.line.map((l) => `${escapeRe(l)}[^\\n]*`),
    ...(lang.block ? [`${escapeRe(lang.block[0])}[\\s\\S]*?(?:${escapeRe(lang.block[1])}|$(?![\\s\\S]))`] : []),
  ];
  const parts = [
    ...(lang.preproc ? [String.raw`(?<pre>^[ \t]*#[ \t]*[a-z]+)`] : []),
    ...(com.length ? [`(?<com>${com.join('|')})`] : []),
    `(?<str>${lang.strings.join('|')})`,
    String.raw`(?<num>\b(?:0[xX][\da-fA-F_]+|0[bB][01_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)[a-zA-Z\d]*\b)`,
    String.raw`(?<word>[A-Za-z_$][\w$]*)` + (lang.macros ? '(?<bang>!)?' : ''),
  ];
  const re = new RegExp(parts.join('|'), 'gm');
  compiled.set(lang, re);
  return re;
}

/** A name followed by `(` is a call. Sticky: tested right where the name ends. */
const CALL = /[ \t]*\(/y;

/** Keywords whose next name is a function being defined, `(` or not (`fn get<T>(`). */
const DEFINES = new Set(['fn', 'def', 'func', 'function', 'fun']);

export interface Span {
  text: string;
  token: Token | null;
}

/** `text` cut into tokens. Joined back together, the spans are `text` exactly. */
export function tokenize(text: string, lang: Lang): Span[] {
  const re = pattern(lang);
  re.lastIndex = 0;
  const out: Span[] = [];
  let at = 0;
  /** Where the last defining keyword ended, if a name may follow it. */
  let defined = -1;
  const plain = (end: number) => {
    if (end > at) out.push({ text: text.slice(at, end), token: null });
  };
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m[0] === '') {
      re.lastIndex++;
      continue;
    }
    const g = m.groups!;
    let token: Token | null = null;
    let match = m[0];
    if (g.pre) token = 'pre';
    else if (g.com) token = 'com';
    else if (g.str) token = 'str';
    else if (g.num) token = 'num';
    else if (g.word) {
      const w = g.word;
      CALL.lastIndex = m.index + match.length;
      const call = CALL.test(text);
      if (g.bang && !call) {
        // `x != y`, not a macro: the `!` belongs to what follows.
        match = w;
        re.lastIndex = m.index + w.length;
      }
      const named = defined >= 0 && text.slice(defined, m.index).trim() === '';
      defined = -1;
      if (lang.keywords.has(w)) {
        token = 'kw';
        if (DEFINES.has(w)) defined = m.index + w.length;
      } else if (lang.types.has(w) || (lang.capsTypes && /^[A-Z][a-z\d]/.test(w))) token = 'type';
      else if (call || named) token = 'fn';
    }
    if (token === null) continue; // an ordinary name: part of the plain text around it
    plain(m.index);
    out.push({ text: match, token });
    at = m.index + match.length;
  }
  plain(text.length);
  return out;
}

const ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };
const escape = (s: string) => s.replace(/[&<>"]/g, (c) => ESCAPES[c]);

/** `text` as HTML, every character escaped, tokens wrapped in `<span class="t-…">`. */
export function highlight(text: string, lang: Lang): string {
  return tokenize(text, lang)
    .map((s) => (s.token ? `<span class="t-${s.token}">${escape(s.text)}</span>` : escape(s.text)))
    .join('');
}
