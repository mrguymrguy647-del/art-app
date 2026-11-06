# Lessons

Short rules, one line each, each with the piece or picture that shows it. Read them before
painting; read the two or three nearest gallery pieces after.

**How a lesson gets in (or out).** Agents never edit this file directly. A lesson is added
when (a) the person agrees, or (b) a reviewer agent that did not paint the piece proposes it in
that piece's `notes.md` under "Proposed lessons" *and* the critique numbers of a before/after
pair improved on the thing the lesson claims; the lesson then carries the piece and the number.
A lesson is retired the same way. The first lessons below come from phase 3's notes and phase
4's critique and references, agreed with the person at the design review.

## Values first

1. Decide three to five big value shapes before any colour; if the squint shows one grey mush,
   fix values, not detail. (`gallery/sunset-pond` views: four groups; Friedrich's Wanderer: two.)
2. The focal point gets the strongest contrast in the picture; check `focal contrast` says so.
   (`gallery/sunset-pond`: the duck 1.5x the sun's reflection.)
3. Save the lightest light and the darkest dark for the focal point. (Vermeer in `references/`:
   the face and the pearl; the rest sits in the darkest band.)
4. Mostly mid-greys reads flat unless one small accent carries it. (Monet's Impression,
   Sunrise is "flat" by the numbers and works because of one orange sun: most pictures don't.)
5. A night or a dusk is two dark values and a little light spent sparingly. (The rocket
   reference: 93% dark, 2% light, and the light is the subject.)

## Shapes and edges

6. A shape that does not read as a silhouette will not read with shading on top; check the
   silhouette view. (`gallery/sunset-pond`: the duck's silhouette; Hokusai's wave.)
7. Run backgrounds and big shapes past the canvas edge; a shape that stops at an edge leaves a
   sliver. (`examples/two-ducks.json`.)
8. Blur the fill, not the silhouette: a blur on a whole group softens its edge and lets the
   background through, which reads as haze. (`gallery/sunset-pond` far hills: the haze band;
   `mistakes.md`.)
9. Give every soft shape a crisp side somewhere, or it is a smudge. (`gallery/sunset-pond`
   clouds: soft blobs with no direction, "blotchy" by the critique.)
10. Outline a character once, around the whole group, not part by part. (`gallery/duck-portrait`:
    one `outline` on the group; `library/duck.json`.)

## Colour

11. Pick a key of two to six palette colours and keep every saturated colour within 40 degrees
    of one, or between two of them; what is outside reads as foreign. (`gallery/sunset-pond`:
    the green lily pads in an orange and blue key; `mistakes.md`.)
12. One hue in many values beats many hues in one value. (The coffee cup reference: one warm
    key, five values; the chelsea cat: eight browns.)
13. Cool key, warm accents at the focal point, or the reverse: the oldest trick. (The rocket
    reference; Van Gogh's Starry Night: blue over 80%, yellow accents.)
14. A pale colour is a value, not a hue: do not count it as part of the key. (The critique
    ignores key colours under 0.35 saturation; `gallery/sunset-pond`'s cream sun.)
15. The mood pass (gradient map, vignette) shifts every hue; check `out of key` after it, not
    before. (`gallery/sunset-pond`: the pads went yellow-green under the warm pass.)

## Texture and marks

16. Shade with big soft shapes and a few directed marks, not noise everywhere. (`gallery/
    duck-portrait` body: soft blobs with no direction; `mistakes.md`.)
17. Marks have a direction and an edge; a noise fill has neither. Feathers, grass and strokes
    follow the form. (The chelsea cat's fur; Van Gogh's strokes; `library/grass.json`.)
18. Texture changes where the material changes; it is what tells the eye what things are.
    (The coffee cup: coarse wood, smooth porcelain.)
19. Stamped feathers in rows read as rope; vary length, overlap and lightness, and add a second
    lighter pass. (`gallery/duck-portrait` wing; `mistakes.md`.)
20. A few bright sparkles are marks, not noise; a field of fine grain with no marks is dirt.
    (The critique's grain is a median per cell for this reason.)
21. A light source may be flat; everything else that is flat needs a gradient, a fill or a mark.
    (`gallery/sunset-pond` sun; `examples/two-ducks.json` duck is flat and shows it.)

## Composition

22. Not dead centre, unless it is the point. (Friedrich's Wanderer is centred on purpose; a
    duck at the centre of a pond is not.)
23. The horizon off the middle. (`gallery/sunset-pond`: 65% down.)
24. Put the detail where the eye should go; a background earns ten commands, a face a hundred.
    (`gallery/duck-portrait`: 16% of the marks within four cells of the focal point, an area of
    8%, and still spread too thin.)
25. A focal part cut by the canvas edge looks like a mistake unless the cut is bold. (The
    critique warns; Hokusai's wave is cut boldly.)

## Process

26. Write the intent first; the picture is judged against it, not against what you meant later.
    (Every gallery piece's `script.json`.)
27. Judge at thumbnail size before judging detail; if it does not read at 96 px it does not
    read. (`art-cli views`, the thumb panel.)
28. Numbers are warnings, never goals: never trade a better-looking picture for a better number,
    and the person's rating outranks every number. (`gallery/index.json` ratings.)
29. The blind read is the real test: if a reader who was not told the intent names another
    focal point or mood, the picture does not read yet. (`gallery/*/blind-read.md`.)
30. Write honest notes: what each tool did and what is still weak; the next painter learns
    from the weak list. (`gallery/sunset-pond/notes.md`.)
