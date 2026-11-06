# Duck Quill painting scripts

A painting script describes a picture as a list of steps, in a JSON text file. Playing the script
paints the picture. Scripts are made to be written by hand (or by an AI assistant), rendered,
checked, and improved.

- Playing a script **always gives exactly the same picture**. Even the "random" commands (jitter,
  scatter, hatching) use a seed, so they repeat exactly.
- Mistakes are explained with the **command's number**, e.g.
  `command 12 (stroke): unknown field "sise", expected one of "points", "color", "size", ...`
- **Big shapes are one command**: rectangles, ellipses, polygons, smooth paths, tapered lines,
  hatching and scattering.

Render scripts with `quill` (see the README), or watch them being painted in the app with
**File → Play strokes…**.

## A first script

```json
{
  "format": "duckquill-script",
  "version": 1,
  "title": "Sun over the sea",
  "seed": 1,
  "palette": {
    "sky": "#F6A04D",
    "sea": "#1D4E6E",
    "sun": "#FFE9A8"
  },
  "commands": [
    { "cmd": "canvas", "width": 1600, "height": 1000, "paper": "#FFFFFF" },
    { "cmd": "layer", "name": "Sky" },
    { "cmd": "gradient", "from": [0, 0], "to": [0, 600], "colors": ["#7A4E9C", "sky"] },
    { "cmd": "layer", "name": "Sun" },
    { "cmd": "ellipse", "center": [800, 560], "radius": 120, "fill": "sun" },
    { "cmd": "layer", "name": "Sea" },
    { "cmd": "rect", "x": 0, "y": 600, "width": 1600, "height": 400, "fill": "sea" },
    { "cmd": "snapshot", "name": "blocked in" }
  ]
}
```

## The file

| Field | Needed? | What it is |
|---|---|---|
| `commands` | yes | The steps, in order. The first one must be `canvas`. |
| `format` | no | `"duckquill-script"`. Recommended, so the file is recognisable. |
| `version` | no | `1`. A newer version is refused with a clear message. |
| `title` | no | A title for people. |
| `notes` | no | Anything for people (text, a list, …). Ignored. |
| `seed` | no | Where randomness starts. Default `1`. Change it to get a different random pattern. |
| `palette` | no | Colour names, defined once: `{"sky": "#88BBFF", "shadow": "#1A1A3080"}`. |
| `intent` | no | What the painter means (next section). Paints nothing. |

Every command is an object with a `"cmd"` field. Any command may also have a `"note"` field: a
comment for people, ignored when playing. Misspelt fields are errors (not silently ignored).

## Intent: what the painter means

```json
"intent": {
  "subject": "a duck on a pond at sunset",
  "focal": "duck",
  "light": "top right",
  "mood": "calm, warm, end of the day",
  "key_colors": ["sky_low", "sun", "water_dark"]
}
```

An optional block at the top of the script. It paints nothing: `check` prints it back, `views`
marks the focal point, `critique` compares the picture with it, and the blind-read test is
judged against it. Write it first; it's the measure of whether the picture reads.

| Field | |
|---|---|
| `subject` | A short sentence: what the picture is of. |
| `focal` | Where the eye should go: `[x, y]`, or the name of a named group or `use` (the middle of its box, worked out when the script is played). |
| `light` | Where the light comes from: `"top right"`, `"left"`, `"above"`... (eight directions), degrees clockwise (0 = from the right), `[x, y]` (a sun, a lamp), `"behind"` or `"front"`. |
| `mood` | A few words. |
| `key_colors` | 2 to 6 palette names that set the picture's key; colours far from them count as out of key. |

A `focal` name that no group or `use` has is a warning from `check`; a key colour that isn't in
the palette is an error.

## The basics

**Positions** are canvas pixels: `x` from the left, `y` from the **top** (y grows downwards).
A point is `[x, y]`. In strokes and paths a point can also be `[x, y, pressure]`, with pressure
from 0 to 1 (default 1): lighter pressure makes the brush smaller and fainter, like a pen.

**Colours** are written as:
- `"#RRGGBB"` (like `"#FF8800"`), `"#RGB"` (`"#F80"`), or with opacity last: `"#RRGGBBAA"`
  (`"#FF880080"` is half see-through), `"#RGBA"`;
- a name from the `palette`;
- `"transparent"`.

**Angles** are in degrees, clockwise: 0 points right, 90 points down.

**Values from 0 to 1**: opacity, flow, hardness, tolerance, pressure, gradient stop positions.

**Layers.** Until the first `layer` command, painting goes onto a layer called "Layer 1". The
first `layer` command renames that layer if nothing has been painted on it yet, so no empty layer
is left behind. Layers are found by name; if two have the same name, the topmost one is used.

**The brush** starts as: colour black, size 24, opacity 1, flow 1, hardness 0.8, spacing 0.1,
pressure changes size and opacity. Change it with `brush`. Drawing commands can also change the
brush for themselves only (`color`, `size`, `opacity`, `hardness`, `flow`).

## Commands

### `canvas` — the first command

```json
{ "cmd": "canvas", "width": 1600, "height": 1000, "paper": "#FDF8F0" }
{ "cmd": "canvas", "open": "my-drawing.quill" }
```

| Field | |
|---|---|
| `width`, `height` | Size in pixels (1 to 50000). |
| `paper` | Paper colour (opaque), default white. |
| `open` | Start from a saved picture instead (path relative to the script's folder). Not with `width`/`height`. |

In the app, **Play strokes** paints onto the picture that's open; `open` is ignored there.

### `paper`

```json
{ "cmd": "paper", "color": "#F4EEDC" }
```

### `layer` — select or create a layer

```json
{ "cmd": "layer", "name": "Shadows", "blend": "multiply", "opacity": 0.6 }
{ "cmd": "layer", "name": "Glow", "above": "Sky", "blend": "screen" }
```

Selects the layer called `name`; if there isn't one, creates it **on top** (or `above` / `below`
another layer). Then applies any settings given:

| Field | |
|---|---|
| `opacity` | 0 to 1. |
| `blend` | Blend mode: `normal`, `multiply`, `screen`, `overlay`, `darken`, `lighten`, `color-dodge`, `color-burn`, `hard-light`, `soft-light`, `difference`, `exclusion`, `add`, `hue`, `saturation`, `color`, `luminosity`. |
| `visible` | `true` / `false`. |
| `clip` | `true`: a clipping layer, shown only where the nearest unclipped layer below it has paint. See [Clipping and masks](#clipping-and-masks). |
| `lock` | `true`: the alpha lock. Painting only recolours what's already on the layer; the eraser does nothing. |
| `from` | `"below"`: fill the layer with everything below it, flattened (hidden layers left out, no paper), for a [filter](#filters) over the whole picture. |
| `above` / `below` | Put the layer just above / below another layer (also moves an existing layer). |

### `delete_layer`, `rename_layer`, `move_layer`, `clear_layer`

```json
{ "cmd": "delete_layer", "name": "Sketch" }
{ "cmd": "rename_layer", "name": "Layer 1", "to": "Ink" }
{ "cmd": "move_layer", "name": "Ink", "above": "Colours" }
{ "cmd": "move_layer", "name": "Ink", "index": 0 }
{ "cmd": "clear_layer" }
{ "cmd": "clear_layer", "name": "Sketch" }
```

`move_layer` takes one of `above`, `below` or `index` (0 = bottom). `clear_layer` empties the
named layer, or the selected one.

### `transform_layer` — move, turn, resize or mirror what's on a layer

```json
{ "cmd": "transform_layer", "name": "Duck", "mirror": "x", "origin": [800, 500] }
{ "cmd": "transform_layer", "name": "Duck", "translate": [40, -10], "scale": 1.5, "rotate": 15, "origin": [620, 410] }
```

Moves everything already painted on a layer (`name`, or the selected one), with the same fields as
a [group](#groups-move-turn-resize-and-mirror): `translate`, `scale`, `rotate` and `mirror`, around
`origin` (default `[0, 0]`), in canvas pixels. Mirroring around the canvas's middle and moving by
whole pixels copy pixels exactly; turning and resizing blend neighbouring pixels. What ends up off
the canvas is cut off. A group around it doesn't change it: it always works on the whole layer.
The app's **Layer → Flip** and **Layer → Transform…** are written down as this command.

### `brush`

```json
{ "cmd": "brush", "color": "ink", "size": 8, "hardness": 0.9, "opacity": 1 }
```

| Field | |
|---|---|
| `color` | Brush colour. A colour's opacity (`#RRGGBBAA`) multiplies the brush opacity. Or a pattern, `{ "noise": {...} }` / `{ "texture": ... }`: strokes get the pattern's colour pixel by pixel (see [Noise and texture](#noise-and-texture)). |
| `size` | Diameter in pixels at full pressure (above 0, up to 1000). |
| `opacity` | The most opaque one stroke gets. Going over the same spot within a stroke never gets darker than this. |
| `flow` | How much paint each dab adds. Below 1, paint builds up where dabs overlap, like an airbrush. |
| `hardness` | 1 = crisp edge, 0 = very soft edge. |
| `spacing` | Distance between dabs as a fraction of the size (0.01 to 5). Default 0.1. |
| `pressure_size`, `pressure_opacity`, `pressure_flow` | Which settings point pressure changes (`true`/`false`). |
| `reset` | `true`: back to the plain brush first (round, 24 wide, dark grey, no dynamics), then any settings given. |
| `tip` | The dab's shape: `"round"` (default), `"square"`, `"bristle"`, `"chalk"`, `"leaf"`, `"grass"`, `"sparkle"`, or `{ "file": "tips/splat.png" }`. See [Brushes with character](#brushes-with-character). |
| `angle` | How far the tip is turned, degrees clockwise. |
| `dynamics` | How dabs vary along a stroke (see below). Replaces the dynamics as a whole: `{}` turns them all off. |
| `pressure_size_min`, `pressure_opacity_min` | What the lightest touch still gives, 0 to 1, of the size and the opacity (default 0). A pencil keeps its width (0.85) but not its darkness (0.15). |
| `pressure_curve` | How pressure is felt (0.1 to 10, default 1). Above 1, you must press harder for the full effect. |
| `grain` | Paper grain, 0 to 1 (default 0). A light touch marks only the paper's bumps; pressing harder fills the dips. The grain is fixed to the canvas, so every stroke meets the same bumps. |
| `grain_scale` | How coarse the grain is (0.25 to 16, default 1, which is fine paper). |
| `build_up` | The most the layer darkens however often you go over it, 0 to 1 (default 0: no limit), as graphite turns shiny grey and never black. |
| `preset` | A built-in preset by name: all its settings except the colour, then any other fields in this command on top. List the presets with `quill presets`. |

Only the settings given change.

```json
{ "cmd": "brush", "preset": "Pencil (graphite)", "color": "#2A2A30" }
{ "cmd": "brush", "preset": "Ink pen", "size": 3 }
```

The built-in presets are **Pencil (graphite)**, **Ink pen**, **Marker**, **Soft airbrush**,
**Chalk** and **Soft eraser**. They're part of the program, so a script that names one paints
the same on every machine. Presets you make in the app aren't available to scripts: give the
settings themselves instead. For the eraser preset, paint with `erase`.

### `stroke` and `erase` — like a pen

```json
{ "cmd": "stroke", "points": [[100, 400], [300, 380, 0.6], [500, 420, 0.3]] }
{ "cmd": "stroke", "points": [[100, 100], [200, 150]], "color": "#C03030", "size": 4 }
{ "cmd": "erase", "points": [[400, 200], [400, 400]], "size": 30, "hardness": 0.5 }
```

Paints (or erases) through the points with the brush, exactly as if a pen moved through them: a
smooth curve through every point, with no gaps. Optional: `color`, `size`, `opacity`,
`hardness`, `flow` (this stroke only), `jitter` (a gentle hand-drawn wobble, in pixels), `seed`,
and `symmetry`: mirror lines the stroke is painted again across, as one stroke (where the copies
overlap they don't darken each other). `"x"` is a line going down at that x (left and right mirror
each other), `"y"` a line going across; both give four copies. Inside a group the lines move and
turn with it. The app's symmetry strokes are saved this way by Export strokes.

```json
{ "cmd": "stroke", "points": [[300, 200], [380, 260, 0.5]], "symmetry": { "x": 512 } }
{ "cmd": "erase", "points": [[100, 100], [140, 120]], "symmetry": { "x": 512, "y": 384 } }
```

### `line` — a tapered straight line

```json
{ "cmd": "line", "from": [100, 800], "to": [900, 760], "size": 6, "taper": 0.3 }
```

`taper`: how much of each end gets thinner (0 to 1; default 0.25). One number for both ends, or
`[start, end]` (e.g. `[0, 0.6]` for a line that only thins at the end). `0` for an even line. Also:
`color`, `size`, `opacity`, `hardness`, `flow`, `jitter`, `seed`.

### `path` — a smooth curve through points

```json
{ "cmd": "path", "points": [[100, 500], [300, 380], [500, 520], [700, 400]], "size": 5, "taper": [0.1, 0.4] }
{ "cmd": "path", "points": [[300, 300], [420, 260], [480, 380], [340, 420]], "closed": true, "fill": "leaf", "color": "ink", "size": 3 }
```

A smooth curve through all the points (it never overshoots or loops), drawn with the brush.

| Field | |
|---|---|
| `closed` | Join the end back to the start. |
| `fill` | Fill the inside (closed paths only): a colour or a [gradient](#gradients-as-fills). Painted before the line. |
| `line` | `false` to fill only, without drawing the curve. |
| `taper` | As for `line` (default 0). |
| also | `color`, `size`, `opacity`, `hardness`, `flow`, `jitter`, `seed`. Points can have pressure. |

### `rect`, `ellipse`, `polygon` — shapes

```json
{ "cmd": "rect", "x": 100, "y": 100, "width": 300, "height": 200, "radius": 20, "fill": "#E8D8B0", "outline": "ink", "outline_size": 4 }
{ "cmd": "ellipse", "center": [800, 400], "radius": [200, 120], "rotation": -15, "fill": "sun" }
{ "cmd": "polygon", "points": [[100, 900], [400, 500], [700, 900]], "fill": "mountain", "jitter": 4 }
```

| Field | |
|---|---|
| `x`, `y`, `width`, `height` | (`rect`) The top-left corner and size, before rotating. |
| `radius` | (`rect`) Rounds the corners. (`ellipse`) One number for a circle, or `[rx, ry]`. |
| `center` | (`ellipse`) The middle. |
| `points` | (`polygon`) At least 3 corners. Crossing edges are fine: everything enclosed is filled. |
| `rotation` | (`rect`, `ellipse`) Degrees, clockwise, around the middle. |
| `fill` | A colour, a [gradient](#gradients-as-fills), [noise or a texture](#noise-and-texture). Edges are smooth (anti-aliased). |
| `outline` | A colour: draws the edge with the brush (`outline_size` sets its size). |
| `opacity` | 0 to 1, for both. |
| `erase` | `true`: cut the shape out of the layer instead of painting it. |
| `jitter`, `seed` | Wobble the edge by up to `jitter` pixels, for a hand-made look. |

Give at least one of `fill` and `outline`.

### `fill` — the paint bucket

```json
{ "cmd": "fill", "at": [520, 330], "color": "#4A7A3A", "tolerance": 0.15, "grow": 2 }
```

Fills the area around `at` whose colour is close to the colour there, like the paint bucket.

| Field | |
|---|---|
| `color` | Default: the brush colour. |
| `tolerance` | How different a colour may be and still be filled (0 to 1, default 0.1). |
| `sample` | `"visible"` (default): stops at lines on any visible layer. `"layer"`: looks at this layer only. |
| `grow` | Fill this many pixels further, under the soft edges of lines (default 1, at most 50). |
| `opacity` | 0 to 1. |

### `gradient`

```json
{ "cmd": "gradient", "from": [0, 0], "to": [0, 700], "colors": ["#203060", "#E08040", "#F8D090"] }
{ "cmd": "gradient", "kind": "radial", "from": [800, 500], "to": [800, 200], "stops": [[0, "#FFFFFFC0"], [1, "transparent"]] }
{ "cmd": "gradient", "from": [0, 600], "to": [0, 1000], "colors": ["#00000000", "#00000060"], "area": { "rect": [0, 600, 1600, 400] } }
```

| Field | |
|---|---|
| `kind` | `"linear"` (default: changes along the line from `from` to `to`) or `"radial"` (rings around `from`, reaching the last colour at the distance of `to`). |
| `colors` | Colours spread evenly from start to end. |
| `stops` | Or colours at chosen places: `[[position, colour], ...]`, positions 0 to 1. |
| `area` | Only inside this [area](#areas). Without it, the whole layer is covered. |
| `fill` | Instead of `from`/`to`/`colors`: any fill (a colour, a gradient object, [noise or a texture](#noise-and-texture)) over the area or the whole layer. |
| `opacity` | 0 to 1. |
| `erase` | `true`: fade the layer out instead (the gradient's opacity says how much). |

Fading to `"transparent"` keeps the colour (no grey fringe). Big gradients are dithered very
slightly so they show no bands.

#### Gradients as fills

A shape's (or closed path's) `fill` can be a gradient instead of a colour:

```json
"fill": { "gradient": "radial", "from": [800, 400], "to": [800, 250], "colors": ["#FFF6D0", "#F0A040"] }
```

It takes `gradient` (`"linear"` or `"radial"`), `from`, `to`, and `colors` or `stops`, in canvas
pixels. A fill can also be [noise or a texture](#noise-and-texture).

### `hatch` — strokes in one direction

```json
{ "cmd": "hatch", "area": { "ellipse": [600, 500, 150, 90] }, "angle": 30, "spacing": 7, "size": 2, "jitter": 2, "color": "shadow" }
```

Fills an [area](#areas) with parallel brush strokes, for a drawn or painted look.

| Field | |
|---|---|
| `angle` | Direction of the strokes, degrees (default 45). |
| `spacing` | Pixels between strokes (default 1.5 × the brush size). |
| `cross` | `true`: a second set across the first (cross-hatching). |
| `taper` | As for `line` (default 0.15). |
| `jitter` | Moves each stroke's ends and wobbles it by up to this many pixels (default 0). |
| also | `color`, `size`, `opacity`, `hardness`, `flow`, `seed`. |

### `scatter` — many dabs or strokes

```json
{ "cmd": "scatter", "area": { "circle": [400, 300, 120] }, "count": 300, "size": [3, 9], "colors": ["leaf1", "leaf2", "leaf3"], "opacity": [0.6, 1], "hardness": 0.3 }
{ "cmd": "scatter", "area": { "rect": [0, 800, 1600, 200] }, "count": 400, "kind": "stroke", "length": [10, 30], "angle": [-100, -80], "size": [1, 3], "color": "grass" }
```

Puts `count` round dabs (or short tapered strokes) at random places inside an [area](#areas).
Sizes, opacities, lengths and angles can be one number or `[min, max]` (picked at random for each).

| Field | |
|---|---|
| `count` | How many (at most 200000). |
| `kind` | `"dab"` (default) or `"stroke"`. |
| `size` | Diameter(s), default the brush size. |
| `color` / `colors` | One colour, or a list to pick from at random. Default: the brush colour. |
| `opacity` | Default: the brush opacity. |
| `hardness` | Default: the brush hardness. |
| `length` | (strokes) Default 3 × the size. |
| `angle` | (strokes) Default `[0, 360]`. |
| `seed` | See [randomness](#randomness). |

### `text` — letters

```json
{ "cmd": "text", "text": "Mario's Duck!", "at": [400, 40], "size": 64, "color": "sun", "align": "center", "outline": "ink", "outline_size": 3 }
{ "cmd": "text", "text": "First line\nsecond line", "at": [20, 500], "size": 24, "color": "ink", "rotate": -5 }
```

Writes `text` in Duck Quill's own font, [Inter](https://rsms.me/inter/), which is built into the
program, so it looks the same on every computer. `"\n"` starts a new line. The letters are
filled shapes with smooth edges, so inside a [group](#groups-move-turn-resize-and-mirror) or a
[symbol](#symbols-define-once-use-many-times) they move, turn, resize and mirror like everything else.

| Field | |
|---|---|
| `text` | The words. Every printable ASCII character works, and so do most accented letters and symbols (a character the font doesn't have shows as a box). |
| `at` | `[x, y]`: the **top** of the first line, at its left end (`align` `"left"`, the default), its middle (`"center"`) or its right end (`"right"`). |
| `size` | Font size in pixels (default 32). |
| `color` | Default: the brush colour. |
| `align` | `"left"`, `"center"` or `"right"`: each line is placed against `at` this way. |
| `outline` | A colour for a border around the letters. It's drawn behind them, so it doesn't make them thinner. |
| `outline_size` | How wide that border is, in pixels (default 2). |
| `rotate` | Degrees clockwise, around `at`. |
| `opacity` | Default 1. |
| `fit` | A width in pixels: the text is shrunk (never enlarged) so its widest line fits. `quill measure "the text" --size 32 --fit 180` says the size beforehand. |

Inside a [`repeat`](#repeat--copies-with-a-step), `{i}` in the text becomes the copy number (1, 2,
3...) and `{i0}` the same counted from 0.

Sizes, for placing text without guessing: each line is **1.21 × `size`** high. Capital letters
start **0.24 × `size`** below the top of their line and are **0.73 × `size`** tall, so they sit on
a baseline 0.97 × `size` below it. For `"size": 100` at `[x, 0]`: capitals from y = 24 to 97, the
next line's top at 121. Letters with a tail (g, p, y) go down to about 1.2 × `size`.

### `snapshot`

```json
{ "cmd": "snapshot", "name": "blocked in" }
```

A named checkpoint. `quill render --snapshots` saves a picture here, so you can check the
picture at each stage. It doesn't change the picture.

## Groups: move, turn, resize and mirror

```json
{ "cmd": "group", "name": "left wing", "translate": [300, 40], "rotate": -20, "origin": [60, 60],
  "commands": [
    { "cmd": "ellipse", "center": [60, 60], "radius": [50, 22], "fill": "#F2C230", "outline": "#3A2A10", "outline_size": 3 },
    { "cmd": "line", "from": [20, 60], "to": [100, 60], "size": 3, "color": "#3A2A10" }
  ] }
```

A `group` draws its `commands` through a transform. Draw a part once, in its own coordinates
(often around `[0, 0]`), then place it where you want it, as big as you want it, turned or
mirrored. Everything inside follows: points, shapes, paths, gradients, a fill's `at`, and the areas
of `hatch` and `scatter`.

| Field | |
|---|---|
| `commands` | The commands to draw inside the group (needed). Groups can hold groups. |
| `name` | A name for messages, like `command 12 (group "left wing") > 3 (ellipse): ...`. |
| `translate` | Move by `[x, y]`. |
| `scale` | Resize: one number (`1.5`), or `[across, down]` like `[2, 1]`. Above 0. |
| `rotate` | Turn by this many degrees, clockwise. |
| `mirror` | `"x"` swaps left and right, `"y"` swaps up and down, `"xy"` both. |
| `origin` | The point the group mirrors, resizes and turns around, in the group's own coordinates. Default `[0, 0]`. |
| `opacity` | 0 to 1: everything inside is drawn fainter (overlapping parts inside show through each other). |
| `scale_brush` | `true` (default): brush and outline sizes resize with the group. `false`: they stay as written. |
| `mask` | Paint only inside this: an area, a symbol's silhouette, a layer's paint, or `not` one of those. See [Clipping and masks](#clipping-and-masks). |
| `filter` | Filters on what the group draws, on its own, before it lands on the layers: `{ "glow": {...} }`, `{ "shadow": {...} }`... See [Filters](#filters). |
| `outline` | A line around the silhouette of everything the group draws, behind it, with optional rim light: `{ "color": "ink", "size": 3, "rim": { "color": "cream", "size": 2, "angle": -45 } }`. See [Outlines for whole shapes](#outlines-for-whole-shapes). |

The order is always the same: around `origin`, first **mirror**, then **scale**, then **rotate**;
then **translate**. So `"mirror": "x", "origin": [64, 0]` flips a drawing left to right around
x = 64, and `"translate": [200, 0]` then moves the flipped drawing 200 pixels right.

**After a group**, the brush and the selected layer are back to what they were before it: a group
can set its own brush and draw on other layers without changing what comes after it.

Random patterns inside a group (jitter, scatter, hatch) depend on where the command is inside the
group, so a group's pattern doesn't change when you add commands before the group.

Round gradients stay round when a group is stretched more one way than the other.

## Symbols: define once, use many times

```json
{ "cmd": "define", "name": "duck", "commands": [
  { "cmd": "ellipse", "center": [0, 0], "radius": [34, 24], "fill": "yellow", "outline": "ink", "outline_size": 3 },
  { "cmd": "ellipse", "center": [26, -26], "radius": 16, "fill": "yellow", "outline": "ink", "outline_size": 3 },
  { "cmd": "polygon", "points": [[38, -28], [56, -24], [38, -20]], "fill": "beak" }
] }
{ "cmd": "use", "name": "duck", "translate": [110, 110] }
{ "cmd": "use", "name": "duck", "translate": [250, 110], "mirror": "x", "colors": { "yellow": "#C04040" } }
```

`define` stores a drawing as a **symbol** without painting anything. `use` paints it, placed with the
same fields as a `group` (`translate`, `scale`, `rotate`, `mirror`, `origin`, `opacity`,
`scale_brush`). Draw symbols around `[0, 0]`, so `translate` says where they go.

| | |
|---|---|
| `define` | `name` and `commands`. Only at the top level of the script (not inside a group, symbol or repeat), each name once, and before it's used. |
| `use` | `name`, the group fields above (`mask`, `filter` and `outline` too), and `colors`. |
| `colors` | Colours to swap for this use: `{ "yellow": "#C04040" }` paints red wherever the symbol paints the palette colour `yellow`. The left side can also be a hex code (`"#F2C230"`). For enemy variants, team colours and so on. |

- **Symbols can use other symbols.** A symbol using itself, even through others, is an error:
  `symbol "a" uses itself (a > b > a)`. Colour swaps reach into the symbols inside, and the inner
  `use`'s own swaps come first.
- **Layers:** a symbol can switch layers with `layer` commands. Layer names are the picture's
  layers, the same as anywhere else: two uses of a character symbol paint onto the same `Body` and
  `Shadow` layers (created if missing). A symbol without `layer` commands paints on the layer
  selected when it's used. After a `use`, the brush and the selected layer are back to what they
  were before it.
- **A symbol starts from the plain brush**, whatever the brush was before the `use` (so it looks
  the same wherever it's used); set its own `brush` inside if it needs one. After the `use` the
  brush from before is back. Brush changes inside a `group` stay inside it too.
- **Random patterns** (jitter, scatter, hatch) in a symbol are the same every time it's used.
- Mistakes inside a symbol name the `use` and the symbol's command:
  `command 9 (use "duck") > 2 (ellipse): unknown colour "yelow"`.

## `include` — symbols from another file

```json
{ "cmd": "include", "file": "../skills/duck-quill/library/cloud.json" }
{ "cmd": "use", "name": "cloud", "translate": [200, 80], "with": { "puff": 1.3 } }
```

`include` (top level only) reads another script's `define`s and palette entries, not its
commands or animations. The path is relative to the script; an included file may include others,
relative to itself. A file that includes itself, directly or through others, is a mistake that
shows the chain; a symbol or colour defined twice says both files; mistakes inside an included
symbol name it (`command 9 (use "cloud" (from cloud.json, command 2)) > 3 (ellipse): ...`).
Included symbols and colours aren't "never used" warnings: a library holds more than one picture
needs.

**The library** in `skills/duck-quill/library/` holds ready parts, each with parameters, an example
script and a preview: cloud, water, grass, reed, tree, lily pad and the duck. Its `README.md`
lists them.

## `repeat` — copies with a step

```json
{ "cmd": "repeat", "count": 9, "step": { "translate": [52, 0] }, "commands": [
  { "cmd": "rect", "x": 14, "y": 150, "width": 12, "height": 70, "fill": "wood" }
] }
{ "cmd": "repeat", "count": 8, "step": { "rotate": 45, "origin": [410, 70] }, "commands": [
  { "cmd": "ellipse", "center": [410, 45], "radius": [8, 22], "fill": "petal" }
] }
```

Draws its `commands` `count` times (1 to 10000). The first copy is drawn as written; each next copy
has the `step` applied once more: `translate`, `rotate` (degrees, around `origin`) and `scale`
(around `origin`). A row of fence posts, an arc of coins (translate and rotate together), petals
around a centre (rotate around it). Each copy is like a group: afterwards the brush and layer are
as before, and random patterns differ from copy to copy.

**Copy numbers in text:** inside a `repeat`, a `text` can say which copy it's in: `{i}` becomes
1, 2, 3... and `{i0}` becomes 0, 1, 2.... So four frame labels are one command:

```json
{ "cmd": "repeat", "count": 4, "step": { "translate": [128, 0] }, "commands": [
  { "cmd": "text", "text": "frame {i}", "at": [64, 108], "size": 14, "color": "ink", "align": "center" }
] }
```

The innermost `repeat` counts, also from inside a group or a symbol used in the repeat. Outside
any `repeat`, `{i}` is a mistake.

**Values that differ per copy:** `each` lists them, and each copy gets one as a `"$name"`
parameter, round and round if there are more copies than values:

```json
{ "cmd": "repeat", "count": 6, "step": { "translate": [30, 0] }, "each": { "paint": ["yellow", "wing", "beak"] }, "commands": [
  { "cmd": "rect", "x": 100, "y": 350, "width": 24, "height": 24, "fill": "$paint", "outline": "ink" }
] }
```

## Brushes with character

```json
{ "cmd": "brush", "tip": "chalk", "size": 18, "color": "rock",
  "dynamics": { "follow": true, "size": 0.3, "angle": 20, "opacity": 0.4, "scatter": 3, "hue": 4, "lightness": 0.08, "saturation": 0.1 } }
{ "cmd": "brush", "tip": { "file": "tips/splat.png" } }
```

A brush's **tip** is the shape of each dab. The built-in tips are made by the engine (the same
on every computer): `round` is the plain brush; `square` is hard, for pixels and blocks;
`bristle` is a row of bristles across the stroke, streaky like a flat brush; `chalk` is grainy;
`leaf` points along the stroke; `grass` stands up; `sparkle` is a four-pointed star with a glow.
`{ "file": "..." }` is a greyscale PNG, relative to the script: white paints, black doesn't,
see-through is nothing. A dab stamps the tip scaled to `size`, turned by `angle`, and
`hardness` softens its edge as before. Tips work on a pixel canvas too (they come out crisp).

**Dynamics** make each dab a little different, from the stroke's `seed` (so the same script
always paints the same picture). `follow` turns the tip along the stroke (a leaf tip paints a
vine); the rest are the most each dab may vary, either way:

| | |
|---|---|
| `size` | A fraction: 0.3 is from 70% to 130% of the size. |
| `angle` | Degrees (0 to 180). |
| `opacity` | A fraction. |
| `scatter` | Pixels off the line (scaled with the brush in a group). |
| `hue` | Degrees around the colour wheel (0 to 180). |
| `lightness`, `saturation` | Fractions. |

The brush carries its tip and dynamics, so `stroke`, `line`, `path`, `hatch` and `scatter`
all use them (`scatter`'s own `size`, `colors` and `angle` still win; its dabs with a shaped
tip are turned within `angle`).

### `stamp` — copies along a path

```json
{ "cmd": "stamp", "symbol": "blade", "points": [[20, 300], [300, 290], [620, 310]], "spacing": 9,
  "size": [0.8, 1.3], "angle": [-10, 10], "colors": ["grass1", "grass2"], "seed": 4 }
{ "cmd": "stamp", "tip": "leaf", "points": [[100, 100], [160, 40], [240, 60]], "spacing": 14, "size": [10, 16], "color": "leaf" }
```

Puts copies of a `symbol`, or single dabs of a `tip` (default: the brush's own), along a smooth
path through `points`, one every `spacing` units, each turned to follow the path. A row of
grass, a vine, a string of lights, pebbles along a shore.

| Field | |
|---|---|
| `symbol` / `tip` | What to put down: one or the other. A symbol copy is a `use` (drawn around `[0, 0]`); a tip copy is one dab with the brush's settings. |
| `points`, `spacing` | The path and the distance between copies. Both may be keyframes. |
| `size` | A number or `[min, max]` to pick from. For a symbol a scale (default 1); for a tip pixels across (default: the brush size). |
| `angle` | Degrees added to each copy's turn: a number or `[min, max]`. |
| `follow` | `true` (default): turned along the path. `false`: only `angle`. |
| `scatter` | Copies land up to this many units off the path. |
| `color` / `colors` | One colour, or colours picked from per copy. For a symbol it goes to the symbol's `color` parameter (`"params": { "color": "..." }` in its define). |
| `opacity` | For tip dabs. |
| `with` | Values for the symbol's other parameters (the same for every copy). |
| `seed` | Which random picks. |

## Noise and texture

```json
{ "cmd": "rect", "x": 0, "y": 200, "width": 640, "height": 160,
  "fill": { "noise": { "scale": 60, "detail": 3, "contrast": 1.3, "colors": ["water_dark", "water_light"], "offset": [0, 0], "seed": 2 } } }
{ "cmd": "gradient", "area": { "rect": [0, 0, 640, 200] },
  "fill": { "noise": { "scale": 180, "detail": 4, "stops": [[0, "sky"], [0.6, "sky"], [1, "cloud"]] } } }
{ "cmd": "layer", "name": "Grain", "blend": "multiply", "opacity": 0.25 }
{ "cmd": "rect", "x": 0, "y": 0, "width": 640, "height": 360, "fill": { "texture": "paper" } }
{ "cmd": "rect", "x": 0, "y": 0, "width": 640, "height": 360, "fill": { "texture": { "file": "textures/canvas.png", "scale": 0.5 } } }
{ "cmd": "brush", "size": 30, "color": { "noise": { "scale": 40, "colors": ["grass1", "grass2"] } } }
```

A **noise** fill is smooth, natural-looking randomness (clouds, water, rock, grass, skin): its
value at each point, 0 to 1, picks a colour from `colors` (spread evenly) or `stops`.

| | |
|---|---|
| `scale` | Pixels per wave of the coarsest layer (default 64). |
| `detail` | Layers of finer noise, 1 to 6 (default 3): more is rougher. |
| `contrast` | 1 = as is (default); more pushes values to the two ends, less towards the middle. |
| `offset` | `[x, y]`: moves the pattern. |
| `seed` | Which pattern (mixed with the script's `seed`). |

A **texture** is a picture tiled over the area: `"paper"` (built-in, light grain for a paper
look on a `multiply` or `overlay` layer at low opacity), or a PNG `file` relative to the script
with `scale` (2 = twice as big), `offset`, and optionally `colors`/`stops` to tint it: the
picture's lightness picks the colour (its see-through parts stay see-through).

- Noise and textures go anywhere a fill goes (shapes, closed paths, `gradient` with a `fill`,
  a brush `color`), through clipping and masks like any paint, and on a pixel canvas (the vote
  snaps them to the palette, which gives a dithered texture for free).
- **They live in the coordinates of the group they're written in**, like the shapes there: a
  textured rock placed by a `use` or a `group` keeps its grain as it's moved, resized or turned,
  and a walking duck doesn't swim through its texture. (`scale` and `offset` are in those
  coordinates too.) For deliberate drift (clouds, water), give `offset` keyframes; `scale` and
  `contrast` may be keyframes as well.
- A brush `color` pattern is sampled where the stroke lands; the next plain `color` (on the
  brush or on a drawing command) switches back to one colour.

## Clipping and masks

Three ways to keep paint where it belongs, so shading, light and texture stay inside a shape.

**A clipping layer** (`"clip": true`) is shown only where the nearest layer below it that isn't
itself clipped (its *base*) has paint. Several clipping layers can sit on one base. The paint
isn't limited, only what's shown: the hatch below covers the whole canvas, but only the duck's
part of it is seen, and if the duck is repainted or moved later, the shading follows.

```json
{ "cmd": "layer", "name": "Duck" }
{ "cmd": "use", "name": "duck", "translate": [64, 96] }
{ "cmd": "layer", "name": "Duck shading", "clip": true, "blend": "multiply" }
{ "cmd": "hatch", "area": { "rect": [0, 0, 128, 128] }, "color": "shade", "angle": 40, "opacity": 0.4 }
{ "cmd": "layer", "name": "Duck light", "clip": true, "blend": "screen" }
{ "cmd": "gradient", "from": [90, 40], "to": [40, 100], "colors": ["#FFFFFF80", "transparent"] }
```

**The alpha lock** (`"lock": true`) makes painting change only the colour of what's already on
the layer: new paint lands as strongly as the pixel is opaque, the pixel's opacity never changes,
and the eraser does nothing. For recolouring a line, or shading a sprite that's all on one layer.
`"lock": false` turns it off again.

```json
{ "cmd": "layer", "name": "Lines", "lock": true }
{ "cmd": "gradient", "from": [0, 0], "to": [0, 300], "colors": ["#402010", "#A06030"] }
{ "cmd": "layer", "name": "Lines", "lock": false }
```

**A mask on a group or `use`** limits where its commands paint: every stroke, shape, gradient,
fill and dab inside is multiplied by the mask. A mask is one of:

| Mask | |
|---|---|
| an [area](#areas) | `{ "circle": [64, 96, 40] }`, `{ "polygon": [...] }`... in the group's own coordinates. |
| `{ "symbol": "duck", ... }` | The silhouette of a symbol: everywhere it would paint, placed with the fields of a `use` (`translate`, `scale`, `rotate`, `mirror`, `origin`, `with`). Nothing is painted; its `layer` commands are ignored. |
| `{ "layer": "Water" }` | Where a layer already has paint (as it is when the group starts). |
| `{ "not": ... }` | Everything but one of the above: a hole. |

```json
{ "cmd": "group", "name": "reflection", "mask": { "layer": "Water" }, "commands": [
  { "cmd": "use", "name": "duck", "translate": [64, 150], "mirror": "y", "opacity": 0.4 } ] }
{ "cmd": "group", "name": "shadow", "mask": { "symbol": "duck", "translate": [64, 96] }, "commands": [
  { "cmd": "gradient", "from": [64, 60], "to": [64, 116], "colors": ["transparent", "#00000060"] } ] }
{ "cmd": "use", "name": "wall", "mask": { "not": { "circle": [320, 180, 60] } } }
```

- A mask is worked out when the group starts, in the group's coordinates, and placed by the
  group's transform like everything else: with keyframes it moves along (a shadow clipped to a
  walking duck). Its numbers may be keyframes or parameters.
- Masks nest: a group inside a masked group paints only where both masks are. A `layer` command
  inside still selects its layer; the mask applies on whichever layer is painted.
- On a pixel canvas, a mask has hard edges (a pixel is in or out).
- A mask that covers nothing (off the canvas, an empty layer) is a warning: the group paints
  nothing.
- A named group or `use` whose shapes are small but which painted most of the canvas is a
  warning too (`painted 100% of the canvas, but its shapes cover 1%`): usually a `gradient` or
  `fill` without an `area` inside a `not` mask. Give it an `area`.

### `select`: a selection, like the app's

From a `select` on, every command that paints (strokes, shapes, fills, gradients, text, filters)
stays inside the selection, as inside a group's `mask`, until the next `select`. It's the same as
selecting in the app, and Export strokes writes the app's selections this way.

```json
{ "cmd": "select", "rect": [100, 80, 300, 200] }
{ "cmd": "select", "ellipse": [400, 300, 120, 80], "mode": "add" }
{ "cmd": "select", "circle": [250, 180, 40], "mode": "subtract", "feather": 6 }
{ "cmd": "select", "layer": "Duck" }
{ "cmd": "select", "invert": true }
{ "cmd": "select", "none": true }
```

| Field | |
|---|---|
| `rect`, `ellipse`, `circle`, `polygon` | A shape, written like an [area](#areas); placed by the groups around it. |
| `layer` | Where that layer has paint (as much as each pixel is opaque). |
| `all`, `none`, `invert` | `true`: everything; no selection (paint anywhere again); everything not selected. |
| `mode` | `"replace"` (the default), `"add"` or `"subtract"`: how the shape combines with the selection there is. |
| `feather` | Pixels the shape's edge fades over. On its own, it softens the selection there is. |

One shape (or `layer`, `all`, `none`, `invert`) per command. A selection that comes out empty is
dropped, with a warning. Unlike a group's `mask`, a selection stays after the group that made it.

## Filters

```json
{ "cmd": "filter", "blur": 8 }
{ "cmd": "filter", "layer": "Far hills", "blur": 3, "adjust": { "saturation": -0.3, "brightness": 0.1 } }
{ "cmd": "filter", "glow": { "radius": 24, "threshold": 0.75, "strength": 0.8 } }
{ "cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.5 }
{ "cmd": "filter", "gradient_map": { "stops": [[0, "#1B2433"], [0.5, "#C2651C"], [1, "#FFF3DC"]] } }
{ "cmd": "group", "name": "sun", "filter": { "glow": { "radius": 40, "strength": 1 } }, "commands": [
  { "cmd": "ellipse", "center": [520, 90], "radius": 34, "fill": "#FFF2B0" } ] }
{ "cmd": "use", "name": "duck", "translate": [64, 96], "filter": { "shadow": { "offset": [6, 8], "blur": 6, "color": "#00000070" } } }
```

The **`filter` command** changes everything on a layer (`layer`, or the selected one) in place.
Give any of these; they're applied in this order (use two `filter` commands for another order):

| | |
|---|---|
| `adjust` | `hue` (degrees), then `saturation`, `brightness`, `contrast`, each -1 to 1 (0 = as is). |
| `gradient_map` | Every pixel recoloured by its lightness through `stops` or `colors` (dark to light); opacity stays. |
| `blur` | Pixels of softening (up to 500). |
| `glow` | `radius` (how far, default 16), `threshold` (how light a pixel must be, 0 to 1, default 0.7), `strength` (default 1): the light parts spread and are added on top, brighter inside, a halo outside. |
| `shadow` | `offset` `[x, y]` (default `[4, 4]`), `blur` (default 4), `color` (default half-transparent black): a soft copy of the silhouette, behind. |
| `outline` | `{ "color", "size", "rim": {...} }`: a line around everything on the layer, behind it, and rim light on top (see below). |

Every number may be keyframes. Sizes are in the drawing units of where the command is, so in a
group scaled by 2 a blur of 3 is 6 pixels. In the app, **Filters** in the top bar has the same
five for the selected layer, each one undo step, written down by Export strokes.

**The whole picture:** `"from": "below"` on a layer fills it with everything below, flattened.
A gradient map or an adjustment on that layer then unifies the picture; at `opacity` 0.5, half
way.

**`filter` on a group or `use`** applies to what the group draws, on its own, before it lands
on the layers: a sun gets a glow, a duck a drop shadow, a far hill a haze, without touching
what's already there. When the group paints on several layers (a symbol that switches layers),
`blur`, `glow`, `adjust` and `gradient_map` are applied to each layer's part separately, and
each part lands on its own layer. A `shadow` is made from the **combined silhouette** of all the
parts and laid on the **lowest** of those layers, under everything the group drew, so one shadow
falls behind the whole duck and no seam shows between its layers. Filtered groups can nest.

- A `fill` (paint bucket) inside a filtered group spreads over the real picture, as it would
  outside.
- **Pixel canvas:** `blur`, `glow` and a `shadow` with blur are refused (pixels would melt);
  `shadow` with `"blur": 0`, `adjust` and `gradient_map` work and snap to the palette.

## Outlines for whole shapes

```json
{ "cmd": "use", "name": "duck", "translate": [64, 96],
  "outline": { "color": "ink", "size": 3, "rim": { "color": "cream", "size": 2, "angle": -45 } } }
{ "cmd": "group", "name": "logo", "outline": { "color": "#FFFFFF", "size": 6 }, "commands": [
  { "cmd": "text", "text": "DUCK FORGE", "at": [320, 40], "size": 64, "color": "ember", "align": "center" } ] }
```

A group or `use` with `outline` draws a line around the **silhouette of everything it draws
together**, on the lowest layer it painted, behind it. So a head on a body shows no seam, even
when they're on different layers, and text gets one clean outline around all its letters.

| | |
|---|---|
| `color` | The line's colour (default black). |
| `size` | How far out the line reaches, in pixels (default 2). Ends and corners are round. |
| `rim` | Rim light: `color` (default white), `size` (default 1.5) and `angle` (degrees clockwise, 0 = light from the right, -45 from the top right; default -45). A line of that colour along the inside edge, on the side facing the light, on top of the drawing. |

- It happens after the drawing, so it works with keyframes, `mirror`, `scale` and `rotate`;
  `size` and the rim's numbers may be keyframes, and they scale with the group.
- Shapes inside keep their own `outline` fields. `outline` and `filter` go together: shadow
  furthest back, then the outline, then the drawing, then the rim light.
- On a pixel canvas the line is whole pixels (like `pixel`'s own `outline`).

## Animations: frames and timing

```json
{
  "commands": [
    { "cmd": "canvas", "width": 128, "height": 128 },
    { "cmd": "define", "name": "box", "commands": [
      { "cmd": "rect", "x": -10, "y": -10, "width": 20, "height": 20, "fill": "#F2C230", "outline": "#202020" } ] }
  ],
  "animations": {
    "idle": { "frames": 4, "fps": 8, "loop": true, "commands": [
      { "cmd": "use", "name": "box", "translate": [64, 64] } ] },
    "still": { "frames": 1, "commands": [
      { "cmd": "use", "name": "box", "translate": [64, 100] } ] }
  }
}
```

A script with an `animations` section is an animation (or several, by name). Each one has
`frames` (how many, 1 to 100000), `fps` (default 12), `loop` (default false: the last frame leads
back to the first), `time` (`"frames"`, the default, or `"seconds"`: then plain numbers in its
keyframes are seconds) and its own `commands`. The canvas is the size of one frame. Each frame
plays the script's `commands` (the canvas, symbols, a background) and then the animation's. Time
is the frame number, counting from 0; keyframes (next section) make values change with it.
Animations are kept in the order written: the first is the one `render` shows by default. A top
level `"pivot": [x, y]` is the character's anchor point for sprite sheets (default: the bottom
centre of the canvas). A script without `animations` is a single picture, exactly as before.

### Keyframes: values that change with the frame

Any number of a `group` or `use` (`translate`, `scale`, `rotate`, `origin`, `opacity`, `mirror`,
the new colour in `colors`, and a parameter in `with`) can be keyframes instead:

```json
{ "cmd": "group", "name": "bounce", "origin": [64, 100],
  "translate": { "keys": [[0, [0, -60]], [6, [0, 0], "out"], [9, [0, -8]], [12, [0, 0]]] },
  "scale": { "keys": [[0, [0.9, 1.1]], [6, [1.2, 0.8], "out"], [12, [1, 1]]], "ease": "in-out" },
  "commands": [ { "cmd": "ellipse", "center": [64, 90], "radius": 10, "fill": "#F2C230" } ] }
```

`keys` is a list of `[frame, value]` pairs in frame order. Before the first key the value is the
first key's, after the last the last's; between keys it blends: numbers, `[x, y]` lists (each
number on its own) and colours (each channel on its own). Anything else, like a symbol name or
`"x"` for `mirror`, holds until the next key: that's how a mouth is swapped at a frame.

| Field | |
|---|---|
| `keys` | `[[frame, value], ...]`, or `[frame, value, "ease"]` to name the curve *into* that key. A key's time may be seconds instead: `"1.5s"` (at the animation's `fps`), or a plain number when the animation has `"time": "seconds"`. |
| `ease` | The curve between keys: `linear` (default), `in` (starts slow), `out` (ends slow), `in-out`, `step` (holds the earlier value until the next key). |
| `shift` | Plays the same keys this many frames (or `"0.5s"`) later, wrapping round in a looping animation. A walk's far leg is the near leg's keys with `"shift"` of half the cycle. |

Exactly on a key's frame, the value is the key's. In a single picture (no animation) keyframes
are read at frame 0. To animate text or a shape, put it in a group. A `seed` with keys holds
each seed until the next key (a pattern changes at a key, never half way): a twinkle is
`"seed": { "keys": [[0, 1], [4, 2], [8, 3]] }`.

### Symbol parameters, and `upright`

```json
{ "cmd": "define", "name": "leg", "params": { "angle": 0 }, "commands": [
  { "cmd": "group", "name": "thigh", "rotate": "$angle", "commands": [
    { "cmd": "line", "from": [0, 0], "to": [0, 17], "size": 4, "color": "#E08020", "taper": 0 },
    { "cmd": "use", "name": "foot", "translate": [0, 17], "upright": true } ] } ] }
{ "cmd": "use", "name": "leg", "translate": [60, 96], "with": { "angle": { "keys": [[0, -22], [4, 22], [8, -22]] } } }
{ "cmd": "use", "name": "leg", "translate": [68, 96], "with": { "angle": { "keys": [[0, -22], [4, 22], [8, -22]], "shift": 4 } } }
```

`define` takes `params`: names and their defaults. Inside the symbol, `"$angle"` stands for the
value, anywhere a keyframe could go (a `use`'s `name` too: `"name": "$mouth"`), and in any
colour field (`"fill": "$paint"`), so a symbol can take its colours as parameters. A `use` gives
values with `with`; a name the symbol doesn't have is a mistake. A value in `with` may be
keyframes, so one symbol covers every pose of a walk. There's no arithmetic (`"-$angle"`): use
`shift`, a second parameter or `mirror`.

`"upright": true` on a group or `use` undoes the turning of everything around it: a foot inside a
turned leg still moves with the leg (its position is turned) but stays flat on the ground. It still
mirrors and resizes with the groups around it.

### Seeing an animation without watching it

`quill render` paints frame 0 of the first animation written unless told otherwise:
`--frame 3`, `--frames 0-7` or `--all-frames`, and `--animation walk` to choose the animation.
Frames are saved as `name-walk-003.png`. A thing that is off the canvas in some frames but on
it in others (an entrance, an exit) isn't warned about; one that never is, is. Then:

- `--frame-sheet` (or `--frame-sheet 2` for every 2nd frame): every frame in one numbered
  picture, `name-walk-frames.png`, to judge a whole walk at a glance.
- `--frame 3 --onion`: frame 3 with frame 2 faint in red and frame 4 faint in blue behind it,
  `name-walk-003-onion.png` (in a loop the neighbours wrap round).
- `--gif`: the animation as `name-walk.gif`, at its fps, looping if it loops, for showing a
  person. `--bg` puts a colour behind see-through frames; `--zoom 4` enlarges with sharp pixels.

`quill export script.json` writes the animations for a game or a video: a sprite sheet with
its JSON (`--sheet`, the default: see [sprite-sheet-format.md](sprite-sheet-format.md)), every
frame as a PNG (`--frames`), a GIF (`--gif`) and an MP4 (`--mp4`, through ffmpeg), with `--scale 8`
for sharp enlargement and `--bg` behind see-through parts.

`quill check` plays every frame of every animation, so a mistake in any of them is found
(`animation "walk" > command 2 (use "duck"): ...`), and prints a **motion summary**: for each
named group and `use`, where its middle is in the first and last frame and its biggest step
between two frames. Warnings follow: a part that jumps more than a quarter of the canvas between
two frames, a part that draws nothing in some frames, and for loops, a part that moves much
further from the last frame back to the first than in any step inside (the loop "pops"). Boxes
are geometry, so a part faded to `opacity` 0 still counts as drawn.

## Pixel art

```json
{ "cmd": "canvas", "width": 32, "height": 32, "pixel": true }
{ "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 8, "outline": "ink" } }
```

A **pixel canvas** gives crisp pixel art from ordinary commands. It's painted `supersample` times
bigger (4 unless given; up to 16) with everything the script says, shapes, strokes, text, symbols
and transforms alike, then shrunk to its real size: each pixel is painted if more than half of its
samples were, in the palette colour that the most of them had. So every pixel is whole, there are
no blurry or half see-through edges, and the picture only ever has the palette's colours. An
`outline` colour (a palette name) is put on every see-through pixel next to a painted one, on each
layer, after shrinking. Positions are in canvas pixels, decimals allowed (`"radius": 5.5`).

- **The palette is locked:** on a pixel canvas every colour is a palette name; a `#RRGGBB` is a
  mistake, and a pixel canvas needs a palette. `transparent` is allowed.
- **Animation moves by whole pixels:** `translate` is rounded, so a moving sprite stays crisp.
- A pixel canvas can't `open` a picture, and in the app it can't be played onto an open picture
  (it's painted big and shrunk at the end): play it on a new picture, or export its frames.
- Previews (`--preview`) and `--zoom` work as usual; export with `--scale 8` for people.

### `pixels` — rows of characters

```json
{ "cmd": "pixels", "at": [12, 8], "colors": { "y": "yellow", "k": "ink" }, "rows": [
  "..kkk...", ".kyyyk..", "kyyykyy.", ".kyyyk..", "..kkk..." ] }
```

Paints each character as one canvas pixel, the top-left one at `at`: `.` (or a space) is
see-through, every other character is a palette colour from `colors` (keys are single characters).
It works on any canvas (on a big one the "pixels" are canvas pixels too). Single pixels, lines,
rectangles, circles and the paint bucket need nothing new: `rect`, `line`, `ellipse`, `polygon`
and `fill` already come out crisp on a pixel canvas.

## Areas

`gradient`, `hatch` and `scatter` take an `area`, and a group's `mask` can be one:

```json
{ "rect": [x, y, width, height] }
{ "ellipse": [cx, cy, rx, ry] }
{ "circle": [cx, cy, r] }
{ "polygon": [[x, y], [x, y], [x, y], ...] }
```

## Randomness

`jitter`, `hatch` and `scatter` use random numbers that are **the same every time** the script is
played. They come from the script's `seed` mixed with the command's own `seed` or, if it has none,
the command's number.

So: adding or removing a command before a random command changes its pattern (its number
changes). To keep a pattern you like while editing the script, give that command its own
`"seed"`. To try a different pattern, change the seed. A command's `seed` may be keyframes or a
symbol parameter (`"seed": "$seed"`), so a library part can take one.

## Errors

Every error says where it is and what's wrong:

```
line 14: expected `,` or `}` (column 52)
command 7 (stroke): unknown field `colour`, expected one of `points`, `color`, `size`, ...
command 12 (group "duck") > 3 (ellipse): unknown colour "yelow"; the palette has: ink, yellow
command 20 (use "pair") > 1 (use "duck"): symbol "duck" uses itself (duck > pair > duck)
command 9 (layer): unknown blend mode "multipy"; use one of: normal, multiply, screen, ...
command 12 (fill): unknown colour "skyblue"; the palette has: sea, sky, sun
command 15 (delete_layer): there's no layer called "Sketch" (the layers are: Sky, Sea)
```

Command numbers count from 1, in the order of the `commands` list. Inside a group, `>` leads to
the command: `command 12 (group "duck") > 3 (ellipse)` is the 3rd command inside the group that is
the script's 12th.

## Tips

- Work **back to front**: sky and background first, then the middle, then the front, each on its
  own layer.
- Block in big shapes first, then add details on new layers. Use `snapshot` after each stage.
- Soft brushes (`hardness` 0–0.3) and low `flow` blend; hard brushes (0.8–1) give clean edges.
- Shadows: a layer with `"blend": "multiply"` painted in a cool, dark colour. Light and glow:
  `"blend": "screen"` or `"add"`.
- `jitter` of 1–4 pixels makes shapes look hand-made instead of computer-perfect.

## Scripts from the app

**File → Export strokes…** in Duck Quill writes everything painted since the picture was started or
opened as a script. It uses only `canvas`, `layer`, `brush`, `stroke`, `erase`, `fill`, `gradient`,
`clear_layer`, `delete_layer`, `move_layer` and `rename_layer` (plus the commands of any script that
was played), with every stroke point exactly where the pen went: `[x, y, pressure]`. It's an easy
way to see how hand-drawn strokes are written, and to continue a drawing with a script.
