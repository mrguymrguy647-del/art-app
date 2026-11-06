# Hokusai, The Great Wave off Kanagawa

**Shapes.** One huge wave curling from the left over the whole picture, its crest breaking
into claws of foam; three long boats low in the water; a small snow-capped mountain (Fuji)
in the centre distance, under the wave's arch; a pale sky.

**Values** (`hokusai-great-wave-values.png`: white 68%, dark 15%, mid 6%, light 7%). Four
groups when squinted, most of the picture pale: the wave's dark blue is the only big dark
shape, so its outline is the picture.

**Palette** (`hokusai-great-wave-palette.png`): cream and pale yellow, two blues, grey-greens.
Two hues, flat colour, every shape outlined. The deep blue is the famous Prussian blue.

**Why it works.** Flat fills with a clean outline (the storybook style in `styles/`): the
critique's "flat part" rule is about shading inside a part that needs it; here flatness is
the style, and the silhouettes do all the work. Check the silhouette view of your own flat
pictures: if a shape doesn't read as a silhouette, no shading will save it.

**Study.** `hokusai-great-wave-study.json` blocks in the big shapes with the palette above (about twenty commands, no detail). `art-cli compare hokusai-great-wave-study.json hokusai-great-wave.*`: values similar: 73; colours similar: 75, the study is cooler and darker. A study that scores 70 or more on values has the picture's value structure; the colour score says whether the palette is in the same key, not whether the painting is any good.
