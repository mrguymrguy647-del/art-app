# Painting with Art App: a guide for AI assistants

**New to Art App scripts? Read the [quick start](QUICKSTART.md) first** (one page: a first script,
the loop, the rules that matter). This guide goes further.

You can paint pictures with Art App by writing **painting scripts** (JSON) and rendering them with
**`art-cli`**, a command-line program that needs no screen or graphics card. You can look at the
PNGs it saves, so you can judge your own work and improve it, step by step, like a painter
stepping back from the easel.

This guide is the practical part. Every command is described in
[script-format.md](script-format.md). Complete examples: two paintings, [examples/sunset.json](../examples/sunset.json)
and [examples/night-hills.json](../examples/night-hills.json); for games, a 4-frame walk cycle drawn
with symbols, [examples/duck-walk.json](../examples/duck-walk.json), and a character sheet with a
colour swap, a mirrored pose and labels, [examples/duck-sheet.json](../examples/duck-sheet.json).

## 1. Set up (once)

You need Rust (`cargo`). From the repository folder:

```bash
cargo build --release -p art_cli
```

The program is `target/release/art-cli`. It only depends on the painting engine (no window
libraries), so it builds in under a minute on a plain Linux machine. Check that it works:

```bash
target/release/art-cli render examples/sunset.json --preview --out /tmp/try
```

## 2. The loop

```bash
art-cli check picture.json                          # mistakes, and where everything landed
art-cli render picture.json --preview --sheet       # quick look: half size + every stage in one image
art-cli render picture.json --crop 600,400,300,200 --zoom 3 --grid 50  # close-up with coordinates
art-cli render picture.json --snapshots             # full size, plus a PNG per stage
art-cli render picture.json --preview --layers "Duck"   # one layer on its own (or --hide "Sketch")
art-cli render sprite.json --transparent --bg "#4080A0" # see-through PNG; previews on a colour
art-cli info out/picture.artapp                     # layers, blend modes, what's painted where
art-cli info picture.json                           # the same, plus which commands painted each layer
```

Files go to `out/` (or `--out folder`): `picture.png`, `picture.artapp`, `picture-preview.png`,
`picture-sheet.png`, `snapshots/NN-name.png`, `picture-crop-x-y-w-h.png`, `picture-view.png`.

**Read the numbers before the picture.** `render` ends with a summary of 12 lines or fewer:

```
Rendered sprite.json in 0.01 s
  canvas 256 x 128, 11 commands
  layers, bottom to top:
    Sketch: empty | Body: x 33..223, y 41..103 | Labels: x 57..201, y 107..120
  warnings (3):
    command 10 (rect) draws entirely outside the canvas (x 400..420, y 10..30)
    symbol "unused" (command 3) is defined but never used
    layer "Sketch" is empty
  saved: out/sprite.png, out/sprite.artapp
```

`check` paints the same way without saving, and lists every layer, the outermost named `group`s
and `use`s of symbols with the box around what they drew (`x left..right, y top..bottom`, in
canvas pixels) and how many others are inside each, then all the warnings. `check --detail` (and
`render --detail`) lists every group and use, also the ones inside others, indented. That answers "did the duck land where I meant?" without opening a
picture. Warnings are things that are probably mistakes: something drawn entirely off the canvas,
a layer left empty, a symbol never used, a palette colour never used.

**Viewing aids:** `--grid 100` draws a light grid with coordinates, `--bg "#4080A0"` (with
`--transparent`) shows see-through pictures on a colour, `--layers`/`--hide` choose which layers
show. They go on previews, crops, snapshots and sheets. `picture.png` and `picture.artapp` never
get the grid or the colour; at full size, the picture with them is `picture-view.png`.

The same script always gives exactly the same picture, so a change you see is the change you made.
Errors name the command (counting from 1) and what's wrong:

```
error: picture.json: command 14 (ellipse): unknown colour "skyblue"; the palette has: sea, sky, sun
```

The exit code is 0 when it worked, 1 for a problem in the script or a file, 2 for a wrong command
line.

**Look at the PNGs.** After each render, open the preview or the contact sheet and judge it before
writing more. Then fix the script and render again. Small steps beat one huge script.

## 3. The format in one page

```json
{
  "format": "artapp-script", "version": 1, "title": "…", "seed": 1,
  "palette": { "sky": "#5080D0", "shadow": "#1A1A3080" },
  "commands": [
    { "cmd": "canvas", "width": 1600, "height": 1000, "paper": "#FFFFFF" },
    { "cmd": "layer", "name": "Sky" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 600], "colors": ["#203060", "sky"] },
    { "cmd": "snapshot", "name": "sky" }
  ]
}
```

- Positions are pixels; **y grows downwards**. Angles are degrees, clockwise (0 = right, 90 = down).
- Colours: `"#RRGGBB"`, `"#RRGGBBAA"` (last pair = opacity), a palette name, or `"transparent"`.
- 0-to-1 values: opacity, flow, hardness, tolerance, pressure.
- Any command can have a `"note"`: a comment for yourself. Use them; they make scripts readable.

| Command | Use it for |
|---|---|
| `canvas` | First command: `width`, `height`, `paper`; or `open` an `.artapp` to continue a picture. |
| `layer` | Select or create a layer: `name`, `opacity`, `blend`, `visible`, `above`/`below`. |
| `brush` | `color`, `size`, `opacity`, `flow`, `hardness`, `spacing`, pressure settings. |
| `stroke` / `erase` | Pen-like line through `points` (`[x, y]` or `[x, y, pressure]`). |
| `line` | Straight line, `from` → `to`, tapered (default `taper` 0.25). |
| `path` | Smooth curve through points; `closed` + `fill` for organic shapes; `line: false` to fill only. |
| `rect`, `ellipse`, `polygon` | Shapes with `fill` (colour or gradient) and/or `outline`; `rotation`, `radius`, `erase`. |
| `gradient` | `linear` or `radial`, `colors` or `stops`, whole layer or inside an `area`. |
| `fill` | Paint bucket at a point (`tolerance`, `grow`, `sample`). |
| `hatch` | Parallel strokes filling an `area`: shading, texture, a drawn look. |
| `scatter` | Many random dabs or short strokes in an `area`: stars, leaves, grass, sparkle, ripples. |
| `text` | Letters in the built-in font: `text` (`\n` for lines), `at` (top of the first line), `size`, `color`, `align`, `outline`, `outline_size`, `rotate`. |
| `snapshot` | A named checkpoint for `--snapshots` and `--sheet`. |
| `group` | Draws its `commands` moved, resized, turned and/or mirrored (`translate`, `scale`, `rotate`, `mirror`, `origin`). |
| `define` / `use` | Store a drawing as a symbol once; paint it many times, placed like a group, with `colors` swapped. |
| `repeat` | Copies of its `commands`, with a `step` (`translate`, `rotate`, `scale` around `origin`) added each time. In `text`, `{i}` is the copy number. |
| `delete_layer`, `rename_layer`, `move_layer`, `clear_layer`, `paper` | Housekeeping. |
| `transform_layer` | Move, turn, resize or mirror what's already on a layer (written by the app's Layer menu). |
| `animations` (top level) | Named animations: `frames`, `fps`, `loop`, and `commands` played after the script's own, once per frame. Render with `--frame`, `--frames`, `--all-frames`. |

Areas: `{"rect": [x, y, w, h]}`, `{"ellipse": [cx, cy, rx, ry]}`, `{"circle": [cx, cy, r]}`,
`{"polygon": [[x, y], ...]}`.

Drawing commands can change the brush just for themselves: `color`, `size`, `opacity`, `hardness`,
`flow`. Add `jitter` (pixels) for a hand-made wobble.

## 4. A workflow that works

1. **Plan before painting.** Choose the canvas size and the composition: where's the horizon,
   where's the main subject (a third of the way across often works better than the middle),
   where does the light come from? Write a **palette of 8–15 colours** with names that say what
   they're for (`sky_top`, `shadow`, `rim`). A small palette looks deliberate.
2. **Block in, back to front**, each part on its own layer: sky → far things → middle → near
   things. Big shapes only: gradients, `polygon`, closed `path`s, `ellipse`s. `snapshot`.
3. **Check:** `--preview --sheet`. Does the picture read at a glance? Are the darks and lights in
   the right places? Fix the big things now; it's cheap.
4. **Light and shadow** on their own layers: a `multiply` layer for shadows, `screen` or `add`
   for glows, a few thin `screen` strokes for rim light. `snapshot`.
5. **Texture and detail**: `scatter`, `hatch`, `jitter`, small strokes. Mostly near the focal
   point and in front; far things need less. `snapshot`.
6. **Inspect details** with `--crop … --zoom 3`. Then the full render.
7. **Deliver** the PNG, the `.artapp` (layers kept, can be opened in the app) and the script.

## 5. Techniques that work (all from the examples)

**Sky:** a gradient through 3–5 colours, darker at the top.

```json
{ "cmd": "gradient", "from": [0, 0], "to": [0, 620],
  "stops": [[0, "#2A1E48"], [0.35, "#6B3A6E"], [0.7, "#E0704A"], [0.88, "#F7B25A"], [1, "#FFDCA0"]] }
```

**A glow** (sun, moon, lamp): a radial gradient fading to transparent on a `screen` layer, under
the light source itself.

```json
{ "cmd": "layer", "name": "Moon glow", "blend": "screen" },
{ "cmd": "gradient", "kind": "radial", "from": [1180, 230], "to": [1180, -170],
  "stops": [[0, "#9FB4E8A0"], [0.3, "#5A70B040"], [1, "transparent"]] }
```

**A round body with light on it:** an ellipse filled with a radial gradient whose centre (`from`)
is moved towards the light.

```json
{ "cmd": "ellipse", "center": [1180, 230], "radius": 70,
  "fill": { "gradient": "radial", "from": [1160, 210], "to": [1180, 300], "colors": ["#FFFFFF", "#F4F1DE", "#D8D2B8"] } }
```

**Hills and land:** a closed `path` (smooth) with `line: false` and a little `jitter`. Run it off
the canvas edges so there are no gaps.

```json
{ "cmd": "path", "closed": true, "line": false, "fill": "far_hill", "jitter": 4,
  "points": [[-50, 620], [200, 520], [420, 560], [700, 470], [980, 540], [1250, 490], [1650, 570], [1650, 1000], [-50, 1000]] }
```

**Depth (aerial perspective):** far things are lighter, bluer and lower in contrast; near things
darker and sharper. A soft `screen` gradient band between layers reads as mist.

**Stars, sparkles, snow:** `scatter` small dabs; two scatters (many faint, a few bright) look
natural.

```json
{ "cmd": "scatter", "area": { "rect": [0, 0, 1600, 560] }, "count": 500, "size": [1, 2.5], "color": "#FFFFFF", "opacity": [0.2, 0.7], "seed": 1 }
```

**Water:** short horizontal `scatter` strokes, smaller and denser towards the horizon (three
bands), plus bright glints in a column under the light.

```json
{ "cmd": "scatter", "area": { "rect": [0, 626, 1600, 70] }, "count": 900, "kind": "stroke",
  "length": [6, 28], "angle": [-1, 1], "size": [1, 2], "color": "#141530", "opacity": [0.15, 0.35] }
```

**Grass:** `scatter` strokes pointing up: `"angle": [-100, -80]`, a few lighter ones where the
light hits.

**Soft light on the water or a soft cloud:** a very soft brush (`hardness` 0–0.1, `flow` 0.3–0.6)
and a big `size`; `line` or `path` with `taper`.

```json
{ "cmd": "line", "from": [800, 622], "to": [800, 1060], "size": 300, "hardness": 0, "flow": 0.6,
  "opacity": 0.45, "color": "#D07848", "taper": [0.9, 0] }
```

**Shadows:** a `multiply` layer in a dark, cool colour. A flattened ellipse under an object on
the side away from the light grounds it. `hatch` at low opacity gives shaded texture.

**Rim light:** thin tapered `path`s on a `screen` layer along the edges that face the light.

**Hand-made edges:** `jitter` of 2–6 pixels on shapes and paths. Without it, shapes look like
computer graphics.

**Draw once, place many times:** draw a part (a wing, a leg, a tree) inside a `group`, in its own
coordinates around `[0, 0]`, and use `translate`, `scale`, `rotate` and `mirror` to put it in
place. The other wing is the same group with `"mirror": "x"` and an `origin` at the body's middle.
No coordinate maths by hand.

**Characters and props as symbols:** `define` each character (or tree, house, coin) once, around
`[0, 0]`, using palette names for its colours, then `use` it wherever it's needed. A colour-swapped
variant is one line: `"colors": { "yellow": "#C04040" }`. Facing the other way: `"mirror": "x"`.
Parts that move (legs, arms) can be symbols of their own, used inside the character.

**Labels and titles:** `text` with an `outline` in a contrasting colour stays readable on any
background. `at` is the top of the first line; capitals take up y from `at` + 0.24 × `size` to
`at` + 0.97 × `size`, and each line is 1.21 × `size` high. Use `"align": "center"` to centre a
label under a character without measuring it. Labels that belong to a drawing go inside its
group or symbol, so they move with it.

```json
{ "cmd": "text", "text": "Player 2", "at": [64, 132], "size": 18, "color": "ink", "align": "center", "outline": "white", "outline_size": 3 }
```

**Rows and rings:** `repeat` with a `step`: fence posts (`translate`), petals or spokes (`rotate`
around the centre as `origin`), shrinking copies (`scale`).

## 6. Light, colour and shapes: rules of thumb

- **Values first.** A picture works when its lights and darks work. Render a tiny preview
  (`--scale 0.2`): if you can't tell what it is, fix the values, not the details.
- **One light source**, used consistently: lit sides, shadows and rim light all agree with it.
- **Shadows** are rarely black: use a dark, cooler version of the scene's colours, on `multiply`.
- **Saturation:** keep most colours calm and save the strongest for the focal point.
- **Edges:** soft far away and in shadow, crisp near the focal point.
- **Layers:** one idea per layer, named for what's on it. It makes fixing things easy.
- **Detail where it matters.** The eye goes to contrast and detail; put them where you want it.

## 7. Checking your own work

When you look at a render, ask:

1. Can you tell what it is at thumbnail size (the contact sheet)? Is there one clear focal point?
2. Are the darks and lights where you meant them? Is anything muddy (everything mid-grey)?
3. Is the light consistent?
4. Edges: any hard, computer-straight edges that should be organic (`jitter`, `path` instead of
   `polygon`)? Any gaps where shapes should meet (run shapes past each other and past the edges)?
5. Zoom in (`--crop … --zoom 3`) on faces, outlines and joins: holes, halos, wrong layer order?
6. Do the numbers agree? The boxes from `check` say where each layer, group and symbol ended up;
   use `--grid` to read positions off the picture, and `--layers` to look at one layer alone.
7. `art-cli info out/x.artapp` or `art-cli check`: are the layers what you expect, in the right
   order, with the right blend modes? Any warnings?

Write down what's wrong, change only that, and render again.

## 8. Common mistakes

- **y goes down.** `[x, 0]` is the top. A "higher" thing has a *smaller* y.
- **A misspelt layer name makes a new layer.** `{"cmd": "layer", "name": "Skye"}` silently creates
  "Skye". `art-cli check` lists the layers: look at the list.
- **`gradient` covers the whole layer** unless it has an `area`. Put big gradients on their own
  layer, or give an area.
- **Drawing on the wrong layer.** Every drawing command paints on the layer selected last.
- **Blend modes only work on what's below.** A `multiply` layer darkens the layers *under* it
  (over plain white paper it just shows its own colour): put shadow layers above the things they
  shade, and light layers (`screen`, `add`) above what they light.
- **Opacity twice.** Layer opacity × command opacity × colour opacity (`#RRGGBBAA`) all multiply.
  If something is fainter than expected, check all three.
- **The paint bucket leaks** through gaps in an outline. Prefer filled shapes (`polygon`, closed
  `path`) over outline + `fill`; if you do use `fill`, close the outline and use `grow` 1–2.
- **Too many tiny strokes.** Use shapes, `scatter` and `hatch` instead of hundreds of `stroke`s.
- **Random patterns change when you insert a command** (they depend on the command's number).
  Give `scatter`, `hatch` and jittered shapes their own `"seed"` once you like them.
- **Previews scale everything**, including brush sizes. A line that would be thinner than a pixel
  is drawn a pixel wide and fainter, like in the full picture shrunk, so fine lines look light in
  previews. Check them with `--crop`, which is always full size (and may go past the canvas's
  edges: what's outside shows as grey squares).
- **Shapes stopping at the canvas edge leave slivers.** Run backgrounds and land past the edges.
- **The paper must be opaque.** For a transparent PNG use `--transparent` instead.
- **Mirror and rotate work around `origin`**, which is `[0, 0]` unless you give it. To flip or
  turn a drawing in place, set `origin` to its middle.
- **Symbols are defined before they're used**, only at the top level, and each name once.
- **Text `at` is the top of the line, not the baseline.** Letters start a little below it. Give
  `size` in pixels (it's the font size; a line is 1.21 × that high).
- **Colour swaps only work on colours the symbol uses.** Give symbols palette colours, and swap by
  those names.

## 9. Working with a person's drawing

People can draw in the Art App window and send you their work:

- **The picture:** an `.artapp` file. Continue it with `{"cmd": "canvas", "open": "their.artapp"}`
  (path relative to your script), then add layers on top. Use `art-cli info` to see its layers.
- **How it was drawn:** **File → Export strokes…** in the app writes everything they painted as a
  script: their layers, brush settings and every stroke as `[x, y, pressure]` points. Read it to
  see what they drew and where, and to match their brush sizes and colours.
- They can watch your script being painted in the app with **File → Play strokes…**.

## 10. Speed

- A 1600×1000 render takes well under a second; `--preview` about a third of that. `check`
  paints at full size too (so its boxes are exact), in about the time of a render.
- Snapshots and the contact sheet cost a little per stage; use a few meaningful snapshots rather
  than one per command.
- `scatter` and `hatch` with thousands of strokes are fine; hundreds of thousands are slow.
