# Art App

A digital painting app for Windows, written in Rust.

**Status:** Phase 1 is complete. Paint and erase with a round brush (pen pressure changes size
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

**Getting new versions:** in GitHub Desktop click **Fetch origin**, then **Pull origin**, then
double-click `run.bat` once. Your desktop shortcut then opens the new version.

### Canvas controls

| Action | Mouse / keyboard | Touchscreen / touchpad |
|---|---|---|
| New picture | **File → New…** or `Ctrl+N`: choose a size (or a common size) | Tap **File** |
| Open / save | **File → Open…** (`Ctrl+O`), **Save** (`Ctrl+S`), **Save as…** (`Ctrl+Shift+S`). Pictures are saved as `.artapp` files, with all layers | Tap **File** |
| Export a PNG image | **File → Export PNG…** (`Ctrl+E`) on the paper colour, or **Export PNG, transparent…** (`Ctrl+Shift+E`) with no paper | Tap **File** |
| Paint | Pen, or drag with the left mouse button | (fingers don't paint) |
| Erase | `E` or the **Eraser** button, or turn the pen round to its eraser end. `B` goes back to the brush | |
| Choose a colour | Colour wheel (ring = hue, square = how vivid / bright), type a hex code like `#FF8800`, or click a recent colour | Same, by tapping |
| Pick a colour from the canvas | Hold `Alt` and click, or press `I` (or the **Pick** button) and click. The pen's side button works too | Tap **Pick**, then tap the canvas |
| Brush / eraser size | `[` smaller, `]` bigger, or the Size slider | |
| Layers | **Layers** panel on the left: click a layer to paint on it, the checkbox shows/hides it, **Add** / **Delete** / **Up** / **Down**, opacity slider, double-click a name to rename | Same, by tapping |
| Undo / redo | `Ctrl+Z` / `Ctrl+Y` (or `Ctrl+Shift+Z`), or the **Undo** / **Redo** buttons | Tap the buttons |
| Zoom | Mouse wheel (zooms where the pointer is), `+` / `-` | Pinch |
| Pan (move) | Hold **Space** and drag, or drag with the middle mouse button | Two-finger drag (touchscreen) |
| Rotate | Hold **Shift + Space** and drag, or `4` / `6` (15° steps) | Two-finger twist (touchscreen) |
| Reset | `0` fits the whole canvas, `5` removes rotation | |

**Opening pictures from File Explorer:** right-click an `.artapp` file → **Open with** →
**Choose another app** → **Choose an app on your PC**, and pick
`C:\dev\art-app\target\release\ArtApp.exe`. Tick "Always use this app" to open them with a
double-click from then on.

### Pen tablet tip

Art App reads pressure and tilt through **Windows Ink**. If your tablet's driver (Wacom, XP-Pen,
Huion, …) has a "Use Windows Ink" option, keep it switched on.

## Project layout

```
art-app/
├── Cargo.toml            workspace: lists the two crates below
├── run.bat / test.bat / create-shortcut.bat
├── docs/decisions.md     why we chose things, in plain words
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
    │       ├── smoothing.rs  steadier lines
    │       ├── history.rs    undo / redo
    │       ├── file.rs       saving and opening .artapp files, PNG export
    │       ├── color.rs      RGB ↔ HSV, hex codes, colour on paper
    │       └── pen.rs        pen samples + tilt maths
    └── app/              the program: window and interface (egui)
        ├── build.rs      puts the icon into ArtApp.exe
        ├── assets/       icon files
        └── src/
            ├── main.rs        starts the app, top bar, switches screens
            ├── files.rs       File menu: new, open, save, export, "Save changes?"
            ├── canvas_view.rs draws the canvas, zoom/pan/rotate
            ├── paint_tool.rs  brush and eraser: pen/mouse input into strokes
            ├── brush_panel.rs the tool panel (brush, eraser, pick, settings)
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
