---
name: art-app
description: Paint pictures, game sprites, sprite sheets, pixel art and short animations (GIF, MP4) with Art App painting scripts (JSON) and the art-cli renderer, on a machine with no screen. Use when asked to draw, paint, animate or illustrate something with Art App, to write, fix or extend a painting script, or to render, check, export or inspect one (art-cli render / check / export / info).
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

## Animation: frames, keyframes, parameters

The canvas is one frame. An `animations` section plays the script's commands and then its own,
once per frame; time is the frame number from 0. Any number of a `group` or `use` can be
keyframes; a symbol's `params` are set by a `use`'s `with` (keyframes allowed).

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
- `shift` plays the same keys later: the far leg, the other arm, the tail.
- `"upright": true` keeps a foot flat while the leg turns. `pivot` is where the character stands.
- `each` on `repeat` gives every copy its own values (`"each": {"paint": ["red", "blue"]}`,
  used as `"$paint"`); `art-cli measure "words" --size 32` and `"fit": 180` size text first.
- Check without watching: `art-cli check walk.json` (motion summary, jump and loop-pop
  warnings), `art-cli render walk.json --frame-sheet --transparent`, `--frame 3 --onion`.
- Export: `art-cli export walk.json --sheet --gif --mp4 --scale 4` writes the sprite sheet with
  its JSON (`docs/sprite-sheet-format.md`), a GIF and an MP4. `examples/duck-animated.json` is
  a full idle and walk; `examples/duck-sheet.json` a character sheet (swap, mirror, labels).

## Pixel art

`{ "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 8, "outline": "ink" } }`:
every command is painted 8 times bigger and shrunk to crisp whole pixels in the palette's colours
(the palette is locked: no hex colours; 4 to 8 colours is right). Decimals are fine in positions.
Tiny things are `pixels` rows: `{ "cmd": "pixels", "at": [6, 3], "colors": { "k": "ink", "w": "body" }, "rows": ["kw"] }`.
Animation moves by whole pixels. Look with `--zoom 8`; export `--scale 1` for the game and
`--scale 8` for people. `examples/duck-pixel.json` is a 32-pixel duck with idle and walk.

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
