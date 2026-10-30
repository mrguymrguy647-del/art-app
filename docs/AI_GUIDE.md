# Painting with Art App: a guide for AI assistants

You can paint pictures with Art App by writing **painting scripts** (JSON) and rendering them with
**`art-cli`**, a command-line program that needs no screen or graphics card. You can look at the
PNGs it saves, so you can judge your own work and improve it, step by step, like a painter
stepping back from the easel.

This guide is the practical part. Every command is described in
[script-format.md](script-format.md). Two complete examples: [examples/sunset.json](../examples/sunset.json)
and [examples/night-hills.json](../examples/night-hills.json).

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
art-cli check picture.json                          # mistakes? (milliseconds, paints nothing)
art-cli render picture.json --preview --sheet       # quick look: half size + every stage in one image
art-cli render picture.json --crop 600,400,300,200 --zoom 3   # close-up of one area, full size, enlarged
art-cli render picture.json --snapshots             # full size, plus a PNG per stage
art-cli info out/picture.artapp                     # layers, blend modes, what's painted where
```

Files go to `out/` (or `--out folder`): `picture.png`, `picture.artapp`, `picture-preview.png`,
`picture-sheet.png`, `snapshots/NN-name.png`, `picture-crop-x-y-w-h.png`.

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
| `snapshot` | A named checkpoint for `--snapshots` and `--sheet`. |
| `delete_layer`, `rename_layer`, `move_layer`, `clear_layer`, `paper` | Housekeeping. |

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
6. `art-cli info out/x.artapp` or `art-cli check`: are the layers what you expect, in the right
   order, with the right blend modes?

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
- **Previews scale everything**, including brush sizes, so a 1 px line can vanish in a preview.
  Check fine lines with `--crop`, which is always full size.
- **Shapes stopping at the canvas edge leave slivers.** Run backgrounds and land past the edges.
- **The paper must be opaque.** For a transparent PNG use `--transparent` instead.

## 9. Working with a person's drawing

People can draw in the Art App window and send you their work:

- **The picture:** an `.artapp` file. Continue it with `{"cmd": "canvas", "open": "their.artapp"}`
  (path relative to your script), then add layers on top. Use `art-cli info` to see its layers.
- **How it was drawn:** **File → Export strokes…** in the app writes everything they painted as a
  script: their layers, brush settings and every stroke as `[x, y, pressure]` points. Read it to
  see what they drew and where, and to match their brush sizes and colours.
- They can watch your script being painted in the app with **File → Play strokes…**.

## 10. Speed

- `check` takes milliseconds; a 1600×1000 render well under a second; `--preview` about a third
  of that.
- Snapshots and the contact sheet cost a little per stage; use a few meaningful snapshots rather
  than one per command.
- `scatter` and `hatch` with thousands of strokes are fine; hundreds of thousands are slow.
