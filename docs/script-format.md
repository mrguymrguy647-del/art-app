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

### `snapshot`

```json
{ "cmd": "snapshot", "name": "blocked in" }
```

A named checkpoint. `art-cli render --snapshots` saves a picture here, so you can check the
picture at each stage. It doesn't change the picture.

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
command 9 (layer): unknown blend mode "multipy"; use one of: normal, multiply, screen, ...
command 12 (fill): unknown colour "skyblue"; the palette has: sea, sky, sun
command 15 (delete_layer): there's no layer called "Sketch" (the layers are: Sky, Sea)
```

Command numbers count from 1, in the order of the `commands` list.

## Tips

- Work **back to front**: sky and background first, then the middle, then the front, each on its
  own layer.
- Block in big shapes first, then add details on new layers. Use `snapshot` after each stage.
- Soft brushes (`hardness` 0–0.3) and low `flow` blend; hard brushes (0.8–1) give clean edges.
- Shadows: a layer with `"blend": "multiply"` painted in a cool, dark colour. Light and glow:
  `"blend": "screen"` or `"add"`.
- `jitter` of 1–4 pixels makes shapes look hand-made instead of computer-perfect.
