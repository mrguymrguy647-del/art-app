# Animation, sprite sheets and pixel art: the design for phase 2

This is the plan for phase 2, before any code. The aim: an AI agent makes game sprites and short
animations on a machine with no screen, with the same script format as before, and a person
watches and exports them in the app. Everything here builds on phase 1: a character is a set of
symbols, and an animation changes how its parts are placed over time.

Four ideas carry the whole phase: **animations** (frames and fps), **keyframes** (a value that
changes over frames), **symbol parameters** (one symbol, every pose) and **pixel mode** (crisp
drawing on a small canvas). Everything else is an export or a check.

## 1. Frames and timing

**One canvas, the size of one frame.** A script has one `canvas`, as now. An animation plays the
whole script once per frame; what changes from frame to frame is written as keyframes. So an
animated script is an ordinary script plus timing, and a script with no `animations` is a single
picture, exactly as before.

```json
{
  "palette": { "yellow": "#F2C230", "ink": "#2A2420" },
  "pivot": [64, 120],
  "commands": [
    { "cmd": "canvas", "width": 128, "height": 128 },
    { "cmd": "define", "name": "duck", "params": { "leg": 0, "bob": 0 }, "commands": [ "..." ] }
  ],
  "animations": {
    "idle": { "frames": 4, "fps": 8, "loop": true, "commands": [
      { "cmd": "use", "name": "duck", "translate": [64, 100], "with": { "bob": { "keys": [[0, 0], [2, -3], [4, 0]] } } }
    ] },
    "walk": { "frames": 8, "fps": 12, "loop": true, "commands": [
      { "cmd": "use", "name": "duck", "translate": [64, 100], "with": { "leg": { "keys": [[0, -25], [4, 25], [8, -25]] } } }
    ] }
  }
}
```

- `commands` (top level) run first for every frame of every animation: the canvas, `define`s,
  a background, layers. Each animation's own `commands` run after them.
- `frames` is how many frames; `fps` how fast; `loop` (default false) says the last frame leads
  back to the first. Several animations in one file share the symbols: idle, walk, jump for one
  character. A file with one animation is the normal case for a video.
- **Time is the frame number, counting from 0.** An 8-frame animation renders frames 0 to 7. A
  keyframe at frame 8 is where a loop closes (it equals frame 0 when the loop is smooth). Seconds
  are `frame / fps`; there's no second way to write time.
- Per-frame durations are left out: hold a pose with a `step` keyframe or repeated keys, and
  Duck Forge's `Animation::new(frames, fps)` takes one fps anyway.
- `pivot` (top level, canvas pixels) is the character's anchor point, for the export. Default:
  the bottom centre of the frame, `[width / 2, height]`, so a character stands on its feet and
  switching from idle to walk never moves it up or down in a game.

**Determinism.** A frame is a function of the script and the frame number: no clocks, no
randomness beyond the script's seeds. Random patterns (`scatter`, `jitter`) are the same in every
frame unless their `seed` is keyframed.

## 2. Keyframes

Any number field of a `group`, `use` or `text` command, and a symbol parameter, can be written
as keyframes instead of a number:

```json
"rotate": { "keys": [[0, 0], [6, 20], [12, 0]], "ease": "in-out" }
"translate": { "keys": [[0, [64, 140]], [8, [64, 100], "out"], [12, [64, 104]]] }
"opacity": { "keys": [[0, 0], [10, 1]] }
"colors": { "yellow": { "keys": [[0, "#F2C230"], [4, "#FFFFFF"], [8, "#F2C230"]] } }
"name": { "keys": [[0, "mouth_closed"], [3, "mouth_open"], [5, "mouth_closed"]] }
```

- `keys` is a list of `[frame, value]`, in order. Before the first key the value is the first
  key's; after the last, the last's. Between two keys the value is interpolated.
- **What interpolates:** numbers; lists of numbers (`translate`, `scale`, `origin`, `at`)
  element by element; colours (red, green, blue and opacity each on their own). Anything else
  (a symbol `name`, `mirror`, a text's words) **holds** until the next key: that's how a mouth or
  an eye is swapped at a frame.
- **Easing** names the curve between keys: `linear` (default), `in` (starts slow: t²), `out`
  (ends slow: 1 − (1 − t)²), `in-out` (both: 2t² below the middle, 1 − 2(1 − t)² above) and
  `step` (holds the earlier value until the next key). `ease` on the field sets it for every
  segment; a third element on a key, like `[8, [64, 100], "out"]`, sets the easing *into* that
  key. Five named curves, no custom curves.
- **`shift`** plays the same keys later: `{ "keys": [...], "shift": 4 }` is the motion 4 frames
  late, wrapping round in a looping animation. A walk's far leg, arms, wings and tail are the near
  leg's motion shifted half a cycle, so one set of keys serves all of them:

```json
{ "cmd": "use", "name": "duck", "translate": [64, 100],
  "with": { "near_leg": { "keys": [[0, -25], [4, 25], [8, -25]] },
            "far_leg":  { "keys": [[0, -25], [4, 25], [8, -25]], "shift": 4 } } }
```

- **A camera** for videos is nothing new: the whole scene in a group with keyframed `translate`
  and `scale` (around `origin`, the point the camera looks at):

```json
{ "cmd": "group", "name": "camera", "origin": [320, 180],
  "scale": { "keys": [[0, 1], [48, 1.6]], "ease": "in-out" },
  "translate": { "keys": [[0, [0, 0]], [48, [-120, -40]]], "ease": "in-out" },
  "commands": [ "...the scene..." ] }
```

- Keyframes work on: `group` and `use` (`translate`, `scale`, `rotate`, `origin`, `opacity`,
  `mirror`, the `name` of a `use`, `colors` and `with`), `text` (`at`, `size`, `rotate`,
  `opacity`, `text`) and `seed` on any command. Not on layers, brushes or shapes: move those by
  putting them in a group.
- Inside a symbol, keyframes see the same frame number, so a symbol can animate itself (a
  breathing body), and a `use` of it can animate on top.

## 3. Symbol parameters, and keeping a foot flat

`define` gets `params` with defaults; inside the symbol, `"$name"` stands for the value. A `use`
gives values with `with`; what it doesn't give keeps the default. A parameter can be keyframed in
`with`, which is what makes one symbol cover every pose of a walk.

```json
{ "cmd": "define", "name": "duck", "params": { "leg": 0, "bob": 0, "eye": "eye_open" }, "commands": [
  { "cmd": "group", "name": "body", "translate": [0, "$bob"], "commands": [
    { "cmd": "use", "name": "body" },
    { "cmd": "use", "name": "$eye", "translate": [26, -28] }
  ] },
  { "cmd": "group", "name": "leg", "origin": [4, -2], "rotate": "$leg", "commands": [
    { "cmd": "line", "from": [4, -2], "to": [4, 16], "size": 4, "color": "leg" },
    { "cmd": "use", "name": "foot", "translate": [4, 16], "upright": true }
  ] }
] }
{ "cmd": "use", "name": "duck", "translate": [64, 100], "with": { "leg": 20, "eye": "eye_blink" } }
```

- `"$name"` replaces a whole value: a number, a list, a colour or a symbol name. No arithmetic
  (no `"-$leg"`): the far leg is the near leg's keys with a `shift` of half the cycle.
  A test will show a `use` with parameters paints exactly the pixels of the pose written by hand.
- **Flat feet: `"upright": true`** on a group or use undoes the turning of everything around it.
  The foot still moves with the leg (its position is turned), but it isn't turned itself, so it
  stays flat on the ground. No ankle arithmetic. Mirroring and resizing from outside still apply,
  so a mirrored duck has mirrored feet.

## 4. Pixel mode

A pixel canvas is a normal canvas with `"pixel": true`. Everything about it stays in the same
format; what changes is how commands paint:

```json
{ "cmd": "canvas", "width": 32, "height": 32, "pixel": true }
{ "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 8, "outline": "ink" } }
```

- **Crisp drawing.** Every command paints whole pixels with hard edges: a pixel is in or out of a
  shape (no soft coverage), brushes never go soft or see-through at the edge, text is thresholded,
  transforms snap to whole pixels, and `rotate` and `scale` use nearest pixels (no blur).
  Animating a pixel character moves it by whole pixels, so it stays crisp in every frame.
- **Painted art into pixel art: `supersample`.** With `"supersample": 8`, commands are still
  written in canvas pixels (decimals allowed) but painted 8 times bigger, then shrunk to the
  canvas: each pixel is opaque if more than half of it was painted, and takes the palette colour
  that the most of its painted samples had (not the colour nearest their average: at the edge
  between an outline and a fill, an average is a muddy colour that can snap to the wrong palette
  entry). So the smooth 256-pixel duck becomes a clean 32-pixel duck. `outline`
  adds a one-pixel line of that colour wherever paint meets transparency, after shrinking.
- **A locked palette.** On a pixel canvas, drawing commands must use palette names; a hex colour
  is an error, and `check` warns about any colour outside the palette (also after shrinking, it
  only ever picks palette colours). Transparency is allowed.
- **One new command, `pixels`**, for art that's natural to write as text:

```json
{ "cmd": "pixels", "at": [12, 8], "colors": { "y": "yellow", "k": "ink", "o": "beak" }, "rows": [
  "..kkk...", ".kyyyk..", "kyykyyko", "kyyyyyk.", ".kyyyk..", "..kkk..." ] }
```

  `.` is transparent; every other character is a palette colour from `colors`. Single pixels,
  lines, rectangles, circles and the paint bucket need nothing new: `rect`, `line`, `ellipse`,
  `polygon` and `fill` already draw crisply on a pixel canvas.

## 5. Exports

`art-cli export script.json --out folder` writes, for an animated script:

- **A sprite sheet** `name.png` (see-through background): one row per animation, frames left to
  right, every frame the canvas size, short rows padded. Pixel art is exported at its true size;
  `--scale 8` enlarges with sharp pixels (for people to look at, or for GIFs and videos).
- **`name.json`**, documented in `docs/sprite-sheet-format.md` so Duck Forge can read it:

```json
{ "image": "duck.png", "frame": { "width": 128, "height": 128 }, "columns": 8, "pivot": [64, 120],
  "animations": {
    "idle": { "frames": [0, 1, 2, 3], "fps": 8, "loop": true },
    "walk": { "frames": [8, 9, 10, 11, 12, 13, 14, 15], "fps": 12, "loop": true } } }
```

  Frames are numbered across the grid (row by row), so `sprite.frames(w, h)` cuts the sheet and
  the lists pick from it; `Animation::new(frames, fps)` gets the rest.
- **Single frames** as `name-walk-003.png` with `--frames`.
- **GIF** with `--gif` (built in, no other program needed; one GIF per animation, looping if the
  animation does; the palette is the script's own on a pixel canvas, otherwise the 256 most used
  colours). **MP4** with `--mp4`: through `ffmpeg` if it's installed, otherwise art-cli writes
  the frames and prints the exact `ffmpeg` command to run.
- A single picture (no animations) keeps the old `render`; `render --frame 3`, `--frames 0-7` and
  `--all-frames` paint frames of an animated script as ordinary pictures, for close looks.

## 6. Checking an animation without watching it

- **Frame sheet** (`render --frame-sheet`, every frame or every Nth): all frames in one image
  with their numbers, so a whole walk is judged at a glance.
- **Onion skin** (`render --frame 4 --onion`): frame 4 with frame 3 faint in red and frame 5
  faint in blue behind it, the classic way to see motion in one still.
- **Motion summary** from `check`: for each animation and each named group or `use`, its box per
  frame and how far its middle moves between frames, then warnings: a part that jumps more than a
  quarter of the frame between two frames, a part that leaves the canvas, and for loops, a first
  and last frame that don't meet (so the loop pops). The numbers come before any picture.
- `--gif` is the quick preview for showing a person.

## 7. What's left out, on purpose

Per-frame durations; custom easing curves and bezier handles; arithmetic in parameters; timelines
inside symbols with their own fps; blending between animations; bones and inverse kinematics;
a camera; sound; masks; animating layer settings or brush settings; different frame sizes inside
one file; dithering and sub-pixel motion in pixel mode; a timeline for drawing animation by hand in
the app (phase 3: the app only plays, steps and exports in phase 2).

## 8. Phase 1 leftovers this covers

Parameters and `upright` are the hand-written walk frames and the turning feet. `repeat` gets
`"each": { "fill": ["red", "green", "blue"] }` (a list used one per copy, step-wise) for the
swatch row. `art-cli measure "text" --size 32` and a `"fit": width` field on `text` (shrink to fit)
answer text width. Overlapping outlines stay: draw the body with a single `path` or hide the seam.
