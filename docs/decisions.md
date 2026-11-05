# Design decisions

Short notes on *why* things are the way they are. Newest at the bottom.

## 1. Rust instead of C++
The first version used C++ with Qt and CMake, and getting it to build on
Windows was too painful. With Rust there is one installer, and Cargo (Rust's
build tool) downloads every library and builds everything with one command.
The result is a single `ArtApp.exe` with no extra files to copy next to it.

## 2. Windows only, for now
Keeps the first version simple. The engine is plain Rust and egui also
runs on other systems, so adding them later would not need a rewrite.

## 3. egui for the interface
egui (with `eframe`, which opens the window) is pure Rust, easy to read and
has panels, sliders and colour pickers built in. It draws with the graphics
card and handles touch and pinch. Its "immediate mode" style means the UI
code simply describes what the screen should show right now, every frame.

Trade-off: it looks less like a standard Windows program than Qt would.
Painting apps usually use their own dark theme anyway.

## 4. We read the pen from Windows Ink ourselves
The window library under egui (winit) passes the pen on as a "touch" with
pressure, but drops tilt, rotation, the eraser end and the side button.
So `pen_input.rs` adds a *message hook*: it sees each Windows message just
before winit does, asks Windows for the full pen details, and then lets
winit carry on as normal. The mouse still comes from egui.

## 5. The engine has no UI code
`crates/engine` doesn't depend on egui or Windows. That means:
- we can test brush maths, tiles and undo with `cargo test`, no window needed;
- the engine can't accidentally depend on how the UI works.

The app converts Windows' pen data into the engine's own `PenSample` type.
Rust has tests built in, so we don't need a test library. They live at the
bottom of the file they test, in a `mod tests` block.

## 6. `run.bat` builds in "release" mode
Release builds are optimised, so the app is fast and opens quickly; the
build takes a little longer. Release builds also don't open a console
window next to the app.

## 7. Placeholder icon
A brush stroke that gets thicker in the middle, like pressing harder with a
pen. It's in `crates/app/assets/` and can be swapped any time.

## 8. Pixels live in 256×256 tiles
Each layer is a set of 256×256 tiles, and a tile only exists once
something is painted on it. An empty 8000×8000 canvas uses almost no
memory, and the screen can update one changed tile instead of the whole
picture. Pixels are 8-bit RGBA with premultiplied alpha. The pixel type is
a Rust trait (`Pixel`), so a 16-bit type can be added later without
rewriting tiles and surfaces.

## 9. Each tile is its own texture on the graphics card
Zooming, panning and rotating only change where the tile textures are
drawn, which the graphics card does very quickly. When a tile's pixels
change, only that tile is sent again.

## 10. We make our own smaller previews for zooming out
egui's graphics backend doesn't make smaller versions of textures
("mipmaps"). Without them, a zoomed-out canvas shimmers and looks grainy.
So the engine makes them: below 50% zoom a tile is sent at 128×128, below
25% at 64×64, and so on. It looks smoother and uses less memory on the
graphics card.

## 11. 100% zoom means real screen pixels
Windows display scaling (e.g. 150%) makes interface units bigger than
screen pixels. The zoom shown in the status bar counts real pixels, so at
100% one canvas pixel is exactly one screen pixel, like other painting
apps.

## 12. A stroke is many round "dabs"
A brush stroke is made of round stamps placed close together along the
pen's path: by default a new dab every 10% of the brush width. The pen
only reports its position every few milliseconds, so between two readings
we place dabs along a smooth curve through them (a "Catmull-Rom spline"),
with the pressure blending from one reading to the next. That's why fast
strokes have no gaps, and fast curves come out round instead of angular.
The curve's shape near a reading depends on the next one, so the line is
drawn one reading behind the pen (a few milliseconds).

## 13. Opacity is a ceiling for the whole stroke
While you paint, the stroke keeps a "coverage" value for each pixel (how
strongly the stroke covers it). Each pixel shows the brush colour laid over
the pixel *as it was before the stroke*, as strong as its coverage. So
going over the same spot again in one stroke never makes it darker than
the opacity you set, the way it works in other painting apps. Flow below
100% makes coverage build up gradually where dabs overlap, like an
airbrush.

The stroke also saves each tile it touches as it was before. Undo (Step 5)
will use those saved tiles.

## 14. Smoothing works like a string
With smoothing on, the line follows the pen as if pulled on a string: it
only moves once the string is tight, so small shakes never reach the
line. When you lift the pen, the line catches up to where the pen ended.
It depends on distance, not time, so it feels the same with a mouse or
a pen. The string length is measured on screen, so it feels the same at
any zoom.

## 15. Exact pen positions, and every reading
Windows gives the pen's position in whole pixels, but the tablet measures
much more finely. We use the fine position (with a safety check against
the whole-pixel one), so slow, careful lines come out smooth. When the app
is busy, Windows bundles several pen readings into one message; we use
all of them instead of only the latest.

## 16. Fingers navigate, they don't paint
On a touchscreen, two fingers zoom, pan and rotate. Fingers don't paint,
so resting your hand on the screen won't leave marks.

## 17. The eraser is the brush in reverse
The eraser uses the same strokes as the brush (dabs, pressure, smoothing,
curves, soft edges), but makes pixels more transparent instead of laying
colour on them. It erases back to the paper. It keeps its own settings, so
switching between brush and eraser doesn't lose your brush size. Turning
the pen round to its eraser end switches to the eraser automatically, and
turning it back returns to the tool you had. When a stroke leaves a tile
completely empty, the tile is removed to free its memory.

## 18. Undo and redo use "commands"
Every change is recorded as a *command*: a small object that knows how to
undo and redo itself (the "command pattern"). Painting, erasing and
clearing all change tiles, so they share one command type that keeps
only the tiles that changed, never the whole picture. Later steps add
their own command types (adding or deleting a layer, for example) to the
same history.

A tile command keeps the *other* version of each changed tile. Right
after a stroke that's the old version. Undo swaps it with the tile on the
canvas, so the command then holds the new version, ready for redo. Each
changed tile is stored only once, which halves the memory compared with
keeping both versions. Painting on empty canvas costs almost nothing to
remember, because the old version of an empty tile is just "no tile".

The history keeps up to 1 GB of changes and forgets the oldest when it's
full. The status bar shows how many steps it holds and how much memory
they use. Undo waits until you finish the stroke you're painting.

## 19. The colour wheel: a hue ring around a square
Around the ring you choose the hue (red, yellow, green, ...). In the
square you choose saturation (grey to vivid, left to right) and value
(dark to bright, bottom to top). This "HSV" way of choosing colours
matches how people think about colour better than red/green/blue
sliders. The wheel is drawn on the graphics card as coloured shapes, so
it updates instantly while you drag.

The picker remembers the hue itself rather than working it out from the
RGB colour each time. Otherwise, dragging down to black or grey (which
have no hue) and back up would always jump to red.

## 20. The eyedropper picks what you see
The eyedropper picks the colour you see on screen: the paint on top of
the paper. You can choose it as a tool (`I`), which switches back to the
brush after one pick. Or hold `Alt`, or the pen's side button, to pick
without changing tools. "Recent colours" lists colours you've actually
painted with, newest first.

## 21. Layers, and how they're combined
A document is a stack of layers on paper. Each layer has its own tiles,
a name, a show/hide switch and an opacity. You paint on the "active"
layer: the one selected in the Layers panel. A document always has at
least one layer, and painting on a hidden layer is refused with a short
message, because you couldn't see what you were doing.

The screen shows the layers combined. The engine combines one tile at a
time, and only the tiles that changed: a stroke changes the tiles it
touched, while showing, hiding, moving or changing a layer's opacity
changes all of that layer's tiles. Each layer has a blend mode ("Normal"
for now); blend modes like Multiply and Screen from the later phases
only need a new case in one function (`blend_over`). The eyedropper
picks from the combined picture.

Each layer has a permanent id number, separate from its position in the
stack, so undo always finds the right layer even after layers have been
moved around.

## 22. Every layer action can be undone
Adding, deleting, moving, showing/hiding, renaming and changing opacity
are all commands in the same undo history as strokes. Adding and
deleting are one command type that "toggles" a layer in or out of the
stack; a deleted layer is kept inside the command, paint and all, so
undo brings it back exactly. Dragging the opacity slider changes the
layer live, but the whole drag is recorded as one undo step when you
let go. Choosing which layer is active isn't undoable, as in other
painting apps.

## 23. Our own file format: a zip of PNG tiles
A picture is saved as an `.artapp` file. Inside it is an ordinary zip
archive (the same idea as Krita's `.kra` and the OpenRaster `.ora`
format), holding:
- `document.json`: the canvas size, paper colour and every layer's name,
  visibility, opacity and blend mode, as readable text;
- one small PNG image per painted tile of each layer.

Why this way:
- **Only painted tiles are stored**, so saving is quick and an almost
  empty picture makes a tiny file, however big the canvas is.
- **Nothing is lost.** Tiles are stored exactly as the app keeps them in
  memory, so opening a file gives back exactly the same pixels.
- **Well-known pieces.** Zip, JSON and PNG are standard and well tested,
  and you can even unzip a file to look inside. It also makes OpenRaster
  import/export (a later phase) easy, because `.ora` is built the same way.
- **It can grow.** The file says which format version it is. A newer app
  (for example with 16-bit colour) can still open old files, and an old
  app politely says "made by a newer version" instead of breaking.

## 24. Saving can't destroy your previous file
Saving first writes a temporary file next to the real one and only
replaces the real file once everything has been written. If saving fails
halfway (disk full, USB stick pulled out), the previous version is still
there. Opening checks everything it reads, so a damaged file gives a
clear message instead of a crash.

## 25. "Save changes?" and the `*` in the title
The undo history remembers *where* you last saved. If you undo back to
exactly that point, there is nothing unsaved again. When there are
unsaved changes, the window title shows a `*`, and starting a new
picture, opening another one, or closing the window first asks
"Save changes?". Opening or starting a picture clears the undo history:
undo can't bring back a picture you've closed.

Exporting a PNG doesn't count as saving, because a PNG has no layers: it's
a copy of what you see, for sharing.

## 26. The canvas follows the window
The app starts maximized, because painting needs room. When the window
changes size (maximizing it, for example), the canvas is fitted again
as long as you haven't zoomed, moved or turned it since it was last
fitted. Once you have, your zoom stays as it is, and the same part of the
picture stays in the middle of the window, like in other painting apps.
Pressing `0` fits the canvas again.

## 27. The paint bucket spreads like water
The paint bucket starts at the pixel you click and spreads to its
neighbours (left, right, up, down) as long as their colour is close
enough to the starting colour. **Tolerance** says how close: 0% only
fills exactly the same colour; 10% (the default) allows a small
difference in red, green, blue or opacity (about 25 out of 255).

- **Stop at lines on all visible layers** looks at the picture as you see
  it. That's what you want when the outlines are on one layer and you
  fill colours on another.
- **Grow** fills a pixel or two further. Lines have soft edges, and
  without growing, a thin light rim would stay between the fill and the
  line.

Filling works in two parts: first find *where* to paint (a "mask" that
says how much of each pixel is inside), then paint that area. Shapes
in the painting scripts (next step) reuse the second part.

## 28. Gradients are smooth, and never grey
A gradient goes from one colour to another along a straight line
(**Straight**) or in rings around a point (**Round**). Colours are mixed
"premultiplied", so fading a colour out to transparent keeps the colour
instead of turning greyish halfway. A tiny regular pattern (an "ordered
dither", always the same) breaks up the visible bands a big smooth
gradient would otherwise show, because each colour channel only has 256
steps. Gradients cover the whole layer, so they're best on a layer of
their own.

Fills and gradients are undone like strokes: they keep only the tiles
they changed.

## 29. Blend modes, and the paper takes part
Each layer has a blend mode: how its colours mix with what's below.
Normal covers; Multiply darkens (shadows); Screen and Add lighten
(light, glow); Overlay and Soft light add contrast; Colour gives a grey
drawing colour; and more. The formulas are the ones in the public W3C
"Compositing and Blending" standard that web browsers use.

The picture is now combined *on the paper*, so a Screen layer over white
paper disappears and a Difference layer over white inverts, as in other
painting apps. A PNG exported with a transparent background is combined
without the paper. Changing the blend mode can be undone like other
layer changes.

## 30. Painting scripts: pictures as text
A painting script is a JSON file: a list of commands like "make a layer",
"draw an ellipse", "paint a stroke through these points". Playing it
gives the picture. It's made for writing by hand or by an AI assistant,
so it's:
- **Predictable.** The same script always gives exactly the same picture.
- **Strict and helpful.** A misspelt field is an error, not silently
  ignored, and every error names the command's number and what's wrong.
  Value problems (an unknown colour name, a layer that doesn't exist) are
  found before anything is painted for that command.
- **Short.** One command draws a whole rectangle, ellipse, polygon,
  smooth curve, tapered line, hatching or scattering, so a picture
  doesn't need thousands of strokes.

Under the hood, every command becomes simple jobs (brush strokes, areas
to fill, paint bucket fills) that reuse exactly the same engine code as
the app's tools. That's why a scripted stroke looks the same as a hand-
drawn one. The jobs can be done all at once (for rendering) or a little
at a time (so the app can show a script being painted).

## 31. "Random" that repeats exactly
Jitter, hatching and scattering look random, but use a small number
generator written into the engine ("SplitMix64"), started from a seed.
The same seed always gives the same numbers on every computer. We don't
use a library for this, because a library update could change the
numbers and with them every scripted picture. Each command mixes the
script's seed with its own seed (or its number), so two scatters don't
repeat the same pattern.

## 32. Quick previews paint the whole script smaller
A preview plays the script at a smaller scale: the canvas, every
position and every brush size are multiplied by the same number (for
example 0.25). That's much faster than painting at full size and
shrinking, and it shows the same picture, only smaller.

## 33. `art-cli`: the engine without a window
`art-cli` is a small program that only uses the engine: no window, no
graphics card, no egui. So it builds and runs anywhere Rust does,
including a server with no screen, where an AI assistant can write a
script, render it, look at the picture and improve the script.

It's made for checking work quickly:
- `--preview` paints the whole script at a smaller size (fast);
- `--crop` saves one area at full size, and `--zoom` enlarges it so
  single pixels are easy to see;
- `--snapshots` saves the picture at each `snapshot` command and each
  time the script moves on from a layer, and `--sheet` puts them all
  in one image (a "contact sheet"), so progress can be judged at once;
- `check` finds mistakes in a script in milliseconds, without painting.

Mistakes print the command's number and what's wrong, and the program
ends with code 1 (or 2 if the command line itself was wrong), so a
script calling it can tell that something failed.

The contact sheet labels use a tiny 5x7 pixel font drawn by hand in
`font.rs`, so no font files are needed.

`render-script.bat` does the same on Windows without a terminal: drag a
script onto it.

## 34. The engine is optimised in test builds too
Tests that render whole pictures were slow (about 25 seconds) because
debug builds don't optimise our own code. The engine now gets the same
optimisation as the libraries in debug and test builds: tests take a few
seconds, and it only recompiles a little slower.

## 35. Play strokes: watch a script being painted
**File → Play strokes…** plays a painting script on the canvas a little
each frame, so you see it being painted, like a time-lapse. The player
does a "budget" of work per frame (about as many pixels of brush travel
as the speed slider says), so a long stroke draws gradually while a
shape fill appears at once. Snapshots are ignored here.

While it plays, painting and the side panels are locked: a stroke of
yours in the middle would get mixed into the script's changes. Zooming
and moving around still work. When it finishes (or you press Stop),
everything it did becomes **one** undo step, built from all its smaller
steps (a "command group"), so one Ctrl+Z takes it all back.

If the script was made for another canvas size, the app asks whether
to play it on a new picture of that size or on this one anyway. Opening
a `.json` script with Art App (or `ArtApp.exe script.json`) plays it on
a new picture straight away.

## 36. Export strokes: your painting, written down as a script
Each undo step also carries what it did, written as script commands: a
stroke is "this layer, this brush, through these points" (the points
exactly as they went into the stroke, after smoothing, so replaying
draws the same line). Exporting collects the steps that are still in the
picture, in order: whatever you undid is left out, and steps forgotten
to save memory are kept. A tidy-up then removes repeats, so the script
only says "change layer" or "change brush" when something changes.

Positions are rounded to 0.01 of a pixel to keep the file readable; that
difference can't be seen. The script starts with a canvas the size of the
picture. If the picture was opened from a file, its "notes" say so: the
strokes are what was painted *on top* of that file. Scripts played with
Play strokes are included too, with their colour names written out; their
random patterns (jitter, scatter, hatch) can come out differently in the
export, because those depend on the script's seed and command numbers.

## 37. A guide for AI painters, and documentation that's tested
`docs/AI_GUIDE.md` is written for an AI assistant painting with scripts:
how to build and run `art-cli`, the whole format on one page, a
workflow (plan, block in back to front, snapshot, check, light and
shadow, detail, inspect close-ups), techniques taken from the two
example pictures, rules of thumb for light and colour, how to check its
own work, and common mistakes (like y going down, or a misspelt layer
name quietly making a new layer).

Wrong examples in a manual teach wrong habits, so the tests read both
documents and check every example command in them, and `art-cli check`
runs on every script in `examples/`.

## 38. art-cli stops quietly when nobody reads its output
When art-cli's output goes into another program that stops reading
early (`art-cli info picture.artapp | head -1`), writing fails with a
"broken pipe". Rust programs normally crash with an error there. art-cli
prints through two small helpers instead: for normal output, a broken
pipe just ends the program quietly and successfully (the work, like
saving pictures, is already done before anything is printed, and nobody
is listening any more), like other command-line tools. Any other
writing problem is reported with exit code 1. Error messages never
crash either; if they can't be written, the exit code still tells.

## 39. Groups: draw once, then move, turn, resize or mirror
A `group` draws its commands through a *transform*: a small piece of
maths (`transform.rs`) that says where each point of a drawing lands on
the canvas. One transform can move, resize, turn and mirror at once, and
two of them combine into one, which is how groups inside groups work.
Scripts always apply the parts in the same order, around an `origin`:
mirror, then resize, then turn, then move. One fixed order means there's
one obvious way to write anything.

Inside the player, commands are played from a stack of "frames": the
script itself at the bottom, and one more for each group being played.
Every drawing command works out its shape in its own units first and
then *places* it with its frame's transform. A quick preview is now
simply the whole script inside a shrinking transform, so previews and
groups share the same code. Curves are still cut into pieces about two
*canvas* pixels long, so an enlarged group stays smooth.

When a group ends, the brush and the selected layer go back to what they
were before it. That keeps a group (and, in the next step, a symbol)
from quietly changing what comes after it.

Errors inside groups give the way to them, like
`command 12 (group "duck") > 3 (ellipse): ...`.

Old scripts paint exactly the same pixels as before: a test renders the
two examples and a script that uses every older command, and compares
every pixel with pictures made before groups were added. (Previews of
old scripts can differ by an invisible rounding step, because curves
are now worked out before shrinking instead of after.)

## 40. Symbols and repeat
`define` stores a drawing under a name; `use` paints it, placed like a
group. A `use` is played as one more frame (see 39), so everything that
works in groups works in symbols, and symbols can use other symbols. A
symbol that uses itself, even through others, would never end, so it's
an error that shows the chain (`a > b > a`). Symbols are defined only at
the top level and each name once: then there's never a question of
which definition a `use` means.

Choices, kept simple on purpose:
- **Layer names in symbols are the picture's own layers.** A character
  symbol that paints on `Body` and `Shadow` paints on the same two layers
  every time it's used. There are no hidden prefixes to remember. After
  the `use`, the layer selected before it is selected again.
- **Colour swaps go by colour.** `"colors": {"yellow": "#C04040"}`
  looks up what `yellow` is and paints red wherever the symbol would
  paint that colour. Matching by the colour itself (not the name) means
  swaps also work for hex codes, and keep working in scripts exported
  from the app, where palette names are written out as hex codes. Swaps
  from an outer `use` apply on top of an inner one's.
- **Randomness:** a symbol's random patterns depend only on its name,
  so every use of it looks the same (a symbol is "the same drawing").
  `repeat` copies each mix in their copy number, so a row of jittered
  fence posts isn't a row of identical twins.

`repeat` is one frame that rewinds for each copy, with its step applied
once more each time. Building all the copies at once would have put
waiting copies into each other's error messages.

A test checks that `use` paints exactly the same pixels as writing the
symbol's commands out by hand.

## 41. Text: a built-in font, letters as shapes
Painting scripts write text with a font that is part of Art App, so a
script gives the same picture on every computer, including a cloud
machine with no fonts installed. The font is **Inter** (Regular): very
clear, with every ASCII character and most accented letters, and its
licence (the SIL Open Font License) lets it be built into a program as
long as the licence goes with it (`assets/fonts/OFL.txt`, and a credit
in the README).

Reading the font file is done by **ttf-parser**, a small library with no
other dependencies that only reads letter shapes; it draws nothing. The
engine then does the rest itself (`text.rs`):
- Each letter's curves are cut into short straight pieces (about one
  canvas pixel long), so a letter becomes a few polygons.
- They're filled like any other shape, with the same smooth edges. A
  letter like "o" is two outlines, the hole going the other way round;
  the fill already used the "non-zero" rule, which leaves such holes
  empty, so it only had to learn to fill several outlines at once.
- Letters are spaced with the font's own advance widths and its
  kerning pairs (so "AV" tucks together). There's no shaping for
  joined-up scripts or right-to-left text: not needed for labels.
- The outline is a crisp stroke along every letter's edge, painted
  first, so the letters cover its inner half and only a border outside
  them shows. Letters don't get thinner when they get an outline.

Because letters are shapes, text moves, turns, resizes and mirrors
inside groups and symbols with no special cases.

`at` is the **top** of the first line, not the baseline: then
`"at": [x, y]` means "the text starts here and goes down", like every
other box in the format. Exact sizes are in script-format.md, so a
painter can place text without trying it first.

A reference picture (`tests/reference/text.png`) keeps text looking
the same from now on, and a test checks that every printable ASCII
character draws something.

## 42. Seeing and checking: numbers first, then pictures
An AI painting on a machine without a screen has two ways to know where
things went: look at a picture, or read numbers. Pictures cost a lot to
look at and are easy to misjudge by a few pixels; numbers are cheap and
exact. So art-cli now gives both.

**`check` paints for real** (at full size, saving nothing). Before, it
only worked each command out without painting, which was quick but
couldn't say what ended up where, and in one case disagreed with
`render` about the layers. Painting takes under a second for the
examples, and then everything it says is exactly what `render` makes.
It lists:
- each layer's box: the smallest rectangle around everything painted
  on it (from its pixels), or "empty";
- each named `group` and each `use` of a symbol, with the box around
  what it drew. The player works these out from the shapes and strokes
  it sends to be painted (a stroke's box includes half its brush
  width), so they're in canvas pixels even in a preview. A paint bucket
  counts only where it starts: where it spreads isn't known until it's
  done;
- warnings for what's probably a mistake: a command drawn entirely
  outside the canvas (if a whole group or `use` is, one warning names
  it rather than one per shape inside), a layer left empty, a symbol
  defined but never used, a palette colour never used.

Boxes are written `x 10..118, y 20..120` (left..right, top..bottom),
the same as `art-cli info` already did.

**`render` prints a short summary** (12 lines or fewer): canvas, the
layers' boxes packed a few to a line, time, up to three warnings and
the files saved. Long lists are left to `check`, so the summary stays
readable after every render.

**Viewing aids**, all for looking only:
- `--layers "Sky,Duck"` shows just those layers (even ones the script
  hid), `--hide "Sketch"` all but those. A name that isn't a layer is
  an error, so a typo can't silently give an empty picture. The
  `.artapp` always keeps every layer.
- `--bg "#4080A0"` (with `--transparent`) puts a colour behind the
  see-through parts, because a transparent sprite is hard to judge
  on nothing.
- `--grid 100` draws a light grid with coordinates. Lines are a little
  darker on light colours and lighter on dark ones, so they show on
  anything; when they'd be crowded (a small grid on a small preview)
  only every 2nd, 5th, 10th... line is drawn.

They go on previews, crops, snapshots and contact sheets (the sheet
gets the colour but not the grid: its pictures are too small for
labels). The real picture, `<name>.png`, and the `.artapp` never get
them; at full size the picture with them is saved next to it as
`<name>-view.png`.

## 43. Short lists by default, and copy numbers in text
A sheet of four frames, each a `use` of a duck that uses an eye, made
`check` list every one of them: 17 lines that mostly said the same
thing. Now `check` lists only the **outermost** named groups and uses,
each with how many others are inside it, and `--detail` (on `check`
and `render`) lists all of them, indented by how deep they are. The
player notes, for each group or use, which one it's inside, so the
counting is simple.

Inside a `repeat`, a `text` can say which copy it's in: `{i}` (1, 2,
3...) and `{i0}` (0, 1, 2...). Four labels "frame 1" to "frame 4" are
one command instead of four. Only the innermost `repeat` counts, to
keep it obvious, and it works from inside groups and symbols too (a
symbol with a label can be used in a repeat). Outside any repeat,
`{i}` is reported as a mistake rather than written out as-is, because
it's almost certainly one.

## 44. Papercuts
- **Thin lines in previews.** Brush dabs are never smaller than one
  pixel. So in a preview, a line that ended up thinner than a pixel was
  drawn as dark as a one-pixel line: a 1-pixel line at a quarter size
  looked almost black, much bolder than the full picture shrunk down.
  Now, only in previews, such a line is drawn one pixel wide and as much
  fainter as it's thinner (a quarter as dark at a quarter size), so it
  looks like the shrunk picture. Full-size pictures don't change.
- **`--crop` past the canvas's edges.** A close-up near an edge or a
  corner keeps the size asked for, and x and y may be below 0. What's
  outside the canvas shows as grey squares, so it can't be mistaken for
  see-through paint, and the grid's coordinates still line up. A crop
  entirely outside the canvas is an error that says the canvas's size.
- **Which commands painted each layer.** The player notes, for every
  layer, the numbers of the script's own commands that painted on it (a
  group, `use` or `repeat` counts as one; clearing a layer starts again).
  `art-cli check` shows them next to each layer, and `art-cli info` now
  also takes a script: the same layer table as for an `.artapp`, plus
  "by commands 11-19". To fix the clouds, look at commands 11 to 19.
- **Play strokes and Export strokes with the new commands.** Playing
  already went through the same player, so groups, symbols, repeats and
  text play in the app like anywhere else. Exporting needed one fix:
  playing two scripts that define the same symbol (or one script twice)
  wrote two `define`s with one name, which a script doesn't allow. Now
  an identical definition is left out, and a different one gets a new
  name ("duck 2") that the `use`s after it use. A test plays a script
  with all of them twice, the way the app does, exports it, and checks
  that the export paints the same picture.

## 45. A one-page quick start, and every example tested
`docs/QUICKSTART.md` is the first thing to read (README and the AI
guide both point to it from their first lines): building art-cli, one
complete script, the check-render loop with what the output means, the
few rules that cause most mistakes, and the commands used most. It's
kept under 120 lines on purpose: everything else is one link away.

Its script is `examples/two-ducks.json` word for word (a test checks
that), so it can be rendered straight away. The test that reads the
JSON examples in the docs now covers the quick start too, and whole
scripts in the docs are also played, so a wrong colour name or a `use`
of a symbol that isn't defined is caught, not only typos.

## 46. A skill for AI assistants, tested against the real thing
`skills/art-app/SKILL.md` is what an assistant like Claude loads when
it's asked to paint with Art App: a name and a description that says
when to use it, then the loop (check, preview, close-up, fix), a script
skeleton, the rules that prevent most mistakes, reuse with symbols,
groups and repeats, text sizes, sprites and sheets, and how to report
back. Under 150 lines, so it's cheap to load; the details stay in the
docs it points to.

An assistant follows such a file to the letter, so a wrong option name
in it costs a failed run every time. A test (`crates/cli/tests/skill.rs`)
checks that:
- its JSON examples, put together, play without a mistake;
- every `--option` and `art-cli` command it names is in `art-cli help`;
- every script word it puts in `backticks` is in the script format;
- every repository file it names exists;
- it has its name and description, and stays under 150 lines.
Planting a made-up option, field or file in it makes the test fail.

## 47. Proof: a walk cycle and a character sheet
To see whether phase 1 helped, the two jobs that were hard before were
done again with the new commands, as examples:
- `examples/duck-walk.json`: a 4-frame walk cycle, each frame 128 × 128
  on a 512 × 128 canvas (render with `--transparent`). The duck is a
  `body` symbol and a `leg` symbol (the far leg is the same leg with its
  colour swapped darker); each frame is a named group with three `use`s.
  **10 commands** at the top level, 32 counting those inside groups and
  symbols. Before phase 1 the same kind of sheet took **50**.
- `examples/duck-sheet.json`: the same duck as player 1, as player 2
  (colours swapped with one `colors`), and facing left (`"mirror": "x"`),
  with labels and colour swatches: 24 top-level commands, 38 in all.

Each took one script and one fix: `check` showed every frame's group
inside its 128-pixel cell before any picture was opened.

What was still slow, honestly:
- **Symbols have no parameters.** Walk frames differ only in two leg
  angles and how high the body is, yet each frame is its own group;
  `repeat` can't change an angle from copy to copy.
- **Feet turn with the leg.** Keeping a foot flat needs a group turned
  back around the ankle, and knowing where the ankle ends up.
- **Overlapping outlines.** Where the head meets the body both
  outlines show; an extra filled shape hides the seam.
- **Text width isn't known before painting.** One label was wider than
  its panel and needed a second render to fix.
- **A repeat can't pick a colour per copy**, so the swatch row is six
  rectangles written out.
- Some arithmetic remains: panel centres and the labels under them.
These are good candidates for phase 2 (keyframes and parameters would
cover the first two). Rendering itself wasn't slow: 0.03 s for the walk
cycle and 0.08 s for the sheet.

## 48. Reference pictures on other systems
The reference pictures were made on Linux, where every pixel must still
match exactly. Windows works out sines and cosines with its own maths
library, which can differ in the last digit; that can nudge a rare edge
pixel when something is turned. So off Linux, up to 0.1% of a picture's
pixels may differ, and `test.bat` doesn't fail for nothing. A real
change to old pictures is still caught exactly on Linux, where the
work is done.

## 49. Text, flip and transform in the app
Three tools for painting by hand, built on what the engine already
does, so the app stays small:

- **Text (T).** Click and type; size, colour, alignment and an outline
  are in the side panel and can change while typing. The text is
  painted with the engine's own `text` command every time it changes
  (and taken off again first), so what you see while typing is exactly
  what you get, in the built-in font. Until it's finished it shows a
  box and a line where the next letter goes. Enter on an empty line,
  a click outside the box or outside the canvas and tool panel, another
  tool, a menu or undo finishes it, as one undo step; Escape takes it
  away. Export strokes writes the same `text` command.
- **Layer → Flip horizontal / vertical** mirror the selected layer
  around the canvas's middle. That puts every pixel exactly on another
  pixel, so a flip copies pixels without blending, and two flips give
  back exactly what was there.
- **Layer → Transform…** puts the layer in a box with handles: inside
  to move, a corner to resize (the opposite corner stays put, as in
  other painting apps; Shift keeps the proportions), a round handle to
  turn. While dragging, the canvas leaves the layer out and draws a
  picture of it where it's going, which stays smooth on big canvases;
  the layer's own show/hide setting isn't touched. Enter or Apply
  paints it there (turning and resizing blend neighbouring pixels),
  Escape or Cancel leaves it. Menus, undo and the panels wait until
  it's applied or cancelled.

Flips and transforms are written down as a new script command,
`transform_layer`. The app builds that command first (numbers rounded
so the script is easy to read) and paints *it*, using the same code
the script player uses, so Export strokes replays exactly what you saw;
tests check that for text, flips and transforms. The handles' maths,
typing and finishing are tested without a screen, and the tools were
also tried in the real app on a virtual screen.

## 50. Animations: one canvas, one play per frame (phase 2, step 1)
An animation is the whole script played once per frame, with the frame
number known to the commands. The plan (`docs/animation-design.md`)
chose that over a canvas per frame or a timeline: an animated script is
an ordinary script plus an `animations` section, nothing changes in how
drawing works, every frame is a function of the script and its number
(so it's deterministic), and a script without the section is a single
picture, bit for bit as before (the reference pictures still match).

Each animation has its own `commands`, played after the script's; the
script's own commands hold what every frame shares (canvas, symbols,
background). Several animations in one file share one character.

In the player, the animation's commands are one more frame on the
stack, pushed when the script's own commands end, with its name for
messages: `animation "walk" > command 2 (use "duck")`. `check` plays
every frame of every animation, since a mistake can hide in frame 7
alone, and reports frame 0's layers and parts (the motion summary comes
in step 3). `render` without a frame option paints frame 0 and says how
to get the others, rather than painting everything unasked.

## 51. Keyframes, parameters, shift and upright (phase 2, step 2)
A `group`'s or `use`'s number can be keyframes, `{"keys": [[0, 0],
[4, 20]], "ease": "in-out", "shift": 4}`, or a symbol parameter,
`"$angle"`. Both are worked out when the command starts, for the frame
being painted and the symbol being used; the rest of the player sees
plain numbers, so nothing about drawing changed. In the script format,
those fields are now "any JSON value" instead of typed numbers, and
the player checks them with messages like "`translate` must be [x, y]".

Choices:
- **Blend or hold by kind.** Numbers, lists of numbers and colours
  blend; anything else (a symbol name, `"x"`) holds until the next key.
  So a mouth swap needs no special syntax: it's a keyframed parameter
  with names in it.
- **Five easings, no curves.** `linear`, `in` (t²), `out`, `in-out`
  and `step`, on the whole field or per key. Enough for bounces and
  walks; bezier handles would need an editor to be worth having.
- **`shift` instead of arithmetic.** The far leg, arms and tail are the
  near leg's motion half a cycle later, so the same keys with a shift
  cover them, wrapping in loops. No `"-$angle"`, so there's no little
  language to learn or to get wrong.
- **Parameters are plain values** (resolved when the `use` starts,
  with that use's keyframes and the outer symbol's parameters), and
  `"$name"` replaces a whole value. A parameter can be used where a
  keyframe could, including a `use`'s `name`.
- **`upright`** undoes the turning of the frames around a group or
  use, around the point where its origin lands: the foot moves with
  the leg but stays flat. Mirroring and resizing from outside still
  apply (a mirrored parent's x-axis points backwards, which is told
  apart from a turn by the sign of the transform's determinant).

Tests: in-between frames equal the pose written by hand, pixel for
pixel; easings match their formulas; a parameter gives the pixels of
the hand-written pose; shift gives the far leg's frame; upright feet
land where the ankle does, flat, also mirrored. `examples/duck-animated.json`
is an 8-frame walk and a 4-frame idle from one `use` each.

## 52. Seeing an animation without watching it (phase 2, step 3)
An agent can't watch a GIF, and a person shouldn't have to watch eight
of them to find the frame that's wrong. So `check` reads the motion
first: it already plays every frame; now it keeps each frame's report
and, for every named group and `use`, follows the middle of its box
from frame to frame. The summary prints the first and last place and
the biggest step; warnings name a jump bigger than a quarter of the
canvas, a part that draws nothing in some frames, and a looping
animation whose last-to-first move is far bigger than any step inside
it (the loop pops). Middles come from the boxes the player already
works out, so this costs nothing extra and needs no pixels.

For eyes: `--frame-sheet` puts every frame (or every Nth) in one
numbered picture at the frames' own size, `--onion` draws one frame
over its neighbours tinted red (before) and blue (after) at a third
strength, and `--gif` writes an animated GIF with the `gif` crate
(MIT/Apache; its colour quantizer reduces a painted frame to 256
colours; pixel art already has fewer). Half see-through pixels become
opaque in a GIF, which can't blend: `--bg` is the way to show a
see-through sprite in one.

## 53. Three review fixes before the exports
- **"Never used" counts every frame of every animation.** A symbol
  swapped in at frame 9 (a blink) or a colour used only in the "hurt"
  animation is used. The report now carries what each frame defined
  and used, and `unused_warnings` is worked out over all the reports;
  for a single picture that's one report, as before.
- **Frame warnings say their frame, once.** Something drawn off the
  canvas in frames 5, 6 and 7 is one line: `animation "walk", frames
  5-7: ...`, not three.
- **`check` says which frame it shows** and takes `--animation` and
  `--frame` to show another. Before, it silently showed frame 0 of the
  first animation alphabetically.

## 54. Exports: one sheet, one JSON, frames, GIF, MP4 (phase 2, step 4)
`art-cli export` paints every frame once (full size, see-through) and
writes what was asked for. The sprite sheet is built in the engine
(`sprites.rs`), so the app can write the same file: one row per
animation, cells the frame size, frames numbered across the grid row
by row. That numbering is what a game gets from cutting the sheet into
equal cells, so the JSON only has to list frame numbers per animation,
plus fps, loop and the pivot. Nothing in the JSON is needed to *find*
a frame, only to play it. The pivot defaults to the bottom centre so a
character stands on its feet whichever animation plays.

MP4 goes through ffmpeg when it's installed (H.264, sizes padded to
even numbers, fast start), on the paper colour unless `--bg` says
otherwise, since a video has no see-through background. Without
ffmpeg, the frames are written and the exact command printed: better
than a dependency on a video library in the engine. `--scale` enlarges
with sharp pixels, and the JSON records it, so a sheet exported at 8×
for people is as usable as one at 1× for the game.
