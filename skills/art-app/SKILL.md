---
name: art-app
description: Paint pictures, game sprites, sprite sheets and character sheets with Art App painting scripts (JSON) and the art-cli renderer, on a machine with no screen. Use when asked to draw, paint or illustrate something with Art App, to write, fix or extend a painting script, or to render, check or inspect one (art-cli render / check / info).
---

# Painting with Art App

A painting script is a JSON file of commands; `art-cli` paints it into a PNG and an `.artapp`
(layers kept). Same script, same pixels, every time. One-page intro: `docs/QUICKSTART.md`. Every
command and option: `docs/script-format.md`. Techniques: `docs/AI_GUIDE.md`.

## Setup (once)

```bash
cargo build --release -p art_cli
```

Then use `target/release/art-cli` (below just `art-cli`). Start new scripts from
`examples/two-ducks.json`.

## The loop

1. Plan: canvas size, a palette of 6–15 named colours, the layers back to front.
2. `art-cli check picture.json`: it paints without saving and prints each layer's box, the
   outermost named groups and uses with their boxes, and warnings. Fix every warning.
3. `art-cli render picture.json --preview`: look at `out/picture-preview.png`. Judge it.
4. Close-ups: `art-cli render picture.json --crop 600,400,300,200 --zoom 3 --grid 50`.
5. Change a few things, go back to 2. Small steps; never write the whole picture blind.
6. Final: `art-cli render picture.json` gives `out/picture.png` and `out/picture.artapp`.

Boxes read `x left..right, y top..bottom` in canvas pixels: compare them with what you meant
before opening any picture. Errors name the command: `command 8 (use "duck") > 2 (ellipse): ...`.

## Skeleton

```json
{
  "palette": { "ink": "#202020", "body": "#F2C230", "sky": "#8EC5E8" },
  "commands": [
    { "cmd": "canvas", "width": 512, "height": 256, "paper": "sky" },
    { "cmd": "layer", "name": "Background" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 256], "colors": ["#5A9BD0", "sky"] },
    { "cmd": "layer", "name": "Characters" },
    { "cmd": "ellipse", "center": [256, 150], "radius": [40, 28], "fill": "body", "outline": "ink", "outline_size": 3 }
  ]
}
```

## Rules that prevent most mistakes

- x right, y **down**; `[0, 0]` is the top-left. `rect` takes `x`, `y` (top-left), `width`, `height`.
- Commands paint on the layer selected last; a new layer goes on top. Misspelt layer names make
  new layers: read the layer list from `check`.
- Colours: palette names, `#RRGGBB`, `#RRGGBBAA`. Symbols should use palette names so `colors`
  swaps work.
- Run backgrounds past the canvas edges; shapes that stop exactly at an edge leave slivers.
- `gradient` without an `area` covers the whole layer.

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
{ "cmd": "group", "name": "big hero", "translate": [420, 120], "scale": 1.5, "rotate": 10, "commands": [
  { "cmd": "use", "name": "hero" }
] }
```

- `define` only at the top level, before its first `use`, each name once. Symbols can use symbols.
- Layer names inside a symbol are the picture's own layers; after a `use` or `group` the brush
  and the selected layer are back as before.
- Name your groups (`"name"`): `check` reports their boxes, and errors say where they are.

## Rows, rings and frames: `repeat`

```json
{ "cmd": "repeat", "count": 4, "step": { "translate": [128, 0] }, "commands": [
  { "cmd": "use", "name": "hero", "translate": [64, 64] },
  { "cmd": "text", "text": "frame {i}", "at": [64, 104], "size": 14, "color": "ink", "align": "center" }
] }
```

Each copy gets the `step` once more (`translate`, `rotate` and `scale` around `origin`). In
`text`, `{i}` is the copy number from 1, `{i0}` from 0.

## Text

```json
{ "cmd": "text", "text": "Player 1", "at": [256, 20], "size": 32, "color": "ink", "align": "center", "outline": "#FFFFFF", "outline_size": 3 }
```

`at` is the **top** of the first line (left end, middle or right end by `align`). A line is
1.21 × `size` high; capitals run from 0.24 × `size` to 0.97 × `size` below the top. `\n` starts
a new line; `rotate` turns it around `at`. The font is built in.

## Sprites and sheets

- See-through background: `art-cli render sheet.json --transparent --bg "#4080A0"`. The PNG
  keeps its transparency; previews, crops and `sheet-view.png` show it on the colour.
- Frames: a canvas of whole frames (four 128 × 128 frames: 512 × 128), the character as a symbol
  drawn around `[0, 0]`, one `use` per frame (or a `repeat` with the frame width as `translate`).
  Change only what moves between frames (legs, arms: symbols of their own, turned with `rotate`).
- `--grid 128` shows the frame edges; `--crop 128,0,128,128 --zoom 4` shows one frame.
- Worked examples: `examples/duck-walk.json` (walk cycle: a body and a leg symbol, one named
  group per frame) and `examples/duck-sheet.json` (character sheet: swap, mirror, labels).

## Seeing and checking

- `art-cli check picture.json --detail`: also the groups and uses inside others.
- `art-cli info picture.json`: each layer's box and which commands painted it.
- `--layers "Characters"` shows only some layers; `--hide "Background"` all but some.
- `--snapshots --sheet` saves a picture at every `snapshot` command and layer change, and one
  sheet with all of them. Add `{ "cmd": "snapshot", "name": "blocked in" }` after big stages.
- `--crop` may go past the canvas edges (outside shows as grey squares).
- Previews make lines thinner than a pixel faint; check fine lines with `--crop`.

## Look and quality

- Block in big shapes back to front first; details last, mostly at the focal point.
- Shadows: a `multiply` layer in a dark cool colour; glows: `screen` or `add`.
- `jitter` (2–6) on shapes gives a hand-made edge; give random commands a `seed` once you like them.
- Judge the picture at thumbnail size: if it doesn't read small, fix values and shapes, not detail.

## Report back

Say what you made, show the PNG, give the `check` summary, and say honestly what's still weak.
