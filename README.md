# Art App

A digital painting app for Windows, written in Rust.

**Status:** Phase 1, Step 2: a canvas you can zoom, pan and rotate (with a test picture until
the brush arrives in Step 3). The pen test is under **Pen test** in the top bar.

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
| Zoom | Mouse wheel (zooms where the pointer is), `+` / `-` | Pinch |
| Pan (move) | Hold **Space** and drag, or drag with the middle mouse button | Two-finger drag (touchscreen) |
| Rotate | Hold **Shift + Space** and drag, or `4` / `6` (15° steps) | Two-finger twist (touchscreen) |
| Reset | `0` fits the whole canvas, `5` removes rotation | |

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
    │       ├── document.rs   canvas size, paper, layer
    │       ├── view.rs       zoom / pan / rotate maths
    │       └── pen.rs        pen samples + tilt maths
    └── app/              the program: window and interface (egui)
        ├── build.rs      puts the icon into ArtApp.exe
        ├── assets/       icon files
        └── src/
            ├── main.rs        starts the app, switches screens
            ├── canvas_view.rs draws the canvas, zoom/pan/rotate
            ├── pen_input.rs   reads pressure/tilt from Windows Ink
            └── pen_test.rs    the pen test screen
```

## Terminal commands (optional)

```
cargo run -p art_app          # build and start (debug build)
cargo test --workspace        # run all tests
```
