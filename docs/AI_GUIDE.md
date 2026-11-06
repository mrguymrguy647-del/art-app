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
with symbols, [examples/duck-walk.json](../examples/duck-walk.json), a character sheet with a
colour swap, a mirrored pose and labels, [examples/duck-sheet.json](../examples/duck-sheet.json),
an animated duck with keyframes, [examples/duck-animated.json](../examples/duck-animated.json), and
a 32-pixel one, [examples/duck-pixel.json](../examples/duck-pixel.json).

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
  "intent": { "subject": "a hill under a cold sky", "focal": [800, 600], "light": "top left", "mood": "still", "key_colors": ["sky", "shadow"] },
  "commands": [
    { "cmd": "canvas", "width": 1600, "height": 1000, "paper": "#FFFFFF" },
    { "cmd": "layer", "name": "Sky" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 600], "colors": ["#203060", "sky"] },
    { "cmd": "snapshot", "name": "sky" }
  ]
}
```

- `intent` says what you mean (subject, `focal` point or part, `light`, `mood`, `key_colors`);
  it paints nothing, and `check`, `views` and `critique` hold the picture to it. Write it first.
- Positions are pixels; **y grows downwards**. Angles are degrees, clockwise (0 = right, 90 = down).
- Colours: `"#RRGGBB"`, `"#RRGGBBAA"` (last pair = opacity), a palette name, or `"transparent"`.
- 0-to-1 values: opacity, flow, hardness, tolerance, pressure.
- Any command can have a `"note"`: a comment for yourself. Use them; they make scripts readable.

| Command | Use it for |
|---|---|
| `canvas` | First command: `width`, `height`, `paper`; or `open` an `.artapp` to continue a picture. |
| `layer` | Select or create a layer: `name`, `opacity`, `blend`, `visible`, `above`/`below`; `clip` (shown only on the paint below), `lock` (recolour only), `from: "below"` (a flattened copy of everything below). |
| `brush` | `color`, `size`, `opacity`, `flow`, `hardness`, `spacing`, pressure settings; `tip` (`round`, `square`, `bristle`, `chalk`, `leaf`, `grass`, `sparkle`, or a PNG file), `angle`, `dynamics` (`follow`, `size`, `angle`, `opacity`, `scatter`, `hue`, `lightness`, `saturation`). |
| `stroke` / `erase` | Pen-like line through `points` (`[x, y]` or `[x, y, pressure]`). |
| `line` | Straight line, `from` → `to`, tapered (default `taper` 0.25). |
| `path` | Smooth curve through points; `closed` + `fill` for organic shapes; `line: false` to fill only. |
| `rect`, `ellipse`, `polygon` | Shapes with `fill` (colour or gradient) and/or `outline`; `rotation`, `radius`, `erase`. |
| `gradient` | `linear` or `radial`, `colors` or `stops`, whole layer or inside an `area`; or `fill` with any paint (noise, a texture). |
| noise / texture | A `fill` (or brush `color`) of `{"noise": {"scale", "detail", "contrast", "colors"/"stops", "offset", "seed"}}` or `{"texture": "paper"}` / `{"texture": {"file", "scale", "colors"}}`. They move with the group they're written in. |
| `fill` | Paint bucket at a point (`tolerance`, `grow`, `sample`). |
| `hatch` | Parallel strokes filling an `area`: shading, texture, a drawn look. |
| `scatter` | Many random dabs or short strokes in an `area`: stars, leaves, grass, sparkle, ripples. |
| `stamp` | Copies of a `symbol` or dabs of a `tip` along a path, every `spacing`, turned to follow, varied in `size`, `angle` and `colors`. |
| `filter` | On a layer: `blur`, `glow`, `shadow`, `adjust` (hue, saturation, brightness, contrast), `gradient_map`. On a `group`/`use` as `"filter": {...}`: applied to what it draws alone (a sun's glow, a duck's drop shadow). |
| `outline` on `group`/`use` | One line around everything it draws (no seams between parts or layers): `{"color", "size", "rim": {"color", "size", "angle"}}`; `rim` is rim light along the inside edge facing the light. |
| `text` | Letters in the built-in font: `text` (`\n` for lines), `at` (top of the first line), `size`, `color`, `align`, `outline`, `outline_size`, `rotate`. |
| `snapshot` | A named checkpoint for `--snapshots` and `--sheet`. |
| `group` | Draws its `commands` moved, resized, turned and/or mirrored (`translate`, `scale`, `rotate`, `mirror`, `origin`); `mask` keeps the paint inside an area, a symbol's silhouette or a layer's paint. |
| `define` / `use` | Store a drawing as a symbol once; paint it many times, placed like a group, with `colors` swapped. A symbol starts from the plain brush (set its own inside); `{"cmd": "brush", "reset": true}` goes back to plain. |
| `include` | `file` (relative to the script): that file's `define`s and palette. The library in `skills/art-app/library/` has a cloud, water, grass, a reed, a tree, a lily pad and the duck, with parameters; see its README. |
| `repeat` | Copies of its `commands`, with a `step` (`translate`, `rotate`, `scale` around `origin`) added each time. In `text`, `{i}` is the copy number. |
| `delete_layer`, `rename_layer`, `move_layer`, `clear_layer`, `paper` | Housekeeping. |
| `transform_layer` | Move, turn, resize or mirror what's already on a layer (written by the app's Layer menu). |
| `animations` (top level) | Named animations (in the order written): `frames`, `fps`, `loop`, `time` (`"seconds"` to write keys in seconds), and `commands` played after the script's own, once per frame. Render with `--frame`, `--frames`, `--all-frames`. |
| keyframes | Any number of a `group` or `use` as `{"keys": [[0, 0], [4, 20]], "ease": "in-out", "shift": 4}`; a key's time can be `"1.5s"`. A `seed` with keys holds until the next key. |
| `params` / `with` / `"$name"` | A symbol's parameters with defaults; a `use` sets them (keyframes allowed); `"upright": true` keeps a foot flat. |
| `"pixel"` on `canvas` | Pixel art: painted bigger, shrunk to whole pixels in the palette's colours; `{"supersample": 8, "outline": "ink"}`. |
| `pixels` | Rows of characters, one canvas pixel each: `"rows": ["..kk.."]`, `"colors": {"k": "ink"}`. |

Areas: `{"rect": [x, y, w, h]}`, `{"ellipse": [cx, cy, rx, ry]}`, `{"circle": [cx, cy, r]}`,
`{"polygon": [[x, y], ...]}`.

Drawing commands can change the brush just for themselves: `color`, `size`, `opacity`, `hardness`,
`flow`. Add `jitter` (pixels) for a hand-made wobble.

## 4. A workflow that works

1. **Plan before painting.** Choose the canvas size and the composition: where's the horizon,
   where's the main subject (a third of the way across often works better than the middle),
   where does the light come from? Write a **palette of 8–15 colours** with names that say what
   they're for (`sky_top`, `shadow`, `rim`), and the **intent block** (subject, focal point,
   light, mood, key colours): the measure the senses hold the picture to. A small palette looks
   deliberate.
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

**Shading that stays inside a shape:** paint the shape on its own layer, then put the shading
on a clipping layer above it (`"clip": true`, usually `multiply`), and the light on another
(`screen`). Paint across the whole shape without care; only the shape's part shows. To keep a
group's paint inside something instead, give it a `mask`: an area, `{"symbol": "duck", ...}`
(the duck's silhouette, placed like a `use`) or `{"layer": "Water"}` (a reflection only on the
water).

```json
{ "cmd": "layer", "name": "Duck" },
{ "cmd": "use", "name": "duck", "translate": [300, 400] },
{ "cmd": "layer", "name": "Duck shade", "clip": true, "blend": "multiply", "opacity": 0.6 },
{ "cmd": "gradient", "from": [300, 300], "to": [300, 480], "colors": ["transparent", "#3A2A60"] },
{ "cmd": "layer", "name": "Water" },
{ "cmd": "group", "name": "reflection", "mask": { "layer": "Water" }, "opacity": 0.4, "commands": [
  { "cmd": "use", "name": "duck", "translate": [300, 560], "mirror": "y" } ] }
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
light hits. Or `stamp` the `grass` tip along the ground line, which gives real blades:

```json
{ "cmd": "stamp", "tip": "grass", "points": [[0, 900], [800, 880], [1600, 900]], "spacing": 5,
  "size": [30, 60], "angle": [-12, 12], "follow": false, "colors": ["grass1", "grass2", "grass3"] }
```

**One outline around a whole character:** put `"outline": {"color": "ink", "size": 3}` on the
`use` instead of outlining each shape, so a head on a body shows no seam; add `"rim":
{"color": "cream", "size": 2, "angle": -45}` for rim light from the top right.

**Depth and light with filters:** far things get `blur` 2–4 and `adjust` with lower
saturation and a little more brightness (haze); a sun or lamp is a `group` with a `glow`
filter; a character is a `use` with a `shadow` filter (one shadow behind all its layers). To
unify the whole picture at the end: `{"cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.4}`
then a `filter` with a `gradient_map` from the shadow colour through a mid tone to the
highlight colour.

```json
{ "cmd": "filter", "layer": "Far hills", "blur": 3, "adjust": { "saturation": -0.35, "brightness": 0.12 } },
{ "cmd": "use", "name": "duck", "translate": [300, 400], "filter": { "shadow": { "offset": [8, 10], "blur": 8, "color": "#00000080" } } },
{ "cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.4 },
{ "cmd": "filter", "gradient_map": { "stops": [[0, "#1B2433"], [0.5, "#C2651C"], [1, "#FFF3DC"]] } }
```

**Clouds, water, rock:** a noise fill. Sky: a `gradient` with an `area` and a noise `fill` whose
stops are mostly sky with cloud at the top end (`[[0, "sky"], [0.6, "sky"], [1, "cloud"]]`),
big `scale` (150–250), `detail` 4. Water: two blues, `scale` 40–80, `contrast` 1.2. Rock: two
browns, small `scale` (15–25), `detail` 4, in the rock symbol itself so every rock's grain
follows its size and turn. Paper: a `multiply` layer at 0.2–0.35 filled with
`{"texture": "paper"}` on top of everything.

```json
{ "cmd": "gradient", "area": { "rect": [0, 0, 1600, 620] },
  "fill": { "noise": { "scale": 220, "detail": 4, "contrast": 1.4, "stops": [[0, "sky"], [0.55, "sky"], [1, "cloud"]], "seed": 5 } } }
```

**Texture and a hand-made look:** give the brush a tip and dynamics once, then paint as usual.
`chalk` with `size`/`opacity`/`lightness` dynamics for rock and bark; `bristle` with `follow`
for painterly strokes; `leaf` with `follow` and `hue` for foliage along a `path`; `sparkle`
with `scatter` for stars and glints.

```json
{ "cmd": "brush", "tip": "bristle", "size": 40, "color": "#8A6A4A", "flow": 0.7,
  "dynamics": { "follow": true, "size": 0.2, "opacity": 0.3, "lightness": 0.1 } },
{ "cmd": "path", "points": [[200, 600], [500, 560], [800, 600]], "seed": 2 }
```

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

## 5b. Making it beautiful: the five moves

The phase 3 tools are for finish. Use them in this order, each on its own layers, checking
with `--preview` between moves; a picture that doesn't read before move 2 won't be saved by
moves 3 to 5.

1. **Block in with living fills.** Big shapes back to front, and wherever a flat fill would
   look dead, a `noise` fill: clouds in the sky, a two-blue water, grain in rock. Far things
   get a `filter` with a little `blur` and `adjust` with less saturation (haze).
2. **Shade inside shapes.** A clipping layer (`"clip": true`, `multiply` for shadow, `screen`
   for light) above each big shape's layer; paint across it carelessly, only the shape shows.
   A `mask` on a group keeps a reflection on the water or a highlight off a notch.
3. **Texture with brushes.** A `tip` and `dynamics` on the brush, then the same `stroke`,
   `path`, `hatch` and `scatter` as always; `stamp` for grass, leaves, pebbles and lights along
   a line. Mostly near the focal point; far things stay smooth.
4. **Light and depth.** A `glow` filter on the sun or a lamp, a `shadow` filter on each
   character's `use`, one `outline` (with `rim` light) around a whole character.
5. **Unify.** A `"from": "below"` layer with a `gradient_map` from the shadow colour through a
   mid tone to the highlight, at 0.2 to 0.4; then `{"texture": "paper"}` on a `multiply` layer
   at 0.2. Stop before it looks like a filter: the picture should still read as painted.

All five on one small scene (26 commands):

```json
{
  "title": "Making it beautiful: the four moves on one small scene",
  "seed": 3,
  "palette": { "sky": "#8EC1E6", "cloud": "#FFFFFF", "far": "#7A9AB0", "hill": "#4E7A4A", "hill_light": "#8CC25A",
               "water_dark": "#2E5C8A", "water_light": "#6FA6D4", "yellow": "#F2C230", "wing": "#D9A21E", "beak": "#E8892B",
               "ink": "#1A1410", "warm": "#FFF3DC", "mid": "#C2651C", "dark": "#1B2433" },
  "commands": [
    { "cmd": "canvas", "width": 640, "height": 400, "paper": "sky" },
    { "cmd": "define", "name": "duck", "commands": [
      { "cmd": "polygon", "points": [[-24, -8], [-44, -20], [-30, 4]], "fill": "yellow" },
      { "cmd": "ellipse", "center": [0, 0], "radius": [30, 20], "fill": "yellow" },
      { "cmd": "ellipse", "center": [22, -28], "radius": 14, "fill": "yellow" },
      { "cmd": "polygon", "points": [[33, -32], [50, -26], [33, -21]], "fill": "beak" },
      { "cmd": "ellipse", "center": [-6, 0], "radius": [17, 9], "rotation": -12, "fill": "wing" },
      { "cmd": "ellipse", "center": [27, -32], "radius": 2.5, "fill": "ink" } ] },

    { "cmd": "layer", "name": "Sky", "note": "1. Block in, back to front: big shapes, noise where a flat fill would look dead" },
    { "cmd": "gradient", "area": { "rect": [0, 0, 640, 240] },
      "fill": { "noise": { "scale": 180, "detail": 4, "contrast": 1.4, "stops": [[0, "sky"], [0.55, "sky"], [1, "cloud"]], "seed": 5 } } },
    { "cmd": "layer", "name": "Far hills" },
    { "cmd": "ellipse", "center": [180, 300], "radius": [280, 110], "fill": "far" },
    { "cmd": "ellipse", "center": [520, 310], "radius": [240, 100], "fill": "far" },
    { "cmd": "filter", "blur": 4, "adjust": { "saturation": -0.3, "brightness": 0.1 }, "note": "haze: far things are soft and pale" },
    { "cmd": "layer", "name": "Hill" },
    { "cmd": "ellipse", "center": [320, 420], "radius": [420, 190], "fill": "hill" },
    { "cmd": "layer", "name": "Hill light", "clip": true, "blend": "screen", "note": "2. Shading that stays inside: clipping layers" },
    { "cmd": "gradient", "from": [320, 230], "to": [320, 330], "colors": ["hill_light", "transparent"], "opacity": 0.5 },
    { "cmd": "layer", "name": "Water" },
    { "cmd": "rect", "x": 0, "y": 300, "width": 640, "height": 100,
      "fill": { "noise": { "scale": 70, "detail": 3, "contrast": 1.0, "colors": ["water_dark", "water_light"], "seed": 2 } } },

    { "cmd": "layer", "name": "Grass", "note": "3. Texture with brushes: a tip and dynamics, stamps along a line" },
    { "cmd": "stamp", "tip": "grass", "points": [[0, 302], [320, 296], [640, 302]], "spacing": 5, "size": [14, 26], "angle": [-12, 12], "follow": false, "colors": ["hill", "hill_light"] },

    { "cmd": "layer", "name": "Duck", "note": "4. Light and depth with filters and outlines" },
    { "cmd": "use", "name": "duck", "translate": [230, 340], "outline": { "color": "ink", "size": 2.5, "rim": { "color": "warm", "size": 2, "angle": -45 } },
      "filter": { "shadow": { "offset": [0, 10], "blur": 8, "color": "#00000060" } } },
    { "cmd": "layer", "name": "Reflection" },
    { "cmd": "group", "mask": { "layer": "Water" }, "opacity": 0.3, "filter": { "blur": 2 }, "commands": [
      { "cmd": "use", "name": "duck", "translate": [230, 364], "mirror": "y" } ] },
    { "cmd": "layer", "name": "Sun" },
    { "cmd": "group", "name": "sun", "filter": { "glow": { "radius": 40, "threshold": 0.6, "strength": 1.2 } }, "commands": [
      { "cmd": "ellipse", "center": [540, 80], "radius": 30, "fill": "warm" } ] },

    { "cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.22, "note": "5. Unify: one gradient map over everything, faint" },
    { "cmd": "filter", "gradient_map": { "stops": [[0, "dark"], [0.5, "mid"], [1, "warm"]] } },
    { "cmd": "layer", "name": "Paper", "blend": "multiply", "opacity": 0.2 },
    { "cmd": "rect", "x": 0, "y": 0, "width": 640, "height": 400, "fill": { "texture": "paper" } }
  ]
}
```

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

Start with the views sheet: `art-cli views picture.json` paints the picture and saves
`out/picture-views.png`, seven panels in one look. Each one is an artist's trick:

| Panel | What to ask |
|---|---|
| picture | the reference for the rest |
| gray | are the darks and lights where you meant them, without colour to flatter them? |
| squint (the gray picture blurred) | do three to five big value shapes remain? If it goes to one grey mush, fix values, not detail |
| thumb (96 px wide) | can you tell what it is at a glance? |
| flip (mirrored) | composition mistakes the eye has stopped seeing: leaning, lopsided, a heavy side |
| silhouettes (each outermost named group or `use` filled flat, in its own colour) | does every shape read on its own? Blobs with no clear outline won't read in the picture either |
| heat (where the eye goes) | the hottest cell is where contrast, edges, colour and detail pile up; the ring is the intent's `focal` point. If they disagree, the picture argues with the intent |

`--gray`, `--squint`, `--thumb`, `--flip`, `--silhouette` or `--heat` save one view alone
(`out/picture-heat.png`); `--frame 3` picks a frame of an animated script. The heat map is a
fixed model of attention (a 32 x 32 grid; contrast 0.35, edges 0.3, saturation 0.15, detail
0.2), not the truth: use it to notice, then look.

Then the numbers: `art-cli critique picture.json` prints about fifteen findings, each with a
place, the number behind it and a hint:

```
critique: pond.json (painted in 4.0 s)
  canvas 1920 x 1080; intent: a duck on a pond at sunset
  values: 4 value groups when squinted (black 29%, dark 42%, mid 21%, light 6%); dark 54%, mid 42%, light 4%
  focal contrast: 16.0x the rest (good; 1.5x or more stands out, under 1x is lost)
  focal point: the eye goes to duck (x 990, y 861); 16 px from the intent's focal point
  colour: 8 hues carry 90% of the colour; warm 78%, cool 9%, neutral 13%
  out of key: a saturated yellow-green at x 505, y 983 (lilypad, 1.1% of the picture) is far from every key colour; tint it towards the key or mute it
  detail: 22% of the marks are within 4 cells of the focal point (an area of 8%), 78% elsewhere
  readability at 96 px: 8 of 18 parts still read
  readability: duck merges with its surroundings at thumbnail size (value gap 0.01, hue gap 14 degrees); separate it by value
  composition: visual weight sits at 49% across, 50% down
  Numbers can't judge beauty: look at the views, run the blind-read test, and ask for a rating.
```

What it measures: *values* (the squint's value bands with real area; "flat" when 70% of the
pixels sit in the middle three of ten bins; the contrast near the focal point against the
median cell); *the focal point* (the heat peak against the intent); *colour* (hues carrying
90% of the saturation, warm against cool, and cells whose hue is more than 40 degrees from
every key colour and not between two of them); *texture* per named part (grain is what a
1.5 px blur removes, marks what a 6 px blur removes beyond that; a part is *noisy* when grain
without marks covers over half of it, *flat* when a quarter of its cells don't change in
value, a *smudge* when its squint has no light and dark side); *readability* at 96 px (a part
merges when its value differs from the ring around it by under 0.12 and its hue by under 30
degrees); *composition* (the heat's centre of weight, a dead-centre focal point, a focal part
cut by the edge, a part called `horizon` in the middle). `--detail` prints every part's
numbers; `--frames 0-7` gives one line per frame of an animation and names the worst.

The numbers are warnings, never goals. Never trade a better-looking picture for a better
number, and a person's rating outranks every number. They can't tell whether a cloud looks
like a cloud: they catch what a person spots in a second, so your time goes to the rest.

Learning from pictures you may use (`skills/art-app/references/`: public domain or CC0 only,
each with its source and licence in `index.json`): `art-cli palette coffee.png` draws out its
eight main colours as swatches (`coffee-palette.png`) and a palette block to paste into a
script; `art-cli values coffee.png` paints it in five greys (`coffee-values.png`) and counts the
value groups; `art-cli compare mine.png coffee.png` puts both side by side with their greys
(`compare.png`) and scores how alike they are, 0 to 100, in value structure (the five greys'
shares and a 4 x 4 grid of mean values) and colour (the hue histograms), with words: "values
similar: 78; colours different: 41, the first is cooler". All three take a script instead of a
picture (its first frame), so a study can be compared with what it studies.

Then, looking at the render, ask:

1. Can you tell what it is at thumbnail size (the views sheet's thumb)? Is there one clear focal point?
2. Are the darks and lights where you meant them? Is anything muddy (everything mid-grey)?
3. Is the light consistent?
4. Edges: any hard, computer-straight edges that should be organic (`jitter`, `path` instead of
   `polygon`)? Any gaps where shapes should meet (run shapes past each other and past the edges)?
5. Zoom in (`--crop … --zoom 3`) on faces, outlines and joins: holes, halos, wrong layer order?
6. Do the numbers agree? The boxes from `check` say where each layer, group and symbol ended up;
   use `--grid` to read positions off the picture, and `--layers` to look at one layer alone.
   For an animation, read `check`'s motion summary first (biggest steps, jumps, loop pops), then
   `--frame-sheet` to see every frame at once, `--onion` for one frame between its neighbours,
   and `--gif` only when a person wants to watch. When it's right, `art-cli export` writes the
   sprite sheet and its JSON for the game, or the GIF and MP4 for a video.
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
  `size` in pixels (it's the font size; a line is 1.21 × that high). Width before painting:
  `art-cli measure "the words" --size 32`, or `"fit": 180` to shrink it into a space.
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

## 10. Animation and pixel art

**Build the character first, then animate it.** A character is a `define` with `params` for what
moves (`leg`, `bob`, `mouth`), its parts as symbols around `[0, 0]`, feet `"upright": true`. An
animation is then one `use` with keyframes in `with`:

```json
{ "cmd": "use", "name": "duck", "translate": [64, 96],
  "with": { "near": { "keys": [[0, -22], [4, 22], [8, -22]], "ease": "in-out" },
            "far":  { "keys": [[0, -22], [4, 22], [8, -22]], "ease": "in-out", "shift": 4 },
            "bob":  { "keys": [[0, 0], [2, -3], [4, 0], [6, -3], [8, 0]] } } }
```

- A loop's last key sits at `frames` (frame 8 of an 8-frame loop equals frame 0): then the loop
  can't pop. `check` warns if it does, and about jumps between frames.
- `shift` gives the other leg, arm or wing: the same keys, half a cycle later.
- Swap parts with a parameter: `"with": {"eye": {"keys": [[0, "eye_open"], [9, "eye_shut"], [11, "eye_open"]]}}`
  and `{"cmd": "use", "name": "$eye"}` inside the symbol. Names hold until the next key.
- A squash and stretch is `scale` keyframes on a group with `origin` at the feet: `[1.2, 0.8]` on
  landing, `[0.9, 1.1]` in the air, `"ease": "out"` into the ground.
- A camera is the scene in a group with keyframed `translate` and `scale` around `origin`.
- The loop: `check` (motion summary, warnings) → `render --frame-sheet` → fix → `export`.

**Pixel art:** `"pixel": {"supersample": 8, "outline": "ink"}` on the canvas, a palette of 4 to 8
colours (it's locked), shapes with decimals (`"radius": 5.5` is fine), and `pixels` rows for
tiny things like an eye: `"rows": ["kw"]`. Keep every part at least 2 pixels wide, or the vote
loses it. Check at `--zoom 8`; export with `--scale 8` for people and `--scale 1` for the game.
The 32-pixel duck in `examples/duck-pixel.json` is the same kind of symbols as the painted one.

## 11. Speed

- A 1600×1000 render takes well under a second; `--preview` about a third of that. `check`
  paints at full size too (so its boxes are exact), in about the time of a render.
- Snapshots and the contact sheet cost a little per stage; use a few meaningful snapshots rather
  than one per command.
- `scatter` and `hatch` with thousands of strokes are fine; hundreds of thousands are slow.
