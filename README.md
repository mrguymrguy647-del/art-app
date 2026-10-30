# Art App

A digital painting app for Windows, written in Rust.

**Status:** Phase 1 is complete, plus a paint bucket, gradients, blend modes, and **painting
scripts**: pictures described as text, step by step (see [docs/script-format.md](docs/script-format.md)). Paint and erase with a round brush (pen pressure changes size
and opacity) on layers, pick colours from a colour wheel or the canvas, undo and redo, zoom, pan
and rotate the canvas, and save, open and export your pictures. The pen test is under
**Pen test** in the top bar.

## Run it on Windows (no terminal needed)

### One-time setup

1. **Install Rust.** Go to <https://rustup.rs> and download and run `rustup-init.exe`.
   - If it asks about the *Visual Studio C++ prerequisites*, type **1** (Quick install) and press
     Enter. A Visual Studio installer window opens and installs what Rust needs; let it finish.
   - When it asks how to install Rust, just press **Enter** (standard installation).
   - Restart your PC afterwards.
2. **Get the code** with [GitHub Desktop](https://desktop.github.com):
   **File → Clone repository → art-app**, local path `C:\dev\art-app`, then set
   **Current branch** to `claude/stoic-cannon-vyu6qr`.

### Every day

Open the `C:\dev\art-app` folder in File Explorer and double-click:

| File | What it does |
|---|---|
| `run.bat` | Builds the app (only what changed) and starts it. The very first time takes a few minutes. |
| `create-shortcut.bat` | Puts an **Art App** icon on your desktop. Do this once, after `run.bat` has worked. |
| `test.bat` | Runs the painting engine's tests. |
| `render-script.bat` | Turns a painting script into a picture without opening the app: **drag a `.json` script onto it**. Double-click it to render the example, `examples\sunset.json`. The picture (and a contact sheet of every stage) is saved in a `render` folder next to the script and opened. |

**Getting new versions:** in GitHub Desktop click **Fetch origin**, then **Pull origin**, then
double-click `run.bat` once. Your desktop shortcut then opens the new version.

### Canvas controls

| Action | Mouse / keyboard | Touchscreen / touchpad |
|---|---|---|
| New picture | **File → New…** or `Ctrl+N`: choose a size (or a common size) | Tap **File** |
| Open / save | **File → Open…** (`Ctrl+O`), **Save** (`Ctrl+S`), **Save as…** (`Ctrl+Shift+S`). Pictures are saved as `.artapp` files, with all layers | Tap **File** |
| Export a PNG image | **File → Export PNG…** (`Ctrl+E`) on the paper colour, or **Export PNG, transparent…** (`Ctrl+Shift+E`) with no paper | Tap **File** |
| Watch a painting script being painted | **File → Play strokes…** and pick a `.json` script. A bar at the bottom has **Pause**, **Skip to end**, **Stop** and a **Speed** slider. Everything it paints is **one** undo step. While it plays you can zoom and move around, but not paint | Tap **File** |
| Save what you painted as a script | **File → Export strokes…**: everything painted since the picture was started or opened (not what you undid), as a `.json` painting script that can be played again or read and continued (for example by Claude) | Tap **File** |
| Paint | Pen, or drag with the left mouse button | (fingers don't paint) |
| Erase | `E` or the **Eraser** button, or turn the pen round to its eraser end. `B` goes back to the brush | |
| Choose a colour | Colour wheel (ring = hue, square = how vivid / bright), type a hex code like `#FF8800`, or click a recent colour | Same, by tapping |
| Fill an area (paint bucket) | `F` or the **Fill** button, then click inside the area. **Tolerance** decides how different a colour may be and still be filled; **Grow** tucks the fill under the soft edges of lines; it can stop at lines on all visible layers or only this one | Tap |
| Gradient | `G` or the **Gradient** button, then drag from where it starts to where it ends (hold `Shift` for straight angles). **Straight** or **Round**, from the colour to transparent or to a second colour. It covers the whole layer, so use a new layer | Drag |
| Pick a colour from the canvas | Hold `Alt` and click, or press `I` (or the **Pick** button) and click. The pen's side button works too | Tap **Pick**, then tap the canvas |
| Brush / eraser size | `[` smaller, `]` bigger, or the Size slider | |
| Layers | **Layers** panel on the left: click a layer to paint on it, the checkbox shows/hides it, **Add** / **Delete** / **Up** / **Down**, opacity slider, **Blend mode** (Multiply for shadows, Screen for light, …), double-click a name to rename | Same, by tapping |
| Undo / redo | `Ctrl+Z` / `Ctrl+Y` (or `Ctrl+Shift+Z`), or the **Undo** / **Redo** buttons | Tap the buttons |
| Zoom | Mouse wheel (zooms where the pointer is), `+` / `-` | Pinch |
| Pan (move) | Hold **Space** and drag, or drag with the middle mouse button | Two-finger drag (touchscreen) |
| Rotate | Hold **Shift + Space** and drag, or `4` / `6` (15° steps) | Two-finger twist (touchscreen) |
| Reset | `0` fits the whole canvas, `5` removes rotation | |

**Opening pictures and scripts from File Explorer:** right-click an `.artapp` file (or a `.json`
painting script, which then plays on a new picture) → **Open with** →
**Choose another app** → **Choose an app on your PC**, and pick
`C:\dev\art-app\target\release\ArtApp.exe`. Tick "Always use this app" to open them with a
double-click from then on.

### Pen tablet tip

Art App reads pressure and tilt through **Windows Ink**. If your tablet's driver (Wacom, XP-Pen,
Huion, …) has a "Use Windows Ink" option, keep it switched on.

## Painting scripts and `art-cli`

A painting script is a JSON file describing a picture step by step: layers, gradients, shapes,
strokes, hatching and more. The format, with examples, is in
[docs/script-format.md](docs/script-format.md); [examples/sunset.json](examples/sunset.json) is a
full example.

`art-cli` renders scripts without a window or graphics card (it works on a server too):

```
art-cli render examples/sunset.json                      # out/sunset.png and out/sunset.artapp
art-cli render examples/sunset.json --preview --sheet    # quick half-size render + contact sheet
art-cli render examples/sunset.json --snapshots          # a picture at every snapshot and layer
art-cli render examples/sunset.json --crop 1040,220,180,100 --zoom 3   # close-up of one area
art-cli check examples/sunset.json                       # find mistakes without painting
art-cli info out/sunset.artapp                           # size, paper and layers of a picture
```

Build it with `cargo build --release -p art_cli`; the program is `target/release/art-cli`
(`art-cli.exe` on Windows). `art-cli help` lists every option.

## Project layout

```
art-app/
├── Cargo.toml            workspace: lists the two crates below
├── run.bat / test.bat / create-shortcut.bat / render-script.bat
├── examples/sunset.json  an example painting script
├── docs/decisions.md     why we chose things, in plain words
├── docs/script-format.md painting scripts: every command, with examples
└── crates/
    ├── engine/           painting engine: plain Rust, no window/UI code
    │   └── src/
    │       ├── lib.rs
    │       ├── tile.rs       256×256 blocks of pixels
    │       ├── surface.rs    a layer's pixels: only painted tiles exist
    │       ├── document.rs   canvas size, paper, the layer stack, combining layers
    │       ├── layer.rs      one layer: name, visible, opacity, pixels
    │       ├── layer_commands.rs  undoable add / delete / move / change layer
    │       ├── view.rs       zoom / pan / rotate maths
    │       ├── brush.rs      brush settings, dab shape
    │       ├── stroke.rs     paints or erases a stroke: dabs, no gaps, opacity
    │       ├── fill.rs       paint bucket, gradients, filling any area
    │       ├── blend.rs      blend modes (Multiply, Screen, ...)
    │       ├── random.rs     random numbers that repeat exactly (for scripts)
    │       ├── image.rs      flat images: crop, shrink, enlarge, save PNG
    │       ├── script.rs     painting scripts: the format, reading, errors
    │       ├── script/
    │       │   ├── player.rs   plays a script: commands into strokes and fills
    │       │   ├── geometry.rs shapes, smooth curves, hatching, wobble
    │       │   └── record.rs   writes down what was painted, as script commands
    │       ├── smoothing.rs  steadier lines
    │       ├── history.rs    undo / redo
    │       ├── file.rs       saving and opening .artapp files, PNG export
    │       ├── color.rs      RGB ↔ HSV, hex codes, colour on paper
    │       └── pen.rs        pen samples + tilt maths
    ├── cli/              art-cli: renders painting scripts, no window needed
    │   └── src/
    │       ├── main.rs        render / check / info commands
    │       ├── sheet.rs       contact sheets (every stage in one image)
    │       └── font.rs        a tiny pixel font for the labels
    └── app/              the program: window and interface (egui)
        ├── build.rs      puts the icon into ArtApp.exe
        ├── assets/       icon files
        └── src/
            ├── main.rs        starts the app, top bar, switches screens
            ├── files.rs       File menu: new, open, save, export, scripts, "Save changes?"
            ├── playback.rs    Play strokes: a script painted bit by bit, with its bar
            ├── canvas_view.rs draws the canvas, zoom/pan/rotate
            ├── paint_tool.rs  the tools: pen/mouse input into strokes, fills, gradients
            ├── brush_panel.rs the tool panel (tools, colour, settings)
            ├── color_panel.rs colour wheel, hex code, recent colours
            ├── layers_panel.rs the Layers panel
            ├── pen_input.rs   reads pressure/tilt from Windows Ink
            └── pen_test.rs    the pen test screen
```

## Terminal commands (optional)

```
cargo run -p art_app          # build and start (debug build)
cargo test --workspace        # run all tests
```
