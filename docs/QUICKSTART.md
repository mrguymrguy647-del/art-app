# Quick start: painting with scripts

Art App paints pictures from **painting scripts**: JSON files that list what to draw, step by step.
`art-cli` turns a script into a PNG (and an `.artapp` file with the layers), with no window or
graphics card. This page is enough to start; [script-format.md](script-format.md) has every
command and option, and [AI_GUIDE.md](AI_GUIDE.md) has techniques and a workflow.

## 1. Build art-cli (once)

```bash
cargo build --release -p art_cli
```

The program is `target/release/art-cli` (`art-cli.exe` on Windows).

## 2. A first script

Save this as `two-ducks.json` (it's also in `examples/`):

```json
{
  "palette": { "sky": "#8EC5E8", "water": "#3F7FB0", "yellow": "#F2C230",
               "beak": "#E08020", "ink": "#202020", "white": "#FFFFFF" },
  "commands": [
    { "cmd": "canvas", "width": 640, "height": 360, "paper": "white" },
    { "cmd": "layer", "name": "Sky" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 220], "colors": ["#5A9BD0", "sky"] },
    { "cmd": "layer", "name": "Water" },
    { "cmd": "rect", "x": -10, "y": 220, "width": 660, "height": 150, "fill": "water" },
    { "cmd": "define", "name": "duck", "commands": [
      { "cmd": "layer", "name": "Ducks" },
      { "cmd": "ellipse", "center": [0, 0], "radius": [34, 22], "fill": "yellow", "outline": "ink", "outline_size": 3 },
      { "cmd": "ellipse", "center": [26, -26], "radius": 15, "fill": "yellow", "outline": "ink", "outline_size": 3 },
      { "cmd": "polygon", "points": [[38, -29], [56, -24], [38, -18]], "fill": "beak" },
      { "cmd": "ellipse", "center": [31, -30], "radius": 3, "fill": "ink" }
    ] },
    { "cmd": "use", "name": "duck", "translate": [200, 230] },
    { "cmd": "use", "name": "duck", "translate": [440, 240], "mirror": "x", "colors": { "yellow": "#C04040" } },
    { "cmd": "layer", "name": "Words" },
    { "cmd": "text", "text": "Two ducks", "at": [320, 24], "size": 48, "color": "white", "align": "center", "outline": "ink", "outline_size": 3 }
  ]
}
```

What it says: a palette of named colours; a 640 × 360 canvas; a sky gradient and water on their
own layers; a duck drawn once (`define`) around `[0, 0]`, then painted twice (`use`): moved, and
the second one mirrored and red; a title with an outline.

## 3. The loop

```bash
art-cli check two-ducks.json             # mistakes, and where everything landed
art-cli render two-ducks.json --preview  # a quick look: out/two-ducks-preview.png
art-cli views two-ducks.json             # seven views on one sheet: out/two-ducks-views.png
art-cli render two-ducks.json            # full size: out/two-ducks.png and .artapp
```

Read what it prints before opening the picture:

```
OK: two-ducks.json (painted in 0.03 s)
  canvas 640 x 360, 10 commands
  layers, bottom to top:
    Sky: x 0..640, y 0..360 (command 3)
    Water: x 0..640, y 220..360 (command 5)
    Ducks: x 164..476, y 187..264 (commands 7, 8)
    Words: x 196..444, y 32..75 (command 10)
  named groups and uses (outermost):
    command 7 (use "duck"): x 164..256, y 187..254
    command 8 (use "duck"): x 384..476, y 197..264
```

Boxes are `x left..right, y top..bottom` in canvas pixels. Warnings follow, if any: drawn off the
canvas, empty layers, unused symbols or colours. Then look at the PNG, change the script, repeat.

Seeing more: `--crop x,y,w,h --zoom 3` (a close-up), `--grid 50` (coordinates on the picture),
`--layers "Ducks"` (one layer alone), `--transparent --bg "#4080A0"` (see-through, on a colour),
`--snapshots --sheet` (every stage). The views sheet shows the picture as painted, in gray,
squinted (blurred: the big shapes), as a thumbnail, flipped, the named parts' silhouettes, and a
heat map of where the eye goes with the intent's focal point ringed; `--heat` (or `--gray`,
`--squint`, `--thumb`, `--flip`, `--silhouette`) saves that one view alone.

## 4. The rules that matter

- **x goes right, y goes down.** `[0, 0]` is the top-left corner. Higher up = smaller y.
- **Commands paint on the layer selected last** (`layer` selects or creates one).
- **Later commands paint on top** within a layer, and a new layer goes on top of the others
  (unless it says `above` or `below` another).
- **Colours:** palette names, `"#RRGGBB"`, or `"#RRGGBBAA"` (see-through).
- **Mistakes name the command:** `command 8 (use "duck"): there's no symbol called "duk"`.
  Commands are counted from 1, `canvas` included.

## 5. The commands you'll use most

| Command | For |
|---|---|
| `layer` | Select or create a layer (`blend`: `multiply` for shadows, `screen` for glows). |
| `gradient` | Skies, light: `from`, `to`, `colors`, optionally only in an `area`. |
| `rect`, `ellipse`, `polygon`, `path` | Shapes with a `fill` and/or `outline`; `jitter` for a hand-made edge. |
| `stroke`, `line` | Pen lines through points; tapered lines. |
| `scatter`, `hatch` | Many dabs (stars, leaves, grass); shading lines. |
| `text` | Words: `at` is the top of the first line; `size` in pixels; `align`, `outline`. |
| `group` | Move, turn, resize or mirror what's inside: `translate`, `rotate`, `scale`, `mirror`, `origin`. |
| `define` / `use` | Draw something once, paint it many times, with `colors` swapped. |
| `repeat` | Copies with a `step`; `"each"` gives every copy its own values; `{i}` in text is the copy number. |
| `snapshot` | A named stage, saved by `--snapshots` / `--sheet`. |

## 6. Animation and pixel art

An `animations` section makes the script an animation: each one plays the script's commands and
then its own, once per frame. Any number of a `group` or `use` can be keyframes:

```json
{
  "palette": { "yellow": "#F2C230", "ink": "#202020" },
  "commands": [
    { "cmd": "canvas", "width": 64, "height": 64 },
    { "cmd": "define", "name": "ball", "commands": [
      { "cmd": "ellipse", "center": [0, -10], "radius": 10, "fill": "yellow", "outline": "ink" } ] }
  ],
  "animations": {
    "bounce": { "frames": 8, "fps": 12, "loop": true, "commands": [
      { "cmd": "use", "name": "ball", "translate": { "keys": [[0, [32, 60]], [4, [32, 30], "out"], [8, [32, 60], "in"]] } } ] }
  }
}
```

```bash
art-cli check bounce.json                         # every frame, plus a motion summary
art-cli render bounce.json --frame-sheet --transparent   # all frames in one numbered picture
art-cli export bounce.json --gif --sheet          # a GIF, and a sprite sheet with its JSON
```

Pixel art: `{ "cmd": "canvas", "width": 32, "height": 32, "pixel": true }` paints the same
commands bigger and shrinks them to crisp whole pixels in the palette's colours. Examples:
`examples/duck-animated.json` and `examples/duck-pixel.json`.

## 6b. Making it beautiful

Once a picture reads, five moves make it rich, each on its own layers: living fills (`noise`
in skies, water and rock; `blur` + `adjust` on far things), shading inside shapes (`"clip":
true` layers, `mask` on groups), texture with brushes (`tip`, `dynamics`, `stamp`), light and
depth (`glow`, `shadow` and `outline` filters on groups), and one faint unifying pass (a
`"from": "below"` layer with a `gradient_map`, paper `texture` on `multiply`). The order and a
worked example are in [AI_GUIDE.md](AI_GUIDE.md), "Making it beautiful".

## 7. Where next

- [script-format.md](script-format.md): every command and option, with examples.
- [AI_GUIDE.md](AI_GUIDE.md): a workflow, techniques for skies, light and texture, common mistakes.
- `art-cli help`: every option of `render`, `check` and `info`.
