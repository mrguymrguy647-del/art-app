# Beautiful: the design for phase 3

Phase 2 made pictures that are clean but flat. This phase adds the tools that make them rich:
shading that stays inside a shape, brushes with character, texture for big areas, blur and glow,
outlines without seams, and parts that are reused across paintings. Everything is built on what
exists (layers, groups, symbols, keyframes, pixel mode) and on **three engine ideas**:

1. **Coverage masks.** A group's drawing can be limited by a mask: every dab, fill and flood
   inside the group is multiplied by the mask's coverage. Clipping, alpha lock and group masks
   are all this one thing, applied in three places.
2. **Group buffers.** A group with a `filter` or an `outline` paints into a scratch surface, the
   effect is applied to that surface as a whole, and the result is laid on the layer. One
   mechanism gives blur, glow, drop shadows, colour adjustments and seamless outlines.
3. **Paint sources.** Wherever a colour goes today (`fill`, brush `color`, gradient stops), a
   noise or a texture may go instead. Brush tips are small pictures the brush stamps.

Each section says how a thing looks in a script, how it combines with the rest, what can be
keyframed, what it means on a pixel canvas, and what the app shows.

## 1. Clipping and masks

```json
{ "cmd": "layer", "name": "Duck" }
{ "cmd": "use", "name": "duck", "translate": [64, 96] }
{ "cmd": "layer", "name": "Duck shading", "clip": true, "blend": "multiply" }
{ "cmd": "hatch", "area": { "rect": [0, 0, 128, 128] }, "color": "shade", "angle": 40, "opacity": 0.4 }
{ "cmd": "layer", "name": "Duck light", "clip": true, "blend": "screen" }
{ "cmd": "gradient", "from": [90, 40], "to": [40, 100], "colors": ["#FFFFFF80", "transparent"] }
```

- **`"clip": true` on a layer:** it shows only where the nearest layer below it that isn't
  itself clipped (its *base*) has paint. Several clipped layers stack on one base. Painting isn't
  limited (the hatch above covers the whole canvas); showing is: the clipped layer's opacity is
  multiplied by the base's at composite time, the way Krita and Procreate do it. So the base can
  be repainted or moved later and the shading follows.
- **`"lock": true` on a layer** (alpha lock): painting changes colour only where the layer
  already has paint; new paint is multiplied by the existing coverage. For recolouring a line or
  shading a single-layer sprite.
- **`"mask"` on a group or `use`:** its commands paint only inside the mask. A mask is an
  [area](script-format.md#areas) (`{"ellipse": [...]}`, `{"polygon": [...]}`), the silhouette of
  a symbol (`{"symbol": "duck", "translate": [64, 96]}`, placed like a `use`), the contents of a
  layer (`{"layer": "Water"}`: where it has paint), or any of those cut out with `"not"`:

```json
{ "cmd": "group", "name": "reflection", "mask": { "layer": "Water" }, "commands": [
  { "cmd": "use", "name": "duck", "translate": [64, 150], "mirror": "y", "opacity": 0.4 } ] }
{ "cmd": "group", "name": "shadow", "mask": { "symbol": "duck", "translate": [64, 96] }, "commands": [
  { "cmd": "gradient", "from": [64, 60], "to": [64, 116], "colors": ["transparent", "#00000060"] } ] }
{ "cmd": "group", "mask": { "not": { "circle": [320, 180, 60] } }, "commands": [ "...everything but a hole..." ] }
```

- A mask is worked out when the group starts, in the group's own coordinates, then placed by
  the group's transform like everything else, so it moves with keyframes (a reflection clipped to
  moving water, a shadow clipped to a walking duck). Its numbers may be keyframes.
- Masks nest: a group inside a masked group is masked by both. A symbol's layer commands still
  work: the mask applies on whichever layer a command paints.
- **Pixel canvas:** masks are thresholded at half, so edges stay crisp; clipping and lock work
  as they are (coverage is already whole pixels).
- **App:** two toggles on each layer row in the Layers panel (clip, lock), saved in the
  `.artapp` (old files have neither), undoable, and written by Export strokes. Group masks are
  script-only.

## 2. Brushes with character

```json
{ "cmd": "brush", "tip": "chalk", "size": 18, "color": "rock",
  "dynamics": { "follow": true, "size": 0.3, "angle": 20, "opacity": 0.4, "scatter": 3, "hue": 4, "lightness": 0.08, "saturation": 0.1 } }
{ "cmd": "brush", "tip": { "file": "tips/splat.png" } }
```

- **Tips:** `round` (today's brush), `square` (hard, for pixels), `bristle`, `chalk`, `leaf`,
  `grass`, `sparkle`, or `{"file": "..."}` for a greyscale PNG (its lightness is the dab's
  shape; paths are relative to the script). The built-in tips are made by the engine from
  formulas with fixed seeds, so they're the same everywhere and need no files. A dab stamps the
  tip, scaled to `size` and turned; `hardness` softens its edge as before.
- **Dynamics**, all from the stroke's `seed`: `follow` turns the tip along the stroke; the rest
  are amounts of random variation per dab: `size` (fraction), `angle` (degrees), `opacity`
  (fraction), `scatter` (pixels off the line, scaled with the brush), `hue` (degrees),
  `lightness` and `saturation` (fractions). The brush carries tip and dynamics, so `stroke`,
  `line`, `path`, `hatch` and `scatter` use them without new fields (`scatter`'s own `size`,
  `colors` and `angle` still win).
- **Stamping along a path:** a `stamp` command puts a symbol or a tip at a spacing along points
  (a smooth path, like `path`), with the same seeded variation, each copy turned to follow:

```json
{ "cmd": "stamp", "symbol": "blade", "points": [[20, 300], [300, 290], [620, 310]], "spacing": 9,
  "size": [0.8, 1.3], "angle": [-10, 10], "colors": ["grass1", "grass2"], "seed": 4 }
{ "cmd": "stamp", "tip": "leaf", "points": [[100, 100], [160, 40], [240, 60]], "spacing": 14, "size": [10, 16], "color": "leaf" }
```

  `symbol` copies are `use`s (so `with` parameters and `colors` apply per copy); `tip` copies
  are single dabs. `size` for a symbol is a scale, for a tip pixels. A row of grass, a vine, a
  string of lights, pebbles along a shore.
- **Keyframes:** on `stamp`'s `points`, `spacing`, `size` and `angle` (it's a placement command
  like `group`); a brush's dynamics are per command, not keyframed (put the stroke in a group).
- **Pixel canvas:** tips stamp crisp (thresholded like everything), colour variation snaps to the
  palette; `square` is the natural tip there.
- **App:** a tip picker (the built-in ones as small pictures) and the dynamics as sliders in the
  brush panel; recorded in Export strokes as `brush` fields.

## 3. Noise and texture

```json
{ "cmd": "rect", "x": 0, "y": 200, "width": 640, "height": 160,
  "fill": { "noise": { "scale": 60, "detail": 3, "contrast": 1.3, "colors": ["water_dark", "water_light"], "offset": [0, 0], "seed": 2 } } }
{ "cmd": "gradient", "area": { "rect": [0, 0, 640, 200] },
  "fill": { "noise": { "scale": 180, "detail": 4, "stops": [[0, "sky"], [0.6, "sky"], [1, "cloud"]] } } }
{ "cmd": "layer", "name": "Grain", "blend": "multiply", "opacity": 0.25 }
{ "cmd": "rect", "x": 0, "y": 0, "width": 640, "height": 360, "fill": { "texture": "paper" } }
{ "cmd": "rect", "x": 0, "y": 0, "width": 640, "height": 360, "fill": { "texture": { "file": "textures/canvas.png", "scale": 0.5 } } }
```

- **Noise** is a paint source: smooth natural noise (layered value noise) with `scale` (pixels
  per wave), `detail` (layers of finer noise, 1 to 6), `contrast`, an `offset`, and a `seed`,
  mapped through two `colors` or gradient `stops`. It goes anywhere a fill goes (shapes, `gradient`
  with an `area`, a brush `color`), through clipping and masks like any paint. Clouds, water,
  rock, grass, skin.
- **Texture** is a tiled picture as a paint source: a PNG (`file`, `scale`, `offset`, and
  optional `colors` to tint it by its lightness) or the built-in `"paper"` grain (made from noise,
  no file). A paper look is a texture-filled layer on `multiply` or `overlay` at low opacity.
- **Keyframes:** `offset` and `scale` inside a noise or texture fill may be keyframes (drifting
  clouds, moving water); so may `contrast`.
- **Pixel canvas:** both work; the vote snaps them to the palette, which gives dithered-looking
  texture for free.
- **App:** nothing new to paint with (a gradient tool already exists); textures and noise are
  script-only in this phase.

## 4. Filters and adjustments

```json
{ "cmd": "filter", "blur": 8 }
{ "cmd": "filter", "layer": "Far hills", "blur": 3, "adjust": { "saturation": -0.3, "brightness": 0.1 } }
{ "cmd": "filter", "glow": { "radius": 24, "threshold": 0.75, "strength": 0.8 } }
{ "cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.5 }
{ "cmd": "filter", "gradient_map": { "stops": [[0, "#1B2433"], [0.5, "#C2651C"], [1, "#FFF3DC"]] } }
{ "cmd": "group", "name": "sun", "filter": { "glow": { "radius": 40, "strength": 1 } }, "commands": [ "..." ] }
{ "cmd": "use", "name": "duck", "translate": [64, 96], "filter": { "shadow": { "offset": [6, 8], "blur": 6, "color": "#00000070" } } }
```

- **`filter` command:** applies to a layer (`layer`, or the selected one) in place, in the order
  written: `blur` (pixels), `glow` (light pixels above `threshold` spread `radius` pixels with
  `strength`), `shadow` (a blurred copy in a colour, `offset` behind), `adjust` (`hue` degrees,
  `saturation`, `brightness`, `contrast` from -1 to 1) and `gradient_map` (every pixel recoloured
  through the gradient by its lightness). One undo step in the app.
- **The whole picture:** `"from": "below"` on a new layer fills it with everything below it
  flattened, so a gradient map or an adjustment on that layer unifies the picture (at `opacity`
  0.5, half way). No second mechanism.
- **`filter` on a group or `use`:** the group paints into a scratch surface (on the selected
  layer only: a `layer` command inside is a mistake), the filter is applied to that drawing
  alone, and it's laid on the layer. That's how a sun gets a glow, a duck a drop shadow, a far
  hill a haze, without touching what's already on the layer.
- **Keyframes:** every number in a group's `filter` (a pulsing glow, a shadow that moves with
  the light); the `filter` command's numbers too, in an animation.
- **Pixel canvas:** `blur`, `glow` and a blurred `shadow` are mistakes (they'd melt the pixels);
  `shadow` with `blur` 0, `adjust` and `gradient_map` work and snap to the palette.
- **Cost:** blur is three box blurs (close to gaussian) in two passes, so it costs the same at
  any radius: about the time of painting the layer once. Measured numbers go in decisions.md.
- **App:** a Filters menu with each of these for the selected layer, with a small window for
  the numbers; one undo step; recorded as a `filter` command.

## 5. Outlines for whole shapes

```json
{ "cmd": "use", "name": "duck", "translate": [64, 96],
  "outline": { "color": "ink", "size": 3, "rim": { "color": "cream", "size": 2, "angle": -45 } } }
```

A group or `use` with `outline` paints into a scratch surface (like a filter), and the outline
is drawn around the **silhouette of everything together**, behind it, so a head on a body shows
no seam. `rim` adds a light line along the inside edge on the side facing `angle` (degrees, where
the light comes from), for rim light in one line. Shapes inside keep their own `outline` fields if
they have them. It works with keyframes and `mirror` because it happens after the drawing. On a
pixel canvas it's one or more whole pixels, like `pixel`'s `outline`.

## 6. Include and a library

```json
{ "cmd": "include", "file": "library/cloud.json" }
{ "cmd": "use", "name": "cloud", "translate": [200, 80], "with": { "puff": 1.3 } }
```

`include` (top level only) reads another script's `define`s and palette entries (not its
commands or animations). Paths are relative to the script; an included file may include others;
a file that includes itself, directly or through others, is a mistake that shows the chain; a
symbol or colour defined twice says both files. Mistakes inside an included file name it:
`library/cloud.json: command 3 (define "cloud") > ...`. `check`'s "never used" warnings skip
included symbols (a library has more than any one picture uses).

`library/` in the repository holds reusable parts made with these tools, each with parameters, a
small example script and a preview picture: cloud, water, grass, reed, tree, lily pad, the duck.

## 7. Fixes from phase 2's reviews

Seeds as keyframes (`"seed": {"keys": ...}`, holding; a twinkle); animation layers reported by
`check` with a warning when one covers most of the picture; animations in file order; `export`
saying what it skipped; `render --scale` for a still saved as `name-50pct.png`; keys in seconds
(`[[0, 0], ["1.5s", 20]]` or `"time": "seconds"` on the animation); no off-canvas warnings for parts
that are on the canvas in some frame; `render --pixels` printing a pixel frame as rows of palette
letters; the app's Text tool default size as a fraction of the canvas.

## 8. What's left out, on purpose

A smudge or mixer brush; liquify and warp; bezier editors; perspective grids; vector text effects;
SVG tips; custom blend formulas; layer effects beyond these (bevel, inner glow); a filter on
*several* layers at once (make a `from: below` layer); keyframed brush dynamics; HDR.
