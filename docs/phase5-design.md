# Phase 5 design: Duck Quill

The app gets its new name and logo, becomes a download-and-run program, and gains the
everyday tools of a drawing app: selections, layer thumbnails, brush presets with a real
pencil, a reference panel, symmetry, autosave. Everything an agent or the owner made before
keeps working.

Work happens on the branch `phase-5`. Pull requests #1 (sketchbook) and #2 (brand) were still
open, so their branches are merged into `phase-5`; when this phase's pull request is merged,
they are merged with it and can be closed.

There are three stops for the owner: after this design, after Step 2 (the rename and the
download), and after Step 6 (the new tools).

## Names

| Thing | Before | After |
|---|---|---|
| The app | Art App, `ArtApp.exe` | Duck Quill, `DuckQuill.exe` |
| The command-line tool | `art-cli` | `quill` (and a small `art-cli` that still works) |
| Picture files | `.artapp`, `"format": "artapp"` inside | `.quill`, `"format": "duckquill"` inside |
| Scripts | `"format": "artapp-script"` | `"format": "duckquill-script"` |
| Agents' folder | `skills/art-app/` | `skills/duck-quill/` (skill name `duck-quill`) |
| Desktop shortcut | Art App | Duck Quill |
| Settings folder | none | `%APPDATA%\Duck Quill\` on Windows, `~/.config/duck-quill/` on Linux |
| Crates (question 1) | `art_engine`, `art_app`, `art_cli` | `quill_engine`, `duck_quill`, `quill_cli` |

The crate folders stay `crates/engine`, `crates/app` and `crates/cli`: short, and every path in
the docs stays right.

**Old names keep working.** `.artapp` files open, and Save writes them back where they came
from (question 3). Scripts with `"artapp-script"` run with no warning in the app; `quill check`
prints one note line ("old format name; it still works"). `art-cli` is a second tiny binary in
the CLI crate: it prints `art-cli is now called quill` to stderr (so anything reading its
output sees exactly what it saw before) and runs the same code.

## Step 1: rename and logo

- **Rename.** A script does the mechanical part (`git mv` and search-and-replace over the
  50 files that say `art-cli`, the 15 that point into `skills/art-app/`, the 25 that say
  Art App), then I read every hit, because some are history: `docs/decisions.md` and the old
  phase designs keep the old names where they describe the past, and gain one entry that
  says what was renamed.
- **Formats.** `file.rs`: write `duckquill`, accept `artapp` or `duckquill`. `script.rs`:
  `FORMAT_NAME` becomes `duckquill-script`, with `OLD_FORMAT_NAME` accepted. Open dialogs list
  `*.quill` and `*.artapp`; Save As offers `.quill`.
- **Icon.** `crates/app/build.rs` already embeds an icon with `winresource`, only when building
  for Windows: it points at `brand/duck-quill.ico` instead of `crates/app/assets/icon.ico`
  (the old assets are removed). The window and taskbar icon come from
  `brand/duck-quill-icon-256.png`. `create-shortcut.bat` sets `IconLocation` to `DuckQuill.exe`
  (which now carries the icon) and names the shortcut Duck Quill.
- **Help → About Duck Quill.** The light or dark logo to match the theme, the version, a line
  of credits. The two logos are embedded in the exe (about 2 MB), so the downloaded zip needs
  no extra files.
- **The brand, lightly.** A small `theme.rs` sets egui's selection colour to night indigo
  `#5546D0` with white text (selected tool, selected layer) and gives one helper for primary
  buttons in duck yellow `#FFCC2E` with navy `#1B1446` text (OK in dialogs, Save). Nothing
  else changes colour.
- **Tests.** Committed fixtures made with the code as it is today, before the rename:
  `crates/engine/tests/fixtures/old-picture.artapp` (three layers, clip, lock, a blend mode)
  and `old-script.json` (with `"artapp-script"`). Tests open both and compare pixels with a
  stored PNG; a CLI test runs `art-cli render` and checks it gives the same picture as
  `quill render` plus the one stderr line.
- **At the end** I tell you how to rename the repository (Settings → General → Repository
  name → `duck-quill`; GitHub redirects the old address) and update the links.

## Step 2: download and run

- **Version in one place:** `version = "0.5.0"` in the workspace `Cargo.toml`. Both programs
  read it with `env!("CARGO_PKG_VERSION")`: About shows it, `quill --version` prints it.
- **`.github/workflows/ci.yml`:** on every push and pull request, on `windows-latest` and
  `ubuntu-latest`: build, `cargo test --workspace`, `cargo clippy --workspace --all-targets
  -- -D warnings`. Uses `actions/checkout` and `Swatinem/rust-cache` (question 7).
- **`.github/workflows/release.yml`:** on a tag `v*`: checks the tag matches the version,
  builds the release on Windows, zips `DuckQuill.exe`, `quill.exe`, `art-cli.exe`,
  `skills/duck-quill/`, `brand/duck-quill-icon-256.png`, `READ ME FIRST.txt` and
  `Create desktop shortcut.bat` (a version of the shortcut script that points at the exe next
  to it) into `DuckQuill-0.5.0-windows.zip`, and publishes a GitHub Release with `gh release
  create` (no third-party release action).
- **README:** "Download" comes first: download the zip from Releases, unzip, double-click
  `DuckQuill.exe`. Building it yourself comes after.
- **Not signed** (question 8): Windows SmartScreen will say "Windows protected your PC" the
  first time. `READ ME FIRST.txt` says to click "More info" then "Run anyway".
- **Tested by:** the workflows running on this branch's pushes (I read their logs). The
  release workflow also runs on pull requests that change it, building the zip as a
  downloadable artifact without publishing anything, so I can check its contents before any
  tag exists.

**STOP 2:** I report what changed, the CI results, and the old-file and old-script tests.

## Step 3: selections

**Engine: `selection.rs`.** A selection is a `Mask` (the 8-bit, anti-aliased mask scripts
already use for group masks) the size of its bounding box, plus its feather. Functions: from
a rectangle, ellipse or polygon (anti-aliased, using the same shape code as script areas), from
a layer's pixels, select all, invert, add, subtract, feather (a blur of the mask), and its
outline (the edge traced into line segments, for drawing). The document keeps
`selection: Option<Selection>` (not saved in the file). Changing the selection is an undo step
(question 5).

**Everything respects it.** The document hands out one clip mask: the selection, combined with
a group mask when a script has one. Brush and eraser already accept a clip (`Stroke::clipped`);
fill, gradient and filters get the same, by blending their result with what was there through
the mask. One code path, so the app and scripts can't differ.

**App.**
- Tools: rectangle, ellipse, lasso, polygon lasso (click points, double-click or Enter to
  close). Shift adds, Alt subtracts. Alt is the eyedropper key everywhere else; while a
  selection tool is in use it means subtract, as in other painting apps.
- **Marching ants:** the outline drawn twice, black then white dashes offset by half a dash,
  moving slowly: visible on any colour. The outline is traced once per selection change, not
  per frame.
- **Select menu:** Select all (Ctrl+A), Deselect (Ctrl+D), Invert (Ctrl+Shift+I), Select layer
  contents, Feather….
- **Edit menu:** Cut, Copy, Paste (onto a new layer at the same place), Delete inside, Fill
  with colour. Copy also puts the image on the Windows clipboard, and Paste takes an image
  from it, with the `arboard` crate (new dependency, MIT/Apache, works on Windows and Linux).
- **Transform a selection:** the selected pixels are lifted onto a temporary layer and the
  existing transform tool works on it (move, scale, rotate, flip, with its preview). Enter
  puts them down on the layer they came from as one undo step; Esc puts everything back.

**Script:** `{"cmd": "select", "rect": [x, y, w, h]}` (or `ellipse`, `polygon`, `layer`),
`{"cmd": "select", "none": true}`, `{"cmd": "select", "invert": true}`, with `"mode": "add"`
or `"subtract"` and `"feather": 4`. Every painting command after it stays inside the
selection, as inside a group mask. The shapes use the existing area syntax.

## Step 4: layers you can see

- **Thumbnails.** Each layer row shows a small picture of the layer. The engine's `Surface`
  gets a second list of changed tiles, separate from the screen's, so the layers panel can
  ask "which tiles changed since I last looked" without stealing the canvas's list. The panel
  keeps a tiny image per tile (a 256-pixel tile shrinks to a few pixels) and only redoes the
  changed ones, and only when no stroke is in progress.
- **Drag to reorder** with egui's built-in drag and drop; the move uses the existing
  `MoveLayerCommand`, so it is undoable and recorded for Export strokes.
- **Eye and lock** become small drawn icons (an eye, a padlock) instead of a checkbox and an
  emoji, so they look the same on every machine.
- Double-click to rename stays.

## Step 5: brush presets, the pencil and the ink pen

**A preset is a `brush` script command with a name.** The file format is the one agents
already know:

```json
{ "name": "Ink pen", "smoothing": 0.5,
  "brush": { "size": 6, "hardness": 1, "pressure_size": true, "pressure_opacity": false } }
```

The colour is never part of a preset (you keep your colour). Built-in presets are JSON files
in `crates/engine/presets/`, compiled into the engine, so `{"cmd": "brush", "preset": "Pencil
(graphite)"}` paints the same on every machine. Your own presets live in
`%APPDATA%\Duck Quill\presets\`, one file each; Export and Import copy them. Scripts accept
only built-in names, so a script never depends on one machine's settings. `quill presets`
lists them.

**Engine changes for the pencil** (all off by default, so every old script paints the same):
- **Pressure ranges:** `pressure_size_min` and `pressure_opacity_min` (what a feather-light
  touch gives, as a fraction) and `pressure_curve` (a gamma). Today pressure maps straight to
  0..1 for each switch that's on. The pencil uses size 85–100% and opacity 15–100%: pressure
  changes darkness more than width.
- **Paper grain:** `grain` (0..1) and `grain_scale`. A fixed grain pattern (made by the
  engine's noise, tiled, the same everywhere) is pinned to the canvas, not to the stroke. Each
  pixel's paint is multiplied by how much of the grain's hill it reaches: a light touch only
  marks the hilltops, pressing harder fills the valleys. Because the grain stays still,
  overlapping strokes hit the same hilltops and the white valleys stay white until you press,
  which is what makes graphite look like graphite. It is a lookup in a small table per pixel,
  so it costs little.
- **Build-up limit:** `build_up` (for example 0.85). Strokes already add up where they cross;
  with a limit, the layer's darkness approaches the limit instead of solid colour, the way
  graphite goes shiny grey, never black.

**The starter set:** Pencil (graphite), Ink pen (hard edge, pressure on size, smoothing 0.5),
Marker (flat, hard edge, opacity 0.7: overlaps come out darker on their own), Soft airbrush
(soft, low flow, big), Chalk (the chalk tip), Soft eraser. Only the pencil needs the engine
changes.

**App:** a Presets panel with a grid of buttons, each with a preview stroke painted by the
engine (an S-curve with rising pressure), cached. Click to use; Save current brush, rename,
delete, drag to reorder, Export, Import. Built-ins can't be deleted or overwritten, only
duplicated (question 6).

**Skill:** a lesson on when an agent uses Pencil (sketches, studies, anything copied from the
sketchbook references: darkness by density and pressure) and when Ink (clean line art, two
line weights, cartoon outlines).

## Step 6: references and symmetry

- **Reference panel:** a side panel holding pictures (open, paste, or drag a file onto the
  window). Each has its own zoom, pan and opacity, a remove button, and Alt-click picks a
  colour from it. They are stored inside the `.quill` file as `references/<n>.png` plus a list
  in `document.json`. The list is optional, so pictures without references, old and new, open
  as they do now.
- **Symmetry:** vertical, horizontal, or both, with a mirror line you drag on the canvas, drawn
  while on. The engine's stroke learns to stamp each dab at its mirrored places into the same
  coverage map, so where the mirrored strokes meet at the line they don't double up and get
  darker. Script equivalent, since it's then free: `{"cmd": "symmetry", "x": 500}` (or `"y"`,
  or both; `{"cmd": "symmetry", "off": true}`).

**STOP 6:** results, timings on the big canvas, and screenshots.

## Step 7: comfort

- **Autosave** every 3 minutes, only when something changed and no stroke is in progress, to
  `%APPDATA%\Duck Quill\recovery\`. The picture is copied on the main thread and written by a
  background thread, so drawing doesn't stutter while the file is compressed. Your own files
  are never touched. A clean exit deletes the recovery file; if one is there at start, Duck
  Quill offers to restore it.
- **Settings file** `settings.json` in the settings folder: recent files (File → Open recent),
  window size and position, last brush and preset, colours, which panels are open and how wide.
- **Shortcut sheet:** `?` or Help → Keyboard shortcuts: every shortcut, grouped by tool, from
  one list in the code that the key handling also uses, so the sheet can't go out of date.
- **Your annoyances:** I'll ask for your list at the start of this step (it would help to
  start writing it down now), fix each, and name each fix in a commit message.

## Step 8: wrap up

Docs and skill updated (short entries), full tests, clippy, the gallery and benchmark checks,
an agent following `SKILL.md` from scratch to render, check and critique a picture with
`quill`, and the pull request `phase-5` → `main`.

## Speed

The bar: a 4000 × 3000 canvas with 20 layers stays as fast as now. Before Step 3 I add a timing
test (ignored by default, run with `--release`) that measures a long stroke, a fill, a blur, a
thumbnail refresh and an autosave copy on that canvas, and I report before and after numbers
at each stop. Expected costs: a selection adds one mask read per pixel painted; grain one table
lookup; mirrors multiply the dabs by the number of copies; thumbnails touch only changed
tiles. The risky one is autosave: tiles are copied, not shared, so copying a big document could
take a noticeable fraction of a second. If the timing test shows that, tiles become shared
(`Arc`, copied only when painted on) as part of Step 7, and I'll say so.

## Tests

- **Compatibility:** the old `.artapp` and old script fixtures, `art-cli` still running, every
  existing script, example and gallery piece still painting the same pixels.
- **Engine unit tests** for each new piece: selection shapes, add/subtract/invert, feather,
  outline; painting, filling and filtering only inside a selection; cut/copy/paste; pressure
  ranges, grain (light touch skips valleys, the grain doesn't move between strokes), build-up
  limit; mirrored strokes not doubling at the line; references saving and opening; thumbnail
  tiles updating.
- **Script tests** for `select`, `brush` with `preset`, and `symmetry`, plus the docs test that
  checks every JSON example in `script-format.md`.
- **App:** what can be tested without a screen is moved into small functions and tested
  (shortcut list, settings file, recovery, preset files). For the window itself I run the app
  under a virtual screen (Xvfb, available here) to take screenshots.
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` at every
  commit; CI runs both on Windows and Linux.

## Risks

- **I can't hold your pen.** I work on Linux. CI builds and tests on Windows, and I check the
  Windows build with clippy, but the pen (pressure, tilt, eraser end), the file dialogs and the
  clipboard on Windows need you to try them. At each stop I give a short checklist.
- **The rename touches almost every file.** One commit for the mechanical part, so it's easy to
  review and to revert; the tests prove nothing old broke.
- **Alt means two things:** eyedropper and subtract-from-selection. Only while a selection tool
  is in use does it subtract.
- **`SKILL.md` is at 199 of its 200 lines.** New features go into `AI_GUIDE.md` and
  `script-format.md`, with one-line pointers in the skill; I'll trim to make room.
- **Unsigned download:** the SmartScreen warning, until you decide on signing.

## Questions for you

1. **Rename the crates too?** I recommend yes (`quill_engine`, `duck_quill`, `quill_cli`): the
   names show up in error messages and in the code you're learning from, and the change is
   mechanical. The folders stay `crates/engine`, `crates/app`, `crates/cli`.
2. **The `art-cli` line on stderr** rather than in the normal output, so programs that read
   `art-cli`'s output see exactly what they did before. OK?
3. **Saving an old `.artapp` picture:** I recommend that Ctrl+S saves it in the new format under
   its old name (it stays `.artapp`, opens fine in Duck Quill), and Save As offers `.quill`. The
   other choice is to ask once "Save as .quill?" the first time.
4. **Light theme?** The app is dark only. "The logo to match the theme" suggests a choice:
   should I add View → Theme (Dark, Light, follow Windows)? I recommend yes, small.
5. **Undoable selections?** I recommend yes: Ctrl+Z after a wrong lasso brings the old
   selection back, as in Photoshop.
6. **Built-in presets read-only** (you duplicate one to change it)? I recommend yes, so a
   script's "Pencil (graphite)" always means the same pencil.
7. **GitHub Actions from others:** `actions/checkout` (GitHub's own) and `Swatinem/rust-cache`
   (widely used, makes CI minutes faster). OK, or GitHub's own only?
8. **Code signing:** none for now (the SmartScreen warning), or do you want to look into a
   certificate later?
9. **Your annoyances list for Step 7:** start writing down what bothers you when drawing, so
   it's ready when I ask.
