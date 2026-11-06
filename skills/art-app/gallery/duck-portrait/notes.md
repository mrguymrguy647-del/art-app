# Duck portrait

The duck from the library, painted large (1024 x 1024) rather than placed: every part gets a
living fill, shading, texture and light. `process.png` is the `--snapshots --sheet` render.

## What each tool did

- **Background:** a radial gradient, a faint `noise` layer for unevenness, and a `screen` layer of
  big soft dots (a `scatter` of dabs inside a group with a `blur` filter) for bokeh.
- **Body:** one group with an `outline` (and `rim` light from the top right) and a `shadow`
  filter, holding the tail, body, head, beak and wing. Each shape has a `noise` fill in two
  yellows, so no part is flat; the beak is a two-colour gradient.
- **Shading:** a `"clip": true` `multiply` layer with a purple shade gradient over the lower body
  and under the head, and a `"clip": true` `screen` layer with a warm radial light on the head.
- **Feathers:** three rows of `leaf` tips stamped along the wing inside a `mask`, in three
  colours; `bristle` strokes with `follow` and lightness dynamics for down on the head and back;
  a faint `hatch` for the body's grain.
- **Face:** the eye with a glint, a line along the beak, and a blurred cheek.
- **Finish:** a `"from": "below"` layer with a warm `gradient_map` at 0.2, a vignette, paper.

## Honest notes

- The stamped feathers still read as rows of leaves; shorter, overlapping, more varied stamps
  (and a second, lighter pass) would look more like plumage.
- The rim light is faint against the outline; a lighter outline colour on the lit side, or a
  bigger `rim`, would show it.
- The down strokes on the head are subtle at full size; they'd want a smaller tip and more of
  them.
- The whole picture paints in about 3.5 s with snapshots.

## Proposed lessons

Put forward from the critique and the phase 3 notes; each already stands in `../../lessons.md` because the person agreed to the first set at the phase 4 design review. The before/after numbers for them come with the repaint.

- Shade with big soft shapes and a few directed marks, not a noise fill: the body reads as marble (critique: `duck is blotchy inside: soft blobs with no direction`). Lesson 16.
- Vary stamped marks in length, overlap and lightness: the wing's leaf rows read as rope (not caught by the numbers; the blind read should). Lesson 19.
