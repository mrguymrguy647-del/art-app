# Benchmark results

The pieces scored with `exercises/benchmark.md`: the numbers (`art-cli critique piece.json
--score`), a separate agent that saw only the picture and the scoring guide, and the person
(`art-cli gallery rate`). A `-` is a score not given yet. One row per scorer per piece, so a
later phase adds rows and the old ones stay.

| Phase | Piece | Scorer | Readability | Values | Colour | Focal point | Detail | Overall | Date |
|---|---|---|---|---|---|---|---|---|---|
| 4 baseline | sunset-pond-simple | numbers | 5 | 5 | 3 | 5 | 4 | - | 2026-11-06 |
| 4 baseline | sunset-pond-simple | agent | 3 | 3 | 3 | 3 | 2 | 2 | 2026-11-06 |
| 4 baseline | sunset-pond-simple | person | - | - | - | - | - | - | - |
| 4 baseline | sunset-pond | numbers | 4 | 5 | 2 | 4 | 4 | - | 2026-11-06 |
| 4 baseline | sunset-pond | agent | 4 | 3 | 4 | 3 | 3 | 3 | 2026-11-06 |
| 4 baseline | sunset-pond | person | - | - | - | - | - | - | - |
| 4 baseline | duck-portrait | numbers | 2 | 4 | 5 | 5 | 3 | - | 2026-11-06 |
| 4 baseline | duck-portrait | agent | 5 | 4 | 4 | 4 | 4 | 4 | 2026-11-06 |
| 4 baseline | duck-portrait | person | - | - | - | - | - | - | - |
| 4 step 9 | sunset-pond-v3 | numbers | 4 | 5 | 4 | 5 | 5 | - | 2026-11-06 |
| 4 step 9 | sunset-pond-v3 | agent | 4 | 3 | 4 | 4 | 3 | 3 | 2026-11-06 |
| 4 step 9 | sunset-pond-v3 | person | - | - | - | - | - | - | - |
| 4 step 9 | duck-portrait-v2 | numbers | 5 | 4 | 5 | 4 | 4 | - | 2026-11-06 |
| 4 step 9 | duck-portrait-v2 | agent | 4 | 3 | 4 | 4 | 3 | 3 | 2026-11-06 |
| 4 step 9 | duck-portrait-v2 | person | - | - | - | - | - | - | - |
| 4 step 9 | forest-at-dawn | numbers | 5 | 5 | 4 | 5 | 3 | - | 2026-11-06 |
| 4 step 9 | forest-at-dawn | agent | 4 | 4 | 4 | 5 | 4 | 4 | 2026-11-06 |
| 4 step 9 | forest-at-dawn | person | - | - | - | - | - | - | - |
| 4 step 9 | sunset-pond | agent (step 9 judge) | 3 | 3 | 3 | 2 | 2 | 2 | 2026-11-06 |
| 4 step 9 | duck-portrait | agent (step 9 judge) | 4 | 3 | 3 | 3 | 2 | 2 | 2026-11-06 |

The pieces: `sunset-pond-simple` is `examples/sunset-pond-simple.json` (the phase 3 example,
flat fills, one duck), `sunset-pond` and `duck-portrait` are this folder's phase 3 pieces
(critiqued in phase 4), and `sunset-pond-v3`, `duck-portrait-v2` and `forest-at-dawn` are the
step 9 pieces painted with the full phase 4 process. The baseline agent and the step 9 agent
were different readers; the step 9 reader also re-scored the two phase 3 pieces (below) so a
before/after pair has one judge.

## What the agent said (one sentence each)

**sunset-pond-simple.** Readability 3: the sun, hills and water read at once, but the duck is a
small flat blob and the reeds vanish when the picture is small. Values 3: clear groups, but the
brightest light is the sun, not the subject. Colour 3: one warm key with blue and purple, but
every area is a flat fill and the two green ovals sit a little outside it. Focal point 3: the eye
goes to the big pale sun first and finds the duck second. Detail 2: the pale dabs are spread
evenly over the whole pond and the duck has no texture. Overall 2: a clean diagram of a sunset
pond rather than a painting.

**sunset-pond.** Readability 4: duck, sun, hills, cattails and lily pads read at thumbnail size;
the clouds become vague blobs and the duck's reflection is a muddle. Values 3: three to four
groups, but the strongest light is the sun and its reflection, not the duck. Colour 4: warm
orange sky against cool violet water, on purpose; the lily pads a touch loud but in key. Focal
point 3: the sun and its reflection pull first, the duck second. Detail 3: the cattails and the
corner grass carry the most marks, the duck only a wing oval. Overall 3: a pleasant storybook
scene, but the heavy blur on everything looks smeared and the flat outlined duck doesn't sit in
the painterly background.

**duck-portrait.** Readability 5: a big yellow duck with beak and eye reads instantly. Values 4:
the light duck against a mid grey ground with a brighter rim top right, lighting subtle. Colour
4: warm yellow and orange against a cool grey-blue bokeh, though the background reads more grey
than blue. Focal point 4: straight to the head and the eye; the beak points out of frame. Detail
4: feather marks, the eye highlight and the cheek blush gather at the subject; the mottled body
texture is a little random. Overall 4: a charming, finished-looking duck, held back by the
pasted-on beak and the head circle sitting on the body without a neck.

## Where the numbers and the agent part ways

- **The numbers over-rate the simple pond** (5, 5, 3, 5, 4 against the agent's 3, 3, 3, 3, 2).
  Flat fills survive a thumbnail, four value groups count as a plan, and the focal point gets 5
  because the duck is a one-part picture with the strongest contrast. The numbers measure
  whether the plan is there; the agent sees that nothing was painted. This is the gap the
  guide warns about: the numbers are warnings, never goals.
- **The numbers under-rate the duck portrait's readability** (2 against 5). Two of its three
  named parts (`bokeh`, `cheek`) are too small to count at 96 px, so "1 of 3 parts read"; the
  agent sees one big duck that could not be clearer. Readability counts named parts, so name
  the parts that must read, not the trimmings; a lesson for the process, not a fix to the
  measure.
- **Both scorers agree on the duck portrait's body** (detail 3 from the numbers for the
  blotchy body, "a little random" from the agent), **and on the pond's focal point** (the sun
  wins over the duck): the critique's "the strongest contrast is elsewhere" finding and the
  agent's "the sun and its reflection pull first" are the same observation.
- **Step 9 repaints both gallery pieces** with the full process, and the rows it adds here
  are the first real measure of whether phase 4 made the painter better.

## Before and after, one judge (step 9)

The step 9 judge scored the phase 3 pieces and their repaints in one sitting, with the same
guide:

| Piece | Readability | Values | Colour | Focal point | Detail | Overall |
|---|---|---|---|---|---|---|
| sunset-pond (phase 3) | 3 | 3 | 3 | 2 | 2 | 2 |
| sunset-pond-v3 | 4 | 3 | 4 | 4 | 3 | 3 |
| duck-portrait (phase 3) | 4 | 3 | 3 | 3 | 2 | 2 |
| duck-portrait-v2 | 4 | 3 | 4 | 4 | 3 | 3 |

- **What moved:** the focal point (the pond's duck on the sun's path instead of under the
  sun; the portrait's head without the busy wing pulling first), colour (a chosen warm
  against cool instead of a neutral grey or a flat gradient) and detail (marks at the subject
  instead of blotches everywhere): one or two steps each.
- **What did not:** values stayed at 3 on both. The judge's sentence both times: the brightest
  spots are the sun, the clouds or the bokeh, not the subject (lesson 3). The numbers give
  values 5 and 4 because they count groups, not where the lights are.
- **Overall went from "only for the lesson" to "fine"**, not to "yes". The forest, painted
  fresh with the process, got "yes" (4) and the reader's one complaint was the deer's drawing.
