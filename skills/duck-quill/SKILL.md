---
name: duck-quill
description: Paint pictures, game sprites, sprite sheets, pixel art and short animations (GIF, MP4) with Duck Quill painting scripts (JSON) and the quill renderer, on a machine with no screen. Use when asked to draw, paint, animate or illustrate something with Duck Quill, to write, fix or extend a painting script, or to render, check, export or inspect one (quill render / check / export / info).
---

# Painting with Duck Quill

A painting script is a JSON file of commands; `quill` paints it into a PNG and a `.quill`
(layers kept). Same script, same pixels, every time. One-page intro: `docs/QUICKSTART.md`. Every
command and option: `docs/script-format.md`. Techniques: `docs/AI_GUIDE.md`.

## Setup (once)

```bash
cargo build --release -p quill_cli
```

Then use `target/release/quill` (below `quill`; `art-cli` still works). Start from `examples/two-ducks.json`.

## The process (after every stage: `quill views` and `quill critique`; fix before moving on)

| Stage | Do | Check |
|---|---|---|
| 1 Intent | the `intent` block; a palette of 8–15 colours named for their jobs; read `skills/duck-quill/lessons.md` and the 2–3 nearest gallery pieces | `quill check`: the intent printed back, no warnings |
| 2 Thumbnail | `render --scale 0.1`: three to five big value shapes, nothing else | the thumb reads; 3+ value groups; the focal point near where it's wanted |
| 3 Values | full size, mostly grey or one hue: light and dark decided for every shape | no part merges at thumbnail size |
| 4 Colour | flat local colours in key, back to front | nothing out of key; warm/cool as the mood wants |
| 5 Light and form | clipped shadow and light layers, one light direction | the squint shows form; the focal point has the strongest contrast |
| 6 Detail | texture, edges, marks, mostly at the focal point | detail gathers there; nothing noisy, blotchy or flat that shouldn't be |
| 7 Finish | glow, shadow, mood pass, paper | views and critique again; then the blind-read test |

Detail budget: a finished 1080p painting is hundreds of commands, most at the focal point; a background earns ten, a
face a hundred. Small steps, never the whole picture blind: `quill check picture.json` after each change (it paints
without saving; boxes read `x left..right, y top..bottom`; errors name the command), `quill render picture.json --preview`
to look, `--crop 600,400,300,200 --zoom 3 --grid 50` for close-ups, `quill render picture.json` for `out/picture.png` and `out/picture.quill`.

The blind-read test before calling it done: `quill blind-read picture.json` makes a packet (`out/blind-read-7f3a/`:
the picture as `piece-7f3a.png` and `questions.txt`, nothing else); give a separate agent or a fresh session only that
folder, never the script, the notes or a named file (they leak the intent); then `quill blind-read picture.json
--record answers.txt --verdict "reads"` (or `"doesn't read: ..."`) writes `blind-read.md`. If the answers don't match
the intent, the picture doesn't read yet. Numbers are warnings, never goals; the person's rating outranks every number.

## Skeleton (write the `intent` first: `check` prints it back, the senses hold the picture to it)

```json
{
  "palette": { "ink": "#202020", "body": "#F2C230", "sky": "#8EC5E8" },
  "intent": { "subject": "a duck under a blue sky", "focal": "duck", "light": "top left", "mood": "cheerful", "key_colors": ["sky", "body"] },
  "commands": [
    { "cmd": "canvas", "width": 512, "height": 256, "paper": "sky" },
    { "cmd": "layer", "name": "Background" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 256], "colors": ["#5A9BD0", "sky"] },
    { "cmd": "layer", "name": "Characters" },
    { "cmd": "group", "name": "duck", "commands": [
      { "cmd": "ellipse", "center": [256, 150], "radius": [40, 28], "fill": "body", "outline": "ink", "outline_size": 3 } ] }
  ]
}
```

## Rules that prevent most mistakes

- x right, y **down**; `[0, 0]` is the top-left. `rect` takes `x`, `y` (top-left), `width`, `height`.
- Commands paint on the layer selected last; a new layer goes on top. Misspelt layer names make new layers: read
  the layer list from `check`. `gradient` without an `area` covers the layer.
- Colours: palette names, `#RRGGBB`, `#RRGGBBAA`; symbols use palette names so `colors` swaps work.
- Run backgrounds past the canvas edges; shapes that stop at an edge leave slivers.

## Ready parts: the library (finished pieces, with scripts and notes, in `skills/duck-quill/gallery/`)

`skills/duck-quill/library/` has a cloud, water, grass, a reed, a tree, a lily pad and the duck, each a `define` with parameters and a preview (see its `README.md`). Include one, then `use` it:

```json
{ "cmd": "include", "file": "../skills/duck-quill/library/tree.json" }
{ "cmd": "use", "name": "tree", "translate": [300, 700], "scale": 1.4, "with": { "seed": 3 } }
```

## Draw once, place many times (no coordinate maths)

Define parts around `[0, 0]`, then place them. `mirror` and `rotate` work around `origin`.

```json
{ "cmd": "define", "name": "hero", "commands": [
  { "cmd": "layer", "name": "Characters" },
  { "cmd": "ellipse", "center": [0, 0], "radius": [24, 30], "fill": "body", "outline": "ink", "outline_size": 2 },
  { "cmd": "ellipse", "center": [8, -10], "radius": 3, "fill": "ink" }
] }
{ "cmd": "use", "name": "hero", "translate": [100, 120] }
{ "cmd": "use", "name": "hero", "translate": [300, 120], "mirror": "x", "colors": { "body": "#C04040" } }
```

A `group` with `translate`, `scale` and `rotate` places whatever is inside it the same way.

- `define` only at the top level, before its first `use`, each name once. Symbols can use symbols.
- Layer names inside a symbol are the picture's own layers; after a `use` or `group` the brush and
  layer are back as before. A symbol starts from the plain brush (`"reset": true` resets). Name your
  groups (`"name"`): `check` reports their boxes, errors say where they are, `views` draws their silhouettes.

## Rows, rings and frames: `repeat`

```json
{ "cmd": "repeat", "count": 4, "step": { "translate": [128, 0] }, "commands": [
  { "cmd": "use", "name": "hero", "translate": [64, 64] },
  { "cmd": "text", "text": "frame {i}", "at": [64, 104], "size": 14, "color": "ink", "align": "center" }
] }
```

Each copy gets the `step` once more (`translate`, `rotate` and `scale` around `origin`). In `text`, `{i}` is the copy number from 1, `{i0}` from 0.
`text` takes `text`, `at` (the **top** of the first line, by `align`), `size`, `color`, `outline` and `outline_size`;
a line is 1.21 × `size` high; `\n` starts a new line; `rotate` turns it around `at`.

## Animation: frames, keyframes, parameters

The canvas is one frame. An `animations` section plays the script's commands and then its own, once per frame
(layers nothing moves on are painted once and reused); time is the frame number from 0. Any number of a `group` or `use` can be keyframes; a symbol's `params` are set by `with`.

```json
{
  "palette": { "ink": "#202020", "body": "#F2C230" },
  "pivot": [64, 116],
  "commands": [
    { "cmd": "canvas", "width": 128, "height": 128 },
    { "cmd": "define", "name": "leg", "params": { "angle": 0 }, "commands": [
      { "cmd": "group", "rotate": "$angle", "commands": [
        { "cmd": "line", "from": [0, 0], "to": [0, 16], "size": 4, "color": "ink", "taper": 0 },
        { "cmd": "group", "translate": [0, 16], "upright": true, "commands": [
          { "cmd": "rect", "x": -3, "y": 0, "width": 12, "height": 4, "fill": "ink" } ] } ] } ] },
    { "cmd": "define", "name": "hero", "params": { "near": 0, "far": 0 }, "commands": [
      { "cmd": "use", "name": "leg", "translate": [-4, 0], "with": { "angle": "$far" } },
      { "cmd": "ellipse", "center": [0, -24], "radius": [24, 20], "fill": "body", "outline": "ink", "outline_size": 2 },
      { "cmd": "use", "name": "leg", "translate": [4, 0], "with": { "angle": "$near" } } ] }
  ],
  "animations": {
    "walk": { "frames": 8, "fps": 12, "loop": true, "commands": [
      { "cmd": "use", "name": "hero", "translate": [64, 100],
        "with": { "near": { "keys": [[0, -25], [4, 25], [8, -25]], "ease": "in-out" },
                  "far":  { "keys": [[0, -25], [4, 25], [8, -25]], "ease": "in-out", "shift": 4 } } } ] }
  }
}
```

- `keys` are `[frame, value]` (a third element names the easing into that key: `linear`, `in`,
  `out`, `in-out`, `step`). Numbers, `[x, y]` and colours blend; names hold (`"name": "$mouth"`
  swaps a mouth). A loop's last key is at `frames`, so it closes without a pop.
- `shift` plays the same keys later (the far leg, the other arm); `"upright": true` keeps a foot flat while
  the leg turns; `pivot` is where the character stands.
- `each` on `repeat` gives every copy its own values (`"each": {"paint": ["red", "blue"]}`, used as `"$paint"`);
  `quill measure "words" --size 32` and `"fit": 180` size text first.
- Check without watching: `quill check walk.json` (motion summary, jump and loop-pop warnings), `quill render
  walk.json --frame-sheet --transparent`, `--frame 3 --onion`. Export: `quill export walk.json --sheet --gif --mp4
  --scale 4` (sprite sheet with its JSON, `docs/sprite-sheet-format.md`, a GIF, an MP4). `examples/duck-animated.json`
  is a full idle and walk; `examples/duck-sheet.json` a character sheet (swap, mirror, labels).

## Pixel art

`{ "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 8, "outline": "ink" } }`:
every command is painted 8 times bigger and shrunk to crisp whole pixels in the palette's colours (the palette is locked: no hex colours; 4 to 8 colours is right). Decimals are fine in positions.
Tiny things are `pixels` rows: `{ "cmd": "pixels", "at": [6, 3], "colors": { "k": "ink", "w": "body" }, "rows": ["kw"] }`. Animation moves by whole pixels. Look with `--zoom 8`; export `--scale 1` for the game, `--scale 8` for people. `examples/duck-pixel.json` is a 32-pixel duck with idle and walk.

## Seeing and checking

- `quill views picture.json` saves `out/picture-views.png`: the picture, gray, squint (big shapes), thumb, flip,
  the named parts' silhouettes, and a heat map of where the eye goes with the `focal` point ringed; `--heat`
  (`--gray`, `--squint`, `--thumb`, `--flip`, `--silhouette`) saves one alone.
- `quill critique picture.json`: about fifteen findings in numbers with hints (values, focal point, colour and
  key, texture, readability at 96 px, composition; `--detail` every part; `--frames 0-7` an animation).
- Learn: `skills/duck-quill/lessons.md` (rules, each with the piece that shows it), `skills/duck-quill/mistakes.md`
  (before pictures and the critique line that catches each), `skills/duck-quill/styles/`, `skills/duck-quill/exercises/`,
  `skills/duck-quill/references/` (public domain or CC0 only; `quill palette`, `quill values`, `quill compare`).
  `skills/duck-quill/references/sketchbook/`: the user's own drawings, for line weight, directional shading, pose
  and energy marks (learn the technique, never redraw their fan-art characters).
- Finished: `quill gallery add piece.json --name name` (folder, sheets, critique, blind-read skeleton, notes,
  index tags); `quill gallery rate name 4 "why"` is the person's rating, and it outranks every number.
- `quill check picture.json --detail` lists the groups and uses inside others; `quill info picture.json` gives each layer's box.
  `--layers "Characters"` shows only some layers; `--hide "Background"` all but some. `--snapshots --sheet` saves a
  picture at every `snapshot` command and layer change (add `{ "cmd": "snapshot", "name": "blocked in" }` after big
  stages) and one sheet of them all. `--crop` may go past the canvas edges; `--scale 0.5` saves `name-50pct.png`;
  `--pixels` prints a pixel canvas as palette letters; thin lines look faint in previews.

## Making it beautiful

- Shadows: a `multiply` layer in a dark cool colour; glows: `screen` or `add`.
- Texture: `brush` with a `tip` (`chalk`, `bristle`, `leaf`, `grass`, `sparkle`, `square`) and `dynamics`
  (`follow`, `size`, `opacity`, `scatter`, `hue`, `lightness`); `stroke`, `path`, `hatch` and `scatter` use it.
  `stamp` puts a symbol or tip along a path every `spacing`. Marks have a direction; a noise fill doesn't.
- A `fill` of `{"noise": {"scale", "detail", "contrast", "colors"}}` is a base for clouds, water, rock, skin,
  never the shading itself (it reads as marble); paper grain: `{"texture": "paper"}` on `multiply` at 0.3.
- Depth and light: `filter` with `blur` + `adjust` on far layers (blur the fill, not the silhouette);
  `"filter": {"glow": ...}` on a sun's group, `{"shadow": ...}` on a character's `use`; a `"from": "below"`
  layer with a `gradient_map` at 0.3 to unify.
- One clean line around a character: `"outline": {"color": "ink", "size": 3}` on its `use` (no seams),
  `"rim": {...}` inside it for rim light. Shading inside a shape: a `"clip": true` layer above it;
  `"lock": true` recolours a layer without changing its edges; a group's `mask` limits its paint, and so does
  `{"cmd": "select", "rect": [x, y, w, h]}` for every command after it (`"none": true` ends it).
- `jitter` (2–6) on shapes gives a hand-made edge; `seed` a random command once you like it. The worked example:
  `docs/AI_GUIDE.md`, "Making it beautiful".

## Report back

The PNG, the views sheet, the critique, the blind read, and honestly what's still weak; then `quill gallery add`.
