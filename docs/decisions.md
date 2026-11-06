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

## 55. Pixel art: paint big, then vote (phase 2, step 5)
A test of game sprites showed that shrinking a painted character to
32 pixels gives blurry, see-through edges and an unreadable eye. The
fix had to be one mechanism, not a crisp version of every rasterizer.

A pixel canvas is painted `supersample` times bigger (4 by default)
with every ordinary command, which is the existing preview machinery
run the other way (the player already scales everything by one
number). When the script has finished, each block of samples becomes
one pixel: painted if more than half its samples are, in the palette
colour the most of them had. The vote, not an average: at the edge
between an outline and a fill, an average is a muddy colour that would
snap to a third palette entry. Shades from anti-aliasing snap to their
nearest palette colour before voting, so they count for it. An
`outline` is a one-pixel line put on see-through pixels next to paint,
per layer, after shrinking.

So shapes, strokes, text, symbols, keyframes and transforms all work
unchanged on a pixel canvas, animation moves by whole pixels because
`translate` is rounded, and the picture can only have palette colours,
which is why the palette is locked (a hex colour is refused with the
fix to make). The one new command, `pixels`, is for art that's natural
to type: rows of characters, each a palette colour, painted as
rectangles like anything else.

The cost: a pixel script can't be played onto an open picture in the
app (it's big until the end), so the player refuses that with a plain
message; the app plays it on a new picture or shows its frames.
`examples/duck-pixel.json` is the 32-pixel duck, idle and walk, six
colours, from the same kind of symbols as the painted one.

## 56. Phase 1 leftovers (phase 2, step 6)
- **Text width before painting:** `art-cli measure "words" --size 32`
  prints the width and height (and each line's width), and `--fit 180`
  the size that fits. On the `text` command, `"fit": 180` shrinks the
  text to fit a width and never enlarges it, so a label can't run out
  of its panel. Both use the engine's own layout, so they're exact.
- **`repeat` with values per copy:** `"each": {"paint": ["yellow",
  "wing", "beak"]}` gives copy 1 `"$paint"` = yellow, and so on, round
  and round. It reuses symbol parameters: a repeat's copy is a frame
  with those parameters set, so nothing new to learn inside it.
- **Colours as parameters:** `"fill": "$paint"` now works anywhere a
  colour is written, from a symbol's `params`/`with` or a repeat's
  `each`. The pixel duck wanted this for its far leg before `colors`
  did the job; now both ways work.
Left alone: overlapping outlines where two shapes meet (hide the seam
with a shape, as before) and arithmetic in parameters (`shift`,
`each` and `fit` cover what was wanted).

## 57. The app watches and exports animations (phase 2, step 7)
Opening a script with animations (Play strokes, Open, or double-click)
paints every frame of every animation once and shows them in an
Animation panel at the bottom: play and pause at a chosen fps, step a
frame, jump to a frame, pick the animation, and an onion skin. The
frame shown *is* the picture on the canvas (stepping replaces it), so
zoom, layers and export PNG work as always, and Close simply keeps the
frame. Painting is locked while the panel is open: drawing on frames by
hand is phase 3's timeline.

The onion skin is blended into each tile between the paper and the
layers, so it sits behind the frame instead of tinting it (drawing it
on top, which was tried first, made the whole duck muddy). File →
Export animation offers the three exports the panel can make from the
frames it holds: the sprite sheet with its JSON (through the engine's
`sprites.rs`, the same file art-cli writes), the frames as PNGs, and a
GIF at the panel's fps. A pixel script is played the same way (its
frames come out shrunk); it can't be played onto an open picture.
The panel's playing, stepping, onion skin and exports are tested
without a screen; the panel was also tried in the real app on a
virtual screen.

## 58. Proof of phase 2, and what was still slow
Three jobs, as examples with reference pictures:
- `examples/duck-animated.json`: the painted duck, idle (4 frames) and
  walk (8 frames) from one set of symbols with parameters. Each
  animation is **one `use`** with keyframes in `with`; the far leg is
  the near leg's keys with `shift: 4`; feet are `upright`. Exported as
  a sprite sheet with JSON (`art-cli export --sheet`) and GIFs.
- `examples/duck-pixel.json`: a 32 × 32 pixel duck, six colours, idle
  and walk, the same kind of symbols on a pixel canvas with
  `supersample: 8` and an outline. Sheet + JSON at 1× for the game and
  GIFs at 8× for people.
- `examples/duck-forge-title.json`: a 3-second title at 640 × 360 and
  24 fps. The words drop in and squash and stretch (keyframed `scale`
  on a group with `origin` at their base), a duck slides in and hops,
  a caption fades in. Exported as GIF and MP4 (72 frames, H.264).

What was still slow or uncomfortable as an agent, for phase 3:
- **Timing by arithmetic.** Keys are frame numbers, so a bounce at
  24 fps is "frames 14, 20, 26, 30, 34" worked out by hand, and
  changing the fps means redoing every key. Seconds (or a `"time"`
  unit) for keys, and a way to say "then, after 6 frames" would help.
- **Squash and stretch is two keyframe lists** (`translate` and
  `scale`) that must agree frame by frame. A named motion ("bounce
  with squash") that takes a few numbers, like a symbol takes
  parameters, would make it one line.
- **Entrance warnings.** A thing that drops in from above is "drawn
  entirely outside the canvas" for its first frames, and layers it
  lands on are "empty" in frame 0. `check` can't tell an entrance from
  a mistake; a way to mark "off-screen on purpose" (or warning only
  when a part is *never* on the canvas) would cut the noise.
- **The motion summary follows boxes, not parts.** It can't say "the
  foot slides on the ground between frames 2 and 3" (the classic walk
  mistake); comparing a part's bottom edge frame to frame would.
- **No way to see a frame's pixels as numbers** for pixel art: at 32
  pixels, "is the eye one pixel or two" needs `--zoom 8` and a look.
  A `--pixels` dump (rows of palette letters, the `pixels` command's
  own format) would close the loop for an agent.
- **Still some coordinate arithmetic** for layouts: where a caption
  goes under a title, the panels of a character sheet.
- **GIF size.** A 72-frame 640 × 360 GIF is 2 MB; fine for a preview,
  not for sharing. (MP4 is 66 KB.)
Rendering was never the slow part: the title's 72 frames take about
2 seconds; the sprites, hundredths of a second.

## 59. Clipping, the alpha lock and masks (phase 3, step 1)

Three ways to keep paint inside a shape, each where a painting app would
put it:
- **Clipping is decided when the picture is shown**, not when it's
  painted. A clipping layer's pixels are multiplied by the opacity of its
  base (the nearest unclipped layer below) while compositing, so the base
  can be repainted or moved and the shading follows, and the app's
  layers panel can switch it on and off. A base that's hidden hides its
  clipping layers; a clipping layer with no base shows nothing. The
  flag lives on the layer, is saved in the `.artapp` (old files simply
  have it off), and is written by Export strokes.
- **The alpha lock is decided when painting.** It's a flag on the
  `Surface`, so every way of painting (strokes, fills, gradients, the
  bucket, the app's own tools) sees it in one place. Paint recolours a
  pixel by its strength times the paint's opacity, and the pixel's own
  opacity never changes; the eraser does nothing. Painting a see-through
  colour is scaled by the pixel's opacity, not added to it, so a locked
  soft edge stays exactly as soft.
- **A group's mask is a `Mask` in painted pixels**, worked out once when
  the group starts (so it's placed by the group's transform and moves
  with keyframes), multiplied by the masks of the groups around it, and
  handed to every job the group queues: strokes get it through
  `Stroke::clipped`, fills and the bucket through the `_within`
  functions. Nothing in the engine knows "a group" at paint time: just a
  coverage mask.
- **A symbol's silhouette is painted, not computed.** The symbol is
  played onto a scratch `Document` of the same size (a stack of them,
  since masks can nest), with its layer commands skipped and nothing
  recorded for undo or the report; the scratch layer's opacity becomes
  the mask. That way everything a symbol can draw, including jitter,
  text and nested groups, has the right silhouette, for the cost of
  painting it once more. On a pixel canvas, masks are thresholded at
  half so edges stay crisp.
- `value()` now resolves `$params` and keyframes inside any object, not
  only keyed ones, so mask numbers can animate without a special case.

## 60. Brush tips, dynamics and `stamp` (phase 3, step 2)

- **A tip is a small greyscale picture, sampled per pixel**, not a
  formula evaluated per dab: the same code stamps a built-in tip and one
  from a PNG. Built-in tips are made once from formulas with fixed seeds
  (3 x 3 samples per texel for smooth edges) at 128 texels across, then
  halved down to 2 x 2, and a dab samples the level nearest its own size
  (bilinear), so a 10-pixel chalk dab doesn't alias. The `round` tip
  keeps the old analytic path, so every existing picture is unchanged
  (the reference tests say so). `hardness` on a shaped tip softens a
  round envelope around the dab's corners, so a square stays square at
  hardness 1 and fades at the corners when softer.
- **Dynamics are per dab, from the stroke's seed.** The stroke owns a
  `Random` (seeded by the player from the script's seed, the command's
  seed and the stroke's number in the command), so a script with chalk
  strokes paints the same picture every time, and a different `seed`
  gives a different one. Colour variation is kept per pixel (which dab
  last raised the pixel's coverage), so the stroke's "never darker than
  its opacity" rule still holds. Black doesn't vary in hue or lightness:
  it has none.
- **`BrushSettings` stays `Copy`**: a tip from a file is `Tip::File(n)`,
  a number into the player's list of loaded shapes (each file loaded
  once), handed to the stroke with `with_tip`. The app's brush panel
  never needs file tips.
- **`stamp` reuses what's there.** Tip copies are single-point strokes
  (so masks, clipping, lock and the brush's hardness all apply);
  symbol copies are a `repeat`-like frame whose copies have their own
  placements instead of a step, and the picked colour reaches the symbol
  as its `color` parameter through `each`. So a stamped copy is exactly
  a `use`, and messages say `stamp, copy 3 > 1 (use "blade")`.
- **Cost** (1920 x 1080, 40 paths of 1840 px with a 40 px brush,
  release build): round 0.89 s; round with size, scatter and hue
  dynamics 0.84 s; chalk 2.4 s; chalk with follow, size, opacity and
  lightness dynamics 2.7 s; leaf with follow 2.2 s. A shaped tip costs
  about 2.7x a round one: its box is twice the area (it may be turned)
  and each pixel samples the shape. Fine for paintings; worth a sampled
  envelope if animations with big textured brushes ever feel slow.

## 61. Noise and textures as paint (phase 3, step 3)

- **A pattern is a `Paint`**, next to colours and gradients, so it goes
  everywhere a fill goes (shapes, `gradient` with a `fill`, the bucket's
  paint, a brush `color`) and through clipping, masks and the alpha lock
  without those knowing. `Prepared` now owns its data, so a stroke can
  carry one and sample it per pixel.
- **Value noise, not Perlin.** Random values on a grid, blended with a
  quintic curve, `detail` layers each twice as fine and half as strong,
  then stretched (blended random values crowd around the middle). It's
  simpler to explain and plenty natural for clouds, water and rock. The
  grid values come from a hash of (seed, x, y), so no table and no
  state: the same on every computer, and a script's `seed` is mixed in.
- **Patterns live in the coordinates they're written in**: each paint
  keeps the inverse of its frame's transform and samples through it, so
  a rock symbol's grain scales and turns with every use, and the test
  that a textured symbol moved by a group equals the texture moved by
  hand passes exactly. Drift is `offset` keyframes, resolved by
  `value()` inside the fill object like any other keyframe.
- **Textures are small premultiplied pictures tiled with bilinear
  wrap-around**; `"paper"` is a 256 x 256 seamless tile made once from
  periodic noise. Tinting maps the picture's lightness through stops and
  keeps its opacity.
- **Cost** (release build). A full 1920 x 1080 fill: one colour 0.10 s,
  a gradient 0.13 s, noise with detail 1 / 3 / 6: 0.22 / 0.29 / 0.39 s,
  paper 0.24 s. A 96-frame 640 x 360 animation with a drifting noise
  fill and a paper layer on every frame, exported as frames: 11.8 s
  against 3.4 s with plain fills (about 0.09 s a frame for the two
  patterns; the PNG writing is most of the rest). Nothing here needs
  caching yet.

## 62. Filters: in place on a layer, or on a group's own drawing (phase 3, step 4)

- **One block of floats.** A filter copies the layer's painted area
  (grown by the filter's reach) into a block of premultiplied floats,
  works there, and writes back tile by tile, keeping the old tiles for
  undo: the same shape as `transform_layer`. Premultiplied, so a blurred
  red edge fades to clear red, never to grey.
- **Blur is three box blurs each way**, with the box widths chosen for
  a Gaussian of sigma = radius / 2, as running sums: the cost doesn't
  depend on the radius. Glow is "the light parts, blurred, added";
  shadow is "the silhouette, moved, blurred, tinted, underneath".
- **A filtered group paints on a scratch picture**, one scratch layer
  per real layer it touches (the same scratch stack that captures a
  symbol's silhouette for a mask: one mechanism, with `single` saying
  whether everything goes on one layer). When the group ends, each
  part is filtered on its own and laid on its layer through the alpha
  lock; a shadow is made once from the combined silhouette and laid on
  the lowest layer first, so it sits under all the parts and no seam
  shows between a head and a body on different layers. Nested filtered
  groups lay onto the scratch of the group around them. A bucket fill
  inside spreads over the real picture (its region is worked out
  there), not the empty scratch.
- **Order:** `adjust`, `gradient_map`, `blur`, `glow`, `shadow`, fixed,
  because JSON objects have no reliable order; two `filter` commands
  give any other order. Sizes scale with the frame's `detail`, so a
  blur inside a scaled group scales too, and a preview blurs less.
- **`from: "below"`** is just "composite the visible layers below onto
  this layer" (no paper), recorded for undo like painting. It's the
  only way to filter the whole picture, and it's enough.
- **The app's Filters menu** writes a `filter` command, reads it back
  into engine filters and applies it: so Export strokes replays exactly
  what the window did, like Layer → Transform.
- **Cost** (release build, 1920 x 1080 with a full-canvas noise fill
  that takes 0.20 s on its own): blur 4 / 40 / 200 px: 0.33 / 0.34 /
  0.44 s; glow 0.38 s; a blurred shadow 0.38 s; adjust 0.36 s; gradient
  map 0.24 s. A 96-frame 640 x 360 animation with a glowing sun, a
  moving shape with a blurred shadow and a gradient-mapped "from below"
  layer on every frame: 1.5 s for all the frames. Blur is the costliest
  thing in phase 3 so far and still well under the painting itself.

## 63. Two review fixes after phase 3, step 4

- **A symbol starts from the plain brush.** Before, a `use` inherited
  the brush as it was (tip, dynamics, pattern and all), so a bristle
  brush set for a texture stroke made every symbol after it hairy, and
  leftover dynamics made outlines blobby. Now `push_frame` sets the
  plain brush for a symbol's frame (the frame's `restore` already put
  the old brush back afterwards), and a symbol that needs a brush sets
  its own. Groups never reset: a group is a way to place commands, and
  it already kept its brush changes to itself. `"reset": true` on
  `brush` is the explicit way back. No reference picture changed: none
  relied on a symbol picking up the brush from outside.
- **"Painted most of the canvas" warning.** Every frame already kept
  the box of everything it drew; it now also keeps the box of its
  *shapes* (jobs with an outline or points: not a whole-layer fill or
  gradient, not the bucket). When a named group or use ends with its
  drawn box over half the canvas and its shapes under a tenth, the
  report warns and guesses the cause (a gradient or fill without an
  `area` inside a `not` mask). A group with no shapes at all (a sky
  gradient) doesn't warn: nothing there is misleading. Percentages are
  of the boxes, which is what an agent needs to find the command.

## 64. Outlines and rim light from the silhouette (phase 3, step 5)

- **An outline is a silhouette filter**, like a shadow: `Outline` and
  `Rim` are `Filter`s, so a group's `outline` joins its `filter` list
  and the same scratch mechanism handles both. In `lay_effect` the
  silhouette filters are laid in a fixed order: shadow, then outline,
  on the lowest layer under the parts; the parts; then the rim light on
  the highest layer, over them. The `filter` command takes `outline`
  too, for one layer.
- **Distances, not dilation.** The line is `size + 0.5 - distance` to
  the silhouette (pixels at least half opaque), from an exact
  Euclidean distance transform (Felzenszwalb-Huttenlocher, two linear
  passes), so corners are round, edges smooth, and the cost doesn't
  depend on the size. Rim light uses the distance *inside* the
  silhouette and the downhill direction of that field as the edge's
  normal; `facing` is its dot with the light direction. On a pixel
  canvas both are thresholded to whole pixels.
- **Cost** (release, 1920 x 1080, an ellipse covering most of the
  canvas: 0.04 s alone): outline 0.17 s; outline with rim 0.26 s; a
  blurred shadow 0.18 s.

## 65. `include` and the library (phase 3, step 6)

- **Includes are played, not spliced.** When the player meets
  `include`, it reads the file, parses it as a script (so the same
  checks apply), and takes its `define`s and palette: a symbol gets a
  source (file and command number) that shows up in messages and in
  the report's part paths; a palette colour gets its file too, for the
  "defined differently" message. Nested includes resolve relative to
  the file that includes them, a chain of real paths catches loops,
  and anything but `define`, `include` and `canvas` in an included file
  is refused: a library file draws nothing.
- **Included symbols and colours are never "unused"**: a library holds
  more than one picture needs, so they're left out of the report's
  `defined` and `palette` lists.
- **`seed` takes keyframes and parameters now** on every command that
  has one (a `Value`, read through `value()` and rounded), because a
  library part wants a `seed` parameter; this was also on the phase 2
  review list.
- **The library lives in `skills/art-app/library/`**, next to the
  gallery, so one folder can be copied as a skill; every part has an
  example and a preview, and a test plays every example and refuses
  warnings, so the library can't rot quietly.

## 66. Phase 2's review list, done (phase 3, step 7)

- **Animations in the order written.** `Script.animations` is a small
  ordered map (`Animations`, a vector of pairs with a map's methods)
  read through a custom deserializer, since JSON objects have an order
  and a script's author means it: the first animation is the one
  `render` shows and `check` summarises. Sprite sheets follow suit; a
  name written twice is refused.
- **Keys in seconds.** The clock carries the animation's `fps` and
  whether it counts in seconds; a key's time is a number (frames, or
  seconds with `"time": "seconds"`) or text like `"1.5s"`, and `shift`
  the same. Everything downstream still sees frames.
- **Seeds hold.** A keyed `seed` gets `"ease": "step"` unless the keys
  say otherwise: a pattern changes at a key, never half way, which is
  the only thing a seed in between could mean.
- **Entrances aren't warnings.** In `check`, an "entirely outside the
  canvas" warning for a part is dropped when the part is on the canvas
  in some other frame; one that never is still warns, with its frame
  range. A layer is "empty in every frame" only when no frame paints it.
  Painting by an animation's commands is numbered after the script's
  own ("animation command 2") instead of colliding with them.
- **Small things:** `export --animation` says which animations it
  skipped; `render --scale 0.5` saves `name-50pct.png` (not
  `-preview`, which is `--preview`'s); `render --pixels` prints a pixel
  canvas as rows of palette letters with a legend, so an agent can read
  a sprite without looking at it; the app's Text tool starts at a
  twelfth of the canvas height instead of a fixed 48 pixels.

## 67. The app catches up with the scripts (phase 3, step 8)

- **Tip and dynamics in the brush panel**, not a new tool: a `Tip`
  dropdown, an angle slider for shaped tips, and a collapsed "Dynamics"
  section with one slider per setting and a "No dynamics" button. They
  live on the same `BrushSettings` the scripts use, so Export strokes
  writes them through `record::brush` with nothing new.
- **Strokes get a seed** in the app: each stroke's number, seeded the
  way the player seeds a recorded `"seed"` (`record::stroke_seed`), and
  written down only when dynamics are on. A test paints a chalk stroke
  with dynamics, exports it and replays it to the same pixels.
- **Clip and lock are two small toggles** on each layer row, each one
  undo step ("Clip layer", "Lock layer paint"), recorded like show and
  hide. (The undo step naming had no case for them before: a clip-only
  change applied but left no undo step.)
- Checked on a virtual screen: the panel fits, the toggles light up,
  and the tip list shows all seven tips.

## 68. Proof of phase 3: the gallery (phase 3, step 10)

Two finished pieces in `skills/art-app/gallery/`, each with its script,
the painted picture, a process sheet and honest notes, listed in
`index.json` with tags; a test plays both and refuses warnings.

- **Sunset pond**: the simple pond (`examples/sunset-pond-simple.json`,
  written here: no such file was on `main`, so the "before" is this
  session's own plain version) repainted at 1920 x 1080 with noise
  skies and water, clipped shading, a glowing sun with a masked
  reflection, library parts, stamped grass, a drop shadow, a gradient
  map and paper; a 48-frame loop drifts the water and clouds, bobs the
  duck and sways the reeds.
- **Duck portrait**: the duck painted large at 1024 x 1024: noise-filled
  feathers, clipped shadow and light, stamped leaf feathers, bristle
  down, one outline with rim light, bokeh, a mood pass.

**Speed** (release build, this machine):
- the pond still at 1920 x 1080 with 20 snapshots: 8.7 s; without
  snapshots about 7 s; 48 preview frames (800 x 450) plus the GIF: 16 s;
  the quarter-size GIF: 7 s. A full-size loop would be about 6 minutes.
- the portrait at 1024 x 1024 with snapshots: 3.5 s.
- costliest single things: the big blurs on the clouds and reflections
  and the full-canvas noise fills; nothing is slow enough to change
  how one works.

**What was still hard, as the agent painting these:**
- **Water.** Noise plus ripple strokes gives texture, not water; a
  reflection is a blurred mirror. A real reflection breaks along the
  ripples: a "displace by noise" filter, or a mask that is itself noise,
  would get there in one line.
- **Feathers and fur.** Stamped tips in rows look like rows. A stamp
  that follows a flow field, or jitters each copy's position and angle
  more freely with overlap, would read as plumage.
- **Soft masks.** Masks are hard-edged shapes or silhouettes; a mask
  from a gradient or noise (feathered edges, the haze band on the
  hills) would remove several workarounds.
- **Seeing small things.** Judging a 1920 x 1080 picture from an 800 px
  preview hides the texture; `--crop` works but needs coordinates. A
  "crop around a named part" would save a round trip each time.
- **Palette clashes with includes** are refused loudly, which is right,
  but a library part's colours then have to be known; a `use` could
  take `colors` for all of them at once ("recolour with my palette").
- **Animated keyframes inside the script's own commands** work (the
  clock applies everywhere), but nothing in `check` says which of the
  script's commands move; the motion summary covers only the animation's
  own commands.

## 69. The intent block (phase 4, step 1)

- **A block, not commands.** `intent` sits at the top of the script next
  to `palette`, so a reader sees what the picture means before how it's
  made, and nothing in the command list depends on it. It's checked
  when the script is read (light words, key colours in the palette,
  2 to 6 of them) and resolved when the script is played: a `focal`
  name is looked up in the report's parts, so it's the middle of what
  that part actually painted, not where the painter thought it was.
- **Light as words, degrees or a point.** Eight direction words map to
  degrees clockwise (0 = from the right, like every other angle here);
  a point is for a visible light source; `behind` and `front` have no
  direction. Reports turn degrees back into the nearest word.
- `check` prints the intent and the focal point's canvas position, and
  warns (not fails) when the focal part doesn't exist: the picture
  still paints, the painter just learns the name is wrong.

## 70. The views sheet (phase 4, step 2)

- **Senses live in the engine, apart from painting.** `senses.rs` only
  reads pixels (`Grey`, `Heat`, the view pictures) and never touches a
  document or a player. Every number is deterministic: the same picture
  gives the same views, byte for byte (tested).
- **A fixed 32 x 32 grid.** The heat map (and everything step 3 measures)
  uses cells that are the same fraction of the picture whatever its size,
  so numbers compare between pictures. The weights (contrast 0.35, edges
  0.3, saturation 0.15, detail 0.2) are a model of attention, not the
  truth; every report says so.
- **Silhouettes from a second painting, never the first.** A part's
  silhouette is recorded by painting it on a scratch picture and laying
  it on the layers after, the way a filtered group is painted. That can
  change pixels in rare cases (a blend-mode brush or an eraser inside
  the part sees the scratch, not the layers below), so `views` paints the
  picture once plainly for every panel and once more with
  `PlayOptions::silhouettes` for the silhouettes alone. Looking never
  changes the picture; the price is one extra paint when the silhouette
  panel is wanted.
- **Outermost parts only, in script order.** Nested parts would be
  covered by the part around them, so the silhouette panel shows the
  parts `check` lists without `--detail`; their colours follow the order
  they start in the script, and runs of the same name are counted
  ("reed (x9)"). Eight colours, then they repeat (the label says so).
- **Thumb at 96 px, shown enlarged.** The file `name-thumb.png` is the
  real 96-pixel thumbnail (what reads at a glance); on the sheet it's
  magnified with sharp pixels so a person can see it.

## 71. The critique (phase 4, step 3)

- **Every threshold is a named constant** in `critique.rs`, so a line in
  a report can be read back to the rule that made it, and changing a
  rule is one number with a comment.
- **Tested against reality, and calibrated by it.** The design's numbers
  were tried on the two gallery pieces and changed where they lied:
  - *Value groups* were going to be runs of histogram bins. On a sunset
    gradient the histogram has no gaps, so the whole picture was "1
    group (mid 99%)" while 54% of it was dark. Groups are now the
    squint's five value bands that cover at least 4% of the picture: a
    count of big value shapes, which is what "three to five value
    groups" means to a painter.
  - *Focal contrast* was the focal cell against the median cell. The
    middle of a shape is flat; its edges carry the contrast. It's now
    the strongest cell within 4 cells of the focal point, over the
    median (floored at 0.02 so a flat picture gives a finite ratio).
  - *Out of key* was 50 degrees from every key colour. The gallery
    pond's lily pads, pulled towards yellow-green by the warm mood pass
    (hue 66 to 81), passed against the sun's pale cream (hue 47). Now
    a key colour sets a hue only when it's at least 0.35 saturated (a
    cream is a value, not a hue), the distance is 40 degrees, and a hue
    *between* two key hues the short way round is in key (dusk violets
    between an orange sky and blue water were being called magenta and
    out of key: a false alarm). With this the pads are flagged and the
    sky isn't.
  - *Detail at the focal point* was "aim for half or more there"; the
    pond's duck, a small subject in a landscape, had 22% within 4 cells
    (an area of 8%), which is plenty. The finding now fires when the
    share is under 1.5 times the area's share.
  - *Heavy on one side* was 5% off centre; the portrait at 55% down was
    "bottom-heavy". It's 8% now (the design's own example called 58%
    "a little", 62% heavy).
  - *Readability* judged every part; nine reeds, small on purpose, gave
    nine lines. Parts under 1% of the canvas are skipped unless they're
    the focal part, and the same finding on parts of one name is said
    once ("reed (x9) is too small").
  - The squint's blur saw black outside the picture, so a flat grey
    picture had a darker rim and a second value group; the blurred
    value is now divided by the blurred opacity, so edges keep their own
    value.
- **What the numbers catch and don't, on the gallery:** the pond's green
  lily pads out of key, yes; the duck's focal point and contrast, yes;
  the duck merging with the sun's reflection at thumbnail size, yes
  (a true weakness the notes hadn't named). The "smudge" clouds are
  not low-structure by the numbers (squint range 0.52: they have light
  tops and dark undersides; they look like smudges because of their
  soft edges, which nothing here measures). The portrait's "noisy
  marbled" body is not dirty by the numbers either: the marbling is
  low-contrast marks, not grain (grain 0.017 of the part's range), and
  the approved rule says only grain without structure is dirt. The haze
  band on the far hills isn't a named part, so nothing judges it. These
  are written here so nobody expects the critique to catch them.
- **Known false alarms on good pictures:** a soft background part (the
  portrait's bokeh) is "too small to read" at thumbnail size because its
  dots never fill half a thumbnail pixel; a blush or a glint that is
  the focal part would be too. Public-domain reference paintings could
  not be fetched in this environment (the network policy blocks
  Wikimedia Commons), so the thresholds are calibrated on the gallery
  and on synthetic pictures only; the reference folder is for step 4
  or later, once a picture can be brought in.
- **Silhouettes come from a second painting** (as for `views`), so the
  critique's picture is exactly the render.

## 72. Palette, values, compare, and the references folder (phase 4, step 4)

- **Pictures, not scripts.** The three study tools take PNG or JPEG
  files (`Image::load`, with the `jpeg-decoder` crate for JPEG), so a
  photograph or a painting from anywhere can be studied; a script is
  accepted too and painted first, so a study can be compared with the
  picture it studies.
- **Median cut, then one honest pass.** Median cut splits every box in
  half, so its shares are always 50/25/12...: true of the boxes, useless
  about the picture. After the cut each pixel is given to its nearest
  colour once and the colours and shares are recounted. Still
  deterministic (fixed split order: the widest channel, ties red before
  green before blue; pixels sorted before the split), and the shares
  now say which colour rules.
- **Five fixed greys, not an adaptive posterise**, so two pictures'
  values pictures and shares compare directly, and the bands match the
  critique's value groups.
- **Compare is two numbers, nothing cleverer.** Value structure is half
  the overlap of the five-grey shares and half the agreement (Pearson,
  mapped to 0..1) of a 4 x 4 grid of mean values; colour is the overlap
  of saturation-weighted hue histograms. They answer "is my study as
  dark, as arranged and as coloured as the reference", not "is it as
  good". The words add warmer/cooler and lighter/darker only when the
  difference is big enough to act on (15% of the warm share, 0.1 of the
  mean value).
- **The references: five paintings and four photographs.** The
  paintings are Wikimedia Commons photographs of works whose artists
  died over a hundred years ago (Monet's Impression, Sunrise; Van
  Gogh's Starry Night; Vermeer's Girl with a Pearl Earring; Friedrich's
  Wanderer; Hokusai's Great Wave), fetched once the environment allowed
  that host (Commons wants a descriptive User-Agent and one of its
  listed thumbnail widths; 1280 px, shrunk here to 1024). The
  photographs come from scikit-image's sample data (NASA and SpaceX
  public-domain photos, two CC0 photos). All have documented licences;
  the index's `allowed_licences` and the test keep the folder honest.
- **The critique's reality test on them.** Eight of the nine pass every
  rule with no threshold change: not flat, three or more value groups,
  a sensible hue count, the visual weight near the middle. The chelsea
  cat is 58% mid-value and still isn't "flat" (the rule looks at the
  middle three of ten bins, 70%), the right call. The one false alarm
  is Monet's Impression, Sunrise: 76% of it sits in the middle bins and
  it has two value groups when squinted, so the numbers say "flat", and
  the painting is a masterpiece *because* it is nearly one value with
  one small warm sun. A test keeps that false alarm on record rather
  than bending the threshold to hide it: a flat finding means "look
  for the one accent that would carry this", not "add contrast".
  Friedrich's Wanderer is dead centre on purpose, another reminder
  that a composition finding is a reason to look, not a rule.

## 73. The critique after review: direction, layers, lights, the haze band (phase 4)

The reviewer found two known mistakes still passing, a focal-contrast
number that always said "good", a heat map blind to big soft lights, and
two false alarms. Every finding, before and after:

- **The marbled duck body (portrait).** Before: "blotchy over 0%", not
  flagged: marks were only 1.5 to 6 px, the marble is 20 to 90 px, and
  nothing measured direction. After: four bands of fineness (grain
  0-1.5 px, marks 1.5-6, blotches 6-24, blobs 24-96), each judged for
  *direction* (the structure tensor's coherence over a window as big
  as the blobs: 1, 5 and 9 cells; a single blob in a small window
  looks like a slope, so the window must hold several) and for *edges*
  (the 95th-percentile gradient over the mean, on the picture blurred
  to the band's scale: smooth blobs 1.5 to 2.5, strokes more). A cell
  is blotchy when some band with energy is both directionless
  (coherence under 0.5) and soft (sharpness under 2.5), and a part when
  over half its cells are. The duck: blotchy over 59% (coherence 0.39,
  sharpness 1.7 at the blob scale, while its fine hatch is 6.7 sharp
  and 0.94 coherent: judged per band, the hatch doesn't excuse the
  marble). Three things had to be learned on the way: the part's
  silhouette edge (its outline) dominates the tensor, so texture is
  judged 8 px in from the edge; the fine hatch made the whole part look
  sharp, so sharpness is per band; and direction over one cell is
  meaningless for blobs bigger than a cell.
- **The smudge clouds (pond).** Before: squint range 0.52 on the
  picture, "has a light and dark side", not flagged, because the sky
  gradient behind the semi-transparent clouds was being measured.
  After: texture and structure are judged on a part's *own paint*
  (kept by the silhouettes pass: every layer's share of the group laid
  together), and on every *layer's* own paint too. The clouds' own
  paint: blotchy over 54% (coherence 0.34, sharpness 2.0): soft blobs
  with no direction, which is what a smudge is.
- **The haze band (pond, far hills).** Before: never checked (not a
  named part). After: every layer is checked, and a new edge measure:
  how soft a part's own silhouette is (255 minus its own opacity over
  the first 3 px inside, per side) and how much lighter the picture is
  than the own paint just inside a soft side (the background showing
  through). The far hills (blur 5 on the whole group): softest edge
  100, the sky shows through +0.08 on the lower left: "reads as a haze
  band". A first version compared the picture's fringe with its
  interior "beyond what the own paint does there", and flagged the
  portrait's duck: its clipped highlight layer lightens the upper
  right edge in the picture but not in the own paint. The rule now
  needs the own edge to be soft, which an outlined duck's isn't.
- **Focal contrast.** Before: "16.0x the rest" on both pieces: the
  median cell is flat, the ratio sat on its floor and always said
  "good". After: the strongest contrast near the focal point (within
  4 cells, or inside the focal part's box) against the strongest
  anywhere else, named: "the focal point has the strongest contrast
  (1.5x the next strongest, at sun reflection)" or "the strongest
  contrast is elsewhere: bokeh is 1.1x the focal point's".
- **The heat map and the sun.** Before: the pond's sun almost cold
  (no edges, no detail, little saturation). After: a fifth term,
  *standout*: the cell's mean brightness against the ring of cells 2
  to 6 away (a ring, so a shape 5 cells wide is compared with what's
  around it, not with itself; with a solid 3-cell surround the sun
  scored 0.9 of the duck, with the ring 0.96 of the maximum). Weights
  are now contrast 0.25, edges 0.2, saturation 0.15, detail 0.15,
  standout 0.25. The sun is the second-warmest area; the eye still
  goes to the duck first, which has edges, saturation and contrast.
- **The sun "flat".** Before: "sun is flat over 47% of it". After: a
  part whose mean value is 0.3 above the ring around it is a *light*:
  flat, noisy, blotchy and smudge are not findings for it (the detail
  line says "a light").
- **Sparkles as grain.** Before: a few bright 2 px glints made a cell's
  mean grain high. After: a cell's grain is the median over its pixels,
  so a few sparkles don't move it and dense noise does (tested with a
  checker against a disc with twenty sparkles).
- **Also:** layers aren't judged "flat" (a sky may be smooth; a named
  part that's flat is a shape with nothing inside), and a layer that is
  one colour with a soft opacity (a vignette, a glow) is a tint with
  nothing to judge.
- **Known limits, on the references.** Judging each reference as one
  whole part: none is noisy but Monet (the canvas grain of a photograph
  of an impressionist painting at 1024 px reads as fine grain), and Van
  Gogh's Starry Night is "blotchy over 87%": his curving strokes at
  this size are soft and point in every direction within a window, so
  the numbers can't tell them from a noise fill. Photographs of soft
  things (a face, fur, a cup) read as soft blobs at the biggest scale
  too. Hokusai, Vermeer and Friedrich pass. A test keeps the Van Gogh
  result on record so a change that hides it is noticed. The rule the
  numbers can't learn: blotchy is a reason to look; if the blobs are
  strokes you meant, say so and move on.
- **Cost.** The critique paints the picture twice and measures every
  layer and part: the pond takes about 14 s now instead of 4.

## 74. The learning folder (phase 4, step 5)

- **One folder, copyable.** Everything an agent learns from lives in
  `skills/art-app/`: the short entry point, lessons, mistakes with
  pictures, styles, references with studies, exercises with answers, the
  library and the gallery. Nothing in it depends on files outside the
  repository, so it can be installed as a skill as it is.
- **Lessons are earned, not written.** `lessons.md` says how a lesson
  gets in: the person agrees, or a reviewer that didn't paint the piece
  proposes it in the piece's notes and the before/after numbers back it.
  The first thirty come from phase 3's notes and phase 4's critique and
  references, agreed at the design review; each names the piece or
  picture that shows it.
- **Mistakes keep their before pictures now and get their after
  pictures with the repaints** (step 9). Each entry quotes the critique
  line that catches it, or says the numbers don't (rope feathers, the
  pillar reflection): so a painter learns both what to look for and what
  the numbers can't see.
- **The gallery command does the bookkeeping.** `gallery add` paints a
  piece in place, saves the process and views sheets, the critique as
  printed and a blind-read skeleton, keeps notes if there are any (adding
  the "Proposed lessons" section) and writes the index entry with tags
  from the intent's mood and the tools the script uses. Includes are
  rewritten so the moved script still finds its library parts. `gallery
  rate` is the only way a rating gets in, and the rating survives a
  re-add. The index keeps key order (`preserve_order`) so a person can
  read it.
- **Exercises are judged by the same critique as everything else**, and
  their pages say which findings are expected ("flat" in a value study,
  "bottom-heavy" when figures stand on the ground) and which would mean
  something is wrong. Writing them turned up two false alarms in the
  critique and fixed them: a cup made of a few flat ellipses was
  "blotchy" (the band's blur dragged the shape's own outline into its
  texture: the edge margin is now per band, 8, 12 and 30 px), and a
  see-through cast shadow under an opaque body was a "soft edge" (the
  rule now wants an opacity *ramp*, rising at every depth, which a blur
  gives and a step of shadow doesn't).
- **Blotchy, calibrated once more.** The direction windows [1, 5, 9
  cells] and the stillness measure (the share of pixels whose gradient
  is under half the window's mean, from log histograms so windows sum)
  replace sharpness; the share that makes a part blotchy is 40%, where
  the marbled duck body and the smudge clouds sit at 42% and the
  smooth-form references as whole pictures at 26 to 34%. The margin is
  small and the gallery test pins it. Judged as one whole part, Van
  Gogh's strokes and Hokusai's lines everywhere at the 24 to 96 px band
  are "blotchy" too: a whole picture is not a part, and the test keeps
  the Van Gogh result on record as the limit.
- **Study scripts** for every reference block in the big shapes with
  the reference's own palette in about twenty commands; `compare` puts
  them at 63 to 89 on values and 17 (Friedrich's near-grey picture,
  where the hue histograms barely exist) to 91 on colour. The point of a
  study is the value structure; the colour score says whether the key
  is the same, not whether the study is good.

## 75. The painting process and the leak-proof blind read (phase 4, step 6)

- **Seven stages, each with its check.** The process in the skill and
  the guide is the order a painter who knows the craft works in: intent,
  thumbnail, values, colour, light and form, detail at the focal point,
  finish. Each stage names what `views` and `critique` should show
  before the next one starts, so the senses are used as a painter uses
  a mirror and a squint, not as a score at the end. The detail budget
  is written down because it's the mistake the agent makes most: detail
  everywhere, little at the focal point.
- **The blind read leaks nothing by construction.** The packet is a
  folder of its own with two files: the picture as `piece-<4 hex>.png`
  and `questions.txt`. The four characters are an FNV-1a hash of the
  pixels (the same picture gets the same packet; nothing in the name
  says what the picture is), and a test checks that neither the folder
  name nor the questions contain the subject, the title, the mood or the
  word "script". The reader is given the folder and nothing else; the
  skill and the guide say in so many words never to send the script,
  the notes or a named file. The record (`--record` with `--verdict`)
  writes the questions, the answers as given, the intent they're
  measured against and the verdict into `blind-read.md` next to the
  script, so the gallery keeps it; the verdict is "reads" or "doesn't
  read: why", and a free-form verdict is refused so the record stays
  comparable.
- **No model calls from art-cli.** The reader is another agent or a
  fresh session by procedure (the design's rule): art-cli only makes
  the packet and keeps the record.

## 76. Faster animation: static layers painted once (phase 4, step 7)

- **The rule is per command, then per layer.** A command is
  frame-dependent when its JSON contains a `keys` array anywhere (a
  keyframe), or when it belongs to the animation's own commands (they
  run once per frame by definition). A layer is dynamic when a
  frame-dependent command paints it, or when a command reads it (a
  layer mask, a bucket fill, a `from: below`) before the layer's last
  painter has run: then the reader would see a half-painted layer in
  frame 0 and a finished one later, so the layer must be repainted. A
  static command that reads a dynamic layer becomes dynamic too, and the
  fix point is run until nothing changes. Every other command paints
  only static layers and is skipped from the second frame on; its layer
  is restored from the first frame's pixels at its restore point (just
  after its last painter) so later readers see the same thing in every
  frame.
- **Same pixels, by test.** `crates/engine/tests/reuse.rs` paints every
  animated example and the gallery loop with and without reuse at frames
  0, 1, 3 and the last, and asserts pixel-identical frames, the same
  parts, the same `painted_by` and the same warnings. The pond loop must
  skip at least ten commands, so a change that quietly disables the
  optimisation fails the test.
- **The numbers.** The sunset pond loop (48 frames, 1920×1080): 88 s
  for a full paint, 57 s with reuse, "12 of 20 layers painted once, 17
  of 55 commands skipped in later frames". The duck walk (every layer
  moves) gains nothing and loses nothing: 0.05 s either way. The
  remaining pond time is the dynamic layers (the water with its moving
  reflection, the mood layer reading everything below it) and the
  per-frame compositing, which a later phase could cache too.
- **Where it is used.** `render` (frames, frame sheets, onion skins),
  `export` and the app's animation view all paint through a `Sequence`
  that keeps the reuse between frames. `check`'s paint of every frame
  does not: it must see each command run to report motion steps and
  warnings per frame, and its time was acceptable. `ART_NO_REUSE=1`
  disables the reuse everywhere, for comparing.
- **The first attempt was wrong** in the summary's sense: the mood
  layer's `from: below` read forced every layer dynamic and only two
  commands were skipped. The per-layer restore point (restore after the
  last painter, not at the start of the frame) is what makes a layer
  read by a later effect still static.

## 77. The benchmark (phase 4, step 8)

- **Four fixed subjects, six scores, three scorers.** The subjects
  (sunset pond, forest at dawn, character portrait, pixel walk) have a
  brief and the intent to use, so a phase 5 pond is comparable with a
  phase 4 pond. Each score has a sentence per step so the numbers, a
  separate agent and the person mean the same thing by a 3. The
  person's rating outranks every number, as everywhere in phase 4.
- **The numbers' mapping is fixed and written down** in
  `exercises/benchmark.md`, and `critique --score` implements it, so a
  score can be re-derived from the critique and a change to the critique
  shows up as a change in the baseline rows.
- **The baselines show the gap the numbers can't close.** The numbers
  give the flat phase 3 example pond 5, 5, 3, 5, 4; the agent gives it
  3, 3, 3, 3, 2 and overall 2 ("a clean diagram"). The numbers give the
  duck portrait readability 2 because two small named parts don't read
  at 96 px; the agent gives it 5. Both are recorded in
  `gallery/benchmark.md` with the reason, as the design said: where the
  scorers part ways is a lesson. The lesson from the portrait is for
  the process (name the parts that must read), not a change to the
  measure.
- **Overall appeal is never a number.** The numbers' row leaves it
  blank; only the agent and the person give it.

## 78. Prove it: three pieces painted with the process (phase 4, step 9)

- **Scripts in stage order.** All three gallery pieces (`sunset-pond-v3`,
  `duck-portrait-v2`, `forest-at-dawn`) create their layers first, empty,
  and then each stage selects them by name: values (grey shapes, a
  `values` snapshot), colour, light and form, detail, finish. The
  process sheet therefore shows the painter's stages, not the layer
  order, and `views` and `critique` were run on the script cut at each
  snapshot. Two things this exposed: a shape painted twice (grey, then
  colour) must jitter with the same `seed` or a grey sliver shows; and a
  values-stage shape on a screen layer stays visible under the colour
  (the pond's grey "egg" under the sun).
- **The critique judges a named part's flatness on the picture inside
  it.** The process puts a part's shading on clipped layers above its
  flat local colour, so the portrait's head was "flat over 100%" and
  "no light and dark side" on its own paint while it was plainly shaded.
  Now a named part's `flat` and `squint_range` come from the picture's
  field inside the part; a layer's still come from its own paint (the
  earlier decision stands: a thin cloud layer over a gradient must not
  borrow the gradient's light and dark). Grain, marks and blotches stay
  on the own paint. The reference tests and the layer test pass
  unchanged.
- **Named parts inside a part merge by construction.** `head`, `beak`
  and `wing` inside a yellow `duck` were "merging with their
  surroundings" at 96 px, because their surroundings are the same
  yellow. The readability score counts named parts, so the portrait
  names only `duck` and puts the focal point at `[600, 400]` (the
  intent's `focal` takes a point). The lesson is in the notes; the
  measure is unchanged.
- **A drop-shadow filter is part of the part's own paint.** The pond's
  duck was "blotchy over 80%" because its `shadow` filter is a soft blob
  with no direction inside the part's mask. The shadow is now its own
  shape on its own layer under the duck. Deliberate: a part's own paint
  is what the group painted, filters included, and the critique is
  right that a drop shadow is a blob; the painting changed, not the
  sense.
- **The blind reads all read**, by fresh readers given only the packet:
  "a lake scene at sunset ... my eye goes first to the duck", "a
  cartoon-like yellow duck ... the eye and beak", "a misty forest ... a
  small dark silhouette of a four-legged animal ... something like a
  deer or a young fawn". Each reader also named real weaknesses
  (the pond's duck painted in a different style from the cut-paper
  rest, the portrait's wing "a scribbled blotch", the deer "crudely
  drawn", two warm trunks that look like an unfinished pass), kept in
  each piece's `notes.md`.
- **The benchmark after.** Numbers: pond v3 4, 5, 4, 5, 5 (v2 was
  4, 5, 2, 4, 4); portrait v2 5, 4, 5, 4, 4 (v1 was 2, 4, 5, 5, 3);
  forest 5, 5, 4, 5, 3. A separate judge, stricter than the baseline's:
  pond v3 4, 3, 4, 4, 3, overall 3; portrait v2 4, 3, 4, 4, 3, overall
  3; forest 4, 4, 4, 5, 4, overall 4. The same judge re-scored the two
  phase 3 pieces in the same sitting: pond 3, 3, 3, 2, 2, overall 2;
  portrait 4, 3, 3, 3, 2, overall 2. So with one reader the repaints
  gained one to two steps on focal point, colour and detail, nothing on
  values (the lights still go to the sun, the clouds, the bokeh: lesson
  3), and went from "only for the lesson" to "fine". The forest, painted
  fresh with the process, got "yes". The honest reading: the process
  removes mistakes and puts the eye where it was meant to go; it does
  not yet put the lightest light on the subject, and "fine" is not
  "show it".
- **Mistakes closed.** All six entries in `mistakes.md` have an after
  picture from the repaints; five are closed by the critique
  (`out of key: nothing`, no blotchy, no edge finding) and one (the
  rope feathers) is only half closed: no rope, but the wing still reads
  as a smear. Lessons 8, 9, 11, 16 and 19 carry their before/after
  numbers.

## 79. Art App becomes Duck Quill (phase 5, step 1)

- **New names, old names still work.** The app is Duck Quill
  (`DuckQuill.exe`), the command line tool `quill`, pictures `.quill`
  (`"format": "duckquill"` inside), scripts `"format":
  "duckquill-script"`, the agents' folder `skills/duck-quill/`, the
  crates `quill_engine`, `quill_app` and `quill_cli` (folders unchanged).
  `.artapp` pictures open, and Ctrl+S writes them back under their own
  name; Save As offers `.quill` but keeps a typed `.artapp`. Scripts
  saying `artapp-script` play with no warning; only `quill check` adds a
  note. Everything above this entry describes the past and keeps the old
  names.
- **`art-cli` is a second program, not a copy.** The CLI's code moved
  into a library (`lib.rs`) with one `run` function; `quill` and `art-cli`
  are three-line programs that call it, so they can't drift apart.
  `art-cli` prints `art-cli is now called quill` on stderr, not stdout,
  so programs that read its output see what they always saw.
- **Fixtures made before the rename** (`crates/engine/tests/fixtures`):
  an `.artapp` picture with a clipped multiply layer, a screen layer and
  an alpha-locked layer, the PNG it exported, and the `artapp-script`
  that painted it, all made with the old `art-cli`. Tests open the
  picture, play the script and compare every pixel with that PNG, and
  run `art-cli` and `quill` side by side.
- **The brand, lightly.** The exe's icon (`build.rs`, Windows only) and
  the window icon come from `brand/`; Help → About shows the logo shrunk
  to 700 px (about 300 KB each) so the exe stays small; View → Theme
  offers dark, light or the system's. Only "chosen" things (selected
  tool, selected layer) use night indigo, and the main button of a
  dialog duck yellow: the canvas stays the star.
- **The version** (0.5.0) is written once, in the workspace
  `Cargo.toml`; About and `quill --version` read it.

## 80. Download and run (phase 5, step 2)

- **CI** (`.github/workflows/ci.yml`) builds, tests and runs clippy with
  warnings as errors on Windows and Linux for every push and pull request,
  with the Rust version pinned (1.97.0) so a new Rust can't turn it red
  on its own.
- **The release** (`release.yml`) runs on a `v*` tag: it checks the tag
  matches the version in `Cargo.toml`, builds on Windows and publishes
  `DuckQuill-<version>-windows.zip` with `gh release create` (no
  third-party release action). A push that changes the release files
  builds the same zip as a run artifact without publishing, so the zip
  can be checked before any tag.
- **The sketchbook stays out of the zip.** The zip carries
  `skills/duck-quill/` for agents, minus `references/sketchbook/`: those
  are the owner's own drawings, kept in the repository to study but not
  shipped in an app.
- **Unsigned for now:** `READ ME FIRST.txt` explains SmartScreen's "Run
  anyway".

## 81. Selections (phase 5, step 3)

- **One code path for the app and scripts.** A selection is the same
  8-bit `Mask` a script's group `mask` already used, so soft edges come
  free and every painting job takes one clip. The app makes each change
  as a `select` command and runs it with `selection::run`; the script
  player reads the command with the same `Request::read` and combines with
  the same `selection::apply` (it only builds the shape itself, because a
  script's shape takes parameters and is placed by its groups). A test
  replays the app's selections as a script and compares the pictures.
- **Undoable, not saved.** Each change is an undo step (the owner's
  choice), recorded as a `select` command for Export strokes. The
  selection lives on the document but not in the file.
- **What obeys it:** brush, eraser, paint bucket, gradient, text, the
  Filters window, Clear layer (which then clears only the selection), and
  in scripts every painting command. The script `filter` command obeys a
  `select` selection but still not a group's mask, so old scripts paint
  the same pixels.
- **An empty selection is no selection**, in the app and in scripts
  (with a warning there): subtracting everything deselects.
- **Alt means two things.** With a selection tool it takes away from the
  selection; with every other tool it still picks a colour.
- **Marching ants** are the selection's edge along pixel sides (from half
  selected up), joined into rings once per selection change and drawn as
  white with black dashes that move: visible on any colour, and smooth
  around curves.
- **Copy, cut, paste.** Copy takes the selected pixels (each as much as
  it's selected) and puts them on the Windows clipboard too, with
  `arboard`, which egui already used for text. Paste goes onto a new layer
  at the place it was copied from; a picture from another program goes in
  the middle of the view. Ctrl+X, C and V arrive from egui as Cut, Copy
  and Paste events, not keys, so the shortcut reader takes those.
- **Moving a selection** lifts its pixels onto a temporary layer and uses
  the existing transform tool; Enter puts them down, moves the selection's
  mask the same way, and removes the temporary layer, all one undo step.
- **Not everything is a script.** Paste and moving a selection have no
  script command. They're marked when recorded (`push_unrecordable`), and
  Export strokes says in its notes how many changes are missing, instead
  of silently replaying a different picture.
- **Speed, 4000 x 3000 with 20 layers** (`tests/big_canvas.rs`, release):
  a long size-60 stroke 96 ms without a selection and 88 ms inside a
  feathered ellipse (no measurable cost); a blur 549 ms plain and 681 ms
  inside a selection (the mixing pass); making a canvas-sized ellipse
  selection 190 ms, feathering it 150 ms, its outline 48 ms: once per
  change, not per stroke.

## 82. Layers you can see (phase 5, step 4)

- **Thumbnails from changed tiles only.** A surface keeps a second list
  of changed tiles for the thumbnails, apart from the screen's (which the
  canvas empties every frame). The Layers panel redoes only the
  thumbnail pixels over those tiles, and only when no stroke is in
  progress. A thumbnail fits 44 x 32 points; on a 4000-pixel-wide canvas
  a tile is about three thumbnail pixels, each the average of 16
  samples, so a whole thumbnail is about 22,000 lookups.
- **Drag by a handle.** Each row starts with a grip of dots to drag;
  dragging the whole row would fight with click-to-select and
  double-click-to-rename. A line in night indigo shows where it will
  land; the move is `MoveLayerCommand`, undoable and recorded.
- **Drawn icons.** The eye and the padlock are drawn with lines, not
  emoji or font symbols, so they look the same on every machine. The
  Layers panel is 50 points wider for them.

## 83. Brush presets and the pencil (phase 5, step 5)

- **A preset is a `brush` command with a name.** The file holds a name,
  a description, whether it's an eraser, the smoothing and a `brush`
  block written exactly as in a script, so there's one way to read and
  check settings (`apply_brush_fields`, shared with the player). A preset
  has no colour: choosing one keeps the colour you had.
- **Built-in presets live inside the program** (`crates/engine/presets`,
  read with `include_str!`), so `{"cmd": "brush", "preset": "..."}` paints
  the same on every machine. Your own presets are not available to
  scripts for the same reason; Export strokes writes their settings out.
- **Your presets are files** in `%APPDATA%\Duck Quill\presets` (on Linux
  `~/.config/duck-quill/presets`), one `.json` each, named after the
  preset with characters Windows forbids replaced; `order.json` keeps the
  grid's order. A file that can't be read is reported and skipped.
  Built-in ones can't be renamed or deleted; Duplicate makes a copy you
  can change.
- **What makes the pencil a pencil.** Four new brush settings, all off
  by default so older brushes paint exactly as before:
  - `pressure_size_min` / `pressure_opacity_min`: what the lightest
    touch keeps. The pencil keeps 85% of its width but only 15% of its
    darkness, so pressure is shading, not thickness.
  - `pressure_curve`: 1.4 for the pencil, so you press harder for dark.
  - `grain`: paper tooth, fixed to the canvas (not the stroke), so every
    stroke meets the same bumps. Revised after the owner's review (the
    first version read as a speckled marker: a crisp capsule with round
    white dots, and the hard stroke as speckled as the light one):
    - the bumps are 1 to 6 pixels, stretched about 3:1 and turned 30°,
      like paper fibres, instead of round;
    - their heights are spread evenly by rank, so a threshold is simply
      the share of paper left white: about half at a light touch, a few
      specks pressing hard (`BrushSettings::tooth`);
    - the threshold rises towards the dab's edge (and the pencil's
      hardness is 0.1), so the outline is ragged and soft;
    - graphite already on the layer, at the pixel or around it, lowers
      the threshold, so going over it again fills the valleys, as
      graphite smears into the tooth.
    The aim is `skills/duck-quill/lessons/pencil-aim-for.png`.
  - `build_up`: the most the layer darkens when you go over it again
    (0.8 for the pencil: shiny grey, never black).
- **Recorded brushes always carry the new settings** (and `tidy` drops
  them while they stay plain): writing them only when used meant a plain
  brush after the pencil played back still grainy.
- **Previews** are real strokes painted with the engine on a 144 x 64
  surface (shown at half size), light-hard-light, sizes squeezed to fit;
  each is drawn once and again only when its settings change.
- **Cost.** On the 4000 x 3000, 20-layer test (`tests/big_canvas.rs`), a
  3600-pixel stroke at size 60 takes about 230 ms with the pencil against
  115 ms with the plain brush (the limit is 2.5 times): the tooth is
  only worked out where a dab could still darken a pixel, its angle
  once per dab, and the pencil's dabs are as far apart as the plain
  brush's. That stroke takes seconds to draw by hand, so the pen never
  waits; at the pencil's own size (5) it is a small fraction of that.

## 84. Symmetry (phase 5, step 6)

- **Mirrored dabs, one stroke.** `Stroke::mirrored(Symmetry)` stamps
  each dab again across the lines, with the same random variation and a
  mirrored angle. Because the copies belong to the same stroke they
  share its coverage: where they overlap near a line they don't darken
  each other, which separate mirrored strokes would. Undo is one step.
- **Scripts get it too** (`"symmetry": {"x": .., "y": ..}` on `stroke`
  and `erase`), so Export strokes replays the app's mirrored strokes
  exactly. Inside a group the lines move and turn with the group.
- **The lines live as fractions of the canvas**, so they stay in the
  middle (or wherever they were put) on a new or resized picture. They
  are dragged by a round handle at the line's top or left end (or where
  the line comes into view), never by grabbing the line itself, which
  would fight with painting next to it. The grab uses where the button
  went down, not where the pointer is a frame later.
- **Brush and eraser only**: a mirrored fill or gradient would be
  surprising, and the lines are only shown for those two tools.
- **Cost.** A four-way stroke costs four times a plain one (421 ms vs
  110 ms for the 3600-pixel size-60 stroke on the 4000 x 3000 test).

## 85. Reference pictures (phase 5, step 6)

- **Kept in the document, not painted.** Each reference keeps the file's
  bytes as they were (PNG or JPEG), the decoded picture and how it's
  shown (zoom, centre, opacity, window). Scripts, exports and the
  layers never see them.
- **Saved in the `.quill` file** as `references/<n>.png|jpg` beside the
  layers, with a `references` list in `document.json` written only when
  there are some. The format version stays 1: older versions of the app
  ignore both, as they ignore anything they don't know, so files keep
  opening both ways. A reference that can't be read is left out rather
  than refusing to open the painting.
- **Pinning and removing are undo steps** that write no script (they
  aren't changes to the painting, so Export strokes doesn't count them
  as missing). Zooming and moving a reference aren't undo steps.
- **Own windows, not a panel**, so a reference can sit right next to the
  part being painted, and several can be open. Pictures over 4096
  pixels are shown shrunk (graphics cards limit texture sizes); colours
  are picked from the full picture. The wheel over a reference zooms the
  reference, not the canvas.

## 86. Autosave, recent files, what's remembered, the shortcut sheet (phase 5, step 7)

- **Autosave copies, it never saves.** Copies go into the settings
  folder's `recovery` folder, never next to (or over) your files, and
  are deleted when you save or choose not to. A copy is made only when
  the picture has changes the last copy doesn't have
  (`History::generation`), at most every two minutes, and only after two
  seconds without input and with no stroke or script running. The
  document is cloned in that quiet moment (about 100 ms for 4000 x 3000
  with 20 layers) and compressed and written by a background thread, so
  painting never waits for it.
- **Telling a crash from a running Duck Quill.** Each run holds a lock
  on its own `<session>.lock` file (`File::try_lock`); the system lets go
  of it when Duck Quill stops, even in a crash. A copy whose lock can be
  taken was left behind, so it's offered at once on the next start, and
  two Duck Quills open together never offer each other's work. (A first
  version used a "still alive" time stamp, which made a crash wait three
  minutes before its copy was offered.)
- **Restoring** opens the copy as unsaved (`History::mark_unsaved`), tied
  to the file it came from, so Save writes there; the copy becomes the
  new run's own, so it stays safe until then. Every remembered file
  (the note, the open picture, the recent list) is kept as a full path,
  so "../r/pen.quill" from a command line still means the same file
  when Duck Quill starts in another folder.
- **Tidying:** a run that crashes with nothing unsaved leaves only its
  lock file. On start, locks that nobody holds and that have no copy
  beside them are deleted.
- **Settings** are one readable `settings.json`: the window's size, the
  brush and eraser (stored as presets, so they're checked like any
  preset), colours, theme, the Presets section and Hide reference
  pictures, and the recent files. Written on closing, when the recent
  list changes, and every 30 seconds in a quiet moment if anything
  changed (so a crash doesn't forget the brush). A broken file means
  the defaults; unknown fields are ignored.
- **The shortcut sheet** is one table in `shortcut_sheet.rs`; a test
  checks every tool key and the main shortcuts are on it once.

## 87. Fixes from the game tests (phase 5, step 7b)

- **Darkening layers in transparent exports.** Multiply, Darken and
  Color Burn now only darken what's below them: over an empty pixel
  they leave it empty, and over a half-see-through edge they keep it as
  see-through. The usual compositing rule paints a multiply layer's own
  colour where there's nothing below, which left a dark band beside
  sprites whose shadow layer was painted a little too big. Over paper
  (always solid) the formula is exactly the old one, so every picture
  rendered on paper is unchanged; only the transparent reference of the
  all-commands script changed, where a multiply texture used to show
  over empty sky.
- **Uneven scales keep lines unstretched.** Brush and outline sizes in
  a group used to grow by the average of the two scales (the square
  root of their product), so a part stretched [3, 1] got lines 1.73
  times fatter all round. They now follow the smaller scale (the
  transform's smaller singular value, so turning doesn't change it):
  [3, 1] keeps lines as drawn, and an even scale is exactly as before.
  `"stretch_brush": true` on a group or `use` brings back the average.
  `check` warns once per group when its scales differ by 1.5 times or
  more. Squash-and-stretch frames have slightly thinner outlines on the
  squashed frames: the duck-walk, duck-sheet and title references were
  updated for it.
- **Scale 0** shrinks a part to nothing instead of being an error: its
  commands still run (a layer it makes, a symbol it defines) but paint
  at no opacity, and `check` reports no area for it. For
  `transform_layer` it stays an error (squashing a whole layer can't be
  undone; `clear_layer` empties one).
- **Symbol parameters in any field.** A `define` with `params` keeps its
  commands as written; one that can't be read with its `"$name"`s in
  place (a point, a width, a brush size) is read with the values put
  in: the defaults when the script is read, each `use`'s own values
  when it plays (cached per set of values, so a repeat or an animation
  reads it once per value). Commands that read fine as written are
  unchanged, so their messages are too. Scripts are written back out
  from the commands as written, so the `"$name"`s survive Export
  strokes. A `"$name"` where a number is expected outside such a define
  now says it would be a parameter, and where.
