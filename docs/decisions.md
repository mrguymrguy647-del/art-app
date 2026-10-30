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
