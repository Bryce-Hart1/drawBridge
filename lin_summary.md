# `.lin` — everything it supports

A reference for building another editor or viewer for `.lin` files. It describes the format
and the behaviour of the drawBridge editor as of November 2026. Where a number matters it is
given exactly; the source file that decides it is named in the last section.

A `.lin` is a small diagram: **shapes** (circle, triangle, rectangle, diamond) and **lines**
(plain, single-arrow, double-arrow) on a fixed-size page, in stacking order, with text in
the shapes and labels on the lines. Lines can **pulse** (a ball or a light travelling along
them). The editor is keyboard-first: the keyboard does everything except say *where*
things go, which is the mouse.

---

## 1. The file

A `.lin` is UTF-8 **JSON text**, written **one part per line** so a diff shows what changed:

```json
{"lin":1,"w":1200,"h":800,"next":5,"items":[
 {"id":1,"t":"rect","x":300,"y":200,"w":160,"h":80,"c":6,"f":"o","text":"hi all","st":"110004"},
 {"id":2,"t":"circle","x":700,"y":200,"w":100,"h":100,"c":4,"f":"i","text":"db","ln":"dash"},
 {"id":3,"t":"line","a":{"to":1},"b":{"to":2},"head":"end","pulse":"fwd","glow":"ball","c":0,"text":"calls","f":"o"},
 {"id":4,"t":"line","a":{"to":2},"b":{"x":900,"y":500},"head":"both","pulse":"mid","glow":"light","c":1,"route":"elbow","ln":"dot"}
]}
```

### 1.1 Top level

| Field | Type | Default | Meaning |
|---|---|---|---|
| `lin` | number | — (required) | Format version. Must be exactly `1`. Greater than 1 is rejected as "made by a newer drawBridge". |
| `w`, `h` | number | `1200`, `800` | Page size in page pixels. Clamped to **200–4000**. (No UI changes it; it can only be set in the file.) |
| `next` | integer | highest id + 1 | The id the next new part gets. If missing, not an integer, or not above every existing id, it is recomputed as highest id + 1. |
| `items` | array | — (required) | The parts, **back to front**. Array order *is* the stacking order. |

### 1.2 A shape (`t` is `circle`, `triangle`, `rect` or `diamond`)

| Field | Type | Default | Meaning |
|---|---|---|---|
| `id` | integer ≥ 1 | required | Unique across the file (shapes and lines share one id space). |
| `t` | string | required | `circle` \| `triangle` \| `rect` \| `diamond`. |
| `x`, `y` | number | required | The shape's **centre**, clamped to `0…w` / `0…h`. |
| `w`, `h` | number | required | Size, clamped to **24–800**. For a circle `h` is forced equal to `w` (width wins). |
| `c` | integer 0–9 | `0` | Colour (see §5). Anything else becomes `0`. |
| `f` | `"i"` \| `"o"` \| `"p"` | `"o"` | Font size: small / medium / large (see §4). |
| `text` | string | `""` | The text inside. Truncated to **200 characters** (Unicode code points). May contain `\n` for line breaks. |
| `ln` | `"solid"` \| `"dash"` \| `"dot"` | `"solid"` | Outline style. |
| `st` | string of digits 0–7 | `""` | Letter styles (see §4.3). |

### 1.3 A line (`t` is `"line"`)

| Field | Type | Default | Meaning |
|---|---|---|---|
| `id`, `t` | | required | As above; `t` is the literal `"line"`. |
| `a`, `b` | end | required | The start and end (see below). |
| `head` | `"none"` \| `"end"` \| `"both"` | `"none"` | Arrowhead at `b`, or at both ends. |
| `pulse` | `"off"` \| `"fwd"` \| `"rev"` \| `"mid"` | `"off"` | `fwd`: a → b. `rev`: b → a. `mid`: out from the middle to both ends. |
| `glow` | `"ball"` \| `"light"` | `"ball"` | What travels: a ball, or a light inside the line. |
| `c` | integer 0–9 | `0` | Colour. `0` is the page's ink colour. |
| `route` | `"straight"` \| `"curve"` \| `"elbow"` | `"straight"` | The path the line takes. |
| `ln` | `"solid"` \| `"dash"` \| `"dot"` | `"solid"` | Stroke style. |
| `text` | string | `""` | A **one-row label** drawn over the middle of the line. Newlines become spaces. Max **40** characters. |
| `f` | `"i"` \| `"o"` \| `"p"` | `"o"` | The label's font size. |

An **end** is one of:

- `{"to": <shapeId>}` — **attached**. The line stops at the shape's outline and follows the
  shape when it moves. No coordinates are stored.
- `{"x": <number>, "y": <number>}` — a **free point**. (Not clamped when read.)

### 1.4 Reading (strict about structure, forgiving about numbers)

- Not JSON, or `lin` missing/not a number → "not a .lin diagram". `items` not an array → error.
- A part that can't be understood is an **error**, never silently dropped (the editor would then save the loss). The message says which item (`item 3: …`).
- Errors: part not an object; `id` missing, not a whole number ≥ 1, or used twice; shape with no `t`; any enum field holding a value outside its list; a number field that isn't a finite number; a line end that isn't an object; a line attached to a shape id that doesn't exist or is itself a line.
- Forgiving: numbers are **clamped** (page size, shape position and size); `c` falls back to `0`; text is truncated; a bad `st` is dropped.
- Unknown extra fields are ignored (and not written back).
- `st` is kept only if it matches `^[0-7]+$`, is **exactly one digit per character of `text`**, and isn't all zeros.

### 1.5 Writing

- First line is the header (`{"lin":1,"w":…,"h":…,"next":…,"items":[`), then one part per line (indented one space), then `]}` and a newline. An empty diagram is one line: `{"lin":1,"w":…,"h":…,"next":…,"items":[]}`.
- `x`, `y`, `w`, `h` and free-end coordinates are **rounded to a tenth**.
- Key order — shape: `id, t, x, y, w, h, c, f, text, [ln], [st]`. Line: `id, t, a, b, head, pulse, glow, c, [route], [ln], [text], [f]`.
- **Defaults are left out** so a file that doesn't use a feature reads and writes as it always did: shape `ln` when solid, shape `st` when empty; line `route` when straight, `ln` when solid, `text` when empty, and `f` when the label is empty *and* the font is `"o"`.
- A diagram is "modified" when its serialised text differs from what was loaded/saved.

---

## 2. Shapes

| Shape | Default size (w × h) | Outline |
|---|---|---|
| `circle` | 100 × 100 | Ellipse (always round: `h = w`) |
| `triangle` | 120 × 100 | Apex at top centre, base along the bottom |
| `rect` | 160 × 80 | Rectangle |
| `diamond` | 140 × 100 | Points at top, right, bottom, left |

- A new shape is centred where you click, medium font, no text, plain colour, solid outline; it is kept wholly on the page.
- **Resize** with the arrow keys, always around the centre: ←/→ change width, ↑/↓ change height, **10 px** per press, **40 px** with ⇧. Limits 24–800. A **circle** stays round: → and ↑ grow, ← and ↓ shrink.
- A resize or font change that would make the existing text overflow is **refused** with a message (unless the text already overflowed, as in a hand-edited file).
- Dropped/moved shapes can't leave the page (a shape larger than the page is centred).
- Shapes are hit-tested by their true outline (diamond and triangle included), anywhere inside.

---

## 3. Lines

- **Arrowheads** cycle none → end (arrow) → both. A head is a filled triangle 14 px long, ±0.4 rad (~23°) wide.
- **Flip** swaps `a` and `b` (the arrow and the pulse direction swap with it).
- **Routes:** `straight`; `curve` (a gentle quadratic bow: 20 % of the length, at most 70 px, drawn as 24 segments); `elbow` (square corners — the first run is horizontal if the centres are further apart sideways than up/down, else vertical, with the middle run halfway).
- **Where an attached end lands:** the point where the straight line between the two ends' centres leaves the shape's outline. For parallel lines (below) it is shifted so lines stay apart.
- **Several lines between the same two shapes** (either direction) don't pile up: they run side by side. Spacing is at least 24 px, or enough that the tallest label among them (1.5 × its font size) has its own row (+6 px), but never wider than 85 % of the smaller shape divided between them. Curves bow apart each to its own side; elbows move their middle run. Order is by `id`.
- **Ends:** drag an end onto a shape (within 8 px of it) and it **attaches**; drop it anywhere else and it's a free point. Both ends can't be on the same shape (the second becomes a free point). A new line needs two different ends (same shape, or two free points within 6 px, is refused).
- **Deleting a shape** doesn't delete its lines: their ends become free points where they last were.
- **Label:** one row, centred on the *middle by length* of the path, drawn over a patch of page colour so the line doesn't run through the words. It must fit: `characters × font px × 0.6 + 12 ≤ line length`, and ≤ 40 characters. No line breaks, no styles.
- **Hit-testing:** a line is hit within 7 px of its path; the selected line's end handles are hit within 10 px (both scale with zoom so they stay a steady size on screen).

---

## 4. Text

### 4.1 Fonts

Monospace, so how much fits is the same on every machine. The font stack is
`ui-monospace, "SF Mono", Menlo, Monaco, Consolas, monospace`.

| Key | Name | Size |
|---|---|---|
| `i` | small | 12 px |
| `o` | medium (default) | 18 px |
| `p` | large | 28 px |

A character is **0.6 em** wide (the advance); a row is **1.3 em** tall (the leading). Bold,
italic and underline use the same advance, so styled and plain letters line up.

### 4.2 How much a shape holds

The text is set in a rectangle inside the shape, **centred horizontally**, with **6 px** padding removed from each side:

| Shape | Text box (before padding) | Vertical offset |
|---|---|---|
| `rect` | w × h | centred |
| `circle` | 0.707·w × 0.707·h (the biggest rectangle inside an ellipse) | centred |
| `diamond` | w/2 × h/2 | centred |
| `triangle` | w/2 × h/2 | shifted down h/4 (sits on the base) |

`columns = floor(boxWidth / (px × 0.6))`, `rows = floor(boxHeight / (px × 1.3))`, and the
capacity is `columns × rows`, capped at **200**. (A word that wraps badly can mean fewer
fit; the real test is that the wrapped text needs no more rows than there are.)
Example: a default `rect` (160 × 80) holds 80 at small, 26 at medium, 8 at large.

### 4.3 Wrapping

- A `\n` always starts a new line; typing a backslash then `n` turns into one break (the backslash disappears). So a **list is one item per line**. Empty lines between items are kept.
- Within a line, break at spaces; a word longer than a row is broken inside the word. Runs of spaces count as one. Leading spaces are dropped.
- Each row is **centred**. Rows are stacked and centred vertically in the text box.

### 4.4 Letter styles — bold, italic, underline (shapes only)

- `st` holds one digit per character of `text`: **1 = bold, 2 = italic, 4 = underline**, added together (0–7). Omitted when nothing is styled.
- Invariant: `st` is either empty or exactly as long as the text (in code points). A line break (`\n`) always has style 0.
- Styles belong to letters that exist: they move and are deleted with the letters when you type or delete in the middle of the text.
- A new typed letter takes the style of the letter before the caret (the next one, at the very start), unless a style has been switched with ⌥ B/I/U and nothing selected ("pending"; it resets when the caret moves).
- Drawn as runs: each run of one style is centred over the columns it covers. Underline is a line 1/14 of the font size thick, half a font size below the row centre.
- **Line labels can't be styled** and have no caret movement (they're typed at the end).

---

## 5. Colours

`c` is `0`–`9`, set with the number keys, on shapes and lines.

| `c` | Name | Hex |
|---|---|---|
| 0 | plain | `#ffffff` (a shape in the page's own colour with an ink outline; a line in ink — follows the theme) |
| 1 | red | `#e5484d` |
| 2 | orange | `#f76b15` |
| 3 | yellow | `#f7c948` |
| 4 | green | `#46a758` |
| 5 | teal | `#12a594` |
| 6 | blue | `#3e63dd` |
| 7 | purple | `#8e4ec6` |
| 8 | pink | `#d6409f` |
| 9 | charcoal | `#30343b` |

- **Shapes:** a colour is a **22 % see-through wash** with a full-strength outline (so it's a tint, not a block). Plain is the page's colour with an ink outline.
- **Lines:** the stroke is the colour at full strength. Plain is ink.
- **Text** is always the ink colour (it is *not* chosen by contrast), on a tinted background.
- **Theme:** the editor canvas follows light/dark mode (paper, ink, dim, faint, accent). **Exports are always the light theme** on white: paper `#ffffff`, ink `#111111`, dim `#6f6f6f`, faint `#d6d6d2`, accent `#1f5fbf`.

### Stroke widths and styles

- Shape outline 2 px; line 2.5 px (5 px while a *light* pulse is on it).
- `dash`: dashes of `max(10, 4×width)` with gaps of `max(6, 2.4×width)`. `dot`: round dots (zero-length dashes with round caps) with the same gaps. In SVG: `10 6` and `0.1 6`.
- The same `ln` applies to shape outlines and to lines.

---

## 6. Pulses and animation

- The viewer **opens playing**. Honours `prefers-reduced-motion` (starts paused).
- Constant speed **180 px/s** along the path (so a long line takes longer), then a **0.5 s** rest before it goes again. Lines pulse independently, off one clock.
- `fwd` travels a→b, `rev` b→a, `mid` sends **two** pulses from the middle out to both ends.
- **Ball:** a filled circle, radius 6, with a 2 px ring in the page colour. On a plain line it's the accent colour; on a coloured line it's that colour.
- **Light:** a **56 px** bright stretch inside the line (the line thickens to 5 px), fading out at both ends; it starts and ends *off* the line so it slides in and out. Bright is the page colour on a plain line, white on a coloured one.
- Pulses follow the line's actual path (curves and elbows too).
- They keep playing while editing. Pausing stops the clock (pulses freeze where they are).
- Not drawn in exports.

---

## 7. The editor

### 7.1 Modes

- **Viewing** (opens here for an existing file): pulses play; drag pans the page. Opening a `.lin` from the file list (Enter) shows it this way.
- **Editing**: `e` toggles. **A new `.lin`** (made with `n` and a name ending in `.lin`, or any name, which gets `.lin` added) opens straight into editing.
- **Typing** (Enter on a shape or line): every key is text, so `i`, `o`, `p` and the digits are free to be text and commands without clashing. Esc finishes.

### 7.2 Mouse (it only says *where*)

| Action | Does |
|---|---|
| Click a shape/line | Selects it (front-most hit wins). |
| ⇧-click | Adds to, or removes from, the selection (never starts a drag). |
| Drag empty space | Draws a selection box. A shape is selected if the box overlaps it; a line if any of it is inside. ⇧ keeps the current selection. |
| Drag a shape | Moves it. Snaps (see below). |
| Drag one of several selected parts | Moves them all together. Free line ends move; ends on shapes follow their shapes. |
| Drag a line | Moves its free ends; attached ends stay on their shapes. |
| Drag a selected line's end handle | Moves that end; onto a shape it attaches. |
| Armed shape tool + click | Places that shape, centred on the click. |
| Armed line tool: click, click | Start, then end. Ends snap onto shapes (lit up while hovering). A dashed rubber band shows the line. |
| Middle-button drag, or any drag while viewing | Pans the page. |
| Wheel / trackpad | Pans. With ⌘ or Ctrl (or a pinch): zooms around the pointer. |

A press that moves less than 3 screen pixels is a click, not a drag. A whole drag is **one undo step**.
Hold **⌥ while dragging** to turn snapping off.

**Snapping:** a dragged shape's edges and middle snap to another shape's edges and middle within **6 px**, with an alignment guide drawn. With the grid on (`g`), anything not aligned falls to the nearest **10 px**; the grid also applies to placing and to pasting at the pointer.

**View:** zoom 25 %–800 % of "fit"; fit is the page scaled to the window (never above 2×). The zoom/pan isn't saved in the file.

### 7.3 Keys — viewing

| Key | Does |
|---|---|
| `space` | Play / pause |
| `e` | Edit |
| `+` `=` / `-` `_` | Zoom in / out |
| `z` | Fit |
| `x` | Export |
| `⌘S` | Save (says "no changes" if none) |
| `esc` | Close (asks first if there are unsaved changes) |

Letters mean the same with Caps Lock on.

### 7.4 Keys — editing (nothing typing)

**Make things**

| Key | Does |
|---|---|
| `c` `t` `r` `d` | Arm circle / triangle / rectangle / diamond; the next click drops it. Clears the selection. |
| `l` | Arm the line tool |
| `esc` | Cancel the tool → deselect → close (asks first if unsaved) |

**Select**

| Key | Does |
|---|---|
| `tab` / `⇧tab` | Next / previous part in stacking order (wraps) |
| `⌘A` | Select all |

**Any selection** (applies to every selected part it makes sense for; says why if none)

| Key | Does |
|---|---|
| `0`–`9` | Colour |
| `i` `o` `p` | Font small / medium / large (shape text or line label) |
| `w` | Outline: solid → dashed → dotted |
| `⌥ arrows` | Move the selection 10 px (`⌥⇧ arrows`: 1 px). Stops at the page edge. |
| `[` `]` | Send back / bring forward one step in the stacking order |
| `{` `}` | Send to the very back / bring to the very front |
| `⌘C` `⌘V` | Copy / paste (paste lands centred on the pointer if it's on the page, else offset 20 px) |
| `⌘D` | Duplicate, offset 20 px |
| `backspace` / `delete` | Delete |
| `⌘Z` / `⇧⌘Z` | Undo / redo |

**A shape**

| Key | Does |
|---|---|
| `← → ↑ ↓` | Resize 10 px (`⇧`: 40 px) |
| `enter` | Type into it |

**A line**

| Key | Does |
|---|---|
| `a` | Arrowheads: none → single → double |
| `f` | Flip direction |
| `b` | Bend: straight → curved → square (elbow) |
| `m` | Pulse: off → a→b → b→a → from the middle |
| `s` | Pulse look: ball ⇄ light |
| `enter` | Type a label |

**The page / file**

| Key | Does |
|---|---|
| `g` | Snap grid on/off (10 px) |
| `+` `-` `z` | Zoom in / out / fit |
| `e` | Back to viewing |
| `⌘S` | Save |
| `x` | Export (see §9) |

### 7.5 Keys — typing

Typing eats every key (so Tab or an arrow doesn't wander off). The bottom row shows only what applies.

**Typing in a shape**

| Key | Does |
|---|---|
| any printable key | Inserts a letter at the caret (replacing the selection) |
| `enter` | New line |
| `backspace` | Deletes the selection, or the letter before the caret |
| `←` `→` | Move the caret one letter; `⇧←` `⇧→` select |
| `⌥B` `⌥I` `⌥U` | Toggle bold / italic / underline: on the selected letters (off if all already have it; line breaks skipped), or on the letters you type next if nothing is selected. "select some letters first" if only line breaks are selected. (Read from the physical key, since ⌥B types a symbol on a Mac.) |
| `esc` | Done |
| `⌘S` | Still saves |

With a selection, an arrow without ⇧ drops the selection at the edge it points to.

**Typing a line's label**: letters, `backspace` (at the end), `enter` or `esc` to finish. No `←`/`→`, no ⌥ styles, no line breaks.

Typing stops when the text no longer fits ("no room: it holds up to N at this size; a bigger shape or a smaller font (i) holds more"). The whole typing session is **one undo step**.

---

## 8. Selecting, copying and ordering

- **Selection:** one or many parts. The last-selected is the one the status line describes. A selected shape shows a dashed accent box 6 px outside it; a selected line a soft accent glow, plus square handles on both ends when it is the only thing selected.
- **Copy/paste** uses the app's own clipboard (not the system's), kept for the rest of the run so it works between diagrams. Copies go on top with fresh ids.
  - Lines between copied shapes join the *copies*.
  - ⌘C: a line end on a shape that wasn't copied becomes a free point where it was (that shape won't be where the copy goes), and free ends move with the paste.
  - ⌘D: a line end on a shape that wasn't duplicated stays attached to it.
  - ⌘D on a lone line makes a second line between the same shapes, which then fans out beside the first.
- **Stacking order:** array order, back to front. New parts go on top. `Tab` steps through it.
- **Undo:** whole-document snapshots, **200** steps, not saved in the file; redo is dropped by any new change. Not available while typing (the text is one step).
- **Status line** describes the selection: shape kind · colour · font · size · characters used/capacity · outline; for a line: arrow type · colour · pulse · look · route · outline · label length.

---

## 9. Saving and exporting

- **Save (`⌘S`)** writes the diagram as a new version of the file, like a note. A new diagram is created in the folder it was started from. A `●` marks unsaved changes. Offline, the diagram stays open and isn't queued. Files over the 1 MiB read limit can't be opened as a diagram.
- **Export (`x`)** works in viewing and editing. A save dialog opens; the **extension picks the format**: `.jpg`/`.jpeg` (the default if none is typed), `.png`, `.svg`. An empty page says so before any dialog.
  - **JPEG / PNG:** cropped to the drawing's bounds plus a **32 px margin** (at least 120 px), at **2×** scale, on **white**, in the light colours, no selection, no pulses. JPEG quality 0.92; PNG has a white background (not transparent).
  - **SVG:** the same drawing as markup (`viewBox` = bounds + margin, white background rect). Styled letters are separate `<text>` elements with `font-weight="bold"`, `font-style="italic"`, `text-decoration="underline"` and `xml:space="preserve"` where a row has more than one run. A plain single-run row is one `<text>`. Text uses `text-anchor="middle"` and `dominant-baseline="central"`.
- **Thumbnails:** the app draws a small JPEG of a diagram for its folder listing (an empty page has none).

---

## 10. Limits and constants

| Thing | Value |
|---|---|
| Format version | `1` |
| Page size (default / range) | 1200 × 800 / 200–4000 per side |
| Shape size | 24–800 per side |
| Resize step | 10 px (⇧: 40 px) |
| Nudge step (`⌥ arrows`) | 10 px (⇧: 1 px) |
| Text in a shape | 200 characters |
| Label on a line | 40 characters, one row |
| Fonts | small 12 px · medium 18 px · large 28 px |
| Character advance / row height | 0.6 em / 1.3 em |
| Text padding | 6 px per side |
| Grid | 10 px |
| Copy/duplicate offset | 20 px |
| Alignment snap distance | 6 px |
| Line attach distance | 8 px |
| Line click slop / end-handle radius | 7 px / 10 px |
| Parallel-line spacing | ≥ 24 px (label-aware), ≤ 85 % of the smaller shape |
| Pulse speed / rest / light length | 180 px/s / 0.5 s / 56 px |
| Arrowhead | 14 px long |
| Curve bow | 20 % of length, max 70 px |
| Zoom | 25 %–800 % |
| Undo depth | 200 |
| Export | 2× scale, 32 px margin, JPEG 0.92 |

---

## 11. Not in `.lin` (so a new editor doesn't need to)

Groups or layers beyond stacking order · images · per-letter colours or sizes (only
bold/italic/underline, and only in shapes) · text alignment other than centred · styling or
line breaks in line labels · line waypoints beyond the three routes · per-line pulse speed ·
a page-size control in the UI · saved zoom/pan · saved undo history · text outside a shape or
line (put words in a shape).

---

## 12. Where each thing lives in the source (`app/src/`)

| File | What it decides |
|---|---|
| `lib/lin/model.ts` | Types, palette, fonts, limits, defaults (`newShape`, `newLine`) |
| `lib/lin/format.ts` | Reading and writing the file; validation and clamping |
| `lib/lin/geometry.ts` | Text capacity and wrapping, outlines, line paths and fan-out, labels, hit-testing, bounds |
| `lib/lin/text.ts` | Caret, selection, typing at the caret, letter styles |
| `lib/lin/edit.ts` | Every edit as a function from one doc to the next; undo history; status description |
| `lib/lin/keys.ts` | The whole key map and the hint row |
| `lib/lin/pulse.ts` | Pulse timing and positions |
| `lib/lin/snap.ts` | Alignment guides and the grid |
| `lib/lin/clip.ts` | The app's copy/paste clipboard |
| `lib/lin/draw.ts` | Canvas drawing, the themes, JPEG/PNG export |
| `lib/lin/svg.ts` | SVG export |
| `components/LinEditor.svelte` | The editor: mouse, zoom/pan, animation clock, save, export |
| `crates/core/src/mime.rs` | `.lin` is `text/x-lin; charset=utf-8` |

Every pure module has a `*.test.ts` beside it; the tests are the most exact statement of
the behaviour.
