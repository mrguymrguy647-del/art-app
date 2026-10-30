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
