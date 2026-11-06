# The benchmark

Four fixed subjects, each with a one-line brief and the intent to use, so pieces are comparable
across phases. Score each piece three ways; results go in `../gallery/benchmark.md`.

## The subjects

| Subject | Canvas | Brief | Intent to use |
|---|---|---|---|
| Sunset pond | 1920 x 1080 | A duck on a pond at sunset: sky, water with the sun's reflection, reeds and lily pads, one duck. | `subject` "a duck on a pond at sunset", `focal` "duck", `light` "top right" or the sun's position, `mood` "calm, warm", `key_colors` the sky's low colour, the sun, the water's dark |
| Forest at dawn | 1920 x 1080 | A path into a forest at dawn, mist between the trunks, low light from one side, one figure or animal on the path. | `subject` "a path into a misty forest at dawn", `focal` the figure or animal, `light` "left" or "right", `mood` "still, cold, hopeful", `key_colors` the mist, the trunks, the dawn light |
| Character portrait | 1024 x 1024 | A head-and-shoulders portrait of one character against a simple background, lit from one side. | `subject` "a portrait of <the character>", `focal` the face (a named group), `light` a direction, `mood` three words, `key_colors` the skin or fur, the background, one accent |
| Pixel walk | 64 x 64, 8 frames | A pixel-art character's walk cycle, looping, in a locked palette of 6 to 8 colours. | `subject` "<the character> walking", `focal` the character, `mood` one or two words, `key_colors` two of the palette |

## The scoring guide

Six things, each 1 to 5. A sentence for each score, so three scorers mean the same thing.

| | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| **Readability** (does it read at a glance?) | A blur at thumbnail size. | The subject is guessable with effort. | The subject reads; some parts don't. | Every main part reads at thumbnail size. | Reads instantly at any size, from across the room. |
| **Values** (light and dark) | One mid grey. | Two values, muddled. | Three groups, but the lights and darks are spread anywhere. | Three to five clear groups, the lights saved for the subject. | The value plan alone would carry the picture. |
| **Colour** (key and temperature) | Colours fight; nothing belongs together. | A key, with foreign colours in it. | One key, nothing out of it, but flat or monotonous. | One key, warm against cool used on purpose. | The colour says the mood before the subject does. |
| **Focal point** (where the eye goes) | Nowhere, or everywhere. | Somewhere other than the subject. | The subject, after a search. | Straight to the subject. | Straight to the subject, and the rest leads back to it. |
| **Detail** (marks and texture) | Noise everywhere, or none. | Detail spread evenly, or blotches. | Some detail at the subject, the rest dirty or flat. | Detail gathers at the subject; the rest is calm. | Every mark is where it should be and says what the thing is made of. |
| **Overall appeal** (would you keep it?) | No. | Only for the lesson. | It's fine. | Yes. | Gladly, and show it. |

## Three scorers

1. **The numbers**: `quill critique piece.json --score` maps the critique to the first five
   with the fixed mapping below (overall appeal is left blank: numbers can't judge it).
2. **A separate agent** that did not paint the piece, given only the picture and this guide
   (not the script, not the notes), asked for six scores and one sentence each.
3. **The person**: `quill gallery rate name SCORE "comment"` (overall appeal, and the comment
   says the rest). The person's rating outranks every number.

Where the agent's and the person's scores part ways is itself a lesson: write it in the piece's
notes.

## The fixed mapping (the numbers' scorer)

- **Readability**: the share of named parts that still read at 96 px: 5 at 90% or more, 4 at
  75%, 3 at 50%, 2 at 25%, else 1; 3 when there are no named parts.
- **Values**: value groups when squinted: 1 group → 1, 2 → 2, 3 → 4, 4 or more → 5; one less if
  the picture "looks flat" (never under 1).
- **Colour**: 5 when nothing is out of key and two to four hues carry 90% of the colour; 4 when
  nothing is out of key; 3 when the out-of-key blobs cover at most 2% of the picture; 2 at most
  5%; else 1. Not judged (no key colours in the intent) → 3.
- **Focal point**: 5 when the eye goes inside the focal part and the focal point has the
  strongest contrast; 4 when it goes inside the focal part, or within 5% of the canvas diagonal
  of the focal point; 3 within 15%; 2 within 30%; else 1. No focal point in the intent → 2.
- **Detail**: the share of marks within four cells of the focal point over that area's share:
  3x or more → 5, 2x → 4, 1.5x → 3, 1x → 2, else 1; one less when any part is noisy or blotchy
  (never under 1). No focal point → 2.
- **Overall appeal**: blank.
