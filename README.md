# Art App

**Painting with scripts, or an AI assistant painting?** Start with the
[quick start](docs/QUICKSTART.md): a first script and the commands to render it, on one page.

A digital painting app for Windows, written in Rust.

**Status:** Phases 1 and 2 are complete, plus a paint bucket, gradients, blend modes, and **painting
scripts**: pictures described as text, step by step (see [docs/script-format.md](docs/script-format.md)),
with groups that move, resize, turn and mirror a drawing, symbols (define once, use many times,
with colours swapped and parameters), repeats, text, **animations** (keyframes, sprite sheets,
GIF and MP4 export) and **pixel art**. Paint and erase with a round brush (pen pressure changes size
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
| Watch an animation | **File → Play strokes…** (or **Open…**) and pick a script with animations: its frames appear in the **Animation** panel at the bottom. **Play / Pause**, `|<` and `>|` step a frame, the **fps** slider, **Onion skin** shows the frame before in red and the one after in blue, the list of frame numbers jumps to a frame, and the drop-down picks the animation. **Close** keeps the frame shown as the picture | Tap the buttons |
| Export an animation | **File → Export animation…** while the panel is open: the **sprite sheet + JSON** (every animation; see `docs/sprite-sheet-format.md`), the **frames** as PNG files into a folder, or a **GIF** of the animation shown at the panel's fps | Tap **File** |
| Save what you painted as a script | **File → Export strokes…**: everything painted since the picture was started or opened (not what you undid), as a `.json` painting script that can be played again or read and continued (for example by Claude) | Tap **File** |
| Paint | Pen, or drag with the left mouse button | (fingers don't paint) |
| Erase | `E` or the **Eraser** button, or turn the pen round to its eraser end. `B` goes back to the brush | |
| Choose a colour | Colour wheel (ring = hue, square = how vivid / bright), type a hex code like `#FF8800`, or click a recent colour | Same, by tapping |
| Fill an area (paint bucket) | `F` or the **Fill** button, then click inside the area. **Tolerance** decides how different a colour may be and still be filled; **Grow** tucks the fill under the soft edges of lines; it can stop at lines on all visible layers or only this one | Tap |
| Gradient | `G` or the **Gradient** button, then drag from where it starts to where it ends (hold `Shift` for straight angles). **Straight** or **Round**, from the colour to transparent or to a second colour. It covers the whole layer, so use a new layer | Drag |
| Text | `T` or the **Text** button, then click where the text starts and type. Size, alignment and an outline are in the panel on the right, with the colour; you can change them while typing. **Enter** starts a new line; **Enter on an empty line**, or a click outside the box, finishes it (one undo step). **Esc** takes it away | Tap, then type on the keyboard |
| Flip a layer | **Layer → Flip horizontal** (left and right swap) or **Flip vertical** (top and bottom swap). One undo step | Tap **Layer** |
| Move, resize, turn a layer | **Layer → Transform…**: drag inside the box to move, a corner to resize (the opposite corner stays put; hold `Shift` to keep the proportions), the round handle to turn. **Apply** (`Enter`) or **Cancel** (`Esc`), in the bar at the bottom. One undo step | Drag with the pen |
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
[docs/script-format.md](docs/script-format.md). Full examples: [examples/sunset.json](examples/sunset.json),
[examples/night-hills.json](examples/night-hills.json) and [examples/two-ducks.json](examples/two-ducks.json); for
games, a 4-frame walk cycle [examples/duck-walk.json](examples/duck-walk.json) (render it with `--transparent`)
and a character sheet [examples/duck-sheet.json](examples/duck-sheet.json).

**Painting with an AI assistant:** [docs/AI_GUIDE.md](docs/AI_GUIDE.md) is a practical manual for an
AI (like Claude) painting with these scripts on a machine without a screen: how to build `art-cli`,
the format on one page, a step-by-step workflow (block in, snapshot, check, refine), techniques that
work, and common mistakes.

**As a skill:** [skills/art-app/SKILL.md](skills/art-app/SKILL.md) is the same know-how as a Claude
skill (a short instruction file an assistant loads when it's asked to paint). Copy the
`skills/art-app` folder into a project's `.claude/skills/` folder, or into `~/.claude/skills/`, and
Claude uses it whenever it's asked to paint with Art App.

`art-cli` renders scripts without a window or graphics card (it works on a server too):

```
art-cli render examples/sunset.json                      # out/sunset.png and out/sunset.artapp
art-cli render examples/sunset.json --preview --sheet    # quick half-size render + contact sheet
art-cli render examples/sunset.json --snapshots          # a picture at every snapshot and layer
art-cli render examples/sunset.json --crop 1040,220,180,100 --zoom 3   # close-up of one area
art-cli render examples/sunset.json --preview --grid 100   # with a grid and coordinates
art-cli render examples/sunset.json --preview --layers "Sky,Sun"   # only some layers (or --hide)
art-cli render sprite.json --transparent --bg "#4080A0"  # see-through PNG, also shown on a colour
art-cli check examples/sunset.json                       # mistakes, and where everything was drawn
art-cli check examples/sunset.json --detail              # ...including groups inside groups
art-cli render examples/duck-animated.json --animation walk --frame-sheet --transparent  # every frame, numbered
art-cli export examples/duck-animated.json --gif --mp4   # sprite sheet + JSON, GIFs and videos
art-cli info out/sunset.artapp                           # size, paper and layers of a picture
art-cli info examples/sunset.json                        # ...and which commands painted each layer
```

After a render it prints a short summary: the canvas, the box around each layer's paint, the time
taken, warnings, and the files saved. `check` paints without saving anything and lists more: every
layer's box, every named group's and symbol `use`'s box, and warnings about things that are
probably mistakes (drawn entirely off the canvas, empty layers, symbols never used, palette colours
never used).

Build it with `cargo build --release -p art_cli`; the program is `target/release/art-cli`
(`art-cli.exe` on Windows). `art-cli help` lists every option.

## Project layout

```
art-app/
├── Cargo.toml            workspace: lists the two crates below
├── run.bat / test.bat / create-shortcut.bat / render-script.bat
├── examples/             example scripts (sunset, night hills, two ducks, duck walk cycle and sheet)
├── docs/QUICKSTART.md    start here: a first script and how to render it, on one page
├── docs/animation-design.md   how animation, sprite sheets and pixel art fit the format
├── docs/sprite-sheet-format.md the exported sheet's PNG and JSON, for games
├── skills/art-app/       the same know-how as a skill for Claude (SKILL.md)
├── docs/decisions.md     why we chose things, in plain words
├── docs/script-format.md painting scripts: every command, with examples
├── docs/AI_GUIDE.md      how an AI assistant paints with scripts and art-cli
└── crates/
    ├── engine/           painting engine: plain Rust, no window/UI code
    │   ├── assets/fonts/ the built-in font for text (Inter, SIL Open Font License)
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
    │       ├── transform.rs  moving, turning, resizing, mirroring (groups, previews)
    │       ├── layer_transform.rs  moving, turning, resizing, mirroring a layer's pixels
    │       ├── sprites.rs    sprite sheets with their JSON
    │       ├── gif.rs        animated GIFs
    │       ├── text.rs       text: letter shapes from the built-in font, laid out in lines
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
    │       ├── grid.rs        the --grid: lines and coordinates over a picture
    │       ├── export.rs      export: sprite sheet + JSON, frames, GIF, MP4
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
            ├── text_tool.rs   the Text tool: click, type, finish
            ├── animation_view.rs the Animation panel: frames, play, onion skin, exports
            ├── transform_tool.rs the Layer menu: flip, and Transform with handles
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

## Credits

Text in painting scripts uses the [Inter](https://rsms.me/inter/) typeface by The Inter Project
Authors, under the SIL Open Font License 1.1 (see `crates/engine/assets/fonts/OFL.txt`). It's
built into the program, so no fonts need to be installed.
