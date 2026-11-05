# Art App painting scripts

A painting script describes a picture as a list of steps, in a JSON text file. Playing the script
paints the picture. Scripts are made to be written by hand (or by an AI assistant), rendered,
checked, and improved.

- Playing a script **always gives exactly the same picture**. Even the "random" commands (jitter,
  scatter, hatching) use a seed, so they repeat exactly.
- Mistakes are explained with the **command's number**, e.g.
  `command 12 (stroke): unknown field "sise", expected one of "points", "color", "size", ...`
- **Big shapes are one command**: rectangles, ellipses, polygons, smooth paths, tapered lines,
  hatching and scattering.

Render scripts with `art-cli` (see the README), or watch them being painted in the app with
**File → Play strokes…**.

## A first script

```json
{
  "format": "artapp-script",
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
| `format` | no | `"artapp-script"`. Recommended, so the file is recognisable. |
| `version` | no | `1`. A newer version is refused with a clear message. |
| `title` | no | A title for people. |
| `notes` | no | Anything for people (text, a list, …). Ignored. |
| `seed` | no | Where randomness starts. Default `1`. Change it to get a different random pattern. |
| `palette` | no | Colour names, defined once: `{"sky": "#88BBFF", "shadow": "#1A1A3080"}`. |

Every command is an object with a `"cmd"` field. Any command may also have a `"note"` field: a
comment for people, ignored when playing. Misspelt fields are errors (not silently ignored).

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
{ "cmd": "canvas", "open": "my-drawing.artapp" }
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
| `color` | Brush colour. A colour's opacity (`#RRGGBBAA`) multiplies the brush opacity. |
| `size` | Diameter in pixels at full pressure (above 0, up to 1000). |
| `opacity` | The most opaque one stroke gets. Going over the same spot within a stroke never gets darker than this. |
| `flow` | How much paint each dab adds. Below 1, paint builds up where dabs overlap, like an airbrush. |
| `hardness` | 1 = crisp edge, 0 = very soft edge. |
| `spacing` | Distance between dabs as a fraction of the size (0.01 to 5). Default 0.1. |
| `pressure_size`, `pressure_opacity`, `pressure_flow` | Which settings point pressure changes (`true`/`false`). |

Only the settings given change.

### `stroke` and `erase` — like a pen

```json
{ "cmd": "stroke", "points": [[100, 400], [300, 380, 0.6], [500, 420, 0.3]] }
{ "cmd": "stroke", "points": [[100, 100], [200, 150]], "color": "#C03030", "size": 4 }
{ "cmd": "erase", "points": [[400, 200], [400, 400]], "size": 30, "hardness": 0.5 }
```

Paints (or erases) through the points with the brush, exactly as if a pen moved through them: a
smooth curve through every point, with no gaps. Optional: `color`, `size`, `opacity`,
`hardness`, `flow` (this stroke only), `jitter` (a gentle hand-drawn wobble, in pixels), `seed`.

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
| `fill` | A colour or a [gradient](#gradients-as-fills). Edges are smooth (anti-aliased). |
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
pixels.

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

Writes `text` in Art App's own font, [Inter](https://rsms.me/inter/), which is built into the
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

A named checkpoint. `art-cli render --snapshots` saves a picture here, so you can check the
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
| `use` | `name`, the group fields above, and `colors`. |
| `colors` | Colours to swap for this use: `{ "yellow": "#C04040" }` paints red wherever the symbol paints the palette colour `yellow`. The left side can also be a hex code (`"#F2C230"`). For enemy variants, team colours and so on. |

- **Symbols can use other symbols.** A symbol using itself, even through others, is an error:
  `symbol "a" uses itself (a > b > a)`. Colour swaps reach into the symbols inside, and the inner
  `use`'s own swaps come first.
- **Layers:** a symbol can switch layers with `layer` commands. Layer names are the picture's
  layers, the same as anywhere else: two uses of a character symbol paint onto the same `Body` and
  `Shadow` layers (created if missing). A symbol without `layer` commands paints on the layer
  selected when it's used. After a `use`, the brush and the selected layer are back to what they
  were before it.
- **Random patterns** (jitter, scatter, hatch) in a symbol are the same every time it's used.
- Mistakes inside a symbol name the `use` and the symbol's command:
  `command 9 (use "duck") > 2 (ellipse): unknown colour "yelow"`.

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
back to the first) and its own `commands`. The canvas is the size of one frame. Each frame plays
the script's `commands` (the canvas, symbols, a background) and then the animation's. Time is
the frame number, counting from 0; keyframes (next section) make values change with it. A top
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
| `keys` | `[[frame, value], ...]`, or `[frame, value, "ease"]` to name the curve *into* that key. |
| `ease` | The curve between keys: `linear` (default), `in` (starts slow), `out` (ends slow), `in-out`, `step` (holds the earlier value until the next key). |
| `shift` | Plays the same keys this many frames later, wrapping round in a looping animation. A walk's far leg is the near leg's keys with `"shift"` of half the cycle. |

Exactly on a key's frame, the value is the key's. In a single picture (no animation) keyframes
are read at frame 0. To animate text or a shape, put it in a group.

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
value, anywhere a keyframe could go (a `use`'s `name` too: `"name": "$mouth"`). A `use` gives
values with `with`; a name the symbol doesn't have is a mistake. A value in `with` may be
keyframes, so one symbol covers every pose of a walk. There's no arithmetic (`"-$angle"`): use
`shift`, a second parameter or `mirror`.

`"upright": true` on a group or `use` undoes the turning of everything around it: a foot inside a
turned leg still moves with the leg (its position is turned) but stays flat on the ground. It still
mirrors and resizes with the groups around it.

### Seeing an animation without watching it

`art-cli render` paints frame 0 of the first animation unless told otherwise: `--frame 3`,
`--frames 0-7` or `--all-frames`, and `--animation walk` to choose the animation. Frames are
saved as `name-walk-003.png`. Then:

- `--frame-sheet` (or `--frame-sheet 2` for every 2nd frame): every frame in one numbered
  picture, `name-walk-frames.png`, to judge a whole walk at a glance.
- `--frame 3 --onion`: frame 3 with frame 2 faint in red and frame 4 faint in blue behind it,
  `name-walk-003-onion.png` (in a loop the neighbours wrap round).
- `--gif`: the animation as `name-walk.gif`, at its fps, looping if it loops, for showing a
  person. `--bg` puts a colour behind see-through frames; `--zoom 4` enlarges with sharp pixels.

`art-cli export script.json` writes the animations for a game or a video: a sprite sheet with
its JSON (`--sheet`, the default: see [sprite-sheet-format.md](sprite-sheet-format.md)), every
frame as a PNG (`--frames`), a GIF (`--gif`) and an MP4 (`--mp4`, through ffmpeg), with `--scale 8`
for sharp enlargement and `--bg` behind see-through parts.

`art-cli check` plays every frame of every animation, so a mistake in any of them is found
(`animation "walk" > command 2 (use "duck"): ...`), and prints a **motion summary**: for each
named group and `use`, where its middle is in the first and last frame and its biggest step
between two frames. Warnings follow: a part that jumps more than a quarter of the canvas between
two frames, a part that draws nothing in some frames, and for loops, a part that moves much
further from the last frame back to the first than in any step inside (the loop "pops"). Boxes
are geometry, so a part faded to `opacity` 0 still counts as drawn.

## Areas

`gradient`, `hatch` and `scatter` take an `area`:

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
`"seed"`. To try a different pattern, change the seed.

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

**File → Export strokes…** in Art App writes everything painted since the picture was started or
opened as a script. It uses only `canvas`, `layer`, `brush`, `stroke`, `erase`, `fill`, `gradient`,
`clear_layer`, `delete_layer`, `move_layer` and `rename_layer` (plus the commands of any script that
was played), with every stroke point exactly where the pen went: `[x, y, pressure]`. It's an easy
way to see how hand-drawn strokes are written, and to continue a drawing with a script.
