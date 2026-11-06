# Phase 4 design: the painter's mind

Phase 3 gave the scripts strong tools. The first gallery pieces show the gap that's left: the
tools were used without judgment (smudges for clouds, green pads in an orange sunset, a marbled
duck, rope feathers). An agent painting headless can check that its script is valid, not whether
the picture reads. This phase gives it **senses** (tools that look at pixels and report), a
**process** (what to check at each stage), and a **memory** (one folder every agent learns from).

Three rules shape everything below:

1. **Senses only look.** They read pixels and the script's intent and print a report; they never
   change a painting. They live in the engine as plain functions on an `Image` (no UI, no window),
   and `art-cli` wraps them.
2. **Deterministic.** The same picture gives the same report, to the pixel and the number. No
   randomness, no floating-point order games: integer histograms, fixed grids, fixed thresholds.
3. **Numbers judge what numbers can judge.** Contrast, balance, where detail gathers, whether a
   colour is out of key, whether a shape survives shrinking: those are measurable. Beauty, charm,
   whether a cloud looks like a cloud: not. The report says so, every time, and points to the
   views, the blind-read test and the person's rating.

## 1. The intent block (step 1)

An optional `"intent"` at the top of a script says what the painter means. It paints nothing.

```json
"intent": {
  "subject": "a duck on a pond at sunset",
  "focal": "duck",
  "light": "top right",
  "mood": "calm, warm, end of the day",
  "key_colors": ["sky_low", "sun", "water_dark"]
}
```

| Field | |
|---|---|
| `subject` | A short sentence: what the picture is of. |
| `focal` | Where the eye should go: `[x, y]`, or the name of a named group or `use` (its box's middle). |
| `light` | Where the light comes from: a direction word (`"top right"`, `"left"`, `"behind"`), degrees clockwise (0 = right), or a point `[x, y]` (a sun, a lamp). |
| `mood` | A few words, for people and for the blind-read comparison. |
| `key_colors` | Palette names (2 to 6) that set the picture's key: the colour tools judge "out of key" against these. |

`check` prints it; `views` marks the focal point; `critique` compares the picture with it. A
script without an intent still works; the report then says "no intent given" where it would have
compared, and the process says to add one first.

## 2. Views: the artist's tricks as pictures (step 2)

`art-cli views script.json` paints the picture (frame 0, or `--frame n`) and saves one sheet,
`name-views.png`, seven panels with titles, so an agent judges everything in one look:

| View | How it's made | What it's for |
|---|---|---|
| picture | as painted | the reference for the rest |
| grey | luminance (0.2126 R + 0.7152 G + 0.0722 B on linear-ish sRGB values, the same formula everywhere) | values without colour |
| squint | the grey picture blurred by 1/40 of its width (the three-box blur from phase 3) | the big shapes: what reads from across the room |
| thumb | the picture shrunk to 96 px wide (box filter), shown enlarged with sharp pixels | what reads at a glance |
| flip | mirrored left to right | composition mistakes the eye has stopped seeing |
| silhouettes | each named group or `use` filled solid in its own flat colour on white, with its name; parts are taken from the player's report (the same boxes `check` shows), painted again through the scratch mechanism filters use, so a part's silhouette is exactly what it painted | whether shapes read on their own |
| heat | the focal heat map (below), with the intended focal point marked by a ring | where the eye goes |

Each view alone: `--gray`, `--squint`, `--thumb`, `--flip`, `--silhouette`, `--heat` save that one
file (`name-gray.png`, ...). `--out` chooses the folder.

**The heat map.** Four maps on a grid of 32 x 32 cells (so a 1920 x 1080 picture has 60 x 34
pixel cells; the grid is the deterministic unit everywhere), each normalised to 0..1 over the
picture:
- *contrast*: the cell's value range (95th minus 5th percentile of its luminance);
- *edges*: the mean gradient magnitude (Sobel on the grey picture);
- *saturation*: the mean HSV saturation times value (grey and dark cells score low);
- *detail*: the energy left after the squint (grey minus squint, absolute, mean).

Heat is their weighted sum (contrast 0.35, edges 0.3, saturation 0.15, detail 0.2), smoothed over
the 8 neighbours. The peak cell is "where the eye goes". The weights are fixed; they are a
model of attention, not the truth, and the report says so.

## 3. Critique: the artist's eye in numbers (step 3)

`art-cli critique script.json` paints the picture and prints about 15 lines. Each finding has a
place (canvas pixels or the part's name), the number behind it, and a hint. The numbers:

**Values.** Luminance histogram in 10 bins. *Value groups*: runs of bins with at least 4% of the
pixels, separated by bins under 1% → "3 clear value groups (dark 31%, mid 48%, light 21%)" or
"mostly mid-greys (72% in bins 4-6): the picture looks flat". *Focal contrast*: the heat-grid
contrast at the focal point compared to the median cell: "the focal point's contrast is 1.4x
the rest (good: 1.5x or more stands out; under 1x is lost)".

**Focal point.** The heat peak compared to the intent: distance in pixels and whether the peak
lies inside the focal part's box: "the eye goes to the sun's reflection (x 1300, y 820); the
intent says the duck (x 1000, y 880), 320 px away". With no intent: just where the eye goes.

**Colour.** Hue histogram (24 bins, saturation-weighted, only pixels with saturation above 0.15
and value above 0.1): *how many hues* carry 90% of the weight; *warm/cool balance* (hue 330-60
warm, 150-270 cool, the rest neutral) as percentages; *out of key*: cells whose dominant hue is
more than 50 degrees from every key colour's hue and from the light's hue, with saturation above
0.35, grouped into blobs; each blob reports its place, hue name ("green"), share of the picture
and the nearest part: "a saturated green at x 500, y 985 (lily pads, 2.1% of the picture) is out
of key with the orange light". Hue names come from a fixed 12-name wheel.

**Texture.** Per named part (its silhouette from the views step) and per 32 x 32 cell: *dirty*:
detail energy inside the part above 0.12 of its value range while the part is bigger than 1% of
the canvas ("the duck's body is noisy inside: fine texture over 78% of it; shading wants big
soft shapes and a few marks, not noise everywhere"); *flat*: cells with value range under 0.02
making up more than 25% of a part ("the water is flat over 61%"); *detail spread*: the share of
detail energy within one grid-radius of the focal point versus the rest ("detail gathers at the
focal point: 34% there, 66% elsewhere; aim for half or more there").

**Readability.** At 96 px wide: each named part's silhouette shrunk; *merges* when the part's
mean value differs from the ring of pixels around it by under 0.12 and its hue by under 30
degrees ("the far hills merge with the sky at thumbnail size"); *too small* when the part covers
under 4 thumbnail pixels; the number of parts still separable.

**Composition.** *Weight balance*: the heat map's centre of mass as a fraction of the width and
height ("visual weight sits at 62% across, 58% down: a little right-heavy"); *dead centre*: the
focal point within 5% of the canvas centre both ways; *cut by the edge*: a focal part's box
touching an edge; *horizon*: if the intent or a part named "horizon" gives one, whether it sits
within 5% of the middle.

**Animation.** `--frame n` critiques one frame; `--frames a-b` prints one short line per frame
(value groups, focal distance, out-of-key share) and a summary of the worst frame; without
either, frame 0.

The report always ends: *Numbers can't judge beauty: look at the views, run the blind-read test,
and ask for a rating.*

**What the numbers can and can't judge.** They judge contrast, balance, where detail and colour
gather, whether a colour is foreign to the key, whether a shape survives shrinking, whether the
focal point is where the painter wanted it. They can't judge whether the clouds look like
clouds, whether the duck has charm, whether the mood is right, whether the picture is finished
or merely busy. A picture can pass every number and be dull; a masterpiece can fail several. The
report's job is to catch the mistakes a person spots in a second, so the agent's time goes to
the rest.

## 4. References: learning from good pictures (step 4)

Three tools on PNG or JPEG files (no scripts needed), all deterministic:

- `art-cli palette image.png`: the picture shrunk to at most 256 px wide, then its colours
  reduced to 8 by median cut (split the box with the biggest range, always at the median, in a
  fixed channel order), sorted by share. Output: a palette block PNG (`image-palette.png`, one
  swatch per colour with its hex and share) and the same as a script palette in JSON on the
  screen (`"c1": "#C4583C"` ...), ready to paste. `--colors 12` for more.
- `art-cli values image.png`: luminance posterised into 5 greys (fixed bands) → `image-values.png`,
  and the percentages, plus "value groups" as in critique: the light/dark structure in words.
- `art-cli compare mine.png ref.png`: both shrunk to 192 px wide, side by side with their 5-grey
  versions underneath (`compare.png`), and two numbers 0..100: *value structure*: histogram
  intersection of the 5-grey shares (50%) plus the correlation of the two 4 x 4 grids of mean
  values (50%); *colour*: intersection of the saturation-weighted hue histograms. With the words
  for them ("values similar: 78; colours different: 41, yours is cooler").

These serve the references folder (section 5): every reference gets its palette and values
pictures, and a study script is compared with it.

## 5. The learning folder: `skills/art-app/` (step 5)

One folder holds everything an agent learns from, so it can be copied or installed as a skill:

```
skills/art-app/
  SKILL.md           the short entry point (under 200 lines)
  lessons.md         at most ~40 short rules, each linked to the gallery piece that shows it
  mistakes.md        before/after pairs with pictures
  styles/            storybook.md, painterly.md, pixel.md
  references/        pictures we may use, each with source, licence, breakdown and a study script
  exercises/         a short course, plus benchmark.md
  library/           parts to include (phase 3)
  gallery/           finished pieces: script, intent, views, critique, blind read, process, notes, rating
```

**lessons.md** rules: one line each, concrete, with the piece that shows it ("Shade with big soft
shapes, not noise: duck-portrait v1 → v2"). Agents never edit it directly. A lesson is added when
(a) the person agrees, or (b) a reviewer agent that didn't paint the piece proposes it in a
gallery `notes.md` "Proposed lessons" section *and* the critique numbers of the before/after
pair improved on the thing the lesson claims (the file says this, and each lesson carries the
piece and the number that supported it). Lessons can be retired the same way.

**mistakes.md** starts with the real ones from phase 3's notes: smudge clouds, out-of-key lily
pads, dirty texture on the duck, rope feathers, the haze band, the pillar reflection. Each is a
before picture, an after picture, what was wrong in one line and the fix in one line.

**styles/** each: palette rules (how many hues, saturation range, value range), brush settings
that fit, what to avoid, and which gallery pieces show it.

**references/** only pictures with the right to use them: the person's own photos and drawings,
public domain, CC0. `references/index.json` lists each with `source`, `licence`, `author`,
`tags`; each gets `palette.png`, `values.png`, a `breakdown.md` (shapes, values, palette, why it
works) and `study.json`, a script that recreates it in Art App's terms, compared with
`art-cli compare`. Never copyrighted artwork or other people's characters; the index is the
only list and a test checks every entry names a licence from a fixed allowed set.

**exercises/** a short course: a value study (three greys, a landscape), a colour study (one key,
five colours), five kinds of clouds, three kinds of water, one character in three poses. Each
says what to do, what a good result looks like (the numbers critique should show, and a picture),
and which lessons it practises.

**gallery/** each piece: `script.json` (with intent), `final.png`, `views.png`, `critique.md` (the
report as printed), `blind-read.md` (the questions, the other agent's answers, and whether they
match the intent), `process.png`, `notes.md` with a required "Proposed lessons" section, and a
`rating` field in `index.json` for the person (1 to 5, with a comment; empty until given).
`art-cli gallery add piece.json --name name --out skills/art-app/gallery` paints the piece,
runs views and critique, writes the folder with a notes.md skeleton (including the blind-read
questions to ask) and adds it to `index.json` with its tags from the intent and the commands it
uses. `index.json` keeps tags and ratings so an agent can pick the 2 or 3 most relevant pieces.

## 6. The painting process (step 6)

In the skill and the AI guide, the stages a good painter follows, with what to check at each:

| Stage | Do | Check |
|---|---|---|
| 1. Intent | Write the intent block and the palette (8-15 colours, named for their jobs). Read `lessons.md` and the 2-3 nearest gallery pieces. | `check` prints the intent back; the key colours exist. |
| 2. Thumbnail | At `--scale 0.1`: three to five big value shapes, nothing else. | views: the thumb reads; critique: 3+ value groups, the focal point near where it's wanted. |
| 3. Values | Full size, still mostly grey or one hue: light and dark decided for every shape. | critique values and readability: no merging at thumbnail size. |
| 4. Colour blocking | Flat local colours in key, back to front. | critique colour: nothing out of key; warm/cool as the mood wants. |
| 5. Light and form | Clipped shadow and light layers, the light's direction kept. | views: the squint shows form; critique: focal contrast 1.5x or more. |
| 6. Detail at the focal point | Texture, edges, marks, mostly within one grid-radius of the focal point. | critique texture: detail gathers at the focal point; nothing dirty; nothing flat that shouldn't be. |
| 7. Finish | Glow, shadow, mood pass, paper. | views and critique again; then the blind-read test. |

After each stage: `views` and `critique`, and fix before moving on. **The detail budget:** a
finished 1080p painting is usually hundreds of commands, and most of the detail commands are at
the focal point; a background earns ten commands, a face a hundred.

**The blind-read test** before calling a painting done: give only the picture to a separate
agent (or a fresh session) and ask three questions without telling it the intent: *What do you
see? What's the focal point? What mood?* Compare with the intent; if the answers don't match,
the picture doesn't read yet. Record the questions, the answers and the verdict in the piece's
`blind-read.md`. The command line can't run another agent, so this is a procedure with a file
format, not a tool.

After painting: `notes.md` (what each tool did, what's still weak) and its "Proposed lessons".

## 7. Faster animation (step 7)

Every frame repaints everything: the 1080p sunset loop takes about 7 minutes for 48 frames.
Most of a frame is the same as the last. The rule: **a layer that only frame-independent
commands paint on is painted once and copied into every other frame.**

A command is *frame-dependent* if it, or anything inside it (a group's commands, a used symbol's
commands, a `with` value, a mask, a filter), contains keyframes, or is inside the animation's own
commands. A layer is *dynamic* if any frame-dependent command paints on it, or if any command
reads pixels from it or into it in a way that could differ between frames: `fill` (the bucket)
sampling visible layers, a `{"layer": ...}` mask, `"from": "below"`, a `filter` on it after a
dynamic command. Everything else is *static*. When frames are painted in sequence (`render
--all-frames`, `--frames`, `export`, the app's animation panel), frame 0 is painted in full; for
each later frame the static layers' tiles are copied from frame 0 and the static commands that
paint only on static layers are skipped. Commands that set state (brush, layer selection,
define) still run, so everything after sees the same state. The pixels are exactly the same by
construction, and a test proves it on every example animation and the gallery loop: the same
PNGs with and without reuse. Measure before and after on the pond loop and the duck walk; the
numbers go in `decisions.md`. The pond's sky drifts (keyframes), so it stays dynamic; the hills,
bank, pads and finishing layers don't, so the win should be large but not total.

## 8. A benchmark (step 8)

`exercises/benchmark.md` fixes four subjects: a sunset pond (1920 x 1080), a forest at dawn
(1920 x 1080), a character portrait (1024 x 1024), a pixel-art character's walk (64 x 64, 8
frames). Each has a one-line brief and an intent to use, so pieces are comparable across phases.

Scoring guide, each 1 to 5 with a sentence of what each score means: readability, values,
colour, focal point, detail, overall appeal. Three scorers: the critique numbers mapped to the
first five (fixed mapping in the file, so it's reproducible), a separate agent judging from the
picture with the guide (not the one that painted it), and the person's rating. Results go in
`gallery/benchmark.md` as a table: phase, piece, the three sets of scores, the date; so we can
see whether each phase makes the paintings better, and where the agent's and the person's
judgments part ways (which is itself a lesson).

## 9. Prove it (step 9)

Following the full process with views, critique and the blind-read test at every stage: repaint
the sunset pond (v3) and the duck portrait (v2), fixing what the senses find; the old versions
go into `mistakes.md` as "before". Paint the forest at dawn the same way. Each goes into the
gallery with all its parts, and the report to the person has the pictures, the views sheets, the
critique reports, the blind-read answers and an honest list of what the senses still miss.

## What this phase doesn't do

No new painting tools (phase 3 has enough), no automatic fixing (senses report, people and agents
decide), no model calls from `art-cli` (the blind read and the benchmark judge are other agents
by procedure), no learned weights (the heat map and thresholds are fixed numbers written in this
document and in `decisions.md`, changed only by a decision).
