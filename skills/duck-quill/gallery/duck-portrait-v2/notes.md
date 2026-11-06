# Duck portrait v2

The duck portrait painted again with the phase 4 process, in stage order (layers first, then
values, colour, light, detail, finish; `process.png` shows them). 1024 x 1024. `before` in the
index points at `../duck-portrait/script.json`, the phase 3 painting it answers: the marbled
noise body and the rope-row wing are the two mistakes it was painted to fix (`mistakes.md`).

## The stages, and what the senses said at each

- **Values**: grey shapes: the background's radial, the duck (tail, body, neck, head, beak,
  wing), the eye. The thumb read as a bird.
- **Colour**: one flat yellow for the whole bird, the beak in two mandibles, the wing a
  gradient; the background a bluer radial (the old one read grey) and the bokeh. The critique
  said the head, beak and wing (named at first) "merge with their surroundings": parts inside
  a part of the same colour merge by construction, so only `duck` stays named and the focal
  point is a point on the head. It also said `head is flat over 100%`: the critique judged a
  part's flatness on its own paint, and the process puts the shading on clipped layers above
  it. The critique was changed: a named part's flatness and light-and-dark side are judged on
  the picture inside it (decisions 78).
- **Light and form**: a clipped multiply shade (a gradient from the top right, three soft
  shapes under the head, under the wing and at the beak's root, a radial on the head's lower
  left), a clipped screen light from the top right, the duck's shadow on the background. 3
  value groups; the eye goes to the eye.
- **Detail**: a few hundred short strokes that follow the body on a clipped layer, more at
  the head; the wing's tiled feathers under four tapered primaries; the eye with an iris
  ring, a highlight and a lid line; the beak's parting line, highlight and nostril; the cheek.
  22% of the marks within four cells of the focal point.
- **Finish**: a cooler mood pass at 0.12, vignette, paper. Final critique: no texture, edge or
  colour finding; focal contrast 1.2x; 1 of 1 parts read. Numbers: readability 5, values 4,
  colour 5, focal point 4, detail 4.
- **Blind read**: reads (`blind-read.md`): "a cartoon-like yellow duck ... my eye goes first
  to the duck's eye and beak ... gentle, soft and childlike".

## What each tool did

- **Body:** one `duck` group with the whole bird's `outline` (dark brown, with a warm `rim`
  from the top right) around tail, body, neck polygon, head, beak and wing; flat fills.
- **Shade and light:** `clip` layers on `multiply` and `screen`, gradients and blurred soft
  shapes.
- **Feathers:** `scatter` strokes (kind `stroke`, short, angled with the form) in six areas on
  a clipped layer; the wing is a gradient shape with four staggered rows of `leaf` stamps along
  curves that follow it (`follow: false`, tips pointing to the tail), close in colour to the
  wing, and four tapered `path` primaries.
- **Face:** the eye as five ellipses, three paths, a blurred cheek.

## Honest notes

- The wing is still the weakest part. Rows of leaf stamps read as rope (v1), straight rows of
  bigger ones read as rope again, scattered leaves read as a pile of leaves, overlapping
  ellipses read as coins; the tiled rows now read as "a muddy darker smear with scribbled
  lines" to the blind reader. A wing wants its feathers drawn as shapes with a lit edge, not
  stamped.
- The reader sees the head "placed on" the body despite the neck polygon: the cast shadow
  under the head is what joins them, and it reads as a gap. A lighter shadow and a lit edge
  on the neck would help.
- The hatching is even over the whole bird; the reader called it a filter. Fewer strokes on
  the head's lit side, more in the shadow, would read as fur.
- The beak's nostril dot "is a little arbitrary".
- Painting time about 1 s.

## Proposed lessons

- Name the parts that must read against their surroundings, not the parts of one thing in one
  colour: a `head` inside a yellow `duck` merges at thumbnail size by construction and costs
  the readability score for nothing.
- Stamped feathers in any arrangement read as stamps; a wing wants a few drawn feathers with a
  lit edge and a shadow edge, like the primaries, more than a texture.
- A cast shadow between two parts of one body (under the head) reads as a gap unless its edge
  on the lit side is soft and light.
