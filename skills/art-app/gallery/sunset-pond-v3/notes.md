# Sunset pond v3

The sunset pond painted a third time, with the phase 4 process: `script.json` is written in
stage order (the layers are made first, empty, then every stage comes back to them by name), so
`process.png` shows the stages as a painter would work them: values, colour, light, detail,
finish. A 1920 x 1080 still (frame 0) and the same 48-frame loop as before (`loop.gif`, at a
quarter size): clouds and water drift, the duck bobs, the reeds sway. `before` in the index
points at `../sunset-pond/script.json`, the phase 3 painting it answers.

## The stages, and what the senses said at each

- **Thumbnail / values** (`render --scale 0.1` of the script cut at the `values` snapshot):
  grey shapes for sky, sun, hills, water, bank, the duck. The thumb read (sun, hills, water,
  a duck). The critique said 2 value groups and that the duck merged (value gap 0.11): the
  duck's grey was too close to the water's, so the colour stage made the duck lighter and put
  it on the sun's path.
- **Colour**: flat local colours in key. 3 value groups; `out of key: nothing` (the pads are
  an olive mixed from the orange and the blue; lesson 11); the clouds were "flat over 50%"
  (one colour) and the duck "blotchy over 80%": the duck's drop-shadow *filter* was part of
  the duck's own paint, a soft blob. Both fixed in the next stages: gradient puffs on the
  clouds, the shadow painted as its own shape under the duck.
- **Light and form**: the sun's glow, haze inside the far hills' crisp silhouette (lesson 8),
  clipped lights on the hills, the sun's path as a pool and three scatters of short glints,
  the duck's clipped shade and light and its rim of light. 4 value groups; the focal point had
  the strongest contrast; a first version with a blurred oval for the sun's path was caught as
  a soft edge ("the lighter background shows through") and as a reflection "egg" in the
  picture: replaced by the glints.
- **Detail**: 35% of the marks within four cells of the duck (an area of 8%): the body's
  feather strokes, the ripples around it, the glints; reeds, grass and the pads' rims
  elsewhere.
- **Finish**: mood pass at 0.2, vignette, paper. Final critique: 4 value groups, focal contrast
  1.2x, `out of key: nothing`, no texture or edge finding, 4 of 4 parts read at 96 px.
  Numbers: readability 4, values 5, colour 4, focal point 5, detail 5.
- **Blind read**: reads (`blind-read.md`): "a lake scene at sunset ... a yellow cartoon duck
  ... my eye goes first to the duck ... calm, quiet".

## What each tool did

- **Clouds:** a `cloudshape` symbol of six overlapping puffs with a gradient from a dark mauve
  top to a lighter base, and a `cloud` symbol that paints it and then a radial pool of lit
  colour masked to the cloudshape's own silhouette (`mask: { symbol }`), so the lit underside
  is soft and the bottom edge crisp. Four `use`s drift with a sway that returns.
- **Sun:** two discs, the lower quarter behind the far hill, a `glow` filter on the layer, a
  radial screen glow behind.
- **Hills:** gradient fills, no blur; the far hills take their haze as a clipped vertical
  gradient and a clipped radial from the sun's side.
- **Water:** the drifting noise fill and the clipped depth gradient as before; the sun's path
  is a radial pool at the waterline plus three `scatter` strokes (horizontal, short, dense near
  the sun) on a screen layer masked to the water; bristle ripples.
- **Duck:** the library duck at 2.7x with its colours set through `with`, a rim of light from
  the sun through the use's `outline.rim`; its shadow is a dark rect masked to the duck's
  silhouette, blurred, on its own layer; shade and light on clipped layers; feather strokes on
  a clipped layer masked to the duck's silhouette; the reflection is a gradient masked to the
  duck's mirrored silhouette.
- **Pads:** notched polygons (an ellipse with a wedge cut, from a few lines of arithmetic),
  gradient-filled in the olive, a thin lit rim, a shadow filter on a group.
- **Bank and reeds:** as before (library reeds with a lean each, grass stamps), the bank with
  a gradient.

## Honest notes

- The blind reader is right that the duck's scribbly strokes don't quite match the flat
  vector look of the rest: the duck is painted and the rest is cut paper. Either the rest
  wants marks too (the hills, the bank) or the duck wants fewer.
- The lily pads and the bank have no reflections; the cattail stems run into the water.
- The clouds are better shapes than the smudges but still simple; a real cumulus has more
  puffs and a less even base.
- The composition's weight is dead centre (50%, 50%) although the focal point is not.
- Painting time: the still in about 2 s; the loop with reuse in about a minute at full size.

## Proposed lessons

- A drop-shadow filter on a named part is part of that part's own paint, and a soft blob
  with no direction: paint a shadow as its own shape on its own layer, under the part. (The
  colour stage's `duck is blotchy over 80%`, gone with the shadow moved.)
- Put the subject on the brightest light: the duck on the sun's path owns the strongest
  contrast without the sun competing (focal contrast 1.2x; the old pond's sun and reflection
  pulled first in the agent's scores).
- Shapes painted twice (a grey in the values stage, a colour over it) must jitter the same:
  give both the same `seed`, or a grey sliver shows at the edge.
