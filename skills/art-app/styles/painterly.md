# Painterly

Soft edges where things turn away, crisp edges where they face the light, texture that follows
form, a mood pass that unifies: the look of a painting.

**Palette rules.** 8 to 15 colours, named for their jobs (`sky_low`, `water_dark`). One key of
three to five hues; everything saturated stays within the key. Values spread over five bands,
with the lights saved for the focal point.

**Brush and fill settings that fit.** `brush` with a `tip` (`bristle`, `chalk`, `leaf`,
`grass`) and `dynamics` (`follow`, `size`, `opacity`, `lightness`); `stroke`, `path` and `stamp`
for marks that follow the form; `noise` fills only as a base under marks, never as the shading;
`"clip": true` `multiply` and `screen` layers for shadow and light; `filter` `blur` on far
layers' fills (masked, so silhouettes stay crisp); a `gradient_map` at 0.2 and `paper` at 0.18
to finish.

**What to avoid.** Noise fills as the whole shading (blotches with no direction: the marbled
duck); blurring a whole group (the haze band); stamps in neat rows (rope feathers); glows
everywhere; more than one light.

**Pieces that show it.** `gallery/sunset-pond`, `gallery/duck-portrait` (and their mistakes in
`mistakes.md`); Vermeer, Monet and Van Gogh in `references/`.

**What the numbers say.** No part `blotchy` or `noisy`; `focal contrast` strongest at the focal
point; detail gathering at the focal point; soft edges only where meant (no `edge is soft and
the lighter background shows through` on a solid thing).
