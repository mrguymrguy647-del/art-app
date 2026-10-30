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
