//! Plays a painting script on a document.
//!
//! Each command is turned into simple jobs: brush strokes (a list of
//! points), areas to fill (a shape and a colour or gradient) and paint
//! bucket fills. The jobs are then carried out one by one. That split
//! lets the app show a script being painted bit by bit (`advance` with a
//! small budget), while the command-line renderer just plays everything
//! at once (`run`).
//!
//! Commands are played from a stack of *frames*: the script itself is the
//! bottom one, and a `group`, a `use` of a symbol or a `repeat` adds one
//! for its own commands, with its own transform (where its drawing lands
//! on the canvas). Every drawing
//! command first works out its shape in its own units, then places it
//! with the frame's transform. A quick preview is simply the whole script
//! inside a shrinking transform.
//!
//! Everything a command needs is checked before anything is painted, and
//! problems are reported with the command's number (and, inside groups,
//! the way to it).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use super::animate::{self, Clock};
use super::geometry::{self, Pt};
use super::{
    BrushCmd, Command, DynamicsCmd, EllipseCmd, FillCmd, FilterCmd, GradientCmd, GroupCmd,
    HatchCmd, IncludeCmd, LayerCmd, LineCmd, MoveLayerCmd, PathCmd, PixelsCmd, Point, PolygonCmd,
    RectCmd, RepeatCmd, ScatterCmd, Script, ScriptError, SelectCmd, StampCmd, StrokeCmd,
    SymmetryCmd, TextCmd, TransformLayerCmd, UseCmd, parse_hex,
};
use crate::brush::{BrushMode, BrushSettings, Dynamics, MAX_SIZE, Tip, TipShape};
use crate::document::{Background, Document, MAX_CANVAS_SIDE};
use crate::file;
use crate::fill::{
    self, FloodFill, Gradient, GradientShape, GradientStop, Mask, Paint, Rgba, SampleFrom,
    paint_mask_within, paint_rect_within,
};
use crate::filters::{self, Filter};
use crate::history::{Command as UndoStep, TileCommand};
use crate::image::Image;
use crate::layer::{BlendMode, Layer, LayerId, LayerProps};
use crate::layer_commands::{
    LayerPresenceCommand, LayerPropsCommand, MoveLayerCommand, PaperCommand,
};
use crate::layer_transform::transform_layer;
use crate::noise::{self, Noise, Texture, TextureImage};
use crate::pixels::{MAX_SUPERSAMPLE, PixelMode};
use crate::random::Random;
use crate::selection::{self, Selection, What};
use crate::stroke::{Stroke, StrokePoint, Symmetry, TilesBefore};
use crate::surface::{PixelRect, Surface, TileCoord};
use crate::text::{self, Align};
use crate::tile::Rgba8;
use crate::transform::Transform;

/// How the script is played.
#[derive(Clone, Debug)]
pub struct PlayOptions {
    /// Paint at this size: 0.25 makes the picture a quarter as wide and
    /// high. Positions, brush sizes and everything else are scaled with
    /// it, so a quick preview looks like the full picture, only smaller.
    pub scale: f32,
    /// The script's folder: `"open"` paths in the `canvas` command are
    /// relative to it.
    pub folder: Option<PathBuf>,
    /// Keep what's needed to undo everything played (the app needs it;
    /// the command-line renderer doesn't).
    pub record_undo: bool,
    /// Only check the script: work out every command, but paint nothing.
    pub dry_run: bool,
    /// Which animation to play (`None`: the first, if the script has any).
    pub animation: Option<String>,
    /// Which frame of it, counting from 0.
    pub frame: u32,
    /// Also keep every named part's silhouette (where it painted), for
    /// [`Report::silhouettes`]. Each named part is then painted on a
    /// scratch picture first and laid on the layers after, like a group
    /// with filters, so this is for looking, not for the final picture.
    pub silhouettes: bool,
    /// What an earlier frame of the same animation taught (see
    /// [`super::reuse`]): its static layers are copied and the commands
    /// that painted them skipped.
    pub reuse: Option<std::sync::Arc<super::reuse::Reuse>>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            folder: None,
            record_undo: false,
            dry_run: false,
            animation: None,
            frame: 0,
            silhouettes: false,
            reuse: None,
        }
    }
}

/// Something worth knowing about while playing, e.g. to save a picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A `snapshot` command was reached.
    Snapshot(String),
    /// The script moved on from a layer it had painted on (or ended).
    LayerDone(String),
}

/// What a played script drew where, and what looks like a mistake, so
/// a script can be checked without looking at the picture. See
/// [`Player::report`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Every named group and every `use`, in the order they started.
    pub parts: Vec<Part>,
    /// Things that are probably mistakes, in plain words.
    pub warnings: Vec<String>,
    /// For each layer, the numbers of the script's own commands that
    /// painted (or erased) on it, in order. A group, `use` or `repeat`
    /// counts as one command. Clearing a layer starts its list again.
    pub painted_by: HashMap<LayerId, Vec<usize>>,
    /// For each layer, the commands that read its pixels while painting
    /// (a bucket fill, a `layer` mask, a `"from": "below"` layer).
    pub read_by: HashMap<LayerId, Vec<usize>>,
    /// Every symbol defined, with its command number, and the symbols
    /// and palette colours used in this frame: see [`unused_warnings`].
    pub defined: Vec<(String, usize)>,
    pub used_symbols: BTreeSet<String>,
    pub used_colors: BTreeSet<String>,
    pub palette: BTreeSet<String>,
    /// With [`PlayOptions::silhouettes`]: where each named part that drew
    /// something has paint, and that paint on its own; in the order the
    /// parts finished.
    pub silhouettes: Vec<Silhouette>,
}

/// A named part's own paint, kept by [`PlayOptions::silhouettes`].
#[derive(Clone, Debug, PartialEq)]
pub struct Silhouette {
    /// The part's place in [`Report::parts`].
    pub part: usize,
    /// Where it has paint, in the pixels being painted.
    pub mask: Mask,
    /// Its paint on its own (every layer's share laid together, bottom
    /// to top), the size of `mask`'s box, see-through where it has none.
    pub own: Image,
}

/// Warnings about symbols defined but never used and palette colours
/// never used, counted over all of `reports` (every frame of every
/// animation): a symbol swapped in at frame 9 is used.
/// Where the intent's focal point is on the canvas: the point as given,
/// or the middle of the named part's box (the first part whose label is
/// `group "name"` or `use "name"`, outermost first). `Err` names a part
/// that isn't in the report.
pub fn focal_point(
    intent: &super::Intent,
    report: &Report,
    canvas: (u32, u32),
) -> Result<Option<(f32, f32)>, String> {
    match intent.focal()? {
        None => Ok(None),
        Some(super::Focal::Point([x, y])) => Ok(Some((x, y))),
        Some(super::Focal::Part(name)) => {
            let wanted = [format!("(group \"{name}\")"), format!("(use \"{name}\")")];
            let part = report.parts.iter().find(|p| {
                wanted.iter().any(|w| {
                    p.path.ends_with(w)
                        || p.path.contains(&format!("{w} "))
                        || p.path.contains(&format!("{w}>"))
                })
            });
            match part {
                Some(part) => match part.area {
                    Some(a) => {
                        let (w, h) = (canvas.0 as f32, canvas.1 as f32);
                        let x = ((a.left + a.right) as f32 / 2.0).clamp(0.0, w);
                        let y = ((a.top + a.bottom) as f32 / 2.0).clamp(0.0, h);
                        Ok(Some((x, y)))
                    }
                    None => Err(format!("the focal part \"{name}\" drew nothing")),
                },
                None => Err(format!(
                    "the intent's focal point names \"{name}\", but no group or use is called that"
                )),
            }
        }
    }
}

pub fn unused_warnings(reports: &[&Report]) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(first) = reports.first() else {
        return warnings;
    };
    let used = |name: &str| reports.iter().any(|r| r.used_symbols.contains(name));
    for (name, command) in &first.defined {
        if !used(name) {
            warnings.push(format!(
                "symbol \"{name}\" (command {command}) is defined but never used"
            ));
        }
    }
    for name in &first.palette {
        if !reports.iter().any(|r| r.used_colors.contains(name)) {
            warnings.push(format!("palette colour \"{name}\" is never used"));
        }
    }
    warnings
}

/// A named group or a `use`, and where its drawing went.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Where it is in the script, like `command 12 (group "duck")` or
    /// `command 20 (use "pair") > 1 (use "duck")`.
    pub path: String,
    /// The box around everything it drew, in canvas pixels (the brush's
    /// width included). `None` if it drew nothing. It isn't cut to the
    /// canvas, so it shows what sticks out.
    pub area: Option<PixelRect>,
    /// The named group or `use` it's inside (its place in
    /// [`Report::parts`]), or `None` if it isn't inside one.
    pub inside: Option<usize>,
}

/// A box around something drawn: left, top, right, bottom, in the pixels
/// being painted.
type Extent = (f32, f32, f32, f32);

/// What `advance` stopped at.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    /// The budget is used up; there's more to do.
    Working,
    Event(Event),
    /// The whole script has been played.
    Finished,
}

/// How much of the budget an area fill uses up when animating (roughly
/// the same as a brush travelling this many pixels).
const AREA_COST: f32 = 600.0;
/// The most dabs or strokes one `scatter` may make.
const MAX_SCATTER: u32 = 200_000;
/// The most lines one `hatch` may draw.
const MAX_HATCH_LINES: usize = 20_000;
/// Random wobble changes direction about this often (in drawing units).
const WOBBLE_WAVELENGTH: f32 = 40.0;
/// Groups, symbols and repeats can hold each other, but not endlessly.
const MAX_DEPTH: usize = 64;
/// The most copies one `repeat` may draw.
const MAX_REPEAT: u32 = 10_000;
/// The biggest font size `text` takes, in pixels.
const MAX_TEXT_SIZE: f32 = 5000.0;

/// One simple job a command was turned into. Positions are on the canvas.
enum Job {
    Stroke {
        layer: LayerId,
        brush: BrushSettings,
        points: Vec<StrokePoint>,
        /// Where the brush's dynamics get their randomness.
        seed: u64,
        /// Mirror lines, in canvas pixels.
        symmetry: Option<Symmetry>,
    },
    /// Fills a shape (or the whole layer, if `shape` is `None`). A shape
    /// is one or more outlines filled together (a letter "o" has two).
    Area {
        layer: LayerId,
        shape: Option<Shape>,
        paint: Paint,
        opacity: f32,
        mode: BrushMode,
    },
    Bucket {
        layer: LayerId,
        fill: FloodFill,
        paint: Paint,
        opacity: f32,
    },
}

/// What an area job fills.
enum Shape {
    /// Outlines, in canvas pixels.
    Rings(Vec<Vec<Pt>>),
    /// Pixels already worked out (a bucket's region).
    Mask(Mask),
}

impl Job {
    fn layer(&self) -> LayerId {
        match self {
            Job::Stroke { layer, .. } | Job::Area { layer, .. } | Job::Bucket { layer, .. } => {
                *layer
            }
        }
    }

    fn on_layer(mut self, to: LayerId) -> Job {
        match &mut self {
            Job::Stroke { layer, .. } | Job::Area { layer, .. } | Job::Bucket { layer, .. } => {
                *layer = to;
            }
        }
        self
    }
}

/// A stroke being painted, a few points at a time.
struct RunningStroke {
    layer: LayerId,
    stroke: Stroke,
    points: Vec<StrokePoint>,
    next: usize,
    /// Painting a symbol's silhouette for a mask, not the picture.
    capture: bool,
}

/// Commands being played: the script itself, or the inside of a group,
/// a symbol or a repeat.
struct Frame {
    commands: Arc<Vec<Command>>,
    /// The next command to start.
    next: usize,
    /// For error messages: this frame's number in the frame below it, and
    /// its name, like `group "duck"`. `None` for the script itself.
    label: Option<(usize, String)>,
    /// Where this frame's drawing lands on the canvas.
    place: Transform,
    /// How many canvas pixels one drawing unit is (how finely to cut curves).
    detail: f32,
    /// Brush and outline sizes are multiplied by this.
    brush_scale: f32,
    /// Everything drawn is made this much fainter (1 = not at all).
    opacity: f32,
    /// Where randomness starts for this frame's commands (`None` for the
    /// script itself, where it's simply the command's number).
    seed_base: Option<u64>,
    /// A named single shape's frame: its one command gets `seed_base`
    /// itself, the seed it would have had without the name.
    keep_seed: bool,
    /// What to put back when the frame ends.
    restore: Option<Saved>,
    /// The symbol being played, if this frame is a `use`.
    symbol: Option<String>,
    /// Colours to paint instead of others (from a `use`'s `colors`).
    swaps: Vec<(Rgba, Rgba)>,
    /// For a `repeat`: the copies still to come.
    copies: Option<Copies>,
    /// Where this frame is in the script, for messages (`None` for the
    /// script itself), like `command 12 (group "duck")`.
    path: Option<String>,
    /// The box around everything drawn in this frame so far.
    drawn: Option<Extent>,
    /// The same, counting only shapes and strokes (not fills or
    /// gradients over the whole layer): for the "painted most of the
    /// canvas" warning.
    shaped: Option<Extent>,
    /// Warnings about drawing outside the canvas from inside this frame,
    /// kept until it ends: if all of it was outside, one warning about
    /// the whole frame replaces them.
    outside: Vec<String>,
    /// For a named group or a `use`: its place in the report's parts.
    part: Option<usize>,
    /// The animation whose own commands this frame plays, if it's that.
    animation: Option<String>,
    /// For a `use`: the symbol's parameters, with the values this use
    /// gave (defaults for the rest). `"$name"` inside looks them up.
    params: Option<Arc<BTreeMap<String, Value>>>,
    /// Where this frame's commands may paint (a group's mask, combined
    /// with the masks around it), in painted pixels. `None`: anywhere.
    mask: Option<Arc<Mask>>,
    /// This frame paints a symbol's silhouette to make a mask (on a
    /// scratch picture, see `Player::capture`), not the picture itself.
    /// `Some(true)`: the mask is everything but the silhouette.
    capture: Option<bool>,
    /// Filters applied to what this frame draws (on a scratch picture)
    /// before it's laid on the layers.
    effect: Option<Vec<Filter>>,
}

/// A scratch picture a frame paints on instead of the real one: for a
/// symbol's silhouette (a mask), or a group with filters.
struct Scratch {
    document: Document,
    /// Everything goes on one layer (a silhouette).
    single: bool,
    /// For a filtered group: the real layer each scratch layer stands for.
    layers: Vec<(LayerId, LayerId)>,
}

impl Scratch {
    fn new(like: &Document, single: bool) -> Self {
        Self {
            document: Document::new(like.width(), like.height()),
            single,
            layers: Vec::new(),
        }
    }

    /// The scratch layer for the real layer `real` (made if needed).
    fn layer_for(&mut self, real: LayerId) -> LayerId {
        if self.single {
            return self.document.active_id();
        }
        if let Some(&(_, scratch)) = self.layers.iter().find(|(r, _)| *r == real) {
            return scratch;
        }
        let scratch = if self.layers.is_empty() {
            self.document.active_id() // the first layer a new picture has
        } else {
            let layer = self.document.new_layer();
            let id = layer.id();
            let count = self.document.layers().len();
            self.document.insert_layer(count, layer);
            id
        };
        self.layers.push((real, scratch));
        scratch
    }
}

/// A symbol: what `define` stored.
struct Symbol {
    commands: Arc<Vec<Command>>,
    /// With parameters: the commands as written, read again for each set
    /// of values a `use` gives (see `script::symbol_commands`).
    raw: Option<Arc<Vec<Value>>>,
    /// Its parameters and their defaults.
    params: Arc<BTreeMap<String, Value>>,
    /// Where it was defined, if in an included file: the file (as
    /// written in the include) and the define's command number there.
    source: Option<(String, usize)>,
}

/// A `repeat` plays its commands once per copy, each with the step
/// applied once more.
struct Copies {
    count: u32,
    /// The copy being played, from 0.
    current: u32,
    step: Transform,
    /// The step applied `current` times.
    so_far: Transform,
    /// The repeat's own frame settings, which each copy starts from.
    base_place: Transform,
    base_detail: f32,
    base_brush_scale: f32,
    /// Randomness for the repeat command itself (each copy mixes in its number).
    seed: u64,
    /// Values that differ per copy (`each`), as `$name` parameters.
    each: BTreeMap<String, Vec<Value>>,
    /// For a `stamp`: where each copy goes, instead of a step.
    placements: Vec<Transform>,
}

/// The parameters a `repeat`'s copy number `copy` gets from `each`.
fn each_params(each: &BTreeMap<String, Vec<Value>>, copy: u32) -> BTreeMap<String, Value> {
    each.iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(name, values)| {
            let value = values[copy as usize % values.len()].clone();
            (name.clone(), value)
        })
        .collect()
}

/// What a new frame plays and how.
struct NewFrame {
    commands: Arc<Vec<Command>>,
    label: String,
    placement: Placement,
    seed_base: u64,
    symbol: Option<String>,
    swaps: Vec<(Rgba, Rgba)>,
    /// True for a named group or a `use`: listed in the report.
    part: bool,
    params: Option<Arc<BTreeMap<String, Value>>>,
    /// Not turned by the frames around it.
    upright: bool,
    /// The group's `mask`, worked out once the frame has started.
    mask: Option<Value>,
    /// See `Frame::capture`.
    capture: Option<bool>,
    /// The group's `filter`, worked out once the frame has started.
    filter: Option<Value>,
    /// The group's `outline`, likewise.
    outline: Option<Value>,
}

/// The brush and selected layer from before a group, put back after it.
struct Saved {
    layer: LayerId,
    brush: BrushSettings,
    brush_alpha: u8,
}

/// `check` mentions a group scaled this much more one way than the other.
const UNEVEN_WARNING: f32 = 1.5;

/// The pixels a one-pixel line through `points` (in samples, `s` to a
/// pixel) goes through: from each point to the next one pixel per step
/// along the longer direction (Bresenham), and no corner pixel where a
/// diagonal step would do (no "L" at a turn).
fn pixel_line(points: &[StrokePoint], s: f32) -> Vec<(i32, i32)> {
    let cell = |p: &StrokePoint| ((p.x / s).floor() as i32, (p.y / s).floor() as i32);
    let mut cells: Vec<(i32, i32)> = vec![cell(&points[0])];
    for pair in points.windows(2) {
        let (mut x, mut y) = cell(&pair[0]);
        let (x1, y1) = cell(&pair[1]);
        let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
        let (sx, sy) = ((x1 - x).signum(), (y1 - y).signum());
        let mut error = dx + dy;
        while (x, y) != (x1, y1) {
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
            if cells.last() != Some(&(x, y)) {
                cells.push((x, y));
            }
        }
    }
    // Drop the middle of an "L": a pixel whose neighbours before and
    // after touch each other diagonally.
    let mut kept: Vec<(i32, i32)> = Vec::with_capacity(cells.len());
    for (i, &here) in cells.iter().enumerate() {
        if let (Some(&before), Some(&after)) = (kept.last(), cells.get(i + 1))
            && (before.0 - after.0).abs() == 1
            && (before.1 - after.1).abs() == 1
        {
            continue;
        }
        kept.push(here);
    }
    kept.dedup();
    kept
}

/// A group's transform and settings, checked.
struct Placement {
    transform: Transform,
    opacity: f32,
    scale_brush: bool,
    /// Brush sizes follow the average of an uneven scale (the old way),
    /// not the smaller of the two.
    stretch_brush: bool,
    /// The scale across and down, as written.
    scale: (f32, f32),
    /// The point it turns and resizes around (drawing units).
    origin: (f32, f32),
}

/// Plays a script, start to finish.
pub struct Player {
    /// The script's own commands.
    commands: Arc<Vec<Command>>,
    palette: HashMap<String, Rgba>,
    /// Symbols defined so far.
    symbols: HashMap<String, Symbol>,
    seed: u64,
    options: PlayOptions,
    /// The commands being played, innermost last.
    frames: Vec<Frame>,
    /// The position of the command being started, in its frame.
    current: usize,
    /// Where randomness starts for the command being started.
    current_seed: u64,
    brush: BrushSettings,
    /// The opacity part of the brush colour (from "#RRGGBBAA").
    brush_alpha: u8,
    jobs: VecDeque<Job>,
    running: Option<RunningStroke>,
    /// Finished undo steps, oldest first.
    undo: Vec<Box<dyn UndoStep>>,
    /// Tiles changed by painting since the last other kind of command, as
    /// they were before, per layer (in the order the layers were first
    /// painted on).
    tiles_before: Vec<(LayerId, TilesBefore)>,
    /// The empty layer a new canvas starts with. The first `layer`
    /// command renames it instead of leaving an empty layer behind.
    spare_layer: Option<LayerId>,
    /// The name of the layer painted on since the last event.
    painted: Option<String>,
    events: VecDeque<Event>,
    finished: bool,
    /// For the report: named groups and uses, warnings so far, the
    /// symbols defined (with their command numbers) and used, and the
    /// palette colours used.
    parts: Vec<Part>,
    silhouettes: Vec<Silhouette>,
    warnings: Vec<String>,
    defined: Vec<(String, usize)>,
    used_symbols: HashSet<String>,
    /// A symbol's commands read with one set of parameter values, by the
    /// symbol's name and the values (as JSON).
    symbol_versions: HashMap<(String, String), Arc<Vec<Command>>>,
    used_colors: RefCell<HashSet<String>>,
    painted_by: HashMap<LayerId, BTreeSet<usize>>,
    read_by: HashMap<LayerId, BTreeSet<usize>>,
    /// The animation being played: its name and commands.
    animation: Option<(String, Arc<Vec<Command>>)>,
    /// Those commands, until they're started (after the script's own).
    pending_animation: Option<Arc<Vec<Command>>>,
    /// The frame being painted, counting from 0.
    frame: u32,
    /// With `PlayOptions::reuse`: the static layers put in place so far.
    restored: HashSet<LayerId>,
    /// The frame and the animation's length, for keyframes.
    clock: Clock,
    /// A pixel canvas: painted this many times bigger, then shrunk.
    pixel: Option<PixelMode>,
    /// How many painted pixels one canvas pixel is: the preview scale
    /// times the pixel canvas's supersample.
    paint_scale: f32,
    /// Scratch pictures: one per frame being played that paints a
    /// silhouette (for a mask) or a filtered group, innermost last.
    capture: Vec<Scratch>,
    /// Brush tips loaded from files, by the number in `Tip::File`, and
    /// which file each came from (so a file is loaded once).
    tips: Vec<Arc<TipShape>>,
    tip_files: HashMap<String, u32>,
    /// Textures loaded from files, by file name.
    textures: HashMap<String, Arc<TextureImage>>,
    /// Patterns a brush paints with, by the number in `BrushSettings::source`.
    sources: Vec<Arc<Paint>>,
    /// The `select` command's selection: painting stays inside it.
    selection: Option<Selection>,
    /// The selection combined with a group's mask, kept so it isn't worked
    /// out again for every stroke: which two masks (by address) it was
    /// made from, and the result.
    clip_cache: Option<(usize, usize, Arc<Mask>)>,
    /// Palette colours that came from included files (not the script's
    /// own, so not "never used" warnings), and which file each came from.
    included_colors: HashMap<String, String>,
}

impl Player {
    /// Gets ready to play `script`, which must start with `canvas`.
    pub fn new(script: Script, options: PlayOptions) -> Result<Self, ScriptError> {
        match script.commands.first() {
            Some(Command::Canvas(_)) => {}
            Some(other) => {
                return Err(ScriptError::in_command(
                    0,
                    Some(&other.label()),
                    "a script starts with a `canvas` command, e.g. \
                     {\"cmd\": \"canvas\", \"width\": 1600, \"height\": 1000}",
                ));
            }
            None => return Err(ScriptError::general("the script has no commands")),
        }
        if !(options.scale > 0.0 && options.scale <= 16.0) {
            return Err(ScriptError::general(
                "the scale must be above 0 and at most 16",
            ));
        }
        let animation = match (&options.animation, script.animations.is_empty()) {
            (_, true) if options.animation.is_some() => {
                return Err(ScriptError::general("this script has no animations"));
            }
            (_, true) => None,
            (Some(name), false) => match script.animations.get(name) {
                Some(animation) => Some((name.clone(), animation)),
                None => {
                    let names: Vec<&str> = script.animations.keys().map(String::as_str).collect();
                    return Err(ScriptError::general(format!(
                        "there's no animation called \"{name}\" (animations: {})",
                        names.join(", ")
                    )));
                }
            },
            (None, false) => script
                .animations
                .iter()
                .next()
                .map(|(name, animation)| (name.clone(), animation)),
        };
        if let Some((name, animation)) = &animation
            && options.frame >= animation.frames
        {
            return Err(ScriptError::general(format!(
                "animation \"{name}\" has {} frames (0 to {}), so there's no frame {}",
                animation.frames,
                animation.frames - 1,
                options.frame
            )));
        }
        let clock = Clock {
            frame: options.frame,
            length: animation.as_ref().map(|(_, a)| (a.frames, a.looping)),
            fps: animation.as_ref().map_or(12.0, |(_, a)| a.fps),
            seconds: animation.as_ref().is_some_and(|(_, a)| a.seconds),
        };
        let animation = animation.map(|(name, a)| (name, Arc::new(a.commands.clone())));
        let palette: HashMap<String, Rgba> = script
            .palette
            .iter()
            .map(|(name, color)| {
                Ok((
                    name.clone(),
                    parse_hex(color).map_err(ScriptError::general)?,
                ))
            })
            .collect::<Result<_, ScriptError>>()?;
        let pixel = match script.commands.first() {
            Some(Command::Canvas(canvas)) => pixel_mode(canvas, &palette)
                .map_err(|m| ScriptError::in_command(0, Some("canvas"), m))?,
            _ => None,
        };
        let paint_scale = options.scale * pixel.as_ref().map_or(1.0, |p| p.supersample as f32);
        // The pixel canvas's outline colour counts as used (it outlines
        // every shape, though no command names it).
        let mut used_colors = HashSet::new();
        if let Some(Command::Canvas(canvas)) = script.commands.first()
            && let Some(Value::Object(fields)) = &canvas.pixel
            && let Some(Value::String(name)) = fields.get("outline")
        {
            used_colors.insert(name.clone());
        }
        Ok(Self {
            commands: Arc::new(script.commands),
            palette,
            symbols: HashMap::new(),
            seed: script.seed,
            frames: Vec::new(),
            current: 0,
            current_seed: 0,
            brush: BrushSettings {
                color: [0, 0, 0],
                ..BrushSettings::default()
            },
            brush_alpha: 255,
            jobs: VecDeque::new(),
            running: None,
            undo: Vec::new(),
            tiles_before: Vec::new(),
            spare_layer: None,
            painted: None,
            events: VecDeque::new(),
            finished: false,
            parts: Vec::new(),
            silhouettes: Vec::new(),
            warnings: Vec::new(),
            defined: Vec::new(),
            used_symbols: HashSet::new(),
            symbol_versions: HashMap::new(),
            used_colors: RefCell::new(used_colors),
            painted_by: HashMap::new(),
            read_by: HashMap::new(),
            frame: options.frame,
            restored: HashSet::new(),
            clock,
            pixel,
            paint_scale,
            animation,
            pending_animation: None,
            options,
            capture: Vec::new(),
            tips: Vec::new(),
            tip_files: HashMap::new(),
            textures: HashMap::new(),
            sources: Vec::new(),
            selection: None,
            clip_cache: None,
            included_colors: HashMap::new(),
        })
    }

    fn canvas(&self) -> &super::CanvasCmd {
        match &self.commands[0] {
            Command::Canvas(canvas) => canvas,
            _ => unreachable!("checked in `new`"),
        }
    }

    /// The frame being painted, counting from 0 (0 for a single picture).
    pub fn frame_number(&self) -> u32 {
        self.frame
    }

    /// The animation being played, if any.
    pub fn animation(&self) -> Option<&str> {
        self.animation.as_ref().map(|(name, _)| name.as_str())
    }

    /// The canvas size the script asks for, if it gives one (at full size).
    pub fn canvas_size(&self) -> Option<(u32, u32)> {
        let canvas = self.canvas();
        canvas.width.zip(canvas.height)
    }

    /// Makes the picture the script starts from: a new canvas, or the
    /// picture it opens. Use this, or `start_on`, before playing.
    pub fn create_document(&mut self) -> Result<Document, ScriptError> {
        let error = |message: String| ScriptError::in_command(0, Some("canvas"), message);
        let canvas = self.canvas().clone();
        let document = match (&canvas.open, canvas.width, canvas.height) {
            (Some(path), None, None) => {
                if self.options.scale != 1.0 {
                    return Err(error(
                        "a script that opens a picture can only be played at full size".into(),
                    ));
                }
                let full_path = match &self.options.folder {
                    Some(folder) => folder.join(path),
                    None => PathBuf::from(path),
                };
                let mut document = file::open(&full_path)
                    .map_err(|e| error(format!("can't open \"{}\": {e}", full_path.display())))?;
                if let Some(paper) = &canvas.paper {
                    document.set_paper(self.paper_color(paper).map_err(error)?);
                }
                document
            }
            (Some(_), _, _) => {
                return Err(error(
                    "use either `open` or `width` and `height`, not both".into(),
                ));
            }
            (None, Some(width), Some(height)) => {
                if width == 0 || height == 0 || width > MAX_CANVAS_SIDE || height > MAX_CANVAS_SIDE
                {
                    return Err(error(format!(
                        "the canvas must be between 1 and {MAX_CANVAS_SIDE} pixels wide and high"
                    )));
                }
                let scaled = |side: u32| ((side as f32 * self.paint_scale).round() as u32).max(1);
                let mut document = Document::new(scaled(width), scaled(height));
                if let Some(paper) = &canvas.paper {
                    document.set_paper(self.paper_color(paper).map_err(error)?);
                }
                self.spare_layer = Some(document.active_id());
                document
            }
            _ => {
                return Err(error(
                    "give `width` and `height` (or `open` with a picture file)".into(),
                ));
            }
        };
        self.start_frames();
        Ok(document)
    }

    /// Gets ready to play on a picture that's already open (in the app).
    /// The canvas's paper colour is applied; its size isn't checked here
    /// (see `canvas_size`) and `open` is ignored.
    pub fn start_on(&mut self, document: &mut Document) -> Result<(), ScriptError> {
        if self.pixel.is_some() {
            return Err(ScriptError::in_command(
                0,
                Some("canvas"),
                "a pixel canvas is painted bigger and shrunk at the end, so it can't be played \
                 onto an open picture: play it on a new one, or export its frames",
            ));
        }
        if let Some(paper) = self.canvas().paper.clone() {
            let color = self
                .paper_color(&paper)
                .map_err(|m| ScriptError::in_command(0, Some("canvas"), m))?;
            if let Some(step) = PaperCommand::apply(document, color) {
                self.record(step);
            }
        }
        // Painting on an open picture stays inside its selection, as the
        // app's own tools do (the Text tool plays its text this way).
        self.selection = document.selection().cloned();
        self.start_frames();
        Ok(())
    }

    /// The bottom frame: the script's own commands, after `canvas`. A
    /// preview is everything shrunk around the canvas's top-left corner.
    fn start_frames(&mut self) {
        let scale = self.paint_scale;
        self.frames = vec![Frame {
            commands: Arc::clone(&self.commands),
            next: 1,
            label: None,
            place: Transform::scale(scale, scale),
            detail: scale,
            brush_scale: scale,
            opacity: 1.0,
            seed_base: None,
            keep_seed: false,
            restore: None,
            symbol: None,
            swaps: Vec::new(),
            copies: None,
            path: None,
            drawn: None,
            shaped: None,
            outside: Vec::new(),
            part: None,
            animation: None,
            params: None,
            mask: None,
            capture: None,
            effect: None,
        }];
        self.pending_animation = self.animation.as_ref().map(|(_, c)| Arc::clone(c));
    }

    /// Plays until `budget` is used up (roughly: pixels of brush travel),
    /// something worth knowing happens, or the script ends.
    pub fn advance(
        &mut self,
        document: &mut Document,
        mut budget: f32,
    ) -> Result<Progress, ScriptError> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Ok(Progress::Event(event));
            }
            if self.finished {
                return Ok(Progress::Finished);
            }
            if budget <= 0.0 {
                return Ok(Progress::Working);
            }
            if self.running.is_some() {
                budget = self.continue_stroke(document, budget);
            } else if let Some(job) = self.jobs.pop_front() {
                budget -= self.start_job(document, job);
            } else if let Some(frame) = self.frames.last_mut() {
                if frame.next < frame.commands.len() {
                    let commands = Arc::clone(&frame.commands);
                    let index = frame.next;
                    frame.next += 1;
                    // With reuse: static layers whose last painter is done
                    // get their pixels now; a static command whose paint
                    // comes from the first frame is skipped.
                    let top_level = self.frames.len() == 1 && self.frames[0].animation.is_none();
                    if top_level && self.options.reuse.is_some() {
                        self.restore_static(document, index);
                        if self
                            .options
                            .reuse
                            .as_ref()
                            .is_some_and(|reuse| reuse.skip.contains(&(index + 1)))
                        {
                            continue;
                        }
                    }
                    let command = &commands[index];
                    self.current = index;
                    self.current_seed = self.default_seed(index);
                    let queued = self.jobs.len();
                    self.start_command(document, command)
                        .map_err(|message| self.error_at(index, command, message))?;
                    self.note_drawn(document, index, command, queued);
                } else {
                    self.next_copy_or_end_frame(document);
                }
            } else if let Some(commands) = self.pending_animation.take() {
                // The script's own commands are done: now the animation's.
                self.restore_static(document, usize::MAX);
                let scale = self.paint_scale;
                let name = self.animation.as_ref().map(|(name, _)| name.clone());
                self.frames.push(Frame {
                    commands,
                    next: 0,
                    label: None,
                    place: Transform::scale(scale, scale),
                    detail: scale,
                    brush_scale: scale,
                    opacity: 1.0,
                    seed_base: None,
                    keep_seed: false,
                    restore: None,
                    symbol: None,
                    swaps: Vec::new(),
                    copies: None,
                    path: None,
                    drawn: None,
                    shaped: None,
                    outside: Vec::new(),
                    part: None,
                    animation: name,
                    params: None,
                    mask: None,
                    capture: None,
                    effect: None,
                });
            } else {
                self.flush_tiles();
                if self.animation.is_none() {
                    self.restore_static(document, usize::MAX);
                }
                if let Some(name) = self.painted.take() {
                    self.events.push_back(Event::LayerDone(name));
                }
                self.finished = true;
            }
        }
    }

    /// With `PlayOptions::reuse`: puts in place the pixels of the static
    /// layers whose last painter (among the first `done` of the script's
    /// own commands) is finished, each once.
    fn restore_static(&mut self, document: &mut Document, done: usize) {
        let Some(reuse) = self.options.reuse.clone() else {
            return;
        };
        let due: Vec<LayerId> = reuse
            .points
            .iter()
            .filter(|(id, point)| **point <= done && !self.restored.contains(*id))
            .map(|(id, _)| *id)
            .collect();
        if due.is_empty() {
            return;
        }
        self.flush_tiles();
        for id in due {
            if let (Some(surface), Some(layer)) = (reuse.surfaces.get(&id), document.layer_mut(id))
            {
                layer.surface = surface.clone();
            }
            self.restored.insert(id);
        }
    }

    /// Notes that the command being started reads `layer`'s pixels.
    fn note_read(&mut self, layer: LayerId) {
        let top = self.position().0;
        self.read_by.entry(layer).or_default().insert(top);
    }

    /// Plays everything that's left. `on_event` is called at every event,
    /// with the picture as it is at that moment.
    pub fn run(
        &mut self,
        document: &mut Document,
        mut on_event: impl FnMut(&Event, &Document),
    ) -> Result<(), ScriptError> {
        loop {
            match self.advance(document, f32::INFINITY)? {
                Progress::Event(event) => on_event(&event, document),
                Progress::Finished => {
                    self.finish_pixels(document);
                    return Ok(());
                }
                Progress::Working => {}
            }
        }
    }

    /// On a pixel canvas, once the script has finished: shrinks the
    /// painting to the canvas's real size, crisp and in palette colours.
    /// (Done by `run`; a caller of `advance` does it when `Finished`.)
    pub fn finish_pixels(&self, document: &mut Document) {
        let Some(pixel) = &self.pixel else {
            return;
        };
        let s = pixel.supersample.max(1);
        let (width, height) = (
            (document.width() / s).max(1),
            (document.height() / s).max(1),
        );
        *document = pixel.shrink(document, width, height);
    }

    /// True for a pixel canvas (see `pixels.rs`).
    pub fn is_pixel(&self) -> bool {
        self.pixel.is_some()
    }

    /// How far along the script's own commands: (started, all of them).
    pub fn position(&self) -> (usize, usize) {
        let own = self.commands.len();
        let total = own + self.animation.as_ref().map_or(0, |(_, c)| c.len());
        match self.frames.first() {
            Some(frame) if frame.animation.is_some() => (own + frame.next, total),
            Some(script) => (script.next, total),
            None if self.finished => (total, total),
            None => (0, total),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The undo steps for everything played so far, oldest first. Only
    /// filled with `record_undo`. Call it when playing has stopped.
    pub fn take_undo(&mut self, document: &mut Document) -> Vec<Box<dyn UndoStep>> {
        if let Some(running) = self.running.take() {
            // Stopped in the middle of a stroke: finish what's there.
            self.finish_stroke(running, document);
        }
        self.flush_tiles();
        std::mem::take(&mut self.undo)
    }

    // ---- Frames -------------------------------------------------------------------

    fn frame(&self) -> &Frame {
        self.frames
            .last()
            .expect("a command is only started inside a frame")
    }

    /// Where randomness starts for command `index` of the innermost frame:
    /// at the script's top level its number (as it always was), inside a
    /// group a mix of the group's and the command's.
    fn default_seed(&self, index: usize) -> u64 {
        let frame = self.frames.last();
        match frame.and_then(|frame| frame.seed_base) {
            None => index as u64 + 1,
            Some(base) if frame.is_some_and(|f| f.keep_seed) => base,
            Some(base) => Random::mix(base, index as u64 + 1),
        }
    }

    /// Starts playing new commands inside the current frame, placed by
    /// their placement on top of the current frame's own.
    fn push_frame(&mut self, document: &Document, new: NewFrame) -> Result<(), String> {
        if self.frames.len() >= MAX_DEPTH {
            return Err(format!(
                "groups, symbols and repeats are nested too deeply (more than {MAX_DEPTH})"
            ));
        }
        let path = self.path_to(self.current, &new.label);
        let inside = self.frames.iter().rev().find_map(|frame| frame.part);
        let part = new.part.then(|| {
            self.parts.push(Part {
                path: path.clone(),
                area: None,
                inside,
            });
            self.parts.len() - 1
        });
        let parent = self.frame();
        let own = new.placement.transform;
        let mut place = own.then(parent.place);
        if new.upright {
            // Undo the turning of everything around it, around the point
            // where its origin lands (so it still moves and mirrors along).
            let (x, y) = place.apply(new.placement.origin);
            let mut turned = parent.place.b.atan2(parent.place.a).to_degrees();
            if parent.place.mirrors() {
                turned -= 180.0; // a mirrored x-axis points backwards
            }
            place = place
                .then(Transform::translate(-x, -y))
                .then(Transform::rotate(-turned))
                .then(Transform::translate(x, y));
        }
        // A silhouette is painted on its own, so the masks around it
        // don't apply while it's captured: they're applied to the mask.
        let mask = match new.capture {
            Some(_) => None,
            None => parent.mask.clone(),
        };
        let symbol_starts = new.symbol.is_some();
        // Scaled to 0 (across, down or both): it shrinks to nothing. Its
        // commands still run (a layer it makes, a symbol it defines), but
        // paint nothing.
        let (sx, sy) = new.placement.scale;
        let vanishes = sx == 0.0 || sy == 0.0;
        // Lines keep their width along the less stretched direction (or
        // the average, the old way, with `stretch_brush`).
        let brush_factor = if new.placement.stretch_brush {
            own.size_factor()
        } else {
            own.line_factor()
        };
        // (Only worth a warning when the widths visibly differ.)
        let uneven = sx.min(sy) > 0.0 && sx.max(sy) / sx.min(sy) >= UNEVEN_WARNING;
        let uneven_warning = (uneven && new.placement.scale_brush && !self.options.dry_run).then(|| {
            format!(
                "{path}: scaled unevenly ({sx} across, {sy} down), so line widths and brush sizes \
                 follow the smaller scale ({:.2}); add \"stretch_brush\": true for the average ({:.2})",
                own.line_factor(),
                own.size_factor()
            )
        });
        let frame = Frame {
            commands: new.commands,
            next: 0,
            label: Some((self.current + 1, new.label)),
            place,
            detail: if vanishes {
                parent.detail
            } else {
                parent.detail * own.size_factor()
            },
            brush_scale: if new.placement.scale_brush && !vanishes {
                parent.brush_scale * brush_factor
            } else {
                parent.brush_scale
            },
            opacity: if vanishes {
                0.0
            } else {
                parent.opacity * new.placement.opacity
            },
            seed_base: Some(new.seed_base),
            keep_seed: false,
            restore: Some(Saved {
                layer: document.active_id(),
                brush: self.brush,
                brush_alpha: self.brush_alpha,
            }),
            symbol: new.symbol,
            swaps: new.swaps,
            copies: None,
            path: Some(path),
            drawn: None,
            shaped: None,
            outside: Vec::new(),
            part,
            animation: None,
            params: new.params,
            mask,
            capture: new.capture,
            effect: None,
        };
        // Once per group (an animation's frames scale it differently).
        if let Some(warning) = uneven_warning
            && !self.warnings.iter().any(|w| {
                w.split_once(": scaled unevenly").map(|(p, _)| p)
                    == warning.split_once(": scaled unevenly").map(|(p, _)| p)
            })
        {
            self.warnings.push(warning);
        }
        self.frames.push(frame);
        // A symbol looks the same wherever it's used: it starts from the
        // plain brush (the brush from before comes back after it).
        if symbol_starts {
            self.brush = BrushSettings::default();
            self.brush_alpha = 255;
        }
        let mut filters = match &new.filter {
            Some(filter) => self
                .read_filters(filter)
                .map_err(|problem| format!("`filter`: {problem}"))?,
            None => Vec::new(),
        };
        if let Some(outline) = &new.outline {
            let spec: OutlineSpec = serde_json::from_value(self.value(outline)?)
                .map_err(|problem| format!("`outline`: {problem}"))?;
            let more = self
                .read_outline(spec)
                .map_err(|problem| format!("`outline`: {problem}"))?;
            filters.extend(more);
        }
        // A named part whose silhouette is wanted is painted on a scratch
        // picture too (with no filters), so its paint can be recorded.
        let wanted = self.options.silhouettes && part.is_some() && new.capture.is_none();
        if !filters.is_empty() || wanted {
            self.frames.last_mut().expect("just pushed").effect = Some(filters);
            self.capture.push(Scratch::new(document, false));
        }
        if new.capture.is_some() {
            self.capture.push(Scratch::new(document, true));
        }
        if let Some(mask) = new.mask {
            self.start_mask(document, &mask)
                .map_err(|problem| format!("`mask`: {problem}"))?;
        }
        Ok(())
    }

    /// Works out a group's mask, now that the group's frame has started
    /// (so it's placed by the group's transform): an area, a layer's
    /// paint, or `not` one of those, straight away; a symbol's
    /// silhouette by playing the symbol onto a scratch picture first.
    fn start_mask(&mut self, document: &Document, written: &Value) -> Result<(), String> {
        let help = "a mask is an area ({\"ellipse\": [...]}, {\"polygon\": [...]}, ...), \
                    {\"symbol\": \"name\", ...placed like a use}, {\"layer\": \"name\"} \
                    or {\"not\": any of those}";
        let value = self.value(written)?;
        let mut object = value.as_object().ok_or(help)?.clone();
        let mut invert = false;
        if object.len() == 1
            && let Some(inner) = object.get("not")
        {
            invert = true;
            object = inner.as_object().ok_or(help)?.clone();
            if object.contains_key("not") {
                return Err("`not` inside `not`: just leave both out".into());
            }
        }
        let limits = document.bounds();
        let mask = if let Some(symbol) = object.remove("symbol") {
            // Played like a use, named by `symbol` instead of `name`.
            object.insert("name".to_owned(), symbol);
            let using: UseCmd =
                serde_json::from_value(Value::Object(object)).map_err(|problem| {
                    format!("{problem} (a symbol mask takes the fields of a use)")
                })?;
            return self.use_symbol(document, &using, Some(invert));
        } else if let Some(name) = object.get("layer").filter(|_| object.len() == 1) {
            let name = name
                .as_str()
                .ok_or("`layer` must be a layer's name in quotes")?;
            let id = find_layer(document, name)?;
            self.note_read(id);
            let layer = document.layer(id).ok_or("no such layer")?;
            Mask::from_alpha(&layer.surface, limits)
        } else {
            let polygon = self.area(&Value::Object(object)).map_err(|_| help)?;
            let placed = self.place_polygon(polygon);
            geometry::polygons_mask(&[placed], limits)
        };
        self.finish_mask(limits, mask, invert);
        Ok(())
    }

    /// Makes `mask` the innermost frame's: hard-edged on a pixel canvas,
    /// turned inside out if `invert`, and combined with the masks around.
    fn finish_mask(&mut self, limits: PixelRect, mut mask: Mask, invert: bool) {
        if invert {
            mask = mask.inverted(limits);
        }
        if self.pixel.is_some() {
            mask.threshold();
        }
        let frame = self.frames.last_mut().expect("a mask belongs to a frame");
        if let Some(outer) = &frame.mask {
            mask = mask.intersect(outer);
        }
        if mask.is_empty()
            && !self.options.dry_run
            && let Some(path) = &frame.path
        {
            self.warnings.push(format!(
                "{path}: its mask covers nothing, so it paints nothing"
            ));
        }
        frame.mask = Some(Arc::new(mask));
    }

    /// A filtered group has finished: filters each layer's part of its
    /// drawing, makes one shadow from the combined silhouette (laid on
    /// the lowest of those layers, under everything), and lays the parts
    /// on their layers (or on the scratch picture of the group around).
    fn lay_effect(
        &mut self,
        document: &mut Document,
        mut scratch: Scratch,
        filters: &[Filter],
        part: Option<usize>,
    ) {
        let limits = document.bounds();
        let per_part: Vec<Filter> = filters
            .iter()
            .filter(|f| !f.from_silhouette())
            .cloned()
            .collect();
        let shadows: Vec<Filter> = filters
            .iter()
            .filter(|f| f.from_silhouette())
            .cloned()
            .collect();
        let mut parts: Vec<(LayerId, Surface)> = Vec::new();
        for &(real, id) in &scratch.layers {
            let Some(layer) = scratch.document.layer_mut(id) else {
                continue;
            };
            if layer.surface.tile_count() == 0 {
                continue;
            }
            filters::apply(&mut layer.surface, &per_part, limits);
            parts.push((real, std::mem::take(&mut layer.surface)));
        }
        if parts.is_empty() {
            return;
        }
        if self.options.silhouettes
            && let Some(index) = part
        {
            let surfaces: Vec<&Surface> = parts.iter().map(|(_, s)| s).collect();
            let mask = Mask::from_alpha(&filters::silhouette(&surfaces), limits);
            // The part's own paint: its layers' shares laid bottom to top.
            let mut order: Vec<&(LayerId, Surface)> = parts.iter().collect();
            order.sort_by_key(|(real, _)| document.layer_index(*real).unwrap_or(0));
            let mut own = Surface::new();
            for (_, surface) in order {
                filters::lay_over(&mut own, surface, limits);
            }
            let rect = mask.rect();
            let mut image = Image::new(
                (rect.right - rect.left).max(0) as u32,
                (rect.bottom - rect.top).max(0) as u32,
                [0, 0, 0, 0],
            );
            for y in rect.top..rect.bottom {
                for x in rect.left..rect.right {
                    let p = own.pixel(x, y);
                    image.set_pixel(x - rect.left, y - rect.top, crate::image::unpremultiply(p));
                }
            }
            self.silhouettes.push(Silhouette {
                part: index,
                mask,
                own: image,
            });
        }
        // Shadows and outlines go under the lowest layer's part (shadows
        // furthest back); rim light goes over the highest layer's.
        let mut rim = Surface::new();
        if !shadows.is_empty() {
            let surfaces: Vec<&Surface> = parts.iter().map(|(_, s)| s).collect();
            let silhouette = filters::silhouette(&surfaces);
            let mut under = Surface::new();
            for filter in &shadows {
                if let Filter::Shadow {
                    offset,
                    blur,
                    color,
                } = filter
                {
                    let one = filters::shadow_of(&silhouette, *offset, *blur, *color, limits);
                    filters::lay_over(&mut under, &one, limits);
                }
            }
            for filter in &shadows {
                match filter {
                    Filter::Outline { .. } => {
                        let one = filters::line_of(&silhouette, filter, limits);
                        filters::lay_over(&mut under, &one, limits);
                    }
                    Filter::Rim { .. } => {
                        let one = filters::line_of(&silhouette, filter, limits);
                        filters::lay_over(&mut rim, &one, limits);
                    }
                    _ => {}
                }
            }
            let lowest = parts
                .iter()
                .map(|(real, _)| *real)
                .min_by_key(|real| document.layer_index(*real).unwrap_or(0))
                .expect("there are parts");
            self.lay_part(document, lowest, &under);
        }
        let highest = parts
            .iter()
            .map(|(real, _)| *real)
            .max_by_key(|real| document.layer_index(*real).unwrap_or(0))
            .expect("there are parts");
        for (real, part) in parts {
            self.lay_part(document, real, &part);
        }
        if rim.tile_count() > 0 {
            self.lay_part(document, highest, &rim);
        }
    }

    /// Lays `part` on the real layer `real`: on the picture, or on the
    /// scratch picture of the frame around (when filtered groups nest).
    fn lay_part(&mut self, document: &mut Document, real: LayerId, part: &Surface) {
        let limits = document.bounds();
        if let Some(outer) = self.capture.last_mut() {
            let id = outer.layer_for(real);
            if let Some(layer) = outer.document.layer_mut(id) {
                filters::lay_over(&mut layer.surface, part, limits);
            }
            return;
        }
        let Some(layer) = document.layer_mut(real) else {
            return;
        };
        let before = filters::lay_over(&mut layer.surface, part, limits);
        if before.is_empty() {
            return;
        }
        self.remember_tiles(real, before);
        self.mark_painted(document, real);
        let top = self.position().0;
        self.painted_by.entry(real).or_default().insert(top);
    }

    /// True while a symbol's silhouette is being painted for a mask.
    fn capturing(&self) -> bool {
        self.capture.last().is_some_and(|scratch| scratch.single)
    }

    /// True while painting goes on a scratch picture (a silhouette, or a
    /// filtered group's drawing).
    fn on_scratch(&self) -> bool {
        !self.capture.is_empty()
    }

    /// The innermost frame has played all its commands: start its next
    /// copy (for a `repeat`), or end it.
    fn next_copy_or_end_frame(&mut self, document: &mut Document) {
        let more_copies = self
            .frames
            .last()
            .and_then(|frame| frame.copies.as_ref())
            .is_some_and(|copies| copies.current + 1 < copies.count);
        if !more_copies {
            if let Some(frame) = self.frames.pop() {
                self.end_frame(document, frame);
            }
            return;
        }
        // Each copy starts with the brush and layer from before the repeat.
        if let Some(saved) = self.frames.last().and_then(|f| f.restore.as_ref()) {
            let (layer, brush, alpha) = (saved.layer, saved.brush, saved.brush_alpha);
            self.restore(document, layer, brush, alpha);
        }
        let frame = self.frames.last_mut().expect("checked above");
        let copies = frame.copies.as_mut().expect("checked above");
        copies.current += 1;
        copies.so_far = match copies.placements.get(copies.current as usize) {
            Some(place) => *place,
            None => copies.so_far.then(copies.step),
        };
        frame.next = 0;
        frame.place = copies.so_far.then(copies.base_place);
        frame.detail = copies.base_detail * copies.so_far.size_factor().max(1e-3);
        frame.brush_scale = copies.base_brush_scale * copies.so_far.line_factor();
        frame.seed_base = Some(Random::mix(copies.seed, copies.current as u64 + 1));
        if !copies.each.is_empty() {
            frame.params = Some(Arc::new(each_params(&copies.each, copies.current)));
        }
        if let Some((_, label)) = &mut frame.label {
            let word = if label.starts_with("stamp") {
                "stamp"
            } else {
                "repeat"
            };
            *label = format!("{word}, copy {}", copies.current + 1);
        }
    }

    /// A frame has played all its commands: put back the brush and the
    /// selected layer from before it.
    fn end_frame(&mut self, document: &mut Document, frame: Frame) {
        if let Some(saved) = frame.restore {
            self.restore(document, saved.layer, saved.brush, saved.brush_alpha);
        }
        if let Some(invert) = frame.capture {
            // The silhouette is complete: it's the mask of the group below.
            let scratch = self
                .capture
                .pop()
                .expect("a capture frame has a scratch picture");
            let limits = scratch.document.bounds();
            let mask = Mask::from_alpha(&scratch.document.active_layer().surface, limits);
            self.finish_mask(limits, mask, invert);
            return;
        }
        if let Some(filters) = &frame.effect {
            let scratch = self
                .capture
                .pop()
                .expect("a filtered frame has a scratch picture");
            self.lay_effect(document, scratch, filters, frame.part);
        }
        // For the report: hand what it drew to the frame around it.
        if let Some(index) = frame.part {
            self.parts[index].area = frame.drawn.map(|area| self.canvas_box(area));
        }
        let mut outside = frame.outside;
        if let (Some(drawn), Some(path)) = (frame.drawn, &frame.path)
            && outside.len() > 1
            && !overlaps(drawn, document.bounds())
        {
            outside = vec![self.outside_warning(path, drawn)];
        }
        // A named part whose shapes are small but which painted most of
        // the canvas: a fill or gradient without an area, probably.
        if let (Some(_), Some(drawn), Some(shaped), Some(path)) =
            (frame.part, frame.drawn, frame.shaped, &frame.path)
        {
            let canvas = document.bounds();
            let (painted, shapes) = (covered(drawn, canvas), covered(shaped, canvas));
            if painted >= 0.5 && shapes <= 0.1 {
                outside.push(format!(
                    "{path} painted {:.0}% of the canvas, but its shapes cover {:.0}%: a gradient \
                     or fill without an area, inside a mask with \"not\"?",
                    painted * 100.0,
                    shapes * 100.0
                ));
            }
        }
        match self.frames.last_mut() {
            Some(parent) => {
                if let Some(drawn) = frame.drawn {
                    parent.drawn = Some(union(parent.drawn, drawn));
                }
                if let Some(shaped) = frame.shaped {
                    parent.shaped = Some(union(parent.shaped, shaped));
                }
                parent.outside.append(&mut outside);
            }
            None => self.warnings.append(&mut outside),
        }
    }

    fn restore(
        &mut self,
        document: &mut Document,
        layer: LayerId,
        brush: BrushSettings,
        alpha: u8,
    ) {
        self.brush = brush;
        self.brush_alpha = alpha;
        if document.layer(layer).is_some() && document.active_id() != layer {
            if let Some(name) = self.painted.take() {
                self.events.push_back(Event::LayerDone(name));
            }
            document.set_active(layer);
        }
    }

    /// The way to command `index` of the innermost frame from the
    /// script's own commands: (number, label) for each step.
    fn steps_to(&self, index: usize, label: &str) -> Vec<(usize, String)> {
        let mut steps: Vec<(usize, String)> = self
            .frames
            .iter()
            .filter_map(|frame| frame.label.clone())
            .collect();
        steps.push((index + 1, label.to_owned()));
        steps
    }

    /// Where command `index` of the innermost frame is, as in error
    /// messages: `command 12 (group "duck") > 3 (ellipse)`.
    fn path_to(&self, index: usize, label: &str) -> String {
        let steps = self.steps_to(index, label);
        let mut path = String::new();
        if let Some(name) = self.frames.iter().find_map(|f| f.animation.as_ref()) {
            path = format!("animation \"{name}\" > ");
        }
        for (i, (number, label)) in steps.iter().enumerate() {
            if i == 0 {
                path.push_str(&format!("command {number} ({label})"));
            } else {
                path.push_str(&format!(" > {number} ({label})"));
            }
        }
        path
    }

    /// The error for a mistake in command `index` of the innermost frame,
    /// with the way to it from the script's own commands.
    fn error_at(&self, index: usize, command: &Command, message: String) -> ScriptError {
        let mut steps = self.steps_to(index, &command.label());
        let (number, label) = steps.remove(0);
        ScriptError {
            command: Some(number),
            kind: Some(label),
            inside: steps
                .into_iter()
                .map(|(number, label)| format!("{number} ({label})"))
                .collect(),
            line: None,
            animation: self.frames.iter().find_map(|f| f.animation.clone()),
            message,
        }
    }

    // ---- The report ---------------------------------------------------------------

    /// What was drawn where, and what looks like a mistake. Complete once
    /// the script has finished (before that, it's what's known so far).
    pub fn report(&self) -> Report {
        let (parts, warnings) = match &self.options.reuse {
            Some(reuse) => self.merged_parts(reuse),
            None => (self.parts.clone(), self.warnings.clone()),
        };
        let mut painted_by: HashMap<LayerId, BTreeSet<usize>> = self.painted_by.clone();
        if let Some(reuse) = &self.options.reuse {
            for (layer, numbers) in &reuse.painted_by {
                painted_by
                    .entry(*layer)
                    .or_default()
                    .extend(numbers.iter().copied());
            }
        }
        Report {
            parts,
            silhouettes: self.silhouettes.clone(),
            warnings,
            defined: self.defined.clone(),
            used_symbols: self.used_symbols.iter().cloned().collect(),
            used_colors: self.used_colors.borrow().iter().cloned().collect(),
            palette: self
                .palette
                .keys()
                .filter(|name| !self.included_colors.contains_key(*name))
                .cloned()
                .collect(),
            painted_by: painted_by
                .iter()
                .map(|(&layer, numbers)| (layer, numbers.iter().copied().collect()))
                .collect(),
            read_by: self
                .read_by
                .iter()
                .map(|(&layer, numbers)| (layer, numbers.iter().copied().collect()))
                .collect(),
        }
    }

    /// The parts and warnings as a full paint would report them: this
    /// frame's own, with the skipped commands' put back in their places
    /// (`inside` pointing at the right entries).
    fn merged_parts(&self, reuse: &super::reuse::Reuse) -> (Vec<Part>, Vec<String>) {
        // Each part with the number of the top-level command it belongs
        // to and which list it came from (0: skipped, 1: this frame).
        let own = self.commands.len();
        let number_of = |part: &Part| -> usize {
            part.path
                .strip_prefix("command ")
                .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(own + 1)
        };
        let mut all: Vec<(usize, usize, usize, &Part)> = Vec::new();
        for (i, (n, part)) in reuse.parts.iter().enumerate() {
            all.push((*n, 0, i, part));
        }
        for (i, part) in self.parts.iter().enumerate() {
            all.push((number_of(part), 1, i, part));
        }
        all.sort_by_key(|(n, source, i, _)| (*n, *source, *i));
        let mut new_index: HashMap<(usize, usize), usize> = HashMap::new();
        for (new, (_, source, i, _)) in all.iter().enumerate() {
            new_index.insert((*source, *i), new);
        }
        let parts = all
            .iter()
            .map(|(_, source, _, part)| Part {
                path: part.path.clone(),
                area: part.area,
                inside: part
                    .inside
                    .and_then(|old| new_index.get(&(*source, old)).copied()),
            })
            .collect();
        let mut warnings = reuse.warnings.clone();
        warnings.extend(self.warnings.iter().cloned());
        (parts, warnings)
    }

    /// After a command has started: adds the box around what it queued to
    /// its frame's, and warns if all of it is outside the canvas.
    fn note_drawn(&mut self, document: &Document, index: usize, command: &Command, queued: usize) {
        if self.capturing() {
            return; // a silhouette for a mask isn't drawing
        }
        if self.frame().opacity <= 0.0 {
            return; // shrunk to nothing (scale 0) or fully faded: nothing shows
        }
        let mut drawn = None;
        // The number of the script's own command this is part of.
        let top = self.position().0;
        let on_scratch = self.on_scratch();
        let mut shaped = None;
        for job in self.jobs.iter().skip(queued) {
            let extent = job_extent(job, document);
            drawn = Some(union(drawn, extent));
            if !matches!(job, Job::Area { shape: None, .. } | Job::Bucket { .. }) {
                shaped = Some(union(shaped, extent));
            }
            if !on_scratch {
                self.painted_by.entry(job.layer()).or_default().insert(top);
            }
        }
        let Some(drawn) = drawn else {
            return;
        };
        if let Some(shaped) = shaped {
            let frame = self.frames.last_mut().expect("a command runs in a frame");
            frame.shaped = Some(union(frame.shaped, shaped));
        }
        let warning = (!overlaps(drawn, document.bounds()))
            .then(|| self.outside_warning(&self.path_to(index, &command.label()), drawn));
        let frame = self.frames.last_mut().expect("a command runs in a frame");
        frame.drawn = Some(union(frame.drawn, drawn));
        frame.outside.extend(warning);
    }

    fn outside_warning(&self, path: &str, drawn: Extent) -> String {
        let area = self.canvas_box(drawn);
        format!(
            "{path} draws entirely outside the canvas (x {}..{}, y {}..{})",
            area.left, area.right, area.top, area.bottom
        )
    }

    /// A box in the pixels being painted, in canvas pixels (they differ
    /// when painting smaller, for a preview).
    fn canvas_box(&self, (left, top, right, bottom): Extent) -> PixelRect {
        let scale = self.paint_scale;
        PixelRect::new(
            (left / scale).floor() as i32,
            (top / scale).floor() as i32,
            (right / scale).ceil() as i32,
            (bottom / scale).ceil() as i32,
        )
    }

    // ---- Placing drawing on the canvas ------------------------------------------

    /// Where a point in drawing units lands on the canvas.
    fn place(&self, point: Pt) -> Pt {
        self.frame().place.apply(point)
    }

    fn place_all(&self, points: &mut [StrokePoint]) {
        let place = self.frame().place;
        if place.is_identity() {
            return;
        }
        for point in points {
            (point.x, point.y) = place.apply((point.x, point.y));
        }
    }

    fn place_polygon(&self, points: Vec<Pt>) -> Vec<Pt> {
        let place = self.frame().place;
        if place.is_identity() {
            return points;
        }
        points.into_iter().map(|p| place.apply(p)).collect()
    }

    /// How many canvas pixels one drawing unit is, here.
    fn detail(&self) -> f32 {
        self.frame().detail
    }

    // ---- Doing the work ---------------------------------------------------------

    /// Feeds the running stroke points until the budget is used up.
    /// Returns what's left of the budget.
    fn continue_stroke(&mut self, document: &mut Document, budget: f32) -> f32 {
        let Some(running) = self.running.take() else {
            return budget;
        };
        if running.capture {
            // The stroke is part of a silhouette: it goes on the scratch picture.
            let mut scratch = self
                .capture
                .pop()
                .expect("a stroke is captured on a scratch");
            let left = self.feed_stroke(&mut scratch.document, running, budget);
            self.capture.push(scratch);
            return left;
        }
        self.feed_stroke(document, running, budget)
    }

    fn feed_stroke(
        &mut self,
        document: &mut Document,
        mut running: RunningStroke,
        mut budget: f32,
    ) -> f32 {
        let Some(layer) = document.layer_mut(running.layer) else {
            return budget;
        };
        while running.next < running.points.len() && budget > 0.0 {
            let point = running.points[running.next];
            budget -= match running.next {
                0 => 1.0,
                i => point.distance_to(&running.points[i - 1]).max(1.0),
            };
            running.stroke.add_point(&mut layer.surface, point);
            running.next += 1;
        }
        if running.next < running.points.len() {
            self.running = Some(running);
        } else {
            self.finish_stroke(running, document);
        }
        budget
    }

    fn finish_stroke(&mut self, running: RunningStroke, document: &mut Document) {
        let RunningStroke {
            layer,
            stroke,
            capture,
            ..
        } = running;
        if let Some(target) = document.layer_mut(layer) {
            let before = stroke.finish(&mut target.surface);
            if !capture {
                self.remember_tiles(layer, before);
            }
        }
        if !capture {
            self.mark_painted(document, layer);
        }
    }

    /// Starts or does one job. Returns how much of the budget it used.
    fn start_job(&mut self, document: &mut Document, job: Job) -> f32 {
        if self.options.dry_run {
            return 0.0;
        }
        if self.on_scratch() {
            // A silhouette for a mask, or a filtered group's drawing:
            // painted on the scratch picture (a silhouette all on one
            // layer; a group on a scratch layer per real layer).
            let mut scratch = self.capture.pop().expect("on a scratch");
            let job = match job {
                // The bucket spreads over the real picture, not the scratch.
                Job::Bucket {
                    layer,
                    fill,
                    paint,
                    opacity,
                } => match fill::flood_region(document, layer, &fill) {
                    Some(mask) => Job::Area {
                        layer,
                        shape: Some(Shape::Mask(mask)),
                        paint,
                        opacity,
                        mode: BrushMode::Paint,
                    },
                    None => {
                        self.capture.push(scratch);
                        return AREA_COST;
                    }
                },
                other => other,
            };
            let target = scratch.layer_for(job.layer());
            let job = job.on_layer(target);
            let cost = self.do_job(&mut scratch.document, job, true);
            self.capture.push(scratch);
            return cost;
        }
        self.do_job(document, job, false)
    }

    /// Does `job` on `document`: the picture, or a scratch picture if
    /// `capture` (then nothing is remembered for undo or the report).
    fn do_job(&mut self, document: &mut Document, job: Job, capture: bool) -> f32 {
        let clip = self.clip();
        match job {
            Job::Stroke {
                layer,
                brush,
                points,
                seed,
                symmetry,
            } => {
                let mut stroke = Stroke::new(brush, document.bounds()).seeded(seed);
                if let Some(symmetry) = symmetry {
                    stroke = stroke.mirrored(symmetry);
                }
                if let Some(clip) = clip {
                    stroke = stroke.clipped(clip);
                }
                if let Tip::File(id) = brush.tip
                    && let Some(shape) = self.tips.get(id as usize)
                {
                    stroke = stroke.with_tip(Arc::clone(shape));
                }
                if let Some(id) = brush.source
                    && let Some(paint) = self.sources.get(id as usize)
                {
                    stroke = stroke.with_source(paint);
                }
                self.running = Some(RunningStroke {
                    layer,
                    stroke,
                    points,
                    next: 0,
                    capture,
                });
                0.0
            }
            Job::Area {
                layer,
                shape,
                paint,
                opacity,
                mode,
            } => {
                let limits = document.bounds();
                let Some(target) = document.layer_mut(layer) else {
                    return 0.0;
                };
                let clip = clip.as_deref();
                let before = match shape {
                    Some(Shape::Mask(mask)) => paint_mask_within(
                        &mut target.surface,
                        &mask,
                        &paint,
                        opacity,
                        mode,
                        limits,
                        clip,
                    ),
                    Some(Shape::Rings(rings)) => {
                        let mask = geometry::polygons_mask(&rings, limits);
                        paint_mask_within(
                            &mut target.surface,
                            &mask,
                            &paint,
                            opacity,
                            mode,
                            limits,
                            clip,
                        )
                    }
                    None => {
                        paint_rect_within(&mut target.surface, limits, &paint, opacity, mode, clip)
                    }
                };
                if !capture {
                    self.remember_tiles(layer, before);
                    self.mark_painted(document, layer);
                }
                AREA_COST
            }
            Job::Bucket {
                layer,
                fill,
                paint,
                opacity,
            } => {
                let before = fill::flood_fill_within(
                    document,
                    layer,
                    &fill,
                    &paint,
                    opacity,
                    clip.as_deref(),
                );
                if !capture {
                    self.remember_tiles(layer, before);
                    self.mark_painted(document, layer);
                }
                AREA_COST
            }
        }
    }

    /// Queues a stroke. `seed` is the command's own seed, if it has one:
    /// each stroke of a command gets its own random numbers from it.
    fn push_stroke(
        &mut self,
        layer: LayerId,
        brush: BrushSettings,
        points: Vec<StrokePoint>,
        seed: Option<u64>,
    ) {
        // On a pixel canvas, a line one pixel wide (or less) goes through
        // whole pixels, one per step: the shrink alone could double it up
        // where it runs at a slant (see `pixels.rs`).
        if self.pixel.is_some()
            && brush.size <= self.paint_scale * 1.001
            && brush.source.is_none()
            && !points.is_empty()
        {
            let s = self.paint_scale;
            let rings = pixel_line(&points, s)
                .into_iter()
                .map(|(x, y)| {
                    let (left, top) = (x as f32 * s, y as f32 * s);
                    vec![
                        (left, top),
                        (left + s, top),
                        (left + s, top + s),
                        (left, top + s),
                    ]
                })
                .collect();
            let [r, g, b] = brush.color;
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(Shape::Rings(rings)),
                paint: Paint::Color([r, g, b, 255]),
                opacity: brush.opacity,
                mode: brush.mode,
            });
            return;
        }
        let seed = Random::mix(
            Random::mix(self.seed, seed.unwrap_or(self.current_seed)),
            self.jobs.len() as u64 + 1,
        );
        self.jobs.push_back(Job::Stroke {
            layer,
            brush,
            points,
            seed,
            symmetry: None,
        });
    }

    /// The mirror lines of a `stroke` command, on the canvas: they move
    /// and turn with the group it's in.
    fn symmetry(&self, written: Option<&SymmetryCmd>) -> Result<Option<Symmetry>, String> {
        let Some(written) = written else {
            return Ok(None);
        };
        if written.x.is_none() && written.y.is_none() {
            return Err(
                "`symmetry` needs `x` (a line going down), `y` (going across) or both".into(),
            );
        }
        let x = finite("symmetry x", written.x.unwrap_or(0.0))?;
        let y = finite("symmetry y", written.y.unwrap_or(0.0))?;
        let place = self.frame().place;
        Ok(Some(Symmetry {
            centre: place.apply((x, y)),
            vertical: written.x.is_some(),
            horizontal: written.y.is_some(),
            angle: place.b.atan2(place.a).to_degrees(),
        }))
    }

    fn mark_painted(&mut self, document: &Document, layer: LayerId) {
        if let Some(layer) = document.layer(layer) {
            self.painted = Some(layer.name().to_owned());
        }
        if self.spare_layer == Some(layer) {
            self.spare_layer = None; // it's been painted on: no longer spare
        }
    }

    // ---- Undo ---------------------------------------------------------------------

    fn record(&mut self, step: impl UndoStep + 'static) {
        if self.options.record_undo {
            self.undo.push(Box::new(step));
        }
    }

    /// Keeps the tiles as they were before a job, unless an earlier job
    /// already kept that tile (its older version is the one undo needs).
    fn remember_tiles(&mut self, layer: LayerId, before: TilesBefore) {
        if !self.options.record_undo || before.is_empty() {
            return;
        }
        let index = match self.tiles_before.iter().position(|(id, _)| *id == layer) {
            Some(index) => index,
            None => {
                self.tiles_before.push((layer, TilesBefore::new()));
                self.tiles_before.len() - 1
            }
        };
        let kept = &mut self.tiles_before[index].1;
        for (coord, tile) in before {
            kept.entry(coord).or_insert(tile);
        }
    }

    /// Turns the kept tiles into undo steps. Done before any command that
    /// changes layers, so undo happens in the right order.
    fn flush_tiles(&mut self) {
        for (layer, tiles) in std::mem::take(&mut self.tiles_before) {
            if !tiles.is_empty() {
                self.undo
                    .push(Box::new(TileCommand::new("Painting", layer, tiles)));
            }
        }
    }

    // ---- Commands -------------------------------------------------------------------

    fn start_command(&mut self, document: &mut Document, command: &Command) -> Result<(), String> {
        if self.capturing() && !paints_or_structures(command) {
            // While a symbol is painted for a mask, its layer commands
            // (and the like) are skipped: only its silhouette matters.
            return Ok(());
        }
        if command.shape_name().is_some() {
            return self.named_shape(document, command);
        }
        match command {
            Command::Canvas(_) => Err("only the first command can be `canvas`".into()),
            Command::Paper(paper) => {
                self.flush_tiles();
                let color = self.paper_color(&paper.color)?;
                if let Some(step) = PaperCommand::apply(document, color) {
                    self.record(step);
                }
                Ok(())
            }
            Command::Layer(layer) => self.layer(document, layer),
            Command::DeleteLayer(delete) => {
                self.flush_tiles();
                let id = find_layer(document, &delete.name)?;
                let step = LayerPresenceCommand::delete_layer(document, id).ok_or_else(|| {
                    format!(
                        "can't delete \"{}\": a picture needs at least one layer",
                        delete.name
                    )
                })?;
                self.record(step);
                Ok(())
            }
            Command::MoveLayer(move_layer) => self.move_layer(document, move_layer),
            Command::RenameLayer(rename) => {
                self.flush_tiles();
                let id = find_layer(document, &rename.name)?;
                check_name(&rename.to)?;
                let props = LayerProps {
                    name: rename.to.clone(),
                    ..layer_props(document, id)
                };
                if let Some(step) = LayerPropsCommand::apply(document, id, props) {
                    self.record(step);
                }
                Ok(())
            }
            Command::ClearLayer(clear) => {
                self.flush_tiles();
                let id = match &clear.name {
                    Some(name) => find_layer(document, name)?,
                    None => document.active_id(),
                };
                self.painted_by.remove(&id);
                if !self.options.dry_run {
                    let before = document.clear_layer(id);
                    if !before.is_empty() {
                        self.record(TileCommand::new("Clear layer", id, before));
                    }
                }
                Ok(())
            }
            Command::TransformLayer(command) => {
                self.flush_tiles();
                let id = match &command.name {
                    Some(name) => find_layer(document, name)?,
                    None => document.active_id(),
                };
                // In canvas pixels, whatever group it's in; in a preview,
                // shrunk like everything else.
                let scale = self.paint_scale;
                let mut transform = command.transform()?;
                if scale != 1.0 {
                    transform = Transform::scale(1.0 / scale, 1.0 / scale)
                        .then(transform)
                        .then(Transform::scale(scale, scale));
                }
                if !self.options.dry_run {
                    let before = transform_layer(document, id, transform);
                    if !before.is_empty() {
                        self.record(TileCommand::new("Transform layer", id, before));
                    }
                }
                let top = self.position().0;
                self.painted_by.entry(id).or_default().insert(top);
                Ok(())
            }
            Command::Brush(brush) => self.set_brush(brush),
            Command::Select(select) => self.select(document, select),
            Command::Stroke(stroke) => self.stroke(document, stroke, BrushMode::Paint),
            Command::Erase(stroke) => self.stroke(document, stroke, BrushMode::Erase),
            Command::Line(line) => self.line(document, line),
            Command::Path(path) => self.path(document, path),
            Command::Rect(rect) => self.rect(document, rect),
            Command::Ellipse(ellipse) => self.ellipse(document, ellipse),
            Command::Polygon(polygon) => self.polygon(document, polygon),
            Command::Fill(fill) => self.bucket(document, fill),
            Command::Gradient(gradient) => self.gradient(document, gradient),
            Command::Hatch(hatch) => self.hatch(document, hatch),
            Command::Scatter(scatter) => self.scatter(document, scatter),
            Command::Text(text) => self.text(document, text),
            Command::Pixels(pixels) => self.pixels(document, pixels),
            Command::Snapshot(snapshot) => {
                self.painted = None; // the snapshot already shows that layer
                self.events
                    .push_back(Event::Snapshot(snapshot.name.clone()));
                Ok(())
            }
            Command::Stamp(stamp) => self.stamp(document, stamp),
            Command::Filter(filter) => self.filter(document, filter),
            Command::Group(group) => self.group(document, group),
            Command::Define(define) => {
                // Duplicate names and `define` inside other commands are
                // refused when the script is read.
                if let Some(Symbol {
                    source: Some((file, number)),
                    ..
                }) = self.symbols.get(&define.name)
                {
                    return Err(format!(
                        "a symbol called \"{}\" was already defined by {file} (command {number})",
                        define.name
                    ));
                }
                self.symbols.insert(
                    define.name.clone(),
                    Symbol {
                        commands: Arc::new(define.commands.clone()),
                        raw: define.raw.clone(),
                        params: Arc::new(define.params.clone().unwrap_or_default()),
                        source: None,
                    },
                );
                self.defined.push((define.name.clone(), self.current + 1));
                Ok(())
            }
            Command::Include(include) => self.include(include),
            Command::Use(using) => self.use_symbol(document, using, None),
            Command::Repeat(repeat) => self.repeat(document, repeat),
        }
    }

    fn group(&mut self, document: &Document, command: &GroupCmd) -> Result<(), String> {
        let placement = self.placement_of(
            command.translate.as_ref(),
            command.scale.as_ref(),
            command.rotate.as_ref(),
            command.mirror.as_ref(),
            command.origin.as_ref(),
            command.opacity.as_ref(),
            command.scale_brush,
        )?;
        let placement = Placement {
            stretch_brush: command.stretch_brush.unwrap_or(false),
            ..placement
        };
        let label = match &command.name {
            Some(name) => format!("group \"{name}\""),
            None => "group".to_owned(),
        };
        let new = NewFrame {
            commands: Arc::new(command.commands.clone()),
            label,
            placement,
            seed_base: self.current_seed,
            symbol: None,
            swaps: Vec::new(),
            part: command.name.is_some(),
            params: None,
            upright: command.upright.unwrap_or(false),
            mask: command.mask.clone(),
            capture: None,
            filter: command.filter.clone(),
            outline: command.outline.clone(),
        };
        self.push_frame(document, new)
    }

    /// A single shape with a `name`: played as a named part of its own
    /// (so `check`, `critique` and `views` list it), painting exactly as
    /// it would without the name.
    fn named_shape(&mut self, document: &Document, command: &Command) -> Result<(), String> {
        let new = NewFrame {
            commands: Arc::new(vec![command.without_shape_name()]),
            label: command.label(),
            placement: Placement {
                transform: Transform::IDENTITY,
                opacity: 1.0,
                scale_brush: true,
                stretch_brush: false,
                scale: (1.0, 1.0),
                origin: (0.0, 0.0),
            },
            // Its own seed, kept (see `Frame::keep_seed`).
            seed_base: self.current_seed,
            symbol: None,
            swaps: Vec::new(),
            part: true,
            params: None,
            upright: false,
            mask: None,
            capture: None,
            filter: None,
            outline: None,
        };
        self.push_frame(document, new)?;
        if let Some(frame) = self.frames.last_mut() {
            frame.keep_seed = true;
        }
        Ok(())
    }

    /// `use`: plays a symbol's commands, placed like a group, with some
    /// colours swapped.
    /// With `capture`, the symbol is painted on a scratch picture to become
    /// the mask of the frame being played (`Some(true)`: its opposite).
    fn use_symbol(
        &mut self,
        document: &Document,
        command: &UseCmd,
        capture: Option<bool>,
    ) -> Result<(), String> {
        let name = &self.string(&Value::String(command.name.clone()), "name")?;
        let Some(symbol) = self.symbols.get(name) else {
            let mut names: Vec<&str> = self.symbols.keys().map(String::as_str).collect();
            names.sort_unstable();
            return Err(if names.is_empty() {
                format!(
                    "there's no symbol called \"{name}\": define it first, with \
                     {{\"cmd\": \"define\", \"name\": \"{name}\", \"commands\": [...]}}"
                )
            } else {
                format!(
                    "there's no symbol called \"{name}\" (defined so far: {})",
                    names.join(", ")
                )
            });
        };
        // A symbol can't use itself, not even through other symbols.
        let chain: Vec<&str> = self
            .frames
            .iter()
            .filter_map(|frame| frame.symbol.as_deref())
            .collect();
        if chain.contains(&name.as_str()) {
            return Err(format!(
                "symbol \"{name}\" uses itself ({} > {name})",
                chain.join(" > ")
            ));
        }
        let mut commands = Arc::clone(&symbol.commands);
        let raw = symbol.raw.clone();
        let defaults = Arc::clone(&symbol.params);
        let label = match &symbol.source {
            Some((file, number)) => format!("use \"{name}\" (from {file}, command {number})"),
            None => format!("use \"{name}\""),
        };
        let placement = self.placement_of(
            command.translate.as_ref(),
            command.scale.as_ref(),
            command.rotate.as_ref(),
            command.mirror.as_ref(),
            command.origin.as_ref(),
            command.opacity.as_ref(),
            command.scale_brush,
        )?;
        let placement = Placement {
            stretch_brush: command.stretch_brush.unwrap_or(false),
            ..placement
        };
        let mut swaps = Vec::new();
        for (from, to) in command.colors.iter().flatten() {
            let error = |problem: String| format!("`colors`: {problem}");
            let to = self.string(to, "colors").map_err(error)?;
            swaps.push((
                self.color(from).map_err(error)?,
                self.color(&to).map_err(error)?,
            ));
        }
        // The symbol's parameters: its defaults, then what this use gives
        // (worked out here, so keyframes and `$names` are the use's own).
        let mut params = (*defaults).clone();
        for (key, value) in command.with.iter().flatten() {
            if !defaults.contains_key(key) {
                let mut names: Vec<&str> = defaults.keys().map(String::as_str).collect();
                names.sort_unstable();
                return Err(if names.is_empty() {
                    format!(
                        "`with`: symbol \"{name}\" has no parameters (give it some with \
                         \"params\" in its define)"
                    )
                } else {
                    format!(
                        "`with`: symbol \"{name}\" has no parameter called \"{key}\" (it has: {})",
                        names.join(", ")
                    )
                });
            }
            params.insert(
                key.clone(),
                self.value(value)
                    .map_err(|p| format!("`with`: {key}: {p}"))?,
            );
        }
        // Read with these values, so "$x" works in any field (once per set
        // of values: a repeat or an animation reuses it).
        if let Some(raw) = raw {
            let key = (
                name.clone(),
                serde_json::to_string(&params).unwrap_or_default(),
            );
            commands = match self.symbol_versions.get(&key) {
                Some(version) => Arc::clone(version),
                None => {
                    let version = Arc::new(
                        crate::script::symbol_commands(&raw, &params)
                            .map_err(|p| format!("with these `with` values, {p}"))?,
                    );
                    self.symbol_versions.insert(key, Arc::clone(&version));
                    version
                }
            };
        }
        let new = NewFrame {
            commands,
            label,
            placement,
            // Every use of a symbol has the same random patterns.
            seed_base: name_seed(name),
            symbol: Some(name.clone()),
            swaps,
            part: capture.is_none(),
            params: Some(Arc::new(params)),
            upright: command.upright.unwrap_or(false),
            mask: command.mask.clone(),
            capture,
            filter: command.filter.clone(),
            outline: command.outline.clone(),
        };
        self.used_symbols.insert(name.clone());
        self.push_frame(document, new)
    }

    /// `repeat`: plays its commands `count` times, each copy with the step
    /// applied once more.
    fn repeat(&mut self, document: &Document, command: &RepeatCmd) -> Result<(), String> {
        if !(1..=MAX_REPEAT).contains(&command.count) {
            return Err(format!(
                "`count` must be from 1 to {MAX_REPEAT}, not {}",
                command.count
            ));
        }
        let step = &command.step;
        let step = placement(
            step.translate,
            step.scale.as_ref(),
            step.rotate,
            None,
            step.origin,
            None,
            None,
        )
        .map_err(|problem| format!("`step`: {problem}"))?
        .transform;
        let new = NewFrame {
            commands: Arc::new(command.commands.clone()),
            label: "repeat".to_owned(),
            placement: Placement {
                transform: Transform::IDENTITY,
                opacity: 1.0,
                scale_brush: true,
                stretch_brush: false,
                scale: (1.0, 1.0),
                origin: (0.0, 0.0),
            },
            seed_base: Random::mix(self.current_seed, 1),
            symbol: None,
            swaps: Vec::new(),
            part: false,
            params: None,
            upright: false,
            mask: None,
            capture: None,
            filter: None,
            outline: None,
        };
        let each = command.each.clone().unwrap_or_default();
        for (name, values) in &each {
            if values.is_empty() {
                return Err(format!(
                    "`each`: \"{name}\" needs a list with at least one value"
                ));
            }
        }
        self.push_frame(document, new)?;
        let frame = self.frames.last_mut().expect("just pushed");
        // Its commands are numbered by copy in messages.
        if let Some((_, label)) = &mut frame.label {
            *label = "repeat, copy 1".to_owned();
        }
        if !each.is_empty() {
            frame.params = Some(Arc::new(each_params(&each, 0)));
        }
        frame.copies = Some(Copies {
            count: command.count,
            current: 0,
            step,
            so_far: Transform::IDENTITY,
            base_place: frame.place,
            base_detail: frame.detail,
            base_brush_scale: frame.brush_scale,
            seed: self.current_seed,
            each,
            placements: Vec::new(),
        });
        Ok(())
    }

    /// `stamp`: copies of a symbol, or dabs of a tip, along a smooth path.
    fn stamp(&mut self, document: &Document, command: &StampCmd) -> Result<(), String> {
        // The path, in drawing units.
        let points: Vec<[f32; 2]> = serde_json::from_value(self.value(&command.points)?)
            .map_err(|_| "`points` is a list of [x, y] points, like [[20, 300], [300, 290]]")?;
        if points.is_empty() {
            return Err("`points` needs at least one point".into());
        }
        let given: Vec<StrokePoint> = points
            .iter()
            .map(|&p| pt(p).map(|(x, y)| StrokePoint::new(x, y, 1.0)))
            .collect::<Result<_, _>>()?;
        let path = if given.len() == 1 {
            given.clone()
        } else {
            geometry::smooth_path(&given, false, self.detail())
        };
        let spacing = match self.value(&command.spacing)? {
            Value::Number(n) => n.as_f64().unwrap_or(0.0) as f32,
            other => return Err(format!("`spacing` must be a number, not {other}")),
        };
        if spacing <= 0.0 || !spacing.is_finite() {
            return Err("`spacing` must be more than 0".into());
        }
        let follow = command.follow.unwrap_or(true);
        let scatter = not_negative("scatter", command.scatter.unwrap_or(0.0))?;
        let size_value = command.size.as_ref().map(|v| self.value(v)).transpose()?;
        let angle_value = command.angle.as_ref().map(|v| self.value(v)).transpose()?;
        let angle = range(angle_value.as_ref(), (0.0, 0.0), "angle")?;
        let colors: Vec<Rgba> = match (&command.colors, &command.color) {
            (Some(_), Some(_)) => return Err("give `color` or `colors`, not both".into()),
            (Some(list), None) if list.is_empty() => return Err("`colors` is empty".into()),
            (Some(list), None) => list
                .iter()
                .map(|c| self.color(c))
                .collect::<Result<_, _>>()?,
            (None, Some(color)) => vec![self.color(color)?],
            (None, None) => Vec::new(),
        };

        // Where each copy goes: every `spacing` along the path, with the
        // direction of the path there.
        let total: f32 = path.windows(2).map(|w| w[0].distance_to(&w[1])).sum();
        let count = (total / spacing).floor() as u32 + 1;
        if count > MAX_REPEAT {
            return Err(format!(
                "that would be {count} copies; at most {MAX_REPEAT} (make `spacing` bigger)"
            ));
        }
        let mut random = self.random(self.seed_of(command.seed.as_ref())?);
        let mut spots: Vec<(f32, f32, f32)> = Vec::with_capacity(count as usize);
        let (mut segment, mut along) = (0, 0.0);
        for i in 0..count {
            let wanted = i as f32 * spacing;
            while segment + 2 < path.len() {
                let length = path[segment].distance_to(&path[segment + 1]);
                if along + length >= wanted {
                    break;
                }
                along += length;
                segment += 1;
            }
            let (x, y, heading) = if path.len() == 1 {
                (path[0].x, path[0].y, 0.0)
            } else {
                let (a, b) = (path[segment], path[segment + 1]);
                let length = a.distance_to(&b).max(1e-6);
                let t = ((wanted - along) / length).clamp(0.0, 1.0);
                let heading = (b.y - a.y).atan2(b.x - a.x).to_degrees();
                (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, heading)
            };
            let turn = if follow { heading } else { 0.0 }
                + random.range(angle.0, angle.1.max(angle.0 + f32::EPSILON));
            let (mut x, mut y) = (x, y);
            if scatter > 0.0 {
                let distance = random.range(0.0, scatter);
                let (sin, cos) = random.range(0.0, std::f32::consts::TAU).sin_cos();
                x += cos * distance;
                y += sin * distance;
            }
            spots.push((x, y, turn));
        }

        match (&command.symbol, &command.tip) {
            (Some(_), Some(_)) => Err("give `symbol` or `tip`, not both".into()),
            (Some(symbol), None) => {
                self.stamp_symbol(document, command, symbol, spots, size_value, colors, random)
            }
            (None, tip) => {
                let tip = match tip {
                    Some(tip) => Some(self.tip(tip)?),
                    None => None,
                };
                self.stamp_tip(document, command, tip, spots, size_value, colors, random)
            }
        }
    }

    /// `stamp` with a tip: one dab per spot, with the brush's settings.
    #[allow(clippy::too_many_arguments)]
    fn stamp_tip(
        &mut self,
        document: &Document,
        command: &StampCmd,
        tip: Option<Tip>,
        spots: Vec<(f32, f32, f32)>,
        size: Option<Value>,
        colors: Vec<Rgba>,
        mut random: Random,
    ) -> Result<(), String> {
        let mut base = self.stroke_brush(
            BrushChanges {
                opacity: command.opacity,
                ..BrushChanges::default()
            },
            BrushMode::Paint,
        )?;
        // The stamp varies the dabs itself.
        base.dynamics = Dynamics::default();
        base.pressure_size = true;
        if let Some(tip) = tip {
            base.tip = tip;
        }
        let size = range(size.as_ref(), (self.brush.size, self.brush.size), "size")?;
        brush_size(size.0)?;
        brush_size(size.1)?;
        let brush_scale = self.frame().brush_scale;
        let layer = document.active_id();
        for (x, y, turn) in spots {
            let (dab_size, fade) = self.sized(
                random.range(size.0, size.1.max(size.0 + f32::EPSILON)),
                brush_scale,
            );
            let mut brush = BrushSettings {
                size: dab_size,
                angle: base.angle + turn,
                ..base
            };
            brush.opacity *= fade;
            if !colors.is_empty() {
                let [r, g, b, a] = colors[random.index(colors.len())];
                brush.color = [r, g, b];
                brush.opacity *= f32::from(a) / 255.0;
            }
            let mut points = vec![StrokePoint::new(x, y, 1.0)];
            self.place_all(&mut points);
            self.push_stroke(layer, brush, points, self.seed_of(command.seed.as_ref())?);
        }
        Ok(())
    }

    /// `stamp` with a symbol: a frame like a `repeat`'s, one copy per
    /// spot, each placed (scaled, turned, moved) in its own way, the
    /// colour picked for it as its `color` parameter.
    #[allow(clippy::too_many_arguments)]
    fn stamp_symbol(
        &mut self,
        document: &Document,
        command: &StampCmd,
        symbol: &str,
        spots: Vec<(f32, f32, f32)>,
        size: Option<Value>,
        colors: Vec<Rgba>,
        mut random: Random,
    ) -> Result<(), String> {
        let Some(defined) = self.symbols.get(symbol) else {
            return Err(format!("there's no symbol called \"{symbol}\""));
        };
        let mut with = command.with.clone().unwrap_or_default();
        let mut each = BTreeMap::new();
        if !colors.is_empty() {
            if !defined.params.contains_key("color") {
                return Err(format!(
                    "`colors` go to the symbol's `color` parameter, but \"{symbol}\" has none: \
                     give it \"params\": {{\"color\": \"...\"}} in its define"
                ));
            }
            let picked: Vec<Value> = spots
                .iter()
                .map(|_| Value::String(super::record::hex(colors[random.index(colors.len())])))
                .collect();
            each.insert("color".to_owned(), picked);
            with.insert("color".to_owned(), Value::String("$color".to_owned()));
        }
        let size = range(size.as_ref(), (1.0, 1.0), "size")?;
        if size.0 <= 0.0 || !size.0.is_finite() {
            return Err("`size` must be more than 0".into());
        }
        let placements: Vec<Transform> = spots
            .iter()
            .map(|&(x, y, turn)| {
                let scale = random.range(size.0, size.1.max(size.0 + f32::EPSILON));
                Transform::scale(scale, scale)
                    .then(Transform::rotate(turn))
                    .then(Transform::translate(x, y))
            })
            .collect();
        let using = Command::Use(UseCmd {
            name: symbol.to_owned(),
            with: (!with.is_empty()).then_some(with),
            ..UseCmd::default()
        });
        let new = NewFrame {
            commands: Arc::new(vec![using]),
            label: "stamp".to_owned(),
            placement: Placement {
                transform: Transform::IDENTITY,
                opacity: 1.0,
                scale_brush: true,
                stretch_brush: false,
                scale: (1.0, 1.0),
                origin: (0.0, 0.0),
            },
            seed_base: Random::mix(self.current_seed, 1),
            symbol: None,
            swaps: Vec::new(),
            part: false,
            params: None,
            upright: false,
            mask: None,
            capture: None,
            filter: None,
            outline: None,
        };
        self.push_frame(document, new)?;
        let outer = self.frames.len() - 2;
        let (base_place, base_detail, base_brush_scale) = {
            let parent = &self.frames[outer];
            (parent.place, parent.detail, parent.brush_scale)
        };
        let frame = self.frames.last_mut().expect("just pushed");
        if let Some((_, label)) = &mut frame.label {
            *label = "stamp, copy 1".to_owned();
        }
        if !each.is_empty() {
            frame.params = Some(Arc::new(each_params(&each, 0)));
        }
        let first = placements[0];
        frame.place = first.then(base_place);
        frame.detail = base_detail * first.size_factor().max(1e-3);
        frame.brush_scale = base_brush_scale * first.line_factor();
        frame.copies = Some(Copies {
            count: placements.len() as u32,
            current: 0,
            step: Transform::IDENTITY,
            so_far: first,
            base_place,
            base_detail,
            base_brush_scale,
            seed: self.current_seed,
            each,
            placements,
        });
        Ok(())
    }

    /// `include`: reads a file's `define`s and palette, relative to the
    /// script (an included file may include others, relative to itself).
    fn include(&mut self, command: &IncludeCmd) -> Result<(), String> {
        let base = self.options.folder.clone();
        let mut chain = Vec::new();
        self.include_file(base.as_deref(), &command.file, &command.file, &mut chain)
    }

    /// Reads `file` (relative to `base`) into the symbols and palette.
    /// `written` is how the file is named in messages; `chain` holds the
    /// files being included, outermost first, to catch a loop.
    fn include_file(
        &mut self,
        base: Option<&std::path::Path>,
        file: &str,
        written: &str,
        chain: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let path = match base {
            Some(folder) => folder.join(file),
            None => PathBuf::from(file),
        };
        let path = path.canonicalize().unwrap_or(path);
        if chain.contains(&path) {
            let names: Vec<&str> = chain.iter().filter_map(|p| p.to_str()).collect();
            return Err(format!(
                "{written} includes itself ({} > {written})",
                names.join(" > ")
            ));
        }
        if chain.len() >= 16 {
            return Err("includes are nested too deeply (more than 16)".into());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("can't read \"{written}\" ({}): {e}", path.display()))?;
        let script = Script::parse(&text).map_err(|e| format!("{written}: {e}"))?;
        for (name, color) in &script.palette {
            let value = super::parse_hex(color).map_err(|e| format!("{written}: palette: {e}"))?;
            if let Some(existing) = self.palette.get(name)
                && *existing != value
            {
                let owner = match self.included_colors.get(name) {
                    Some(from) => from.clone(),
                    None => "the script itself".to_owned(),
                };
                return Err(format!(
                    "{written}: palette colour \"{name}\" is also defined, differently, by {owner}"
                ));
            }
            self.palette.insert(name.clone(), value);
            self.included_colors
                .entry(name.clone())
                .or_insert_with(|| written.to_owned());
        }
        chain.push(path.clone());
        let folder = path.parent().map(std::path::Path::to_path_buf);
        for (index, command) in script.commands.iter().enumerate() {
            match command {
                Command::Define(define) => {
                    if let Some(existing) = self.symbols.get(&define.name) {
                        let owner = match &existing.source {
                            Some((from, number)) => format!("{from} (command {number})"),
                            None => "the script itself".to_owned(),
                        };
                        return Err(format!(
                            "{written}: command {} defines \"{}\", which is also defined by {owner}",
                            index + 1,
                            define.name
                        ));
                    }
                    self.symbols.insert(
                        define.name.clone(),
                        Symbol {
                            commands: Arc::new(define.commands.clone()),
                            raw: define.raw.clone(),
                            params: Arc::new(define.params.clone().unwrap_or_default()),
                            source: Some((written.to_owned(), index + 1)),
                        },
                    );
                }
                Command::Include(inner) => {
                    let inner_written = format!("{written} > {}", inner.file);
                    self.include_file(folder.as_deref(), &inner.file, &inner_written, chain)
                        .map_err(|e| e.replace(&inner_written, &inner.file))?;
                }
                Command::Canvas(_) => {} // a library file may start like a script
                other => {
                    return Err(format!(
                        "{written}: command {} ({}): an included file holds only `define` and \
                         `include` (and a palette); its drawing isn't played",
                        index + 1,
                        other.label()
                    ));
                }
            }
        }
        chain.pop();
        Ok(())
    }

    fn layer(&mut self, document: &mut Document, command: &LayerCmd) -> Result<(), String> {
        self.flush_tiles();
        // (Adding a layer selects it, so remember which one was selected.)
        let previous = document.active_id();
        check_name(&command.name)?;
        if command.above.is_some() && command.below.is_some() {
            return Err("give `above` or `below`, not both".into());
        }
        let id = match layer_named(document, &command.name) {
            Some(id) => {
                if let Some(to) = self.target_index(
                    document,
                    id,
                    command.above.as_deref(),
                    command.below.as_deref(),
                )? && let Some(step) = MoveLayerCommand::move_layer(document, id, to)
                {
                    self.record(step);
                }
                id
            }
            None => match self.spare_layer.take().filter(|&id| is_empty(document, id)) {
                // The canvas's first layer is still empty: use it.
                Some(spare) => {
                    let props = LayerProps {
                        name: command.name.clone(),
                        ..layer_props(document, spare)
                    };
                    if let Some(step) = LayerPropsCommand::apply(document, spare, props) {
                        self.record(step);
                    }
                    if let Some(to) = self.target_index(
                        document,
                        spare,
                        command.above.as_deref(),
                        command.below.as_deref(),
                    )? && let Some(step) = MoveLayerCommand::move_layer(document, spare, to)
                    {
                        self.record(step);
                    }
                    spare
                }
                None => {
                    let index = match (&command.above, &command.below) {
                        (Some(above), _) => layer_index(document, above)? + 1,
                        (_, Some(below)) => layer_index(document, below)?,
                        _ => document.layers().len(),
                    };
                    let step = LayerPresenceCommand::insert_layer(document, &command.name, index);
                    let id = document.active_id();
                    self.record(step);
                    id
                }
            },
        };
        self.spare_layer = None;

        if previous != id
            && let Some(name) = self.painted.take()
        {
            self.events.push_back(Event::LayerDone(name));
        }
        document.set_active(id);

        let mut props = layer_props(document, id);
        if let Some(opacity) = command.opacity {
            props.opacity = unit("opacity", opacity)?;
        }
        if let Some(blend) = &command.blend {
            props.blend = BlendMode::from_name(blend).ok_or_else(|| {
                let names: Vec<&str> = BlendMode::ALL.iter().map(|m| m.name()).collect();
                format!(
                    "unknown blend mode \"{blend}\"; use one of: {}",
                    names.join(", ")
                )
            })?;
        }
        if let Some(visible) = command.visible {
            props.visible = visible;
        }
        if let Some(clip) = command.clip {
            props.clip = clip;
        }
        if let Some(lock) = command.lock {
            props.lock = lock;
        }
        if let Some(step) = LayerPropsCommand::apply(document, id, props) {
            self.record(step);
        }
        if let Some(from) = &command.from {
            if from != "below" {
                return Err(format!(
                    "`from` is \"below\" (everything below the layer, flattened), not \"{from}\""
                ));
            }
            if !self.options.dry_run {
                self.fill_from_below(document, id);
            }
        }
        Ok(())
    }

    /// Covers `layer` with everything below it, flattened (hidden layers
    /// left out; no paper), so a filter on it works on the whole picture.
    fn fill_from_below(&mut self, document: &mut Document, id: LayerId) {
        let Some(index) = document.layer_index(id) else {
            return;
        };
        for below in document.layers()[..index]
            .iter()
            .map(|l| l.id())
            .collect::<Vec<_>>()
        {
            self.note_read(below);
        }
        let mut coords: Vec<TileCoord> = document.layers()[..index]
            .iter()
            .filter(|layer| layer.visible())
            .flat_map(|layer| layer.surface.tiles().map(|(coord, _)| coord))
            .collect();
        coords.sort_unstable_by_key(|c| (c.y, c.x));
        coords.dedup();
        let mut tiles = Vec::with_capacity(coords.len());
        for coord in coords {
            let below = |layer: &Layer| {
                layer.visible() && document.layer_index(layer.id()).is_some_and(|i| i < index)
            };
            if let Some(tile) =
                document.composite_tile_showing(coord, Background::Transparent, below)
            {
                tiles.push((coord, tile));
            }
        }
        let Some(layer) = document.layer_mut(id) else {
            return;
        };
        let mut before = TilesBefore::new();
        for (coord, tile) in tiles {
            let old = layer.surface.replace_tile(coord, Some(tile));
            before.insert(coord, old);
        }
        self.remember_tiles(id, before);
        self.mark_painted(document, id);
        let top = self.position().0;
        self.painted_by.entry(id).or_default().insert(top);
    }

    /// Where painting may land now: inside the group's mask and the
    /// `select` selection, whichever there are.
    fn clip(&mut self) -> Option<Arc<Mask>> {
        let group = self.frame().mask.clone();
        let selected = self.selection.as_ref().map(|s| Arc::clone(s.mask()));
        match (group, selected) {
            (None, selected) => selected,
            (group, None) => group,
            (Some(group), Some(selected)) => {
                let key = (
                    Arc::as_ptr(&group) as usize,
                    Arc::as_ptr(&selected) as usize,
                );
                if let Some((a, b, both)) = &self.clip_cache
                    && (*a, *b) == key
                {
                    return Some(Arc::clone(both));
                }
                let both = Arc::new(group.intersect(&selected));
                self.clip_cache = Some((key.0, key.1, Arc::clone(&both)));
                Some(both)
            }
        }
    }

    /// `select`: sets, adds to, takes from, inverts, feathers or drops
    /// the selection that painting stays inside. The checks and the
    /// combining are the app's (`selection::Request`, `selection::apply`);
    /// only the shape is made here, because a script's shape can use
    /// parameters and is placed by the groups around it.
    fn select(&mut self, document: &Document, command: &SelectCmd) -> Result<(), String> {
        let limits = document.bounds();
        let mut request = selection::Request::read(command)?;
        request.feather *= self.detail();
        let mask = match &request.what {
            What::Shape(kind, numbers) => {
                let mut area = serde_json::Map::new();
                area.insert((*kind).to_owned(), self.value(numbers)?);
                let polygon = self.area(&Value::Object(area))?;
                let placed = self.place_polygon(polygon);
                let mut mask = geometry::polygons_mask(&[placed], limits);
                if self.pixel.is_some() {
                    mask.threshold(); // a pixel canvas has hard edges
                }
                Some(mask)
            }
            What::Layer(name) => {
                let id = find_layer(document, name)?;
                self.note_read(id);
                let layer = document.layer(id).ok_or("no such layer")?;
                Some(selection::layer_contents(&layer.surface, limits))
            }
            What::All => Some(selection::all(limits)),
            What::Feather if self.selection.is_none() => {
                return Err("`feather` alone softens the selection, but there is none yet".into());
            }
            What::None | What::Invert | What::Feather => None,
        };
        let new = selection::apply(self.selection.as_ref(), &request, mask, limits);
        // As in the app: a selection that comes out empty (everything
        // subtracted, a shape off the canvas) is no selection.
        if new.is_none() && request.what != What::None && !self.options.dry_run {
            self.warnings.push(format!(
                "command {}: the selection came out empty, so it's dropped and painting is no \
                 longer limited",
                self.position().0
            ));
        }
        self.selection = new;
        Ok(())
    }

    /// `filter`: changes a layer in place.
    fn filter(&mut self, document: &mut Document, command: &FilterCmd) -> Result<(), String> {
        let filters = self.read_filters(&command.fields())?;
        if filters.is_empty() {
            return Err(
                "give at least one of `blur`, `glow`, `shadow`, `adjust`, `gradient_map`".into(),
            );
        }
        let id = match &command.layer {
            Some(name) => find_layer(document, name)?,
            None => document.active_id(),
        };
        if self.options.dry_run {
            return Ok(());
        }
        self.flush_tiles();
        let limits = document.bounds();
        // Inside the `select` selection, if there is one. (Not a group's
        // mask: filters never were, and old scripts must paint the same.)
        let clip = self.selection.as_ref().map(|s| Arc::clone(s.mask()));
        let Some(layer) = document.layer_mut(id) else {
            return Ok(());
        };
        let before = filters::apply_within(&mut layer.surface, &filters, limits, clip.as_deref());
        if !before.is_empty() {
            self.record(TileCommand::new("Filter", id, before));
            let top = self.position().0;
            self.painted_by.entry(id).or_default().insert(top);
        }
        Ok(())
    }

    /// An `outline` (and its `rim`) as filters, sizes in this frame's pixels.
    fn read_outline(&self, spec: OutlineSpec) -> Result<Vec<Filter>, String> {
        let scale = self.detail();
        let hard = self.pixel.is_some();
        let mut filters = vec![Filter::Outline {
            size: not_negative("size", spec.size.unwrap_or(2.0))? * scale,
            color: match &spec.color {
                Some(color) => self.color(color)?,
                None => [0, 0, 0, 255],
            },
            hard,
        }];
        if let Some(rim) = spec.rim {
            filters.push(Filter::Rim {
                size: not_negative("rim: size", rim.size.unwrap_or(1.5))? * scale,
                color: match &rim.color {
                    Some(color) => self.color(color).map_err(|p| format!("`rim`: {p}"))?,
                    None => [255, 255, 255, 255],
                },
                angle: finite("rim: angle", rim.angle.unwrap_or(-45.0))?,
                hard,
            });
        }
        Ok(filters)
    }

    /// Filters as written (`blur`, `glow`, `shadow`, `adjust`,
    /// `gradient_map`), with keyframes worked out and sizes in this
    /// frame's pixels, in the order they're applied.
    fn read_filters(&self, written: &Value) -> Result<Vec<Filter>, String> {
        let value = self.value(written)?;
        let spec: FilterFields = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let scale = self.detail();
        let mut filters = Vec::new();
        if let Some(adjust) = spec.adjust {
            let signed = |what: &str, v: Option<f32>| -> Result<f32, String> {
                match v {
                    None => Ok(0.0),
                    Some(v) if (-1.0..=1.0).contains(&v) => Ok(v),
                    Some(v) => Err(format!("`adjust`: `{what}` is from -1 to 1, not {v}")),
                }
            };
            filters.push(Filter::Adjust {
                hue: finite("hue", adjust.hue.unwrap_or(0.0))?,
                saturation: signed("saturation", adjust.saturation)?,
                brightness: signed("brightness", adjust.brightness)?,
                contrast: signed("contrast", adjust.contrast)?,
            });
        }
        if let Some(map) = spec.gradient_map {
            let stops = self
                .stops_of(map.colors.as_deref(), map.stops.as_deref())
                .map_err(|p| format!("`gradient_map`: {p}"))?;
            filters.push(Filter::GradientMap(stops));
        }
        if let Some(blur) = spec.blur {
            let radius = not_negative("blur", blur)?;
            if radius > filters::MAX_BLUR {
                return Err(format!("`blur` is at most {} pixels", filters::MAX_BLUR));
            }
            if radius > 0.0 {
                filters.push(Filter::Blur(radius * scale));
            }
        }
        if let Some(glow) = spec.glow {
            filters.push(Filter::Glow {
                radius: not_negative("glow: radius", glow.radius.unwrap_or(16.0))? * scale,
                threshold: unit("glow: threshold", glow.threshold.unwrap_or(0.7))?,
                strength: not_negative("glow: strength", glow.strength.unwrap_or(1.0))?,
            });
        }
        if let Some(shadow) = spec.shadow {
            let offset = pt(shadow.offset.unwrap_or([4.0, 4.0]))?;
            let color = match &shadow.color {
                Some(color) => self.color(color).map_err(|p| format!("`shadow`: {p}"))?,
                None => [0, 0, 0, 128],
            };
            filters.push(Filter::Shadow {
                offset: (offset.0 * scale, offset.1 * scale),
                blur: not_negative("shadow: blur", shadow.blur.unwrap_or(4.0))? * scale,
                color,
            });
        }
        if let Some(outline) = spec.outline {
            filters.extend(self.read_outline(outline)?);
        }
        if self.pixel.is_some()
            && let Some(blurry) = filters.iter().find(|f| f.blurs())
        {
            let name = match blurry {
                Filter::Blur(_) => "blur",
                Filter::Glow { .. } => "glow",
                _ => "a shadow with blur",
            };
            return Err(format!(
                "a pixel canvas can't take {name}: its pixels would melt (use `shadow` with \
                 \"blur\": 0, `adjust` or `gradient_map`)"
            ));
        }
        Ok(filters)
    }

    /// Where a layer should go for `above` / `below` another layer, as a
    /// position in the stack once it has been taken out of its own place.
    fn target_index(
        &self,
        document: &Document,
        id: LayerId,
        above: Option<&str>,
        below: Option<&str>,
    ) -> Result<Option<usize>, String> {
        let from = document.layer_index(id).unwrap_or(0);
        let other = match (above, below) {
            (Some(name), _) | (_, Some(name)) => layer_index(document, name)?,
            _ => return Ok(None),
        };
        if other == from {
            return Err("a layer can't go above or below itself".into());
        }
        // Taking the layer out shifts the ones above it down by one.
        let other_after = if other > from { other - 1 } else { other };
        Ok(Some(if above.is_some() {
            other_after + 1
        } else {
            other_after
        }))
    }

    fn move_layer(
        &mut self,
        document: &mut Document,
        command: &MoveLayerCmd,
    ) -> Result<(), String> {
        self.flush_tiles();
        let id = find_layer(document, &command.name)?;
        let given = [
            command.above.is_some(),
            command.below.is_some(),
            command.index.is_some(),
        ];
        if given.iter().filter(|&&g| g).count() != 1 {
            return Err("give one of `above`, `below` or `index`".into());
        }
        let to = match command.index {
            Some(index) => index.min(document.layers().len() - 1),
            None => self
                .target_index(
                    document,
                    id,
                    command.above.as_deref(),
                    command.below.as_deref(),
                )?
                .unwrap_or(0),
        };
        if let Some(step) = MoveLayerCommand::move_layer(document, id, to) {
            self.record(step);
        }
        Ok(())
    }

    fn set_brush(&mut self, command: &BrushCmd) -> Result<(), String> {
        if command.reset.unwrap_or(false) {
            self.brush = BrushSettings::default();
            self.brush_alpha = 255;
        }
        let mut brush = self.brush;
        // A preset first (keeping the colour), then the fields given here.
        if let Some(name) = &command.preset {
            let preset = crate::presets::find(name).ok_or_else(|| {
                let names: Vec<&str> = crate::presets::built_in()
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect();
                format!(
                    "unknown preset \"{name}\"; the presets are: {}",
                    names.join(", ")
                )
            })?;
            let (color, source) = (brush.color, brush.source);
            brush = preset.settings()?;
            brush.mode = BrushMode::Paint; // `stroke` or `erase` says which
            brush.color = color;
            brush.source = source;
        }
        match &command.color {
            Some(Value::String(color)) => {
                let [r, g, b, a] = self.color(color)?;
                brush.color = [r, g, b];
                brush.source = None;
                self.brush_alpha = a;
            }
            Some(pattern) => {
                // A pattern: the strokes get its colour pixel by pixel.
                let paint = self.paint(pattern).map_err(|p| format!("`color`: {p}"))?;
                brush.source = Some(self.sources.len() as u32);
                self.sources.push(Arc::new(paint));
                self.brush_alpha = 255;
            }
            None => {}
        }
        apply_brush_fields(&mut brush, command)?;
        if let Some(tip) = &command.tip {
            brush.tip = self.tip(tip)?;
        }
        self.brush = brush;
        Ok(())
    }

    /// A tip as written: a built-in name, or `{"file": "..."}` (loaded
    /// once, relative to the script).
    fn tip(&mut self, written: &Value) -> Result<Tip, String> {
        let names: Vec<&str> = Tip::BUILT_IN.iter().filter_map(|t| t.name()).collect();
        let help = format!(
            "`tip` is one of {}, or {{\"file\": \"tips/splat.png\"}} (a greyscale picture)",
            names.join(", ")
        );
        match written {
            Value::String(name) => {
                Tip::from_name(name).ok_or_else(|| format!("unknown tip \"{name}\"; {help}"))
            }
            Value::Object(object) if object.len() == 1 => {
                let Some(Value::String(file)) = object.get("file") else {
                    return Err(help);
                };
                if let Some(&id) = self.tip_files.get(file) {
                    return Ok(Tip::File(id));
                }
                let path = match &self.options.folder {
                    Some(folder) => folder.join(file),
                    None => PathBuf::from(file),
                };
                let shape = load_tip(&path)
                    .map_err(|problem| format!("can't load the tip \"{file}\": {problem}"))?;
                let id = self.tips.len() as u32;
                self.tips.push(Arc::new(shape));
                self.tip_files.insert(file.clone(), id);
                Ok(Tip::File(id))
            }
            _ => Err(help),
        }
    }

    /// The brush for one stroke: the current brush with this command's
    /// changes, sized and faded for the current frame.
    fn stroke_brush(
        &self,
        changes: BrushChanges,
        mode: BrushMode,
    ) -> Result<BrushSettings, String> {
        let mut brush = self.brush;
        let mut alpha = self.brush_alpha;
        if let Some(color) = changes.color {
            let [r, g, b, a] = self.color(color)?;
            brush.color = [r, g, b];
            brush.source = None;
            alpha = a;
        }
        if let Some(size) = changes.size {
            brush.size = brush_size(size)?;
        }
        if let Some(opacity) = changes.opacity {
            brush.opacity = unit("opacity", opacity)?;
        }
        if let Some(hardness) = changes.hardness {
            brush.hardness = unit("hardness", hardness)?;
        }
        if let Some(flow) = changes.flow {
            brush.flow = unit("flow", flow)?.max(0.01);
        }
        let frame = self.frame();
        let (size, fade) = self.sized(brush.size, frame.brush_scale);
        brush.opacity *= f32::from(alpha) / 255.0;
        brush.opacity *= frame.opacity * fade;
        brush.size = size;
        brush.dynamics.scatter *= frame.brush_scale;
        brush.mode = mode;
        Ok(brush)
    }

    /// A brush size made `brush_scale` times bigger, and how much fainter
    /// to make it (1 = not at all).
    ///
    /// Dabs are never smaller than one pixel. In a preview (painting
    /// smaller than full size), a line that ends up thinner than a pixel
    /// would therefore look as dark as a one-pixel line, much bolder than
    /// the full picture shrunk down. So it's drawn one pixel wide and
    /// fainter, as much as it's thinner: it fades like in a shrunk
    /// picture. At full size nothing changes.
    fn sized(&self, size: f32, brush_scale: f32) -> (f32, f32) {
        let scaled = size * brush_scale;
        let preview = self.options.scale;
        if preview < 1.0 {
            // As wide as it is at full size (at least a pixel), shrunk.
            let wide = (scaled / preview).max(1.0) * preview;
            if wide < 1.0 {
                return (1.0, wide);
            }
        }
        (scaled.max(0.25), 1.0)
    }

    fn stroke(
        &mut self,
        document: &Document,
        command: &StrokeCmd,
        mode: BrushMode,
    ) -> Result<(), String> {
        if command.points.is_empty() {
            return Err("`points` needs at least one point".into());
        }
        let brush = self.stroke_brush(
            BrushChanges {
                color: command.color.as_ref(),
                size: command.size,
                opacity: command.opacity,
                hardness: command.hardness,
                flow: command.flow,
            },
            mode,
        )?;
        let mut points = self.points(&command.points)?;
        self.jitter(
            &mut points,
            command.jitter,
            self.seed_of(command.seed.as_ref())?,
            false,
        )?;
        self.place_all(&mut points);
        let symmetry = self.symmetry(command.symmetry.as_ref())?;
        self.push_stroke(
            document.active_id(),
            brush,
            points,
            self.seed_of(command.seed.as_ref())?,
        );
        if let Some(Job::Stroke {
            symmetry: mirror, ..
        }) = self.jobs.back_mut()
        {
            *mirror = symmetry;
        }
        Ok(())
    }

    fn line(&mut self, document: &Document, command: &LineCmd) -> Result<(), String> {
        let mut brush = self.stroke_brush(
            BrushChanges {
                color: command.color.as_ref(),
                size: command.size,
                opacity: command.opacity,
                hardness: command.hardness,
                flow: command.flow,
            },
            BrushMode::Paint,
        )?;
        let ends = [pt(command.from)?, pt(command.to)?];
        let mut points = to_stroke_points(&geometry::densify(&ends, false, self.detail()));
        let (start, end) = taper(command.taper.as_ref(), 0.25)?;
        if start > 0.0 || end > 0.0 {
            brush.pressure_size = true; // so the taper shows
            geometry::apply_taper(&mut points, start, end);
        }
        self.jitter(
            &mut points,
            command.jitter,
            self.seed_of(command.seed.as_ref())?,
            false,
        )?;
        self.place_all(&mut points);
        self.push_stroke(
            document.active_id(),
            brush,
            points,
            self.seed_of(command.seed.as_ref())?,
        );
        Ok(())
    }

    fn path(&mut self, document: &Document, command: &PathCmd) -> Result<(), String> {
        let closed = command.closed.unwrap_or(false);
        let needed = if closed { 3 } else { 2 };
        if command.points.len() < needed {
            return Err(format!("`points` needs at least {needed} points"));
        }
        let mut brush = self.stroke_brush(
            BrushChanges {
                color: command.color.as_ref(),
                size: command.size,
                opacity: command.opacity,
                hardness: command.hardness,
                flow: command.flow,
            },
            BrushMode::Paint,
        )?;
        let given = self.points(&command.points)?;
        let mut points = geometry::smooth_path(&given, closed, self.detail());
        let (start, end) = taper(command.taper.as_ref(), 0.0)?;
        if start > 0.0 || end > 0.0 {
            brush.pressure_size = true;
            geometry::apply_taper(&mut points, start, end);
        }
        self.jitter(
            &mut points,
            command.jitter,
            self.seed_of(command.seed.as_ref())?,
            closed,
        )?;
        self.place_all(&mut points);

        let layer = document.active_id();
        if let Some(fill) = &command.fill {
            if !closed {
                return Err("only a `closed` path can be filled: add \"closed\": true".into());
            }
            let paint = self.paint(fill)?;
            let opacity = unit("opacity", command.opacity.unwrap_or(1.0))? * self.frame().opacity;
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(Shape::Rings(vec![
                    points.iter().map(|p| (p.x, p.y)).collect(),
                ])),
                paint,
                opacity,
                mode: BrushMode::Paint,
            });
        }
        if command.line.unwrap_or(true) {
            self.push_stroke(layer, brush, points, self.seed_of(command.seed.as_ref())?);
        } else if command.fill.is_none() {
            return Err("with \"line\": false there's nothing to draw: add a `fill`".into());
        }
        Ok(())
    }

    fn rect(&mut self, document: &Document, command: &RectCmd) -> Result<(), String> {
        if !(command.width > 0.0 && command.height > 0.0) {
            return Err("`width` and `height` must be more than 0".into());
        }
        let radius = not_negative("radius", command.radius.unwrap_or(0.0))?;
        let outline = geometry::rect_outline(
            command.x,
            command.y,
            command.width,
            command.height,
            radius,
            command.rotation.unwrap_or(0.0),
            self.detail(),
        );
        let style = ShapeStyle {
            fill: command.fill.as_ref(),
            outline: command.outline.as_ref(),
            outline_size: command.outline_size,
            opacity: command.opacity,
            erase: command.erase,
            jitter: command.jitter,
            seed: self.seed_of(command.seed.as_ref())?,
        };
        self.shape(document, outline, style)
    }

    fn ellipse(&mut self, document: &Document, command: &EllipseCmd) -> Result<(), String> {
        let (rx, ry) = match &command.radius {
            Value::Number(n) => {
                let r = n.as_f64().unwrap_or(0.0) as f32;
                (r, r)
            }
            Value::Array(pair) if pair.len() == 2 && pair.iter().all(Value::is_number) => (
                pair[0].as_f64().unwrap_or(0.0) as f32,
                pair[1].as_f64().unwrap_or(0.0) as f32,
            ),
            _ => return Err("`radius` is a number (a circle) or [rx, ry]".into()),
        };
        if !(rx > 0.0 && ry > 0.0) {
            return Err("the radius must be more than 0".into());
        }
        let centre = pt(command.center)?;
        let outline = geometry::ellipse_outline(
            centre,
            rx,
            ry,
            command.rotation.unwrap_or(0.0),
            self.detail(),
        );
        let style = ShapeStyle {
            fill: command.fill.as_ref(),
            outline: command.outline.as_ref(),
            outline_size: command.outline_size,
            opacity: command.opacity,
            erase: command.erase,
            jitter: command.jitter,
            seed: self.seed_of(command.seed.as_ref())?,
        };
        self.shape(document, outline, style)
    }

    fn polygon(&mut self, document: &Document, command: &PolygonCmd) -> Result<(), String> {
        if command.points.len() < 3 {
            return Err("`points` needs at least 3 points".into());
        }
        let outline = command
            .points
            .iter()
            .map(|&p| pt(p))
            .collect::<Result<Vec<_>, _>>()?;
        let style = ShapeStyle {
            fill: command.fill.as_ref(),
            outline: command.outline.as_ref(),
            outline_size: command.outline_size,
            opacity: command.opacity,
            erase: command.erase,
            jitter: command.jitter,
            seed: self.seed_of(command.seed.as_ref())?,
        };
        self.shape(document, outline, style)
    }

    /// The part all closed shapes share: optional wobble, then the fill,
    /// then the outline on top. `outline` is in drawing units.
    fn shape(
        &mut self,
        document: &Document,
        outline: Vec<Pt>,
        style: ShapeStyle,
    ) -> Result<(), String> {
        if style.fill.is_none() && style.outline.is_none() {
            return Err(
                "give a `fill` (a colour or gradient) and/or an `outline` (a colour)".into(),
            );
        }
        let opacity = unit("opacity", style.opacity.unwrap_or(1.0))?;
        let mode = if style.erase.unwrap_or(false) {
            BrushMode::Erase
        } else {
            BrushMode::Paint
        };
        let mut points = to_stroke_points(&geometry::densify(&outline, true, self.detail()));
        self.jitter(&mut points, style.jitter, style.seed, true)?;
        self.place_all(&mut points);
        let layer = document.active_id();

        if let Some(fill) = style.fill {
            let paint = self.paint(fill)?;
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(Shape::Rings(vec![
                    points.iter().map(|p| (p.x, p.y)).collect(),
                ])),
                paint,
                opacity: opacity * self.frame().opacity,
                mode,
            });
        }
        if let Some(color) = style.outline {
            let mut brush = self.stroke_brush(
                BrushChanges {
                    color: Some(color),
                    size: style.outline_size,
                    ..BrushChanges::default()
                },
                mode,
            )?;
            brush.opacity *= opacity;
            // Back to the start, to close the outline.
            if let Some(&first) = points.first() {
                points.push(first);
            }
            self.push_stroke(layer, brush, points, style.seed);
        }
        Ok(())
    }

    /// Letters: laid out in drawing units, turned around `at`, then placed
    /// like everything else. The outline goes first, so the letters cover
    /// its inner half and only the border outside them shows.
    fn text(&mut self, document: &Document, command: &TextCmd) -> Result<(), String> {
        if command.text.trim().is_empty() {
            return Err("`text` is empty: give the words to write".into());
        }
        let size = command.size.unwrap_or(32.0);
        if !(size > 0.0 && size <= MAX_TEXT_SIZE) {
            return Err(format!(
                "`size` must be more than 0 and at most {MAX_TEXT_SIZE}, not {size}"
            ));
        }
        let align = match command.align.as_deref() {
            None | Some("left") => Align::Left,
            Some("center") => Align::Center,
            Some("right") => Align::Right,
            Some(other) => {
                return Err(format!(
                    "`align` is \"left\", \"center\" or \"right\", not \"{other}\""
                ));
            }
        };
        let rotate = command.rotate.unwrap_or(0.0);
        if !rotate.is_finite() {
            return Err("`rotate` must be a number of degrees".into());
        }
        let opacity = unit("opacity", command.opacity.unwrap_or(1.0))?;
        let color = match &command.color {
            Some(color) => self.color(color)?,
            None => {
                let [r, g, b] = self.brush.color;
                [r, g, b, self.brush_alpha]
            }
        };
        let (x, y) = pt(command.at)?;
        let words = self.with_copy_number(&command.text)?;

        let mut shape = text::layout(&words, size, align, self.detail());
        if let Some(fit) = command.fit {
            if fit <= 0.0 || fit.is_nan() {
                return Err("`fit` is a width in pixels, more than 0".into());
            }
            if shape.width > fit {
                // Smaller, so the widest line fits (never bigger).
                let smaller = size * fit / shape.width;
                shape = text::layout(&words, smaller, align, self.detail());
            }
        }
        let place = Transform::rotate(rotate)
            .then(Transform::translate(x, y))
            .then(self.frame().place);
        let rings: Vec<Vec<Pt>> = shape
            .contours
            .into_iter()
            .map(|ring| ring.into_iter().map(|p| place.apply(p)).collect())
            .collect();
        let layer = document.active_id();

        if let Some(outline) = &command.outline {
            let outline_size = command.outline_size.unwrap_or(2.0);
            if !(outline_size > 0.0 && outline_size * 2.0 <= MAX_SIZE) {
                return Err(format!(
                    "`outline_size` must be more than 0 and at most {}, not {outline_size}",
                    MAX_SIZE / 2.0
                ));
            }
            // A crisp, even border, whatever the brush is set to.
            let mut brush = self.stroke_brush(
                BrushChanges {
                    color: Some(outline),
                    size: Some(outline_size * 2.0),
                    opacity: Some(opacity),
                    hardness: Some(1.0),
                    flow: Some(1.0),
                },
                BrushMode::Paint,
            )?;
            brush.spacing = 0.1;
            for ring in &rings {
                // Points close together all the way round, or the stroke
                // would round off the long straight edges.
                let mut points = to_stroke_points(&geometry::densify(ring, true, 1.0));
                points.push(points[0]); // back to the start, to close it
                self.push_stroke(layer, brush, points, None);
            }
        } else if command.outline_size.is_some() {
            return Err("`outline_size` needs an `outline` colour too".into());
        }
        if !rings.is_empty() {
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(Shape::Rings(rings)),
                paint: Paint::Color(color),
                opacity: opacity * self.frame().opacity,
                mode: BrushMode::Paint,
            });
        }
        Ok(())
    }

    /// `pixels`: rows of characters, each one canvas pixel in a palette
    /// colour (runs of one colour become one rectangle each).
    fn pixels(&mut self, document: &Document, command: &PixelsCmd) -> Result<(), String> {
        let (left, top) = pt(command.at)?;
        if command.rows.is_empty() {
            return Err("`rows` needs at least one row of characters".into());
        }
        let mut colors: HashMap<char, Rgba> = HashMap::new();
        for (key, color) in &command.colors {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c != '.' => {
                    colors.insert(c, self.color(color)?);
                }
                _ => {
                    return Err(format!(
                        "`colors` keys are single characters (not \".\"), like \"y\": not \"{key}\""
                    ));
                }
            }
        }
        let layer = document.active_id();
        let opacity = self.frame().opacity;
        for (row, text) in command.rows.iter().enumerate() {
            let chars: Vec<char> = text.chars().collect();
            let mut column = 0;
            while column < chars.len() {
                let c = chars[column];
                if c == '.' || c == ' ' {
                    column += 1;
                    continue;
                }
                let Some(&color) = colors.get(&c) else {
                    return Err(format!(
                        "row {} has \"{c}\", which isn't in `colors` (\".\" is see-through)",
                        row + 1
                    ));
                };
                let start = column;
                while column < chars.len() && chars[column] == c {
                    column += 1;
                }
                let (x0, y0) = (left + start as f32, top + row as f32);
                let (x1, y1) = (left + column as f32, top + row as f32 + 1.0);
                let ring = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
                self.jobs.push_back(Job::Area {
                    layer,
                    shape: Some(Shape::Rings(vec![self.place_polygon(ring)])),
                    paint: Paint::Color(color),
                    opacity,
                    mode: BrushMode::Paint,
                });
            }
        }
        Ok(())
    }

    /// `text` with "{i}" replaced by the copy number of the `repeat` it's
    /// in (1, 2, 3...) and "{i0}" by the same counted from 0 (0, 1, 2...).
    /// The innermost `repeat` counts, even from inside a group or symbol.
    fn with_copy_number(&self, text: &str) -> Result<String, String> {
        if !text.contains("{i}") && !text.contains("{i0}") {
            return Ok(text.to_owned());
        }
        let Some(copy) = self
            .frames
            .iter()
            .rev()
            .find_map(|frame| frame.copies.as_ref())
            .map(|copies| copies.current)
        else {
            return Err(
                "\"{i}\" and \"{i0}\" are copy numbers, so they only work inside a `repeat`".into(),
            );
        };
        Ok(text
            .replace("{i0}", &copy.to_string())
            .replace("{i}", &(copy + 1).to_string()))
    }

    fn bucket(&mut self, document: &Document, command: &FillCmd) -> Result<(), String> {
        // The bucket spreads over what it sees: every layer, read.
        for id in document.layers().iter().map(|l| l.id()).collect::<Vec<_>>() {
            self.note_read(id);
        }
        let (x, y) = self.place(pt(command.at)?);
        let paint = match &command.color {
            Some(color) => Paint::Color(self.color(color)?),
            None => {
                let [r, g, b] = self.brush.color;
                Paint::Color([r, g, b, self.brush_alpha])
            }
        };
        let sample = match command.sample.as_deref() {
            None | Some("visible") => SampleFrom::Visible,
            Some("layer") => SampleFrom::Layer,
            Some(other) => {
                return Err(format!(
                    "`sample` is \"visible\" or \"layer\", not \"{other}\""
                ));
            }
        };
        let grow = command.grow.unwrap_or(1);
        if grow > 50 {
            return Err("`grow` can be at most 50 pixels".into());
        }
        self.jobs.push_back(Job::Bucket {
            layer: document.active_id(),
            fill: FloodFill {
                x,
                y,
                tolerance: unit("tolerance", command.tolerance.unwrap_or(0.1))?,
                sample,
                expand: (grow as f32 * self.detail()).round() as u32,
            },
            paint,
            opacity: unit("opacity", command.opacity.unwrap_or(1.0))? * self.frame().opacity,
        });
        Ok(())
    }

    fn gradient(&mut self, document: &Document, command: &GradientCmd) -> Result<(), String> {
        let own = command.kind.is_some() || command.colors.is_some() || command.stops.is_some();
        let paint = match (&command.fill, command.from, command.to) {
            (Some(fill), None, None) if !own => self.paint(fill)?,
            (Some(_), _, _) => {
                return Err("give `fill`, or `from`/`to` with `colors`/`stops`, not both".into());
            }
            (None, Some(from), Some(to)) => Paint::Gradient(self.make_gradient(
                command.kind.as_deref(),
                from,
                to,
                command.colors.as_deref(),
                command.stops.as_deref(),
            )?),
            (None, _, _) => {
                return Err(
                    "a gradient needs `from` and `to` (or a `fill` to cover the area with)".into(),
                );
            }
        };
        let shape = match &command.area {
            Some(area) => Some(Shape::Rings(vec![self.place_polygon(self.area(area)?)])),
            None => None,
        };
        let mode = if command.erase.unwrap_or(false) {
            BrushMode::Erase
        } else {
            BrushMode::Paint
        };
        self.jobs.push_back(Job::Area {
            layer: document.active_id(),
            shape,
            paint,
            opacity: unit("opacity", command.opacity.unwrap_or(1.0))? * self.frame().opacity,
            mode,
        });
        Ok(())
    }

    fn hatch(&mut self, document: &Document, command: &HatchCmd) -> Result<(), String> {
        let area = self.area(&command.area)?;
        let mut brush = self.stroke_brush(
            BrushChanges {
                color: command.color.as_ref(),
                size: command.size,
                opacity: command.opacity,
                hardness: command.hardness,
                flow: command.flow,
            },
            BrushMode::Paint,
        )?;
        // In drawing units: by default, half again the brush's own size.
        let spacing = match command.spacing {
            Some(spacing) if spacing > 0.0 => spacing,
            Some(_) => return Err("`spacing` must be more than 0".into()),
            None => (command.size.unwrap_or(self.brush.size) * 1.5).max(2.0),
        };
        let angle = command.angle.unwrap_or(45.0);
        let mut lines = geometry::hatch_lines(&area, angle, spacing);
        if command.cross.unwrap_or(false) {
            lines.extend(geometry::hatch_lines(&area, angle + 90.0, spacing));
        }
        if lines.len() > MAX_HATCH_LINES {
            return Err(format!(
                "that's {} lines: make `spacing` bigger (at most {MAX_HATCH_LINES} lines)",
                lines.len()
            ));
        }
        let (start, end) = taper(command.taper.as_ref(), 0.15)?;
        if start > 0.0 || end > 0.0 {
            brush.pressure_size = true;
        }
        let jitter = not_negative("jitter", command.jitter.unwrap_or(0.0))?;
        let detail = self.detail();
        let mut random = self.random(self.seed_of(command.seed.as_ref())?);
        let layer = document.active_id();
        for (a, b) in lines {
            // With jitter, each line's ends move a little, so it looks drawn by hand.
            let mut nudge = |p: Pt| {
                (
                    p.0 + random.range(-jitter, jitter),
                    p.1 + random.range(-jitter, jitter),
                )
            };
            let (a, b) = if jitter > 0.0 {
                (nudge(a), nudge(b))
            } else {
                (a, b)
            };
            if (b.0 - a.0).hypot(b.1 - a.1) * detail < 1.0 {
                continue;
            }
            let mut points = to_stroke_points(&geometry::densify(&[a, b], false, detail));
            geometry::apply_taper(&mut points, start, end);
            if jitter > 0.0 {
                geometry::wobble(
                    &mut points,
                    jitter / 2.0,
                    WOBBLE_WAVELENGTH,
                    false,
                    &mut random,
                );
            }
            self.place_all(&mut points);
            self.push_stroke(layer, brush, points, self.seed_of(command.seed.as_ref())?);
        }
        Ok(())
    }

    fn scatter(&mut self, document: &Document, command: &ScatterCmd) -> Result<(), String> {
        if command.count > MAX_SCATTER {
            return Err(format!("`count` can be at most {MAX_SCATTER}"));
        }
        let area = self.area(&command.area)?;
        let strokes = match command.kind.as_deref() {
            None | Some("dab") => false,
            Some("stroke") => true,
            Some(other) => return Err(format!("`kind` is \"dab\" or \"stroke\", not \"{other}\"")),
        };
        let size = range(
            command.size.as_ref(),
            (self.brush.size, self.brush.size),
            "size",
        )?;
        brush_size(size.0)?;
        brush_size(size.1)?;
        let opacity = range(
            command.opacity.as_ref(),
            (self.brush.opacity, self.brush.opacity),
            "opacity",
        )?;
        unit("opacity", opacity.0)?;
        unit("opacity", opacity.1)?;
        let length = range(
            command.length.as_ref(),
            (size.0 * 3.0, size.1 * 3.0),
            "length",
        )?;
        let angle = range(command.angle.as_ref(), (0.0, 360.0), "angle")?;
        let colors: Vec<Rgba> = match (&command.colors, &command.color) {
            (Some(_), Some(_)) => return Err("give `color` or `colors`, not both".into()),
            (Some(list), None) if list.is_empty() => return Err("`colors` is empty".into()),
            (Some(list), None) => list
                .iter()
                .map(|c| self.color(c))
                .collect::<Result<_, _>>()?,
            (None, Some(color)) => vec![self.color(color)?],
            (None, None) => {
                let [r, g, b] = self.brush.color;
                vec![[r, g, b, self.brush_alpha]]
            }
        };
        let hardness = match command.hardness {
            Some(h) => unit("hardness", h)?,
            None => self.brush.hardness,
        };

        let (brush_scale, frame_fade, detail) = {
            let frame = self.frame();
            (frame.brush_scale, frame.opacity, frame.detail)
        };
        let (left, top, right, bottom) = geometry::bounds(&area);
        let mut random = self.random(self.seed_of(command.seed.as_ref())?);
        let layer = document.active_id();
        for _ in 0..command.count {
            // Pick spots in the area's box until one is inside the area.
            let mut spot = None;
            for _ in 0..64 {
                let p = (random.range(left, right), random.range(top, bottom));
                if geometry::contains(&area, p) {
                    spot = Some(p);
                    break;
                }
            }
            let Some((x, y)) = spot else { continue };
            let [r, g, b, a] = colors[random.index(colors.len())];
            let (dab_size, fade) = self.sized(
                random.range(size.0, size.1.max(size.0 + f32::EPSILON)),
                brush_scale,
            );
            let mut brush = BrushSettings {
                color: [r, g, b],
                size: dab_size,
                opacity: random
                    .range(opacity.0, opacity.1.max(opacity.0 + f32::EPSILON))
                    .min(1.0)
                    * f32::from(a)
                    / 255.0
                    * frame_fade
                    * fade,
                hardness,
                mode: BrushMode::Paint,
                pressure_size: true,
                ..self.brush
            };
            brush.dynamics.scatter *= brush_scale;
            if !strokes && brush.tip != Tip::Round {
                // A shaped dab (a leaf, say) lands turned within `angle`.
                brush.angle += random.range(angle.0, angle.1.max(angle.0 + f32::EPSILON));
            }
            let mut points = if strokes {
                let half = random.range(length.0, length.1.max(length.0 + f32::EPSILON)) / 2.0;
                let (sin, cos) = random
                    .range(angle.0, angle.1.max(angle.0 + f32::EPSILON))
                    .to_radians()
                    .sin_cos();
                let ends = [
                    (x - cos * half, y - sin * half),
                    (x + cos * half, y + sin * half),
                ];
                let mut points = to_stroke_points(&geometry::densify(&ends, false, detail));
                geometry::apply_taper(&mut points, 0.3, 0.3);
                points
            } else {
                vec![StrokePoint::new(x, y, 1.0)]
            };
            self.place_all(&mut points);
            self.push_stroke(layer, brush, points, self.seed_of(command.seed.as_ref())?);
        }
        Ok(())
    }

    // ---- Reading values -----------------------------------------------------------------

    // ---- Values that change: keyframes and parameters ------------------------------

    /// What a field's value is now: keyframes at this frame, a `"$name"`
    /// parameter's value, a list of such things, or the value as written.
    fn value(&self, written: &Value) -> Result<Value, String> {
        match written {
            Value::String(text) if text.starts_with('$') => self.param(&text[1..]),
            keyed if animate::is_keyed(keyed) => {
                let inner = |v: &Value| self.value(v);
                let color = |name: &str| self.base_color(name).ok();
                animate::evaluate(keyed, self.clock, &inner, &color)
            }
            Value::Array(items) => items
                .iter()
                .map(|item| self.value(item))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            // Any other object (an area, a mask...) may hold keyframes or
            // parameters inside.
            Value::Object(fields) => fields
                .iter()
                .map(|(key, item)| self.value(item).map(|v| (key.clone(), v)))
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object),
            other => Ok(other.clone()),
        }
    }

    /// A parameter's value, from the innermost symbol being used.
    fn param(&self, name: &str) -> Result<Value, String> {
        let Some(params) = self.frames.iter().rev().find_map(|f| f.params.as_ref()) else {
            return Err(format!(
                "\"${name}\" is a parameter, so it only works inside a symbol that has one \
                 (\"params\": {{\"{name}\": ...}} in its define)"
            ));
        };
        match params.get(name) {
            Some(value) => Ok(value.clone()),
            None => {
                let mut names: Vec<&str> = params.keys().map(String::as_str).collect();
                names.sort_unstable();
                Err(format!(
                    "there's no parameter called \"{name}\" (this symbol has: {})",
                    if names.is_empty() {
                        "none".to_owned()
                    } else {
                        names.join(", ")
                    }
                ))
            }
        }
    }

    /// A field that must be a number, once worked out.
    fn number(&self, written: Option<&Value>, what: &str) -> Result<Option<f32>, String> {
        match written {
            None => Ok(None),
            Some(v) => match self.value(v)? {
                Value::Number(n) => Ok(Some(n.as_f64().unwrap_or(f32::NAN as f64) as f32)),
                other => Err(format!("`{what}` must be a number, not {other}")),
            },
        }
    }

    /// A field that must be `[x, y]`, once worked out.
    fn pair(&self, written: Option<&Value>, what: &str) -> Result<Option<[f32; 2]>, String> {
        match written {
            None => Ok(None),
            Some(v) => match self.value(v)? {
                Value::Array(items) if items.len() == 2 => {
                    let n = |i: usize| items[i].as_f64().map(|n| n as f32);
                    match (n(0), n(1)) {
                        (Some(x), Some(y)) => Ok(Some([x, y])),
                        _ => Err(format!("`{what}` must be [x, y], two numbers")),
                    }
                }
                other => Err(format!("`{what}` must be [x, y], not {other}")),
            },
        }
    }

    /// A field that must be text, once worked out.
    fn string(&self, written: &Value, what: &str) -> Result<String, String> {
        match self.value(written)? {
            Value::String(text) => Ok(text),
            other => Err(format!("`{what}` must be text in quotes, not {other}")),
        }
    }

    /// A group's or use's placement, with its fields worked out for this
    /// frame and the symbol's parameters.
    #[allow(clippy::too_many_arguments)]
    fn placement_of(
        &self,
        translate: Option<&Value>,
        scale: Option<&Value>,
        rotate: Option<&Value>,
        mirror: Option<&Value>,
        origin: Option<&Value>,
        opacity: Option<&Value>,
        scale_brush: Option<bool>,
    ) -> Result<Placement, String> {
        let scale = match scale {
            Some(v) => Some(self.value(v)?),
            None => None,
        };
        let mirror = match mirror {
            Some(v) => Some(self.string(v, "mirror")?),
            None => None,
        };
        let mut translate = self.pair(translate, "translate")?;
        if self.pixel.is_some() {
            // Pixel art moves by whole pixels, so it stays crisp.
            translate = translate.map(|[x, y]| [x.round(), y.round()]);
        }
        placement(
            translate,
            scale.as_ref(),
            self.number(rotate, "rotate")?,
            mirror.as_deref(),
            self.pair(origin, "origin")?,
            self.number(opacity, "opacity")?,
            scale_brush,
        )
    }

    /// A colour: "#RRGGBB", "#RRGGBBAA", a palette name, or "transparent",
    /// swapped for another if a `use` says so (innermost first).
    fn color(&self, text: &str) -> Result<Rgba, String> {
        let mut color = self.base_color(text)?;
        for frame in self.frames.iter().rev() {
            if let Some(&(_, to)) = frame.swaps.iter().find(|(from, _)| *from == color) {
                color = to;
            }
        }
        Ok(color)
    }

    /// A colour as written, without any swaps.
    fn base_color(&self, text: &str) -> Result<Rgba, String> {
        if text == "transparent" {
            return Ok([0, 0, 0, 0]);
        }
        // A parameter (of a symbol, or a repeat's `each`) holding a colour.
        if let Some(name) = text.strip_prefix('$') {
            return match self.param(name)? {
                Value::String(color) if !color.starts_with('$') => self.base_color(&color),
                other => Err(format!("\"{text}\" must be a colour, not {other}")),
            };
        }
        if text.starts_with('#') {
            if self.pixel.is_some() {
                return Err(format!(
                    "on a pixel canvas every colour is a palette name, so the picture stays in \
                     its colours: add \"{text}\" to the \"palette\" and use its name"
                ));
            }
            return parse_hex(text);
        }
        if self.palette.contains_key(text) {
            self.used_colors.borrow_mut().insert(text.to_owned());
        }
        self.palette.get(text).copied().ok_or_else(|| {
            let names: Vec<&str> = self.palette.keys().map(String::as_str).collect();
            if names.is_empty() {
                format!(
                    "unknown colour \"{text}\": use \"#RRGGBB\", or define it in the \"palette\""
                )
            } else {
                let mut names = names;
                names.sort_unstable();
                format!(
                    "unknown colour \"{text}\"; the palette has: {}",
                    names.join(", ")
                )
            }
        })
    }

    fn paper_color(&self, text: &str) -> Result<Rgba8, String> {
        let [r, g, b, a] = self.color(text)?;
        if a < 255 {
            return Err(
                "the paper can't be see-through (export with a transparent background instead)"
                    .into(),
            );
        }
        Ok(Rgba8::new(r, g, b, 255))
    }

    /// A `fill`: a colour, or a gradient object.
    fn paint(&mut self, written: &Value) -> Result<Paint, String> {
        let help = "`fill` is a colour (like \"#FF8800\" or a palette name), a gradient like \
                    {\"gradient\": \"linear\", \"from\": [0, 0], \"to\": [0, 100], \"colors\": [\"a\", \"b\"]}, \
                    {\"noise\": {\"scale\": 60, \"colors\": [\"a\", \"b\"]}} or {\"texture\": \"paper\"}";
        // A colour (or "$name" for one): as it always was.
        if let Value::String(color) = written {
            return Ok(Paint::Color(self.color(color)?));
        }
        // Keyframes and parameters inside (a noise's offset, say).
        let value = self.value(written)?;
        match &value {
            Value::String(color) => Ok(Paint::Color(self.color(color)?)),
            Value::Object(object) if object.contains_key("noise") => {
                if object.len() != 1 {
                    return Err("a noise fill is just {\"noise\": {...}}".into());
                }
                let spec: NoiseFill = serde_json::from_value(object["noise"].clone())
                    .map_err(|e| format!("noise: {e}"))?;
                self.noise_paint(spec).map_err(|p| format!("noise: {p}"))
            }
            Value::Object(object) if object.contains_key("texture") => {
                if object.len() != 1 {
                    return Err("a texture fill is just {\"texture\": ...}".into());
                }
                self.texture_paint(&object["texture"])
                    .map_err(|p| format!("texture: {p}"))
            }
            Value::Object(_) => {
                let spec: GradientFill =
                    serde_json::from_value(value.clone()).map_err(|e| format!("fill: {e}"))?;
                let gradient = self.make_gradient(
                    Some(&spec.gradient),
                    spec.from,
                    spec.to,
                    spec.colors.as_deref(),
                    spec.stops.as_deref(),
                )?;
                Ok(Paint::Gradient(gradient))
            }
            _ => Err(help.into()),
        }
    }

    /// From canvas pixels back to this frame's drawing units: where a
    /// pattern is sampled, so it moves, turns and resizes with the group.
    fn to_source(&self) -> Transform {
        self.frame().place.inverse().unwrap_or(Transform::IDENTITY)
    }

    /// Noise as paint, in this frame's coordinates.
    fn noise_paint(&self, spec: NoiseFill) -> Result<Paint, String> {
        let scale = spec.scale.unwrap_or(64.0);
        if scale <= 0.0 || !scale.is_finite() {
            return Err("`scale` (pixels per wave) must be more than 0".into());
        }
        let detail = spec.detail.unwrap_or(3.0).round();
        if !(1.0..=noise::MAX_DETAIL as f32).contains(&detail) {
            return Err(format!("`detail` is from 1 to {}", noise::MAX_DETAIL));
        }
        let contrast = not_negative("contrast", spec.contrast.unwrap_or(1.0))?;
        let stops = self.stops_of(spec.colors.as_deref(), spec.stops.as_deref())?;
        Ok(Paint::Noise(Noise {
            scale,
            detail: detail as u32,
            contrast,
            offset: pt(spec.offset.unwrap_or([0.0, 0.0]))?,
            seed: Random::mix(self.seed, spec.seed.unwrap_or(0)),
            stops,
            to_source: self.to_source(),
        }))
    }

    /// A texture as paint: `"paper"`, or a file (loaded once), in this
    /// frame's coordinates.
    fn texture_paint(&mut self, written: &Value) -> Result<Paint, String> {
        let help = "a texture is \"paper\" or {\"file\": \"textures/canvas.png\", \"scale\": 1, \
                    \"offset\": [0, 0], \"colors\": [\"dark\", \"light\"]}";
        let (image, spec) = match written {
            Value::String(name) if name == "paper" => {
                (Arc::clone(&noise::PAPER), TextureFill::default())
            }
            Value::String(other) => {
                return Err(format!(
                    "there's no built-in texture called \"{other}\"; {help}"
                ));
            }
            Value::Object(_) => {
                let spec: TextureFill =
                    serde_json::from_value(written.clone()).map_err(|e| e.to_string())?;
                let Some(file) = &spec.file else {
                    return Err(format!("`file` is missing; {help}"));
                };
                let image = match self.textures.get(file) {
                    Some(image) => Arc::clone(image),
                    None => {
                        let path = match &self.options.folder {
                            Some(folder) => folder.join(file),
                            None => PathBuf::from(file),
                        };
                        let image = Arc::new(
                            load_texture(&path)
                                .map_err(|p| format!("can't load \"{file}\": {p}"))?,
                        );
                        self.textures.insert(file.clone(), Arc::clone(&image));
                        image
                    }
                };
                (image, spec)
            }
            _ => return Err(help.into()),
        };
        let scale = spec.scale.unwrap_or(1.0);
        if scale <= 0.0 || !scale.is_finite() {
            return Err("`scale` must be more than 0".into());
        }
        let tint = match (&spec.colors, &spec.stops) {
            (None, None) => None,
            (colors, stops) => Some(self.stops_of(colors.as_deref(), stops.as_deref())?),
        };
        Ok(Paint::Texture(Texture {
            image,
            scale,
            offset: pt(spec.offset.unwrap_or([0.0, 0.0]))?,
            tint,
            to_source: self.to_source(),
        }))
    }

    /// Colour stops from `colors` (spread evenly) or `stops`.
    fn stops_of(
        &self,
        colors: Option<&[String]>,
        stops: Option<&[(f32, String)]>,
    ) -> Result<Vec<GradientStop>, String> {
        match (colors, stops) {
            (Some(colors), None) if !colors.is_empty() => {
                let last = (colors.len() - 1).max(1) as f32;
                colors
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        Ok(GradientStop {
                            position: i as f32 / last,
                            color: self.color(c)?,
                        })
                    })
                    .collect()
            }
            (None, Some(stops)) if !stops.is_empty() => stops
                .iter()
                .map(|(position, c)| {
                    Ok(GradientStop {
                        position: unit("a stop's position", *position)?,
                        color: self.color(c)?,
                    })
                })
                .collect(),
            (Some(_), Some(_)) => Err("give `colors` or `stops`, not both".into()),
            _ => Err("it needs `colors` (like [\"dark\", \"light\"]) or `stops`".into()),
        }
    }

    /// A gradient, with its start and end placed on the canvas. (Placing
    /// moves, turns and resizes it with the frame; a round gradient stays
    /// round even if the frame stretches more one way than the other.)
    fn make_gradient(
        &self,
        kind: Option<&str>,
        from: [f32; 2],
        to: [f32; 2],
        colors: Option<&[String]>,
        stops: Option<&[(f32, String)]>,
    ) -> Result<Gradient, String> {
        let shape = match kind {
            None | Some("linear") => GradientShape::Linear,
            Some("radial") => GradientShape::Radial,
            Some(other) => {
                return Err(format!(
                    "a gradient is \"linear\" or \"radial\", not \"{other}\""
                ));
            }
        };
        let (start, end) = (self.place(pt(from)?), self.place(pt(to)?));
        let stops = match (colors, stops) {
            (Some(colors), None) if !colors.is_empty() => {
                let last = (colors.len() - 1).max(1) as f32;
                colors
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        Ok(GradientStop {
                            position: i as f32 / last,
                            color: self.color(c)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
            }
            (None, Some(stops)) if !stops.is_empty() => stops
                .iter()
                .map(|(position, c)| {
                    Ok(GradientStop {
                        position: unit("a stop's position", *position)?,
                        color: self.color(c)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
            (Some(_), Some(_)) => return Err("give `colors` or `stops`, not both".into()),
            _ => {
                return Err(
                    "a gradient needs `colors` (like [\"sky\", \"sea\"]) or `stops`".into(),
                );
            }
        };
        Ok(Gradient::with_stops(shape, start, end, stops))
    }

    /// An area: {"rect": [x, y, w, h]}, {"ellipse": [cx, cy, rx, ry]},
    /// {"circle": [cx, cy, r]} or {"polygon": [[x, y], ...]}, as a polygon
    /// in drawing units.
    fn area(&self, value: &Value) -> Result<Vec<Pt>, String> {
        let help = "an area is {\"rect\": [x, y, width, height]}, {\"ellipse\": [cx, cy, rx, ry]}, \
                    {\"circle\": [cx, cy, r]} or {\"polygon\": [[x, y], ...]}";
        let object = value.as_object().filter(|o| o.len() == 1).ok_or(help)?;
        let (kind, numbers) = object.iter().next().ok_or(help)?;
        let flat = |n: usize| -> Result<Vec<f32>, String> {
            let list = numbers.as_array().filter(|l| l.len() == n).ok_or(help)?;
            list.iter()
                .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| help.to_owned()))
                .collect()
        };
        let detail = self.detail();
        let polygon = match kind.as_str() {
            "rect" => {
                let v = flat(4)?;
                geometry::rect_outline(v[0], v[1], v[2], v[3], 0.0, 0.0, detail)
            }
            "ellipse" => {
                let v = flat(4)?;
                geometry::ellipse_outline((v[0], v[1]), v[2], v[3], 0.0, detail)
            }
            "circle" => {
                let v = flat(3)?;
                geometry::ellipse_outline((v[0], v[1]), v[2], v[2], 0.0, detail)
            }
            "polygon" => {
                let points: Vec<[f32; 2]> =
                    serde_json::from_value(numbers.clone()).map_err(|_| help.to_owned())?;
                points.into_iter().map(pt).collect::<Result<_, _>>()?
            }
            _ => return Err(help.into()),
        };
        if polygon.len() < 3 {
            return Err("an area needs at least 3 points".into());
        }
        Ok(polygon)
    }

    /// Stroke points in drawing units, with their pressure.
    fn points(&self, points: &[Point]) -> Result<Vec<StrokePoint>, String> {
        points
            .iter()
            .map(|p| {
                let (x, y) = pt([p.x, p.y])?;
                let pressure = unit("pressure", p.pressure.unwrap_or(1.0))?;
                Ok(StrokePoint::new(x, y, pressure))
            })
            .collect()
    }

    /// Wobbles points (in drawing units) by up to `jitter` units.
    fn jitter(
        &self,
        points: &mut [StrokePoint],
        jitter: Option<f32>,
        seed: Option<u64>,
        closed: bool,
    ) -> Result<(), String> {
        let Some(amount) = jitter else {
            return Ok(());
        };
        let amount = not_negative("jitter", amount)?;
        geometry::wobble(
            points,
            amount,
            WOBBLE_WAVELENGTH,
            closed,
            &mut self.random(seed),
        );
        Ok(())
    }

    /// A command's `seed` as written: a whole number, keyframes or a
    /// parameter (anything in between is rounded).
    fn seed_of(&self, written: Option<&Value>) -> Result<Option<u64>, String> {
        // Seeds hold between keys (a pattern changes at a key, never
        // halfway), unless the keys say otherwise.
        let held;
        let written = match written {
            Some(Value::Object(keyed)) if animate::is_keyed(written.unwrap_or(&Value::Null)) => {
                let mut keyed = keyed.clone();
                keyed
                    .entry("ease")
                    .or_insert_with(|| Value::String("step".to_owned()));
                held = Value::Object(keyed);
                Some(&held)
            }
            other => other,
        };
        match written {
            None => Ok(None),
            Some(value) => match self.value(value)? {
                Value::Number(n) => n
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .map(|v| Some(v.round() as u64))
                    .ok_or_else(|| "`seed` is a whole number, 0 or more".to_owned()),
                other => Err(format!("`seed` must be a number, not {other}")),
            },
        }
    }

    /// Random numbers for the current command: from the script's seed and
    /// the command's own `seed` (or, without one, where it is).
    fn random(&self, seed: Option<u64>) -> Random {
        Random::new(Random::mix(self.seed, seed.unwrap_or(self.current_seed)))
    }
}

/// Brush settings a drawing command may change for itself only.
#[derive(Default)]
struct BrushChanges<'a> {
    color: Option<&'a String>,
    size: Option<f32>,
    opacity: Option<f32>,
    hardness: Option<f32>,
    flow: Option<f32>,
}

/// How a closed shape is painted.
struct ShapeStyle<'a> {
    fill: Option<&'a Value>,
    outline: Option<&'a String>,
    outline_size: Option<f32>,
    opacity: Option<f32>,
    erase: Option<bool>,
    jitter: Option<f32>,
    seed: Option<u64>,
}

/// The fields of a `filter` command or a group's `filter`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FilterFields {
    blur: Option<f32>,
    glow: Option<GlowSpec>,
    shadow: Option<ShadowSpec>,
    adjust: Option<AdjustSpec>,
    gradient_map: Option<MapSpec>,
    outline: Option<OutlineSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutlineSpec {
    color: Option<String>,
    size: Option<f32>,
    rim: Option<RimSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RimSpec {
    color: Option<String>,
    size: Option<f32>,
    angle: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GlowSpec {
    radius: Option<f32>,
    threshold: Option<f32>,
    strength: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShadowSpec {
    offset: Option<[f32; 2]>,
    blur: Option<f32>,
    color: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdjustSpec {
    hue: Option<f32>,
    saturation: Option<f32>,
    brightness: Option<f32>,
    contrast: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MapSpec {
    colors: Option<Vec<String>>,
    stops: Option<Vec<(f32, String)>>,
}

/// `{"noise": {...}}` as a fill.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoiseFill {
    scale: Option<f32>,
    detail: Option<f32>,
    contrast: Option<f32>,
    offset: Option<[f32; 2]>,
    seed: Option<u64>,
    colors: Option<Vec<String>>,
    stops: Option<Vec<(f32, String)>>,
}

/// `{"texture": {...}}` as a fill.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextureFill {
    file: Option<String>,
    scale: Option<f32>,
    offset: Option<[f32; 2]>,
    colors: Option<Vec<String>>,
    stops: Option<Vec<(f32, String)>>,
}

/// A gradient used as a shape's `fill`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GradientFill {
    gradient: String,
    from: [f32; 2],
    to: [f32; 2],
    #[serde(default)]
    colors: Option<Vec<String>>,
    #[serde(default)]
    stops: Option<Vec<(f32, String)>>,
}

/// Checks a group's (or symbol's) transform settings and makes the
/// transform: around `origin`, mirror, then scale, then rotate; then move.
impl TransformLayerCmd {
    /// What it does to the layer's pixels, in canvas pixels. The app uses
    /// this too, so what it does and what its script replays are the same.
    pub fn transform(&self) -> Result<Transform, String> {
        // Squashing a whole layer flat can't be undone, and is almost
        // certainly a mistake (unlike a part that grows from nothing).
        let flat = match &self.scale {
            Some(Value::Number(n)) => n.as_f64() == Some(0.0),
            Some(Value::Array(pair)) => pair.iter().any(|v| v.as_f64() == Some(0.0)),
            _ => false,
        };
        if flat {
            return Err(
                "`scale` is a number above 0 for transform_layer (to empty a layer, use clear_layer)"
                    .into(),
            );
        }
        Ok(placement(
            self.translate,
            self.scale.as_ref(),
            self.rotate,
            self.mirror.as_deref(),
            self.origin,
            None,
            None,
        )?
        .transform)
    }
}

fn placement(
    translate: Option<[f32; 2]>,
    scale: Option<&Value>,
    rotate: Option<f32>,
    mirror: Option<&str>,
    origin: Option<[f32; 2]>,
    opacity: Option<f32>,
    scale_brush: Option<bool>,
) -> Result<Placement, String> {
    let translate = pt(translate.unwrap_or([0.0, 0.0]))?;
    let origin = pt(origin.unwrap_or([0.0, 0.0]))?;
    let scale_help = "`scale` is a number from 0 (like 1.5; 0 shrinks it to nothing), or [across, down] like [2, 1]";
    let scale = match scale {
        None => (1.0, 1.0),
        Some(Value::Number(n)) => {
            let s = n.as_f64().unwrap_or(0.0) as f32;
            (s, s)
        }
        Some(Value::Array(pair)) if pair.len() == 2 => (
            pair[0].as_f64().ok_or(scale_help)? as f32,
            pair[1].as_f64().ok_or(scale_help)? as f32,
        ),
        Some(_) => return Err(scale_help.into()),
    };
    if !(scale.0 >= 0.0 && scale.1 >= 0.0 && scale.0.is_finite() && scale.1.is_finite()) {
        return Err(format!(
            "{scale_help} (to flip a drawing, use \"mirror\": \"x\" or \"y\")"
        ));
    }
    let rotate = rotate.unwrap_or(0.0);
    if !rotate.is_finite() {
        return Err("`rotate` must be a number of degrees".into());
    }
    let (mirror_x, mirror_y) = match mirror {
        None => (false, false),
        Some("x") => (true, false),
        Some("y") => (false, true),
        Some("xy") => (true, true),
        Some(other) => {
            return Err(format!(
                "`mirror` is \"x\" (swap left and right), \"y\" (swap up and down) or \"xy\", \
                 not \"{other}\""
            ));
        }
    };
    Ok(Placement {
        transform: Transform::about(origin, translate, scale, rotate, mirror_x, mirror_y),
        opacity: unit("opacity", opacity.unwrap_or(1.0))?,
        scale_brush: scale_brush.unwrap_or(true),
        stretch_brush: false,
        scale,
        origin,
    })
}

/// Where a symbol's randomness starts: from its name only, so every use
/// of it looks the same. (A simple, fixed text hash: "FNV-1a".)
fn name_seed(name: &str) -> u64 {
    name.bytes().fold(0xCBF2_9CE4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01B3)
    })
}

/// How the `canvas` command's `pixel` field reads: `true`, `false`, or
/// `{"supersample": 8, "outline": "ink"}`.
fn pixel_mode(
    canvas: &super::CanvasCmd,
    palette: &HashMap<String, Rgba>,
) -> Result<Option<PixelMode>, String> {
    let (supersample, outline) = match &canvas.pixel {
        None | Some(Value::Bool(false)) => return Ok(None),
        Some(Value::Bool(true)) => (4, None),
        Some(Value::Object(fields)) => {
            for field in fields.keys() {
                if !matches!(field.as_str(), "supersample" | "outline") {
                    return Err(format!(
                        "`pixel` has `supersample` and `outline`, not `{field}`"
                    ));
                }
            }
            let supersample = match fields.get("supersample") {
                None => 4,
                Some(Value::Number(n)) => match n.as_u64() {
                    Some(s) if (1..=u64::from(MAX_SUPERSAMPLE)).contains(&s) => s as u32,
                    _ => {
                        return Err(format!(
                            "`supersample` is a whole number from 1 to {MAX_SUPERSAMPLE} (how many \
                             times bigger to paint before shrinking)"
                        ));
                    }
                },
                Some(_) => return Err("`supersample` is a whole number".into()),
            };
            let outline = match fields.get("outline") {
                None => None,
                Some(Value::String(name)) => Some(*palette.get(name).ok_or_else(|| {
                    format!("`outline` is a palette colour's name, and there's no \"{name}\"")
                })?),
                Some(_) => return Err("`outline` is a palette colour's name".into()),
            };
            (supersample, outline)
        }
        Some(_) => {
            return Err("`pixel` is true, or {\"supersample\": 8, \"outline\": \"ink\"}".into());
        }
    };
    if canvas.open.is_some() {
        return Err("a pixel canvas starts new: it can't `open` a picture".into());
    }
    let mut colors: Vec<Rgba> = palette.values().copied().filter(|c| c[3] > 0).collect();
    colors.sort_unstable();
    colors.dedup();
    if colors.is_empty() {
        return Err(
            "a pixel canvas needs a \"palette\": every pixel ends up in one of its colours".into(),
        );
    }
    Ok(Some(PixelMode {
        supersample,
        outline,
        palette: colors,
    }))
}

/// A position, checked to be real numbers (in drawing units).
fn pt([x, y]: [f32; 2]) -> Result<Pt, String> {
    if !(x.is_finite() && y.is_finite()) {
        return Err("positions must be numbers".into());
    }
    Ok((x, y))
}

/// The topmost layer called `name`, if there is one.
pub fn layer_named(document: &Document, name: &str) -> Option<LayerId> {
    document
        .layers()
        .iter()
        .rev()
        .find(|layer| layer.name() == name)
        .map(|layer| layer.id())
}

fn find_layer(document: &Document, name: &str) -> Result<LayerId, String> {
    layer_named(document, name).ok_or_else(|| {
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        format!(
            "there's no layer called \"{name}\" (the layers are: {})",
            names.join(", ")
        )
    })
}

fn layer_index(document: &Document, name: &str) -> Result<usize, String> {
    let id = find_layer(document, name)?;
    Ok(document.layer_index(id).unwrap_or(0))
}

/// A brush's dynamics as written, checked.
fn read_dynamics(written: &DynamicsCmd) -> Result<Dynamics, String> {
    let fraction = |what: &str, value: Option<f32>| -> Result<f32, String> {
        match value {
            None => Ok(0.0),
            Some(v) if (0.0..=1.0).contains(&v) => Ok(v),
            Some(v) => Err(format!(
                "`dynamics`: `{what}` is a fraction from 0 to 1, not {v}"
            )),
        }
    };
    let degrees = |what: &str, value: Option<f32>| -> Result<f32, String> {
        match value {
            None => Ok(0.0),
            Some(v) if (0.0..=180.0).contains(&v) => Ok(v),
            Some(v) => Err(format!(
                "`dynamics`: `{what}` is from 0 to 180 degrees, not {v}"
            )),
        }
    };
    Ok(Dynamics {
        follow: written.follow.unwrap_or(false),
        size: fraction("size", written.size)?,
        angle: degrees("angle", written.angle)?,
        opacity: fraction("opacity", written.opacity)?,
        scatter: not_negative("scatter", written.scatter.unwrap_or(0.0))
            .map_err(|p| format!("`dynamics`: {p}"))?,
        hue: degrees("hue", written.hue)?,
        lightness: fraction("lightness", written.lightness)?,
        saturation: fraction("saturation", written.saturation)?,
    })
}

/// A number that must be a real one (not infinite).
fn finite(what: &str, value: f32) -> Result<f32, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("`{what}` must be a number"))
    }
}

/// Loads a brush tip from a PNG: its lightness is the dab's shape (and
/// a see-through pixel is nothing).
fn load_tip(path: &std::path::Path) -> Result<TipShape, String> {
    let (width, height, channels, buffer) = read_png(path)?;
    if width > 1024 || height > 1024 {
        return Err("a tip picture is at most 1024 x 1024 pixels".into());
    }
    let lightness: Vec<u8> = buffer[..width * height * channels]
        .chunks(channels)
        .map(|px| {
            let light = match channels {
                1 | 2 => u32::from(px[0]),
                _ => (u32::from(px[0]) * 54 + u32::from(px[1]) * 183 + u32::from(px[2]) * 19) / 256,
            };
            let alpha = match channels {
                2 => u32::from(px[1]),
                4 => u32::from(px[3]),
                _ => 255,
            };
            (light * alpha / 255) as u8
        })
        .collect();
    Ok(TipShape::from_lightness(width, height, &lightness))
}

/// Loads a texture from a PNG, as premultiplied pixels.
fn load_texture(path: &std::path::Path) -> Result<TextureImage, String> {
    let (width, height, channels, buffer) = read_png(path)?;
    if width > 2048 || height > 2048 {
        return Err("a texture is at most 2048 x 2048 pixels".into());
    }
    let pixels = buffer[..width * height * channels]
        .chunks(channels)
        .map(|px| match channels {
            1 => Rgba8::new(px[0], px[0], px[0], 255),
            2 => Rgba8::from_straight(px[0], px[0], px[0], px[1]),
            3 => Rgba8::new(px[0], px[1], px[2], 255),
            _ => Rgba8::from_straight(px[0], px[1], px[2], px[3]),
        })
        .collect();
    Ok(TextureImage {
        width,
        height,
        pixels,
    })
}

/// Reads a PNG as 8-bit pixels: (width, height, channels per pixel, the
/// pixels row by row). Channels: 1 grey, 2 grey + alpha, 3 RGB, 4 RGBA.
fn read_png(path: &std::path::Path) -> Result<(usize, usize, usize, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 {
        return Err("the picture is empty".into());
    }
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err("the picture's pixels aren't a kind I can read".into()),
    };
    Ok((width, height, channels, buffer))
}

/// True for the commands that matter while a symbol is painted for a
/// mask: those that paint, and those that hold other commands.
fn paints_or_structures(command: &Command) -> bool {
    !matches!(
        command,
        Command::Canvas(_)
            | Command::Paper(_)
            | Command::Layer(_)
            | Command::DeleteLayer(_)
            | Command::MoveLayer(_)
            | Command::RenameLayer(_)
            | Command::ClearLayer(_)
            | Command::TransformLayer(_)
            | Command::Pixels(_)
            | Command::Snapshot(_)
            | Command::Filter(_)
    )
}

/// The settings a `brush` command sets besides its colour, tip and
/// preset: the same for scripts and for presets (which are `brush`
/// commands with a name), so a preset paints the same in both.
pub(crate) fn apply_brush_fields(
    brush: &mut BrushSettings,
    command: &BrushCmd,
) -> Result<(), String> {
    if let Some(size) = command.size {
        brush.size = brush_size(size)?;
    }
    if let Some(opacity) = command.opacity {
        brush.opacity = unit("opacity", opacity)?;
    }
    if let Some(flow) = command.flow {
        brush.flow = unit("flow", flow)?.max(0.01);
    }
    if let Some(hardness) = command.hardness {
        brush.hardness = unit("hardness", hardness)?;
    }
    if let Some(spacing) = command.spacing {
        if !(0.01..=5.0).contains(&spacing) {
            return Err(format!(
                "`spacing` must be between 0.01 and 5, not {spacing}"
            ));
        }
        brush.spacing = spacing;
    }
    if let Some(on) = command.pressure_size {
        brush.pressure_size = on;
    }
    if let Some(on) = command.pressure_opacity {
        brush.pressure_opacity = on;
    }
    if let Some(on) = command.pressure_flow {
        brush.pressure_flow = on;
    }
    if let Some(angle) = command.angle {
        brush.angle = finite("angle", angle)?;
    }
    if let Some(dynamics) = &command.dynamics {
        brush.dynamics = read_dynamics(dynamics)?;
    }
    if let Some(min) = command.pressure_size_min {
        brush.pressure_size_min = unit("pressure_size_min", min)?;
    }
    if let Some(min) = command.pressure_opacity_min {
        brush.pressure_opacity_min = unit("pressure_opacity_min", min)?;
    }
    if let Some(curve) = command.pressure_curve {
        if !(0.1..=10.0).contains(&curve) {
            return Err(format!(
                "`pressure_curve` must be between 0.1 and 10, not {curve}"
            ));
        }
        brush.pressure_curve = curve;
    }
    if let Some(grain) = command.grain {
        brush.grain = unit("grain", grain)?;
    }
    if let Some(scale) = command.grain_scale {
        if !(0.25..=16.0).contains(&scale) {
            return Err(format!(
                "`grain_scale` must be between 0.25 and 16, not {scale}"
            ));
        }
        brush.grain_scale = scale;
    }
    if let Some(limit) = command.build_up {
        brush.build_up = unit("build_up", limit)?;
    }
    Ok(())
}

fn layer_props(document: &Document, id: LayerId) -> LayerProps {
    document
        .layer(id)
        .map(|layer| layer.props())
        .unwrap_or_else(|| LayerProps::named(""))
}

fn is_empty(document: &Document, id: LayerId) -> bool {
    document
        .layer(id)
        .is_some_and(|layer| layer.surface.tile_count() == 0)
}

fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        Err("a layer needs a name".into())
    } else {
        Ok(())
    }
}

/// Checks a value that must be from 0 to 1.
fn unit(what: &str, value: f32) -> Result<f32, String> {
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(format!("`{what}` must be from 0 to 1, not {value}"))
    }
}

/// Checks a value that must be a number, 0 or more.
fn not_negative(what: &str, value: f32) -> Result<f32, String> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(format!("`{what}` can't be negative, not {value}"))
    }
}

fn brush_size(size: f32) -> Result<f32, String> {
    if size > 0.0 && size <= MAX_SIZE {
        Ok(size)
    } else {
        Err(format!(
            "`size` must be more than 0 and at most {MAX_SIZE}, not {size}"
        ))
    }
}

/// A `taper`: one number for both ends, or `[start, end]`.
fn taper(value: Option<&Value>, default: f32) -> Result<(f32, f32), String> {
    let help = "`taper` is a number from 0 to 0.5 (like 0.3), or [start, end] like [0.1, 0.4]";
    let (start, end) = match value {
        None => (default, default),
        Some(Value::Number(n)) => {
            let t = n.as_f64().unwrap_or(-1.0) as f32;
            (t, t)
        }
        Some(Value::Array(pair)) if pair.len() == 2 => (
            pair[0].as_f64().ok_or(help)? as f32,
            pair[1].as_f64().ok_or(help)? as f32,
        ),
        Some(_) => return Err(help.into()),
    };
    if (0.0..=1.0).contains(&start) && (0.0..=1.0).contains(&end) {
        Ok((start, end))
    } else {
        Err(help.into())
    }
}

/// A number, or `[min, max]` to pick from at random.
fn range(value: Option<&Value>, default: (f32, f32), what: &str) -> Result<(f32, f32), String> {
    let help = || format!("`{what}` is a number, or [min, max] to pick from at random");
    match value {
        None => Ok(default),
        Some(Value::Number(n)) => {
            let v = n.as_f64().ok_or_else(help)? as f32;
            Ok((v, v))
        }
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let a = pair[0].as_f64().ok_or_else(help)? as f32;
            let b = pair[1].as_f64().ok_or_else(help)? as f32;
            Ok((a.min(b), a.max(b)))
        }
        Some(_) => Err(help()),
    }
}

/// The box around what a job paints, in the pixels being painted.
fn job_extent(job: &Job, document: &Document) -> Extent {
    let around = |points: &mut dyn Iterator<Item = Pt>, margin: f32| {
        let (l, t, r, b) = points.fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(l, t, r, b), (x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
        );
        (l - margin, t - margin, r + margin, b + margin)
    };
    let canvas = document.bounds();
    let whole = (
        canvas.left as f32,
        canvas.top as f32,
        canvas.right as f32,
        canvas.bottom as f32,
    );
    match job {
        Job::Stroke { brush, points, .. } => {
            around(&mut points.iter().map(|p| (p.x, p.y)), brush.size / 2.0)
        }
        Job::Area {
            shape: Some(Shape::Rings(rings)),
            ..
        } => around(&mut rings.iter().flatten().copied(), 0.0),
        Job::Area {
            shape: Some(Shape::Mask(mask)),
            ..
        } => {
            let r = mask.content_bounds();
            (r.left as f32, r.top as f32, r.right as f32, r.bottom as f32)
        }
        Job::Area { shape: None, .. } => whole,
        // Where it ends up isn't known until it's done: count where it starts.
        Job::Bucket { fill, .. } => (fill.x, fill.y, fill.x + 1.0, fill.y + 1.0),
    }
}

/// The box around both (`a` may be nothing yet).
fn union(a: Option<Extent>, b: Extent) -> Extent {
    match a {
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
        None => b,
    }
}

/// True if any of `area` is inside `canvas`.
/// How much of the canvas a box covers, 0 to 1.
fn covered((left, top, right, bottom): Extent, canvas: PixelRect) -> f32 {
    let w = (right.min(canvas.right as f32) - left.max(canvas.left as f32)).max(0.0);
    let h = (bottom.min(canvas.bottom as f32) - top.max(canvas.top as f32)).max(0.0);
    let area = (canvas.right - canvas.left) as f32 * (canvas.bottom - canvas.top) as f32;
    if area <= 0.0 {
        0.0
    } else {
        (w * h / area).min(1.0)
    }
}

fn overlaps((left, top, right, bottom): Extent, canvas: PixelRect) -> bool {
    left < canvas.right as f32
        && right > canvas.left as f32
        && top < canvas.bottom as f32
        && bottom > canvas.top as f32
}

fn to_stroke_points(points: &[Pt]) -> Vec<StrokePoint> {
    points
        .iter()
        .map(|&(x, y)| StrokePoint::new(x, y, 1.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Background;
    use crate::history::CommandGroup;

    /// Plays a script (given as JSON text) and returns the picture.
    fn play_with(text: &str, options: PlayOptions) -> Result<Document, ScriptError> {
        let script = Script::parse(text)?;
        let mut player = Player::new(script, options)?;
        let mut document = player.create_document()?;
        player.run(&mut document, |_, _| {})?;
        Ok(document)
    }

    fn play(text: &str) -> Document {
        play_with(text, PlayOptions::default()).unwrap_or_else(|e| panic!("{e}"))
    }

    fn error(text: &str) -> String {
        play_with(text, PlayOptions::default())
            .unwrap_err()
            .to_string()
    }

    /// The whole picture as PNG bytes, to compare two pictures exactly.
    fn png(document: &Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        file::write_png(document, Background::Paper, &mut bytes).unwrap();
        bytes
    }

    fn seen(document: &Document, x: i32, y: i32) -> Rgba8 {
        document.composite_pixel(x, y, Background::Paper)
    }

    /// A script using every kind of drawing, including randomness.
    const EVERYTHING: &str = r##"{
      "seed": 5,
      "palette": { "sky": "#5080D0", "sun": "#FFC040", "ink": "#202020" },
      "commands": [
        { "cmd": "canvas", "width": 400, "height": 300, "paper": "#F8F4EC" },
        { "cmd": "layer", "name": "Sky" },
        { "cmd": "gradient", "from": [0, 0], "to": [0, 300], "colors": ["sky", "#FFFFFF"] },
        { "cmd": "layer", "name": "Shapes" },
        { "cmd": "ellipse", "center": [200, 150], "radius": 60, "fill": "sun", "outline": "ink", "jitter": 3 },
        { "cmd": "rect", "x": 20, "y": 200, "width": 120, "height": 60, "radius": 10, "fill": { "gradient": "radial", "from": [80, 230], "to": [140, 230], "colors": ["sun", "sky"] } },
        { "cmd": "polygon", "points": [[300, 250], [380, 250], [340, 180]], "fill": "ink", "opacity": 0.5 },
        { "cmd": "brush", "size": 6, "color": "ink" },
        { "cmd": "stroke", "points": [[10, 10], [100, 40, 0.5], [190, 10]] },
        { "cmd": "line", "from": [10, 290], "to": [390, 290], "taper": 0.3 },
        { "cmd": "path", "points": [[250, 30], [300, 80], [350, 30]], "jitter": 2 },
        { "cmd": "hatch", "area": { "rect": [20, 60, 100, 100] }, "angle": 30, "spacing": 8, "size": 2, "jitter": 2 },
        { "cmd": "scatter", "area": { "circle": [300, 120, 40] }, "count": 50, "size": [2, 8], "colors": ["sky", "sun"] },
        { "cmd": "scatter", "area": { "circle": [300, 120, 40] }, "count": 10, "kind": "stroke", "length": [10, 20] },
        { "cmd": "fill", "at": [395, 5], "color": "#00000020", "tolerance": 0.05 },
        { "cmd": "erase", "points": [[200, 100], [200, 200]], "size": 10 }
      ]
    }"##;

    #[test]
    fn playing_the_same_script_twice_gives_the_same_picture() {
        let first = play(EVERYTHING);
        let second = play(EVERYTHING);
        assert_eq!(png(&first), png(&second));
        // And a different seed gives a different picture.
        let reseeded = play(&EVERYTHING.replace("\"seed\": 5", "\"seed\": 6"));
        assert_ne!(png(&first), png(&reseeded));
    }

    #[test]
    fn shapes_land_where_they_are_asked_to() {
        let document = play(EVERYTHING);
        // The middle of the sun ellipse, minus the erased line.
        assert_eq!(seen(&document, 230, 150), Rgba8::new(0xFF, 0xC0, 0x40, 255));
        // The erased line goes through the sun, so the sky shows there.
        let erased = seen(&document, 200, 150);
        assert!(erased.b > erased.r, "{erased:?}");
    }

    #[test]
    fn the_first_layer_command_uses_the_empty_starting_layer() {
        let document = play(EVERYTHING);
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Sky", "Shapes"], "no empty \"Layer 1\" left behind");
    }

    #[test]
    fn layers_can_be_placed_and_set_up() {
        let document = play(
            r#"{ "commands": [
                { "cmd": "canvas", "width": 50, "height": 50 },
                { "cmd": "layer", "name": "Top" },
                { "cmd": "layer", "name": "Bottom", "below": "Top" },
                { "cmd": "layer", "name": "Middle", "above": "Bottom", "opacity": 0.5, "blend": "multiply" },
                { "cmd": "layer", "name": "Gone" },
                { "cmd": "delete_layer", "name": "Gone" },
                { "cmd": "rename_layer", "name": "Top", "to": "Lid" },
                { "cmd": "move_layer", "name": "Bottom", "index": 2 },
                { "cmd": "move_layer", "name": "Bottom", "below": "Middle" }
            ] }"#,
        );
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Bottom", "Middle", "Lid"]);
        let middle = document.layers()[1].props();
        assert_eq!((middle.opacity, middle.blend), (0.5, BlendMode::Multiply));
    }

    #[test]
    fn errors_name_the_command_and_explain() {
        let start = r#"{ "commands": [ { "cmd": "canvas", "width": 50, "height": 50 }, "#;
        let with = |command: &str| error(&format!("{start}{command} ] }}"));

        let message = with(r#"{ "cmd": "brush", "color": "skye" }"#);
        assert!(
            message.starts_with("command 2 (brush): unknown colour \"skye\""),
            "{message}"
        );
        let message = with(r#"{ "cmd": "delete_layer", "name": "Sky" }"#);
        assert!(
            message.contains("no layer called \"Sky\" (the layers are: Layer 1)"),
            "{message}"
        );
        let message = with(r#"{ "cmd": "layer", "name": "A", "blend": "sparkle" }"#);
        assert!(
            message.contains("unknown blend mode \"sparkle\"; use one of: normal, multiply"),
            "{message}"
        );
        let message = with(r#"{ "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10 }"#);
        assert!(message.contains("give a `fill`"), "{message}");
        let message = with(r#"{ "cmd": "brush", "opacity": 1.5 }"#);
        assert!(
            message.contains("`opacity` must be from 0 to 1, not 1.5"),
            "{message}"
        );
        let message = with(r#"{ "cmd": "hatch", "area": { "square": [0, 0, 5] } }"#);
        assert!(message.contains("an area is"), "{message}");
        let message = with(r#"{ "cmd": "canvas", "width": 5, "height": 5 }"#);
        assert!(message.contains("only the first command"), "{message}");

        let message = error(r#"{ "commands": [ { "cmd": "snapshot", "name": "x" } ] }"#);
        assert!(
            message.starts_with("command 1 (snapshot): a script starts with a `canvas`"),
            "{message}"
        );
    }

    #[test]
    fn snapshots_and_finished_layers_are_reported_in_order() {
        let script = Script::parse(
            r##"{ "commands": [
                { "cmd": "canvas", "width": 50, "height": 50 },
                { "cmd": "layer", "name": "A" },
                { "cmd": "stroke", "points": [[5, 5], [40, 40]] },
                { "cmd": "layer", "name": "B" },
                { "cmd": "stroke", "points": [[5, 40], [40, 5]] },
                { "cmd": "snapshot", "name": "cross" },
                { "cmd": "layer", "name": "A" },
                { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "#F00" }
            ] }"##,
        )
        .unwrap();
        let mut player = Player::new(script, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        let mut events = Vec::new();
        player
            .run(&mut document, |event, _| events.push(event.clone()))
            .unwrap();
        assert_eq!(
            events,
            [
                Event::LayerDone("A".into()),
                Event::Snapshot("cross".into()),
                Event::LayerDone("A".into()),
            ]
        );
    }

    #[test]
    fn a_preview_is_the_same_picture_smaller() {
        let full = play(EVERYTHING);
        let options = PlayOptions {
            scale: 0.5,
            ..PlayOptions::default()
        };
        let half = play_with(EVERYTHING, options).unwrap();
        assert_eq!((half.width(), half.height()), (200, 150));
        // The sun's middle is at half the position.
        assert_eq!(seen(&half, 115, 75), seen(&full, 230, 150));
    }

    #[test]
    fn playing_bit_by_bit_gives_the_same_picture_as_all_at_once() {
        let at_once = play(EVERYTHING);
        let mut player =
            Player::new(Script::parse(EVERYTHING).unwrap(), PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        let mut rounds = 0;
        while player.advance(&mut document, 50.0).unwrap() != Progress::Finished {
            rounds += 1;
        }
        assert!(rounds > 20, "it really went in small steps ({rounds})");
        assert_eq!(png(&document), png(&at_once));
    }

    #[test]
    fn everything_played_can_be_undone_in_one_go() {
        let mut document = Document::new(400, 300);
        let empty = png(&document);
        let options = PlayOptions {
            record_undo: true,
            ..PlayOptions::default()
        };
        let mut player = Player::new(Script::parse(EVERYTHING).unwrap(), options).unwrap();
        player.start_on(&mut document).unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let painted = png(&document);
        assert_ne!(painted, empty);

        let mut group = CommandGroup::new("Play strokes", player.take_undo(&mut document));
        group.undo(&mut document);
        assert_eq!(png(&document), empty);
        assert_eq!(document.layers().len(), 1, "the added layers are gone");
        group.redo(&mut document);
        assert_eq!(png(&document), painted);
    }

    #[test]
    fn checking_paints_nothing_but_finds_mistakes() {
        let options = PlayOptions {
            dry_run: true,
            ..PlayOptions::default()
        };
        let document = play_with(EVERYTHING, options.clone()).unwrap();
        assert!(
            document
                .layers()
                .iter()
                .all(|l| l.surface.tile_count() == 0)
        );
        assert_eq!(document.layers().len(), 2, "layers are still made");

        let broken = EVERYTHING.replace(
            "\"colors\": [\"sky\", \"sun\"]",
            "\"colors\": [\"sky\", \"moon\"]",
        );
        let message = play_with(&broken, options).unwrap_err().to_string();
        assert!(
            message.starts_with("command 13 (scatter): unknown colour \"moon\""),
            "{message}"
        );
    }

    #[test]
    fn a_script_can_start_from_a_saved_picture() {
        let folder = std::env::temp_dir().join(format!("artapp-open-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let base = play(EVERYTHING);
        file::save(&base, &folder.join("base.quill")).unwrap();

        let options = PlayOptions {
            folder: Some(folder.clone()),
            ..PlayOptions::default()
        };
        let document = play_with(
            r#"{ "commands": [
                { "cmd": "canvas", "open": "base.quill" },
                { "cmd": "layer", "name": "Extra" }
            ] }"#,
            options,
        )
        .unwrap();
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Sky", "Shapes", "Extra"]);
        assert_eq!(png(&document), png(&base));
        std::fs::remove_dir_all(folder).ok();
    }

    #[test]
    fn the_first_example_in_the_documentation_plays() {
        // Unix line endings, however Git checked the file out.
        let docs = include_str!("../../../../docs/script-format.md").replace("\r\n", "\n");
        let start = docs.find("```json\n{").expect("an example") + "```json\n".len();
        let end = start + docs[start..].find("```").expect("its end");
        let document = play(&docs[start..end]);
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Sky", "Sun", "Sea"]);
    }

    // ---- Selections -------------------------------------------------------------

    /// A 100 x 60 canvas, `before` (select commands) then a red rect over
    /// everything: what ends up red is what was selected.
    fn selected(before: &str) -> Document {
        play(&format!(
            r##"{{ "commands": [
              {{ "cmd": "canvas", "width": 100, "height": 60 }},
              {before}
              {{ "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 60, "fill": "#FF0000" }}
            ] }}"##
        ))
    }

    fn alpha(document: &Document, x: i32, y: i32) -> u8 {
        document.active_layer().surface.pixel(x, y).a
    }

    #[test]
    fn select_limits_painting_to_the_shape() {
        let d = selected(r#"{ "cmd": "select", "rect": [10, 10, 30, 20] },"#);
        assert_eq!(alpha(&d, 20, 20), 255);
        assert_eq!(alpha(&d, 50, 20), 0);
        let d = selected(r#"{ "cmd": "select", "circle": [50, 30, 10] },"#);
        assert_eq!(alpha(&d, 50, 30), 255);
        assert_eq!(alpha(&d, 50, 45), 0);
        // `none` drops it again.
        let d = selected(
            r#"{ "cmd": "select", "rect": [0, 0, 5, 5] }, { "cmd": "select", "none": true },"#,
        );
        assert_eq!(alpha(&d, 90, 50), 255);
    }

    #[test]
    fn select_adds_subtracts_inverts_and_feathers() {
        let d = selected(
            r#"{ "cmd": "select", "rect": [0, 0, 20, 60] },
               { "cmd": "select", "rect": [80, 0, 20, 60], "mode": "add" },
               { "cmd": "select", "rect": [0, 0, 100, 10], "mode": "subtract" },"#,
        );
        assert_eq!(alpha(&d, 10, 30), 255);
        assert_eq!(alpha(&d, 90, 30), 255);
        assert_eq!(alpha(&d, 50, 30), 0);
        assert_eq!(alpha(&d, 10, 5), 0);
        let d = selected(
            r#"{ "cmd": "select", "rect": [0, 0, 50, 60] }, { "cmd": "select", "invert": true },"#,
        );
        assert_eq!(alpha(&d, 10, 30), 0);
        assert_eq!(alpha(&d, 70, 30), 255);
        let d = selected(r#"{ "cmd": "select", "rect": [30, 0, 40, 60], "feather": 6 },"#);
        assert_eq!(alpha(&d, 50, 30), 255);
        let edge = alpha(&d, 30, 30);
        assert!(edge > 40 && edge < 220, "a soft edge, got {edge}");
        // Feather alone softens the selection there is.
        let d = selected(
            r#"{ "cmd": "select", "rect": [30, 0, 40, 60] }, { "cmd": "select", "feather": 6 },"#,
        );
        let edge = alpha(&d, 30, 30);
        assert!(edge > 40 && edge < 220, "a soft edge, got {edge}");
    }

    #[test]
    fn select_layer_uses_where_a_layer_has_paint() {
        let d = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 60 },
              { "cmd": "layer", "name": "Shape" },
              { "cmd": "ellipse", "center": [30, 30], "radius": 15, "fill": "#000000" },
              { "cmd": "layer", "name": "Colour" },
              { "cmd": "select", "layer": "Shape" },
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 60, "fill": "#FF0000" }
            ] }"##,
        );
        assert_eq!(alpha(&d, 30, 30), 255);
        assert_eq!(alpha(&d, 80, 30), 0);
    }

    #[test]
    fn a_filter_stays_inside_the_selection() {
        let d = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 60 },
              { "cmd": "rect", "x": 0, "y": 20, "width": 100, "height": 20, "fill": "#000000" },
              { "cmd": "select", "rect": [0, 0, 50, 60] },
              { "cmd": "filter", "blur": 4 }
            ] }"##,
        );
        assert!(alpha(&d, 25, 18) > 0, "blurred inside");
        assert_eq!(alpha(&d, 75, 18), 0, "untouched outside");
    }

    /// The app runs each selection change with `selection::run` and
    /// records the command; replaying the commands in a script must give
    /// exactly the same picture.
    #[test]
    fn the_apps_selections_replay_exactly_in_a_script() {
        use crate::fill::paint_mask_within;
        let commands = [
            r#"{ "cmd": "select", "rect": [10.5, 5.5, 40.2, 30.7] }"#,
            r#"{ "cmd": "select", "ellipse": [70.3, 30.1, 20.6, 14.2], "mode": "add" }"#,
            r#"{ "cmd": "select", "polygon": [[0, 0], [30, 50], [5, 55]], "mode": "subtract", "feather": 3 }"#,
            r#"{ "cmd": "select", "invert": true }"#,
            r#"{ "cmd": "select", "invert": true }"#,
            r#"{ "cmd": "select", "feather": 2 }"#,
        ];
        // The app's way.
        let mut app = Document::new(100, 60);
        for text in commands {
            let command: SelectCmd = match serde_json::from_str::<Command>(text).unwrap() {
                Command::Select(c) => c,
                _ => unreachable!(),
            };
            let selection = selection::run(&command, &app).unwrap();
            app.set_selection(selection);
        }
        let clip = app.clip();
        let limits = app.bounds();
        paint_mask_within(
            &mut app.active_layer_mut().surface,
            &selection::all(limits),
            &Paint::Color([255, 0, 0, 255]),
            1.0,
            BrushMode::Paint,
            limits,
            clip.as_deref(),
        );
        // The script's way.
        let script = selected(&format!("{},", commands.join(",\n")));
        assert_eq!(png(&app), png(&script));
    }

    #[test]
    fn select_mistakes_are_explained() {
        let base = r#"{ "commands": [ { "cmd": "canvas", "width": 10, "height": 10 }, SELECT ] }"#;
        let e = |select: &str| error(&base.replace("SELECT", select));
        assert!(
            e(r#"{ "cmd": "select", "rect": [0, 0, 5, 5], "circle": [5, 5, 2] }"#)
                .contains("one at a time")
        );
        assert!(
            e(r#"{ "cmd": "select", "rect": [0, 0, 5, 5], "mode": "xor" }"#).contains("\"add\"")
        );
        assert!(e(r#"{ "cmd": "select" }"#).contains("needs a shape"));
        assert!(e(r#"{ "cmd": "select", "feather": 3 }"#).contains("there is none yet"));
        assert!(e(r#"{ "cmd": "select", "none": true, "feather": 3 }"#).contains("leave out"));
        assert!(e(r#"{ "cmd": "select", "layer": "Nope" }"#).contains("Nope"));
    }

    #[test]
    fn an_empty_selection_is_dropped_with_a_warning() {
        let report = report_of(
            r#"{ "commands": [
              { "cmd": "canvas", "width": 10, "height": 10 },
              { "cmd": "select", "rect": [50, 50, 5, 5] }
            ] }"#,
        );
        assert!(
            report.warnings.iter().any(|w| w.contains("came out empty")),
            "{:?}",
            report.warnings
        );
    }

    // ---- Groups ---------------------------------------------------------------

    /// A script on a white 200 x 200 canvas with `commands` (a JSON list body).
    fn small(commands: &str) -> String {
        format!(
            r##"{{ "palette": {{ "ink": "#202020", "red": "#E02020" }}, "commands": [
                {{ "cmd": "canvas", "width": 200, "height": 200 }}, {commands} ] }}"##
        )
    }

    /// The largest difference in any colour channel between two pictures.
    fn largest_difference(a: &Document, b: &Document) -> u8 {
        let mut most = 0;
        for y in 0..a.height() as i32 {
            for x in 0..a.width() as i32 {
                let (p, q) = (seen(a, x, y), seen(b, x, y));
                for (u, v) in [(p.r, q.r), (p.g, q.g), (p.b, q.b), (p.a, q.a)] {
                    most = most.max(u.abs_diff(v));
                }
            }
        }
        most
    }

    #[test]
    fn a_mirrored_group_equals_the_shapes_mirrored_by_hand() {
        let grouped = play(&small(
            r##"{ "cmd": "group", "mirror": "x", "origin": [100, 0], "commands": [
                { "cmd": "polygon", "points": [[10, 10], [60, 20], [30, 80]], "fill": "red" },
                { "cmd": "rect", "x": 20, "y": 120, "width": 50, "height": 30, "fill": "ink" },
                { "cmd": "stroke", "points": [[10, 190], [60, 160], [90, 185]], "size": 6, "color": "ink" }
            ] }"##,
        ));
        // Mirroring left-right around x = 100: x becomes 200 - x.
        let by_hand = play(&small(
            r##"{ "cmd": "polygon", "points": [[190, 10], [140, 20], [170, 80]], "fill": "red" },
                { "cmd": "rect", "x": 130, "y": 120, "width": 50, "height": 30, "fill": "ink" },
                { "cmd": "stroke", "points": [[190, 190], [140, 160], [110, 185]], "size": 6, "color": "ink" }"##,
        ));
        assert_eq!(png(&grouped), png(&by_hand));
    }

    #[test]
    fn nested_groups_combine_their_transforms() {
        // Inner: twice as big. Outer: moved 100 right. A 20 x 20 square at
        // (10, 10) ends up 40 x 40 at (120, 20), and its outline twice as thick.
        let nested = play(&small(
            r##"{ "cmd": "group", "translate": [100, 0], "commands": [
                { "cmd": "group", "scale": 2, "commands": [
                    { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "red", "outline": "ink", "outline_size": 2 }
                ] }
            ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "rect", "x": 120, "y": 20, "width": 40, "height": 40, "fill": "red", "outline": "ink", "outline_size": 4 }"##,
        ));
        assert_eq!(png(&nested), png(&by_hand));
    }

    #[test]
    fn rotation_turns_around_the_origin() {
        // 90 degrees clockwise around (100, 100): (x, y) goes to (200 - y, x).
        let turned = play(&small(
            r##"{ "cmd": "group", "rotate": 90, "origin": [100, 100], "commands": [
                { "cmd": "rect", "x": 100, "y": 50, "width": 40, "height": 10, "fill": "red" }
            ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "rect", "x": 140, "y": 100, "width": 10, "height": 40, "fill": "red" }"##,
        ));
        // Turning uses sines and cosines, so allow the tiniest rounding.
        assert!(largest_difference(&turned, &by_hand) <= 2);
        assert_eq!(seen(&turned, 145, 120), Rgba8::new(0xE0, 0x20, 0x20, 255));
        assert_eq!(
            seen(&turned, 120, 55),
            Rgba8::WHITE,
            "nothing left where it started"
        );
    }

    #[test]
    fn scale_brush_and_opacity() {
        let thin = play(&small(
            r##"{ "cmd": "group", "scale": 4, "scale_brush": false, "commands": [
                { "cmd": "stroke", "points": [[5, 10], [45, 10]], "size": 4, "color": "ink" }
            ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "stroke", "points": [[20, 40], [180, 40]], "size": 4, "color": "ink" }"##,
        ));
        assert_eq!(png(&thin), png(&by_hand), "the brush kept its size");

        let faint = play(&small(
            r##"{ "cmd": "group", "opacity": 0.5, "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 50, "height": 50, "fill": "#FF0000" }
            ] }"##,
        ));
        // Half-strength red is stored with opacity 128 of 255, so 127/255
        // of the white paper shows through: 255 * 127 / 255 = 127.
        assert_eq!(seen(&faint, 25, 25), Rgba8::new(255, 127, 127, 255));
    }

    #[test]
    fn areas_fills_and_gradients_move_with_the_group() {
        let document = play(&small(
            r##"{ "cmd": "group", "translate": [100, 100], "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 60, "height": 60, "outline": "ink", "outline_size": 3 },
                { "cmd": "fill", "at": [30, 30], "color": "red", "grow": 0 },
                { "cmd": "scatter", "area": { "rect": [10, 10, 40, 40] }, "count": 50, "size": 2, "color": "ink", "seed": 1 },
                { "cmd": "gradient", "from": [0, 0], "to": [60, 0], "colors": ["#0000FF", "#0000FF"], "area": { "rect": [0, 70, 60, 10] } }
            ] }"##,
        ));
        assert_eq!(
            seen(&document, 105, 105),
            Rgba8::new(0xE0, 0x20, 0x20, 255),
            "filled inside"
        );
        assert_eq!(
            seen(&document, 50, 50),
            Rgba8::WHITE,
            "nothing where it would be without the move"
        );
        assert_eq!(
            seen(&document, 130, 175),
            Rgba8::new(0, 0, 255, 255),
            "the gradient's area moved"
        );
        // Every scattered dab is inside the moved area.
        for y in 0..200 {
            for x in 0..200 {
                let p = seen(&document, x, y);
                let ink = p.r < 100 && p.g < 100 && p.b < 100;
                if ink && !(98..=162).contains(&x) {
                    panic!("a dark pixel outside the group at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn a_group_gives_back_the_brush_and_layer_it_found() {
        let document = play(&small(
            r##"{ "cmd": "layer", "name": "Back" },
                { "cmd": "brush", "size": 4, "color": "ink" },
                { "cmd": "group", "commands": [
                    { "cmd": "layer", "name": "Front" },
                    { "cmd": "brush", "size": 40, "color": "red" },
                    { "cmd": "stroke", "points": [[20, 20], [60, 20]] }
                ] },
                { "cmd": "stroke", "points": [[20, 150], [180, 150]] }"##,
        ));
        let back = layer_named(&document, "Back").unwrap();
        assert_eq!(document.active_id(), back, "Back is selected again");
        // The last stroke is thin, dark and on Back.
        let pixel = document.layer(back).unwrap().surface.pixel(100, 150);
        assert_eq!(pixel, Rgba8::new(0x20, 0x20, 0x20, 255));
        assert_eq!(
            document.layer(back).unwrap().surface.pixel(100, 158),
            Rgba8::TRANSPARENT
        );
    }

    #[test]
    fn mistakes_inside_groups_say_where_they_are() {
        // Found while reading the script.
        let message = error(&small(
            r##"{ "cmd": "group", "name": "duck", "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "red" },
                { "cmd": "group", "commands": [ { "cmd": "ellipse", "center": [0, 0], "radius": 3, "colour": "red" } ] }
            ] }"##,
        ));
        assert!(
            message.starts_with(
                "command 2 (group \"duck\") > 2 (group) > 1 (ellipse): unknown field `colour`"
            ),
            "{message}"
        );
        // Found while playing.
        let message = error(&small(
            r##"{ "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "red" },
                { "cmd": "group", "name": "duck", "commands": [
                    { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "red" },
                    { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "blu" }
                ] }"##,
        ));
        assert!(
            message.starts_with("command 3 (group \"duck\") > 2 (rect): unknown colour \"blu\""),
            "{message}"
        );
        let message = error(&small(r#"{ "cmd": "group", "scale": -1, "commands": [] }"#));
        assert!(message.contains("use \"mirror\""), "{message}");
        let message = error(&small(r#"{ "cmd": "group", "mirror": "left" }"#));
        assert!(message.contains("`commands` is missing"), "{message}");
    }

    // ---- Symbols and repeat ---------------------------------------------------

    /// A duck-ish drawing: shapes, a stroke, a seeded wobble, a seeded
    /// scatter, a gradient and two layers.
    const DUCK: &str = r##"{ "cmd": "define", "name": "duck", "commands": [
        { "cmd": "layer", "name": "Shadow" },
        { "cmd": "ellipse", "center": [50, 92], "radius": [30, 6], "fill": "#00000040" },
        { "cmd": "layer", "name": "Body" },
        { "cmd": "polygon", "points": [[20, 40], [80, 40], [70, 80], [30, 80]], "fill": "yellow", "outline": "ink", "outline_size": 2 },
        { "cmd": "rect", "x": 60, "y": 20, "width": 20, "height": 20, "fill": "yellow" },
        { "cmd": "stroke", "points": [[80, 30], [92, 32]], "size": 4, "color": "beak" },
        { "cmd": "ellipse", "center": [40, 60], "radius": 12, "fill": "yellow", "jitter": 2, "seed": 3 },
        { "cmd": "scatter", "area": { "rect": [30, 45, 40, 30] }, "count": 10, "size": 2, "color": "ink", "seed": 4 },
        { "cmd": "gradient", "from": [0, 40], "to": [0, 80], "colors": ["#FFFFFF40", "transparent"], "area": { "rect": [20, 40, 60, 40] } }
    ] }"##;

    /// The duck's commands, written out by hand, moved by (dx, dy).
    fn duck_by_hand(dx: f32, dy: f32) -> String {
        let p = |x: f32, y: f32| format!("[{}, {}]", x + dx, y + dy);
        format!(
            r##"{{ "cmd": "layer", "name": "Shadow" }},
            {{ "cmd": "ellipse", "center": {}, "radius": [30, 6], "fill": "#00000040" }},
            {{ "cmd": "layer", "name": "Body" }},
            {{ "cmd": "polygon", "points": [{}, {}, {}, {}], "fill": "yellow", "outline": "ink", "outline_size": 2 }},
            {{ "cmd": "rect", "x": {}, "y": {}, "width": 20, "height": 20, "fill": "yellow" }},
            {{ "cmd": "stroke", "points": [{}, {}], "size": 4, "color": "beak" }},
            {{ "cmd": "ellipse", "center": {}, "radius": 12, "fill": "yellow", "jitter": 2, "seed": 3 }},
            {{ "cmd": "scatter", "area": {{ "rect": [{}, {}, 40, 30] }}, "count": 10, "size": 2, "color": "ink", "seed": 4 }},
            {{ "cmd": "gradient", "from": {}, "to": {}, "colors": ["#FFFFFF40", "transparent"], "area": {{ "rect": [{}, {}, 60, 40] }} }}"##,
            p(50.0, 92.0),
            p(20.0, 40.0),
            p(80.0, 40.0),
            p(70.0, 80.0),
            p(30.0, 80.0),
            60.0 + dx,
            20.0 + dy,
            p(80.0, 30.0),
            p(92.0, 32.0),
            p(40.0, 60.0),
            30.0 + dx,
            45.0 + dy,
            p(0.0, 40.0),
            p(0.0, 80.0),
            20.0 + dx,
            40.0 + dy,
        )
    }

    fn with_duck(commands: &str) -> String {
        small(&format!(
            r##"{{ "cmd": "layer", "name": "Back" }}, {DUCK}, {commands}"##
        ))
        .replace(
            r##""red": "#E02020""##,
            r##""red": "#E02020", "yellow": "#F2C230", "beak": "#E08020""##,
        )
    }

    fn layer_names(document: &Document) -> Vec<String> {
        document
            .layers()
            .iter()
            .map(|l| l.name().to_owned())
            .collect()
    }

    #[test]
    fn using_a_symbol_paints_exactly_what_its_commands_would() {
        for (dx, dy) in [(0.0, 0.0), (64.0, 32.0)] {
            let used = play(&with_duck(&format!(
                r##"{{ "cmd": "use", "name": "duck", "translate": [{dx}, {dy}] }}"##
            )));
            let by_hand = play(&with_duck(&duck_by_hand(dx, dy)));
            assert_eq!(png(&used), png(&by_hand), "moved by ({dx}, {dy})");
            assert_eq!(layer_names(&used), layer_names(&by_hand));
        }
    }

    #[test]
    fn symbols_paint_on_their_layers_then_give_the_layer_back() {
        let document = play(&with_duck(
            r##"{ "cmd": "use", "name": "duck" },
                { "cmd": "use", "name": "duck", "translate": [100, 0] },
                { "cmd": "rect", "x": 0, "y": 150, "width": 10, "height": 10, "fill": "red" }"##,
        ));
        assert_eq!(
            layer_names(&document),
            ["Back", "Shadow", "Body"],
            "both ducks share the layers"
        );
        let back = layer_named(&document, "Back").unwrap();
        assert_eq!(document.active_id(), back);
        assert!(
            document.layer(back).unwrap().surface.pixel(5, 155).a > 0,
            "the rect went on Back"
        );
        let body = layer_named(&document, "Body").unwrap();
        assert!(
            document.layer(body).unwrap().surface.pixel(170, 70).a > 0,
            "the second duck"
        );
    }

    #[test]
    fn colours_can_be_swapped_for_one_use() {
        let document = play(&with_duck(
            r##"{ "cmd": "use", "name": "duck" },
                { "cmd": "use", "name": "duck", "translate": [100, 0], "colors": { "yellow": "#C04040", "#E08020": "#2040C0" } }"##,
        ));
        assert_eq!(
            seen(&document, 70, 30),
            Rgba8::new(0xF2, 0xC2, 0x30, 255),
            "the first duck"
        );
        assert_eq!(
            seen(&document, 170, 30),
            Rgba8::new(0xC0, 0x40, 0x40, 255),
            "red instead of yellow"
        );
        let beak = seen(&document, 187, 31);
        assert_eq!(
            beak,
            Rgba8::new(0x20, 0x40, 0xC0, 255),
            "swapped by its hex code"
        );
    }

    #[test]
    fn symbols_can_use_symbols_but_not_themselves() {
        let document = play(&with_duck(
            r##"{ "cmd": "define", "name": "pair", "commands": [
                    { "cmd": "use", "name": "duck" },
                    { "cmd": "use", "name": "duck", "translate": [100, 0], "mirror": "x", "origin": [50, 0] }
                ] },
                { "cmd": "use", "name": "pair", "translate": [0, 90], "colors": { "yellow": "#40A040" } }"##,
        ));
        assert_eq!(
            seen(&document, 70, 120),
            Rgba8::new(0x40, 0xA0, 0x40, 255),
            "the outer swap reaches inside"
        );

        let message = error(&with_duck(
            r##"{ "cmd": "define", "name": "a", "commands": [ { "cmd": "use", "name": "b" } ] },
                { "cmd": "define", "name": "b", "commands": [ { "cmd": "use", "name": "a" } ] },
                { "cmd": "use", "name": "a" }"##,
        ));
        assert!(
            message.starts_with(
                "command 6 (use \"a\") > 1 (use \"b\") > 1 (use \"a\"): symbol \"a\" uses itself (a > b > a)"
            ),
            "{message}"
        );
        let message = error(&with_duck(r##"{ "cmd": "use", "name": "goose" }"##));
        assert!(
            message.contains("no symbol called \"goose\" (defined so far: duck)"),
            "{message}"
        );
    }

    #[test]
    fn define_mistakes_are_found_while_reading() {
        let message = error(&with_duck(
            r##"{ "cmd": "define", "name": "duck", "commands": [] }"##,
        ));
        assert!(
            message.contains("already defined by command 3"),
            "{message}"
        );
        let message = error(&small(
            r##"{ "cmd": "group", "commands": [ { "cmd": "define", "name": "x", "commands": [] } ] }"##,
        ));
        assert!(
            message.contains("> 1 (define \"x\"): `define` only works at the top level"),
            "{message}"
        );
        // A mistake inside a symbol names the symbol's command.
        let message = error(&small(
            r##"{ "cmd": "define", "name": "x", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "nope" } ] },
                { "cmd": "use", "name": "x" }"##,
        ));
        assert!(
            message.starts_with("command 3 (use \"x\") > 1 (rect): unknown colour"),
            "{message}"
        );
    }

    #[test]
    fn repeat_draws_copies_with_the_step_added_each_time() {
        let repeated = play(&small(
            r##"{ "cmd": "repeat", "count": 4, "step": { "translate": [40, 0] }, "commands": [
                { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 30, "fill": "red" }
            ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 30, "fill": "red" },
                { "cmd": "rect", "x": 50, "y": 10, "width": 20, "height": 30, "fill": "red" },
                { "cmd": "rect", "x": 90, "y": 10, "width": 20, "height": 30, "fill": "red" },
                { "cmd": "rect", "x": 130, "y": 10, "width": 20, "height": 30, "fill": "red" }"##,
        ));
        assert_eq!(png(&repeated), png(&by_hand));

        // Petals: turned around the middle, a quarter turn each.
        let petals = play(&small(
            r##"{ "cmd": "repeat", "count": 4, "step": { "rotate": 90, "origin": [100, 100] }, "commands": [
                { "cmd": "ellipse", "center": [100, 60], "radius": [10, 30], "fill": "red" }
            ] }"##,
        ));
        let red = Rgba8::new(0xE0, 0x20, 0x20, 255);
        for (x, y) in [(100, 50), (150, 100), (100, 150), (50, 100)] {
            assert_eq!(seen(&petals, x, y), red, "a petal at {x},{y}");
        }
        assert_eq!(
            seen(&petals, 140, 60),
            Rgba8::WHITE,
            "nothing between the petals"
        );

        let message = error(&small(
            r#"{ "cmd": "repeat", "count": 0, "step": {}, "commands": [] }"#,
        ));
        assert!(message.contains("`count` must be from 1"), "{message}");
    }

    #[test]
    fn random_patterns_differ_between_copies_but_not_between_uses() {
        let scatter = r##"{ "cmd": "scatter", "area": { "rect": [0, 0, 40, 40] }, "count": 20, "size": 3, "color": "ink" }"##;
        // Two uses of the same symbol: the same pattern, moved.
        let document = play(&small(&format!(
            r##"{{ "cmd": "define", "name": "dots", "commands": [ {scatter} ] }},
                {{ "cmd": "use", "name": "dots" }},
                {{ "cmd": "use", "name": "dots", "translate": [100, 0] }},
                {{ "cmd": "repeat", "count": 2, "step": {{ "translate": [100, 0] }}, "commands": [
                    {{ "cmd": "group", "translate": [0, 100], "commands": [ {scatter} ] }}
                ] }}"##
        )));
        let region = |left: i32, top: i32| -> Vec<Rgba8> {
            (0..40)
                .flat_map(|y| (0..40).map(move |x| (x, y)))
                .map(|(x, y)| seen(&document, left + x, top + y))
                .collect()
        };
        assert_eq!(
            region(0, 0),
            region(100, 0),
            "uses of a symbol look the same"
        );
        assert_ne!(
            region(0, 100),
            region(100, 100),
            "copies of a repeat differ"
        );
    }

    // ---- Text ---------------------------------------------------------------

    /// Where the picture isn't plain paper: (how many pixels, left, top,
    /// right, bottom).
    fn painted(document: &Document) -> (usize, i32, i32, i32, i32) {
        let paper = seen(document, 0, 0);
        let mut found = (0, i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for y in 0..document.height() as i32 {
            for x in 0..document.width() as i32 {
                if seen(document, x, y) != paper {
                    found = (
                        found.0 + 1,
                        found.1.min(x),
                        found.2.min(y),
                        found.3.max(x),
                        found.4.max(y),
                    );
                }
            }
        }
        found
    }

    #[test]
    fn every_printable_character_paints_something_except_space() {
        for code in 0x21u8..=0x7E {
            let c = serde_json::to_string(&char::from(code).to_string()).unwrap();
            let document = play(&small(&format!(
                r##"{{ "cmd": "text", "text": {c}, "at": [100, 60], "size": 40, "color": "ink", "align": "center" }}"##
            )));
            assert!(painted(&document).0 > 0, "{c} painted nothing");
        }
        // A space alone is almost certainly a mistake.
        let problem = error(&small(r##"{ "cmd": "text", "text": " ", "at": [0, 0] }"##));
        assert!(problem.contains("`text` is empty"), "{problem}");
    }

    #[test]
    fn the_outline_goes_around_the_letters_not_over_them() {
        // A big "I" is a plain upright bar.
        let document = play(&small(
            r##"{ "cmd": "text", "text": "I", "at": [100, 10], "size": 150, "color": "red", "align": "center", "outline": "ink", "outline_size": 4 }"##,
        ));
        let red = Rgba8::new(0xE0, 0x20, 0x20, 255);
        let row: Vec<Rgba8> = (0..200).map(|x| seen(&document, x, 110)).collect();
        let first_red = row
            .iter()
            .position(|&p| p == red)
            .expect("the letter is filled");
        let last_red = row.iter().rposition(|&p| p == red).unwrap();
        // The letter's colour covers the middle of the bar completely...
        assert!(row[first_red..=last_red].iter().all(|&p| p == red));
        // ...and about 4 pixels of outline show on each side of it.
        let darker = |p: &&Rgba8| p.r < 0x80;
        let left = row[..first_red].iter().filter(darker).count();
        let right = row[last_red + 1..].iter().filter(darker).count();
        assert!(
            (3..=5).contains(&left) && (3..=5).contains(&right),
            "{left} {right}"
        );
    }

    #[test]
    fn text_moves_turns_and_mirrors_with_groups_and_symbols() {
        // Used from a symbol, moved: exactly as if written there.
        let used = play(&small(
            r##"{ "cmd": "define", "name": "label", "commands": [
                    { "cmd": "text", "text": "Hi, Duck!", "at": [10, 20], "size": 30, "color": "ink", "outline": "red" } ] },
                { "cmd": "use", "name": "label", "translate": [40, 100] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "text", "text": "Hi, Duck!", "at": [50, 120], "size": 30, "color": "ink", "outline": "red" }"##,
        ));
        assert_eq!(png(&used), png(&by_hand));

        // Mirrored left-right in a group: the same picture, flipped.
        let words = r##"{ "cmd": "text", "text": "Quack", "at": [100, 60], "size": 48, "color": "ink", "align": "center" }"##;
        let plain = play(&small(words));
        let mirrored = play(&small(&format!(
            r##"{{ "cmd": "group", "mirror": "x", "origin": [100, 0], "commands": [ {words} ] }}"##
        )));
        let mut most = 0;
        for y in 0..200 {
            for x in 0..200 {
                let (p, q) = (seen(&plain, x, y), seen(&mirrored, 199 - x, y));
                most = most.max(p.r.abs_diff(q.r)).max(p.a.abs_diff(q.a));
            }
        }
        assert!(most <= 2, "differs by {most}");

        // Turned 90 degrees around `at`: the line now runs downwards,
        // and the letters' tops face right, so it all lies left of `at`.
        let turned = play(&small(
            r##"{ "cmd": "text", "text": "HHHH", "at": [100, 20], "size": 30, "color": "ink", "rotate": 90 }"##,
        ));
        let (count, left, top, right, bottom) = painted(&turned);
        assert!(
            count > 0 && right <= 100 && top >= 20,
            "{left} {top} {right} {bottom}"
        );
        assert!(bottom - top > 2 * (right - left), "taller than wide");
    }

    #[test]
    fn text_mistakes_are_explained() {
        for (command, expected) in [
            (
                r#""align": "middle""#,
                "`align` is \"left\", \"center\" or \"right\", not \"middle\"",
            ),
            (r#""size": 0"#, "`size` must be more than 0"),
            (
                r#""outline_size": 3"#,
                "`outline_size` needs an `outline` colour too",
            ),
            (r#""color": "blue""#, "blue"),
        ] {
            let problem = error(&small(&format!(
                r#"{{ "cmd": "text", "text": "Hi", "at": [10, 10], {command} }}"#
            )));
            assert!(problem.contains(expected), "{problem}");
            assert!(problem.starts_with("command 2 (text)"), "{problem}");
        }
    }

    // ---- The report -----------------------------------------------------------

    fn report_with(text: &str, options: PlayOptions) -> Report {
        let mut player = Player::new(Script::parse(text).unwrap(), options).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        player.report()
    }

    fn report_of(text: &str) -> Report {
        report_with(text, PlayOptions::default())
    }

    #[test]
    fn named_groups_and_uses_report_where_they_drew() {
        let report = report_of(&small(
            r##"{ "cmd": "define", "name": "dot", "commands": [
                    { "cmd": "ellipse", "center": [0, 0], "radius": 5, "fill": "red" } ] },
                { "cmd": "define", "name": "pair", "commands": [
                    { "cmd": "use", "name": "dot" },
                    { "cmd": "use", "name": "dot", "translate": [20, 0] } ] },
                { "cmd": "group", "name": "box", "commands": [
                    { "cmd": "rect", "x": 10, "y": 20, "width": 50, "height": 30, "fill": "ink" } ] },
                { "cmd": "group", "commands": [
                    { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "ink" } ] },
                { "cmd": "use", "name": "pair", "translate": [100, 100] }"##,
        ));
        let parts: Vec<(&str, Option<PixelRect>)> = report
            .parts
            .iter()
            .map(|part| (part.path.as_str(), part.area))
            .collect();
        assert_eq!(
            parts,
            [
                (
                    r#"command 4 (group "box")"#,
                    Some(PixelRect::new(10, 20, 60, 50))
                ),
                // (The group without a name isn't listed.)
                (
                    r#"command 6 (use "pair")"#,
                    Some(PixelRect::new(95, 95, 125, 105))
                ),
                (
                    r#"command 6 (use "pair") > 1 (use "dot")"#,
                    Some(PixelRect::new(95, 95, 105, 105))
                ),
                (
                    r#"command 6 (use "pair") > 2 (use "dot")"#,
                    Some(PixelRect::new(115, 95, 125, 105))
                ),
            ]
        );
        // Brush widths count: a 10-pixel line reaches 5 pixels past its ends.
        let line = report_of(&small(
            r##"{ "cmd": "group", "name": "line", "commands": [
                { "cmd": "stroke", "points": [[50, 50], [100, 50]], "size": 10, "color": "ink" } ] }"##,
        ));
        assert_eq!(line.parts[0].area, Some(PixelRect::new(45, 45, 105, 55)));
    }

    #[test]
    fn boxes_are_in_canvas_pixels_in_previews_too() {
        let text = small(
            r##"{ "cmd": "group", "name": "box", "commands": [
                { "cmd": "rect", "x": 10, "y": 20, "width": 50, "height": 30, "fill": "ink" } ] }"##,
        );
        let half = report_with(
            &text,
            PlayOptions {
                scale: 0.5,
                ..PlayOptions::default()
            },
        );
        assert_eq!(half.parts[0].area, Some(PixelRect::new(10, 20, 60, 50)));
    }

    #[test]
    fn drawing_outside_the_canvas_is_reported_once() {
        let report = report_of(&small(
            r##"{ "cmd": "ellipse", "center": [300, 50], "radius": 10, "fill": "ink" },
                { "cmd": "rect", "x": 190, "y": 0, "width": 50, "height": 10, "fill": "ink" },
                { "cmd": "define", "name": "two", "commands": [
                    { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "red" },
                    { "cmd": "rect", "x": 20, "y": 0, "width": 10, "height": 10, "fill": "red" } ] },
                { "cmd": "use", "name": "two", "translate": [-100, 0] }"##,
        ));
        assert_eq!(
            report.warnings,
            [
                "command 2 (ellipse) draws entirely outside the canvas (x 290..310, y 40..60)",
                // (The rect half on the canvas is fine.)
                r#"command 5 (use "two") draws entirely outside the canvas (x -100..-70, y 0..10)"#,
            ]
        );
    }

    #[test]
    fn unused_symbols_and_palette_colours_are_reported() {
        let report = report_of(&small(
            r##"{ "cmd": "define", "name": "tree", "commands": [
                    { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "red" } ] },
                { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "ink" }"##,
        ));
        assert_eq!(
            unused_warnings(&[&report]),
            [
                r#"symbol "tree" (command 2) is defined but never used"#,
                // Only used inside the symbol that's never used.
                r#"palette colour "red" is never used"#,
            ]
        );
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn use_in_any_frame_of_any_animation_counts() {
        // "blink" is only used at frame 3 (through a keyframed parameter),
        // and "red" only in the second animation.
        let text = r##"{ "palette": { "ink": "#202020", "red": "#E02020", "blue": "#2020E0" }, "commands": [
            { "cmd": "canvas", "width": 50, "height": 50 },
            { "cmd": "define", "name": "open", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 4, "fill": "ink" } ] },
            { "cmd": "define", "name": "blink", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 1, "fill": "ink" } ] },
            { "cmd": "define", "name": "never", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 1, "fill": "blue" } ] },
            { "cmd": "define", "name": "face", "params": { "eye": "open" }, "commands": [
                { "cmd": "use", "name": "$eye", "translate": [10, 10] } ] }
          ], "animations": {
            "idle": { "frames": 4, "commands": [
                { "cmd": "use", "name": "face", "with": { "eye": { "keys": [[0, "open"], [3, "blink"]] } } } ] },
            "hurt": { "frames": 1, "commands": [
                { "cmd": "rect", "x": 20, "y": 20, "width": 4, "height": 4, "fill": "red" } ] }
          } }"##;
        let mut reports = Vec::new();
        for (animation, frames) in [("idle", 4), ("hurt", 1)] {
            for frame in 0..frames {
                let mut player = Player::new(
                    Script::parse(text).unwrap(),
                    PlayOptions {
                        animation: Some(animation.to_owned()),
                        frame,
                        ..PlayOptions::default()
                    },
                )
                .unwrap();
                let mut document = player.create_document().unwrap();
                player.run(&mut document, |_, _| {}).unwrap();
                reports.push(player.report());
            }
        }
        let all: Vec<&Report> = reports.iter().collect();
        assert_eq!(
            unused_warnings(&all),
            [
                r#"symbol "never" (command 4) is defined but never used"#,
                r#"palette colour "blue" is never used"#
            ]
        );
        // Frame 0 alone would have blamed "blink" and "red" too.
        assert_eq!(unused_warnings(&all[..1]).len(), 4);
    }

    #[test]
    fn text_in_a_repeat_can_use_the_copy_number() {
        let repeated = play(&small(
            r##"{ "cmd": "repeat", "count": 3, "step": { "translate": [60, 0] }, "commands": [
                { "cmd": "text", "text": "f{i}/{i0}", "at": [10, 10], "size": 20, "color": "ink" } ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "text", "text": "f1/0", "at": [10, 10], "size": 20, "color": "ink" },
                { "cmd": "text", "text": "f2/1", "at": [70, 10], "size": 20, "color": "ink" },
                { "cmd": "text", "text": "f3/2", "at": [130, 10], "size": 20, "color": "ink" }"##,
        ));
        assert_eq!(png(&repeated), png(&by_hand));

        // From a symbol used in a repeat, too.
        let used = play(&small(
            r##"{ "cmd": "define", "name": "label", "commands": [
                    { "cmd": "text", "text": "frame {i}", "at": [0, 0], "size": 20, "color": "ink" } ] },
                { "cmd": "repeat", "count": 2, "step": { "translate": [0, 50] }, "commands": [
                    { "cmd": "use", "name": "label", "translate": [10, 10] } ] }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "text", "text": "frame 1", "at": [10, 10], "size": 20, "color": "ink" },
                { "cmd": "text", "text": "frame 2", "at": [10, 60], "size": 20, "color": "ink" }"##,
        ));
        assert_eq!(png(&used), png(&by_hand));

        let problem = error(&small(
            r##"{ "cmd": "text", "text": "frame {i}", "at": [0, 0] }"##,
        ));
        assert!(problem.contains("only work inside a `repeat`"), "{problem}");
    }

    #[test]
    fn parts_know_what_they_are_inside() {
        let report = report_of(&small(
            r##"{ "cmd": "define", "name": "dot", "commands": [
                    { "cmd": "ellipse", "center": [0, 0], "radius": 5, "fill": "red" } ] },
                { "cmd": "group", "name": "row", "commands": [
                    { "cmd": "use", "name": "dot" },
                    { "cmd": "group", "commands": [ { "cmd": "use", "name": "dot" } ] } ] },
                { "cmd": "use", "name": "dot" }"##,
        ));
        let inside: Vec<Option<usize>> = report.parts.iter().map(|p| p.inside).collect();
        // The row; two dots in it (one through a group without a name); a dot alone.
        assert_eq!(inside, [None, Some(0), Some(0), None]);
    }

    #[test]
    fn thin_lines_fade_instead_of_vanishing_in_previews() {
        // How dark a straight 1-pixel line is at its darkest, in a preview
        // at `scale` (0 = paper, 255 = black).
        let darkest = |scale: f32, size: f32| {
            let text = format!(
                r##"{{ "commands": [
                    {{ "cmd": "canvas", "width": 800, "height": 400, "paper": "#FFFFFF" }},
                    {{ "cmd": "line", "from": [100, 101], "to": [700, 101], "size": {size}, "color": "#000000", "taper": 0 }}
                ] }}"##
            );
            let document = play_with(
                &text,
                PlayOptions {
                    scale,
                    ..PlayOptions::default()
                },
            )
            .unwrap();
            let x = (400.0 * scale) as i32;
            (0..document.height() as i32)
                .map(|y| 255 - seen(&document, x, y).r)
                .max()
                .unwrap()
        };
        let full = darkest(1.0, 1.0);
        // Shrunk 4 times, a 1-pixel line covers about a quarter of a pixel:
        // it should look about a quarter as dark, not disappear.
        let quarter = darkest(0.25, 1.0);
        assert!(
            (40..=100).contains(&quarter),
            "{quarter} (full size: {full})"
        );
        let half = darkest(0.5, 1.0);
        assert!((90..=170).contains(&half), "{half} (full size: {full})");
    }

    #[test]
    fn the_report_says_which_commands_painted_each_layer() {
        let text = small(
            r##"{ "cmd": "layer", "name": "A" },
                { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "ink" },
                { "cmd": "layer", "name": "B" },
                { "cmd": "group", "commands": [
                    { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "ink" },
                    { "cmd": "layer", "name": "A" },
                    { "cmd": "ellipse", "center": [50, 50], "radius": 5, "fill": "red" } ] },
                { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "ink" },
                { "cmd": "clear_layer", "name": "A" },
                { "cmd": "layer", "name": "A" },
                { "cmd": "rect", "x": 0, "y": 0, "width": 10, "height": 10, "fill": "ink" }"##,
        );
        let mut player =
            Player::new(Script::parse(&text).unwrap(), PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let report = player.report();
        let on = |name: &str| report.painted_by[&layer_named(&document, name).unwrap()].clone();
        assert_eq!(on("B"), [5, 6]);
        // Commands 3 and 5 painted on A, but it was cleared by command 7.
        assert_eq!(on("A"), [9]);
    }

    #[test]
    fn transform_layer_moves_what_is_already_painted() {
        let rect =
            r##"{ "cmd": "rect", "x": 10, "y": 20, "width": 30, "height": 40, "fill": "red" }"##;
        let flipped = play(&small(&format!(
            r##"{rect}, {{ "cmd": "transform_layer", "mirror": "x", "origin": [100, 100] }}"##
        )));
        // The same rectangle, drawn where the flip puts it.
        let by_hand = play(&small(
            r##"{ "cmd": "rect", "x": 160, "y": 20, "width": 30, "height": 40, "fill": "red" }"##,
        ));
        assert_eq!(png(&flipped), png(&by_hand));

        // Named, and in a preview: the same, shrunk.
        let text = small(&format!(
            r##"{{ "cmd": "layer", "name": "A" }}, {rect},
                {{ "cmd": "layer", "name": "B" }},
                {{ "cmd": "transform_layer", "name": "A", "translate": [100, 0] }}"##
        ));
        let half = play_with(
            &text,
            PlayOptions {
                scale: 0.5,
                ..PlayOptions::default()
            },
        )
        .unwrap();
        let red = |x, y| seen(&half, x, y).r > 200 && seen(&half, x, y).g < 80;
        assert!(red(60, 20) && !red(15, 20), "moved 50 preview pixels right");

        for (fields, expected) in [
            (r#""name": "Nope""#, "no layer called \"Nope\""),
            (r#""scale": 0"#, "`scale` is a number above 0"),
            (r#""mirror": "z""#, "`mirror` is"),
        ] {
            let problem = error(&small(&format!(
                r#"{{ "cmd": "transform_layer", {fields} }}"#
            )));
            assert!(problem.contains(expected), "{problem}");
        }
    }

    // ---- Animations -----------------------------------------------------------

    /// A square that an animation moves across the picture by hand (one
    /// `use` per frame, until keyframes arrive in step 2).
    const ANIMATED: &str = r##"{ "palette": { "ink": "#202020", "red": "#E02020" }, "commands": [
        { "cmd": "canvas", "width": 200, "height": 100 },
        { "cmd": "define", "name": "box", "commands": [
            { "cmd": "rect", "x": 0, "y": 0, "width": 20, "height": 20, "fill": "red" } ] },
        { "cmd": "rect", "x": 0, "y": 80, "width": 200, "height": 20, "fill": "ink" }
      ], "animations": {
        "slide": { "frames": 3, "fps": 6, "loop": true, "commands": [
            { "cmd": "use", "name": "box", "translate": [10, 10] } ] },
        "still": { "frames": 1, "commands": [] }
      } }"##;

    fn play_frame(
        text: &str,
        animation: Option<&str>,
        frame: u32,
    ) -> Result<Document, ScriptError> {
        play_with(
            text,
            PlayOptions {
                animation: animation.map(str::to_owned),
                frame,
                ..PlayOptions::default()
            },
        )
    }

    #[test]
    fn an_animation_plays_the_scripts_commands_then_its_own() {
        let script = Script::parse(ANIMATED).unwrap();
        assert_eq!(script.animations.len(), 2);
        let slide = &script.animations["slide"];
        assert_eq!((slide.frames, slide.fps, slide.looping), (3, 6.0, true));
        assert_eq!(script.animations["still"].fps, 12.0, "the default");

        let frame = play_frame(ANIMATED, Some("slide"), 2).unwrap();
        let by_hand = play(
            &small(
                r##"{ "cmd": "rect", "x": 0, "y": 80, "width": 200, "height": 20, "fill": "ink" },
                { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "red" }"##,
            )
            .replace("\"height\": 200 }", "\"height\": 100 }"),
        );
        assert_eq!(png(&frame), png(&by_hand));
        // "still" draws only the script's own commands; so does the first
        // animation when none is named.
        let still = play_frame(ANIMATED, Some("still"), 0).unwrap();
        assert_eq!(seen(&still, 15, 15), seen(&still, 100, 15), "no box");
        let first = play_frame(ANIMATED, None, 0).unwrap();
        assert_eq!(
            png(&first),
            png(&play_frame(ANIMATED, Some("slide"), 0).unwrap())
        );
    }

    #[test]
    fn animation_mistakes_say_where_they_are() {
        let problem = |text: &str| Script::parse(text).err().map(|e| e.to_string());
        assert_eq!(
            problem(&ANIMATED.replace("\"frames\": 3", "\"frames\": 0")),
            Some("animation \"slide\" > `frames` must be from 1 to 100000, not 0".into())
        );
        assert_eq!(
            problem(&ANIMATED.replace("\"fps\": 6", "\"fps\": 0")),
            Some("animation \"slide\" > `fps` must be above 0 and at most 1000, not 0".into())
        );
        let define_inside = ANIMATED.replace(
            r#"{ "cmd": "use", "name": "box", "translate": [10, 10] }"#,
            r#"{ "cmd": "define", "name": "x", "commands": [] }"#,
        );
        assert!(
            problem(&define_inside)
                .unwrap()
                .starts_with("animation \"slide\" > command 1 (define \"x\"): `define` only works"),
        );
        // Mistakes while playing, too.
        let wrong = ANIMATED.replace(
            "\"name\": \"box\", \"translate\"",
            "\"name\": \"bix\", \"translate\"",
        );
        let error = play_frame(&wrong, Some("slide"), 0)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with("animation \"slide\" > command 1 (use \"bix\"): there's no symbol"),
            "{error}"
        );
        let error = play_frame(ANIMATED, Some("jump"), 0)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "there's no animation called \"jump\" (animations: slide, still)"
        );
        let error = play_frame(ANIMATED, Some("slide"), 3)
            .unwrap_err()
            .to_string();
        assert_eq!(
            error,
            "animation \"slide\" has 3 frames (0 to 2), so there's no frame 3"
        );
        let plain =
            small(r##"{ "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "ink" }"##);
        let error = play_frame(&plain, Some("slide"), 0)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "this script has no animations");
    }

    #[test]
    fn animations_are_written_back_and_read_again() {
        let script = Script::parse(ANIMATED).unwrap();
        let again = Script::parse(&script.to_json()).unwrap();
        assert_eq!(again, script);
        assert_eq!(again.pivot, None);
        let with_pivot =
            ANIMATED.replace("\"animations\"", "\"pivot\": [100, 100], \"animations\"");
        let script = Script::parse(&with_pivot).unwrap();
        assert_eq!(script.pivot, Some([100.0, 100.0]));
        assert_eq!(Script::parse(&script.to_json()).unwrap(), script);
        // The report says where the animation's parts are.
        let mut player = Player::new(script, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        assert_eq!(
            player.report().parts[0].path,
            "animation \"slide\" > command 1 (use \"box\")"
        );
        assert_eq!(player.animation(), Some("slide"));
        assert_eq!(player.frame_number(), 0);
    }

    // ---- Keyframes, parameters and upright --------------------------------------

    /// An 8-frame loop where a box slides and a leg symbol turns.
    fn keyed(animation_commands: &str) -> String {
        format!(
            r##"{{ "palette": {{ "ink": "#202020", "red": "#E02020", "leg": "#E08020" }}, "commands": [
                {{ "cmd": "canvas", "width": 200, "height": 200 }},
                {{ "cmd": "define", "name": "box", "commands": [
                    {{ "cmd": "rect", "x": -10, "y": -10, "width": 20, "height": 20, "fill": "red" }} ] }},
                {{ "cmd": "define", "name": "foot", "commands": [
                    {{ "cmd": "rect", "x": -4, "y": 0, "width": 16, "height": 5, "fill": "ink" }} ] }},
                {{ "cmd": "define", "name": "leg", "params": {{ "angle": 0, "foot": "foot" }}, "commands": [
                    {{ "cmd": "group", "name": "thigh", "origin": [0, 0], "rotate": "$angle", "commands": [
                        {{ "cmd": "line", "from": [0, 0], "to": [0, 30], "size": 4, "color": "leg", "taper": 0 }},
                        {{ "cmd": "use", "name": "$foot", "translate": [0, 30], "upright": true }} ] }} ] }}
              ], "animations": {{ "walk": {{ "frames": 8, "fps": 8, "loop": true, "commands": [ {animation_commands} ] }} }} }}"##
        )
    }

    fn frame_of(text: &str, frame: u32) -> Document {
        play_frame(text, None, frame).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn in_between_frames_are_exactly_right() {
        let sliding = keyed(
            r##"{ "cmd": "use", "name": "box", "translate": { "keys": [[0, [20, 100]], [4, [100, 60]], [8, [20, 100]]] },
                 "rotate": { "keys": [[0, 0], [8, 90]], "ease": "in" } }"##,
        );
        // Frame 2 is halfway to the second key: (60, 80); and in-eased
        // rotation at t = 0.25 is 90 * 0.0625 = 5.625 degrees.
        let by_hand = play(&small(
            r##"{ "cmd": "define", "name": "box", "commands": [
                    { "cmd": "rect", "x": -10, "y": -10, "width": 20, "height": 20, "fill": "red" } ] },
                { "cmd": "use", "name": "box", "translate": [60, 80], "rotate": 5.625 }"##,
        ));
        assert_eq!(png(&frame_of(&sliding, 2)), png(&by_hand));
        // Frame 6: three quarters round: (60, 80) again, turned 90 * 0.5625.
        let by_hand = play(&small(
            r##"{ "cmd": "define", "name": "box", "commands": [
                    { "cmd": "rect", "x": -10, "y": -10, "width": 20, "height": 20, "fill": "red" } ] },
                { "cmd": "use", "name": "box", "translate": [60, 80], "rotate": 50.625 }"##,
        ));
        assert_eq!(png(&frame_of(&sliding, 6)), png(&by_hand));
    }

    #[test]
    fn parameters_paint_the_same_pixels_as_the_pose_written_by_hand() {
        let with_param = keyed(
            r##"{ "cmd": "use", "name": "leg", "translate": [100, 100], "with": { "angle": 25 } }"##,
        );
        let by_hand = play(&small(
            r##"{ "cmd": "define", "name": "foot", "commands": [
                    { "cmd": "rect", "x": -4, "y": 0, "width": 16, "height": 5, "fill": "ink" } ] },
                { "cmd": "group", "translate": [100, 100], "commands": [
                    { "cmd": "group", "name": "thigh", "origin": [0, 0], "rotate": 25, "commands": [
                        { "cmd": "line", "from": [0, 0], "to": [0, 30], "size": 4, "color": "#E08020", "taper": 0 },
                        { "cmd": "use", "name": "foot", "translate": [0, 30], "upright": true } ] } ] }"##,
        ));
        assert_eq!(png(&frame_of(&with_param, 3)), png(&by_hand));
        // The default (angle 0) when `with` doesn't say; and a keyframed
        // parameter at a known frame (keys 0 and 8: frame 4 is halfway).
        let defaulted = keyed(r##"{ "cmd": "use", "name": "leg", "translate": [100, 100] }"##);
        let keyed_param = keyed(
            r##"{ "cmd": "use", "name": "leg", "translate": [100, 100], "with": { "angle": { "keys": [[0, 0], [8, 50]] } } }"##,
        );
        assert_eq!(
            png(&frame_of(&defaulted, 5)),
            png(&frame_of(&keyed_param, 0))
        );
        assert_eq!(
            png(&frame_of(&with_param, 0)),
            png(&frame_of(&keyed_param, 4))
        );
    }

    #[test]
    fn upright_parts_move_with_the_leg_but_stay_flat() {
        // With the leg turned 90 degrees clockwise (down becomes left), the
        // ankle (0, 30) lands at (-30, 0) from the hip: the foot sits
        // there, still flat.
        let turned = keyed(
            r##"{ "cmd": "use", "name": "leg", "translate": [100, 100], "with": { "angle": 90 } }"##,
        );
        let by_hand = play(&small(
            r##"{ "cmd": "line", "from": [100, 100], "to": [70, 100], "size": 4, "color": "#E08020", "taper": 0 },
                { "cmd": "rect", "x": 66, "y": 100, "width": 16, "height": 5, "fill": "ink" }"##,
        ));
        assert!(largest_difference(&frame_of(&turned, 0), &by_hand) <= 2);
        // Mirrored from outside, the foot is mirrored too (and still flat).
        let mirrored = keyed(
            r##"{ "cmd": "group", "mirror": "x", "origin": [100, 0], "commands": [
                { "cmd": "use", "name": "leg", "translate": [100, 100], "with": { "angle": 90 } } ] }"##,
        );
        let by_hand = play(&small(
            r##"{ "cmd": "line", "from": [100, 100], "to": [130, 100], "size": 4, "color": "#E08020", "taper": 0 },
                { "cmd": "rect", "x": 118, "y": 100, "width": 16, "height": 5, "fill": "ink" }"##,
        ));
        assert!(largest_difference(&frame_of(&mirrored, 0), &by_hand) <= 2);
    }

    #[test]
    fn shift_plays_the_far_leg_half_a_cycle_later() {
        let legs = keyed(
            r##"{ "cmd": "use", "name": "leg", "translate": [60, 100], "with": { "angle": { "keys": [[0, -30], [4, 30], [8, -30]] } } },
                { "cmd": "use", "name": "leg", "translate": [140, 100], "with": { "angle": { "keys": [[0, -30], [4, 30], [8, -30]], "shift": 4 } } }"##,
        );
        // At frame 1 the near leg is at -15, the far leg at the keys'
        // frame 5 (wrapped): 15.
        let by_hand = keyed(
            r##"{ "cmd": "use", "name": "leg", "translate": [60, 100], "with": { "angle": -15 } },
                { "cmd": "use", "name": "leg", "translate": [140, 100], "with": { "angle": 15 } }"##,
        );
        assert_eq!(png(&frame_of(&legs, 1)), png(&frame_of(&by_hand, 0)));
    }

    #[test]
    fn parameter_and_keyframe_mistakes_are_explained() {
        let problem = |commands: &str| {
            play_frame(&keyed(commands), None, 0)
                .unwrap_err()
                .to_string()
        };
        let error = problem(r##"{ "cmd": "use", "name": "leg", "with": { "angel": 5 } }"##);
        assert!(
            error.contains("no parameter called \"angel\" (it has: angle, foot)"),
            "{error}"
        );
        let error = problem(r##"{ "cmd": "use", "name": "box", "with": { "angle": 5 } }"##);
        assert!(
            error.contains("symbol \"box\" has no parameters"),
            "{error}"
        );
        let error = problem(r##"{ "cmd": "group", "rotate": "$angle", "commands": [] }"##);
        assert!(error.contains("only works inside a symbol"), "{error}");
        let error =
            problem(r##"{ "cmd": "use", "name": "box", "rotate": { "keys": [[4, 1], [2, 0]] } }"##);
        assert!(
            error.starts_with(
                "animation \"walk\" > command 1 (use \"box\"): keys must be in frame order"
            ),
            "{error}"
        );
        let error =
            problem(r##"{ "cmd": "use", "name": "box", "translate": { "keys": [[0, 5]] } }"##);
        assert!(error.contains("`translate` must be [x, y]"), "{error}");
    }

    // ---- Pixel canvases -------------------------------------------------------

    fn pixel_script(canvas_extra: &str, commands: &str) -> String {
        format!(
            r##"{{ "palette": {{ "ink": "#202020", "red": "#E02020", "blue": "#2020E0" }}, "commands": [
                {{ "cmd": "canvas", "width": 16, "height": 16, "pixel": {canvas_extra} }}, {commands} ] }}"##
        )
    }

    /// Every pixel as a straight colour.
    fn pixel_grid(document: &Document) -> Vec<Vec<[u8; 4]>> {
        (0..document.height() as i32)
            .map(|y| {
                (0..document.width() as i32)
                    .map(|x| {
                        crate::image::unpremultiply(document.active_layer().surface.pixel(x, y))
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_pixel_canvas_paints_whole_crisp_pixels_in_palette_colours() {
        let document = play(&pixel_script(
            "true",
            r##"{ "cmd": "ellipse", "center": [8, 8], "radius": 5.5, "fill": "red", "outline": "ink", "outline_size": 1 },
                { "cmd": "line", "from": [0, 0], "to": [15, 15], "size": 1, "color": "blue", "taper": 0 }"##,
        ));
        assert_eq!(
            (document.width(), document.height()),
            (16, 16),
            "shrunk to its real size"
        );
        let grid = pixel_grid(&document);
        let palette = [
            [0x20, 0x20, 0x20, 255],
            [0xE0, 0x20, 0x20, 255],
            [0x20, 0x20, 0xE0, 255],
        ];
        let mut painted = 0;
        for row in &grid {
            for &p in row {
                assert!(
                    p[3] == 0 || p[3] == 255,
                    "no half see-through pixels: {p:?}"
                );
                assert!(
                    p[3] == 0 || palette.contains(&p),
                    "only palette colours: {p:?}"
                );
                painted += usize::from(p[3] == 255);
            }
        }
        assert!(painted > 80 && painted < 130, "{painted} pixels painted");
        assert_eq!(
            grid[8][8],
            [0x20, 0x20, 0xE0, 255],
            "the line runs through the middle"
        );
        assert_eq!(grid[8][5], [0xE0, 0x20, 0x20, 255], "beside it, red");
        assert_eq!(
            grid[8][2],
            [0x20, 0x20, 0x20, 255],
            "the edge is the outline"
        );
    }

    #[test]
    fn hex_colours_are_refused_on_a_pixel_canvas() {
        let problem = error(&pixel_script(
            "true",
            r##"{ "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 4, "fill": "#FF00FF" }"##,
        ));
        assert!(
            problem.contains("every colour is a palette name"),
            "{problem}"
        );
        let rect = r##"{ "cmd": "rect", "x": 0, "y": 0, "width": 4, "height": 4, "fill": "red" }"##;
        let problem = Script::parse(&pixel_script("{ \"outline\": \"gold\" }", rect))
            .map(|s| Player::new(s, PlayOptions::default()).err());
        let problem = problem.unwrap().unwrap().to_string();
        assert!(problem.contains("no \"gold\""), "{problem}");
        let no_palette =
            r##"{ "commands": [ { "cmd": "canvas", "width": 8, "height": 8, "pixel": true } ] }"##;
        let problem = Player::new(Script::parse(no_palette).unwrap(), PlayOptions::default())
            .err()
            .unwrap()
            .to_string();
        assert!(problem.contains("needs a \"palette\""), "{problem}");
    }

    #[test]
    fn pixel_rows_paint_one_pixel_per_character() {
        let document = play(&pixel_script(
            "true",
            r##"{ "cmd": "pixels", "at": [2, 3], "colors": { "r": "red", "k": "ink" }, "rows": [
                ".rr.", "rkkr", "....", "r..r" ] }"##,
        ));
        let grid = pixel_grid(&document);
        let red = [0xE0, 0x20, 0x20, 255];
        let ink = [0x20, 0x20, 0x20, 255];
        assert_eq!(grid[3][2], [0, 0, 0, 0]);
        assert_eq!(grid[3][3], red);
        assert_eq!(grid[4][3], ink);
        assert_eq!(grid[4][5], red);
        assert_eq!(grid[5][3], [0, 0, 0, 0]);
        assert_eq!(grid[6][2], red);
        assert_eq!(grid[6][3], [0, 0, 0, 0]);
        let painted: usize = grid.iter().flatten().filter(|p| p[3] == 255).count();
        assert_eq!(painted, 8);
        let problem = error(&pixel_script(
            "true",
            r##"{ "cmd": "pixels", "at": [0, 0], "colors": { "r": "red" }, "rows": ["rx"] }"##,
        ));
        assert!(problem.contains("row 1 has \"x\""), "{problem}");
    }

    #[test]
    fn the_outline_option_and_supersample_are_read_from_the_canvas() {
        let document = play(&pixel_script(
            r##"{ "supersample": 8, "outline": "ink" }"##,
            r##"{ "cmd": "rect", "x": 4, "y": 4, "width": 6, "height": 6, "fill": "red" }"##,
        ));
        let grid = pixel_grid(&document);
        assert_eq!(grid[4][4], [0xE0, 0x20, 0x20, 255]);
        assert_eq!(grid[3][4], [0x20, 0x20, 0x20, 255], "outline above");
        assert_eq!(grid[4][10], [0x20, 0x20, 0x20, 255], "outline right");
        assert_eq!(grid[3][3], [0, 0, 0, 0], "not the corner");
    }

    #[test]
    fn pixel_art_moves_by_whole_pixels_when_animated() {
        let moved = |dx: f32| {
            let text = format!(
                r##"{{ "palette": {{ "red": "#E02020" }}, "commands": [
                    {{ "cmd": "canvas", "width": 16, "height": 8, "pixel": true }} ],
                  "animations": {{ "slide": {{ "frames": 1, "commands": [
                    {{ "cmd": "group", "translate": [{dx}, 0], "commands": [
                        {{ "cmd": "rect", "x": 2, "y": 2, "width": 3, "height": 3, "fill": "red" }} ] }} ] }} }} }}"##
            );
            pixel_grid(&play_frame(&text, None, 0).unwrap())
        };
        assert_eq!(moved(2.4), moved(2.0), "2.4 pixels is 2 pixels");
        assert_eq!(moved(2.6), moved(3.0), "2.6 pixels is 3 pixels");
        assert_ne!(moved(2.0), moved(3.0));
        let painted =
            |grid: &Vec<Vec<[u8; 4]>>| grid.iter().flatten().filter(|p| p[3] == 255).count();
        assert_eq!(painted(&moved(2.4)), 9, "still exactly 3 x 3");
    }

    // ---- Phase 1 leftovers --------------------------------------------------------

    #[test]
    fn fit_shrinks_text_to_a_width_and_never_enlarges_it() {
        let wide = play(&small(
            r##"{ "cmd": "text", "text": "A rather long label", "at": [0, 10], "size": 40, "color": "ink", "fit": 100 }"##,
        ));
        let area = wide.active_layer().surface.painted_area().unwrap();
        assert!(area.right - area.left <= 101, "{area:?}");
        assert!(area.right - area.left > 90, "{area:?}");
        // Already narrow: the same as without `fit`.
        let short = play(&small(
            r##"{ "cmd": "text", "text": "Hi", "at": [0, 10], "size": 40, "color": "ink", "fit": 500 }"##,
        ));
        let plain = play(&small(
            r##"{ "cmd": "text", "text": "Hi", "at": [0, 10], "size": 40, "color": "ink" }"##,
        ));
        assert_eq!(png(&short), png(&plain));
    }

    #[test]
    fn each_gives_every_copy_of_a_repeat_its_own_values() {
        let row = play(&small(
            r##"{ "cmd": "repeat", "count": 4, "step": { "translate": [30, 0] },
                 "each": { "color": ["red", "ink"], "size": [10, 20, 30] }, "commands": [
                { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "$color" },
                { "cmd": "group", "translate": [0, "$size"], "commands": [
                    { "cmd": "rect", "x": 10, "y": 60, "width": 20, "height": 5, "fill": "$color" } ] } ] }"##,
        ));
        // Colours go round: red, ink, red, ink; sizes 10, 20, 30, then 10 again.
        let by_hand = play(&small(
            r##"{ "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "red" },
                { "cmd": "rect", "x": 10, "y": 70, "width": 20, "height": 5, "fill": "red" },
                { "cmd": "rect", "x": 40, "y": 10, "width": 20, "height": 20, "fill": "ink" },
                { "cmd": "rect", "x": 40, "y": 80, "width": 20, "height": 5, "fill": "ink" },
                { "cmd": "rect", "x": 70, "y": 10, "width": 20, "height": 20, "fill": "red" },
                { "cmd": "rect", "x": 70, "y": 90, "width": 20, "height": 5, "fill": "red" },
                { "cmd": "rect", "x": 100, "y": 10, "width": 20, "height": 20, "fill": "ink" },
                { "cmd": "rect", "x": 100, "y": 70, "width": 20, "height": 5, "fill": "ink" }"##,
        ));
        assert_eq!(png(&row), png(&by_hand));
        let problem = error(&small(
            r##"{ "cmd": "repeat", "count": 2, "step": {}, "each": { "color": [] }, "commands": [] }"##,
        ));
        assert!(
            problem.contains("`each`: \"color\" needs a list"),
            "{problem}"
        );
    }

    #[test]
    fn a_symbol_parameter_can_be_a_colour() {
        let used = play(&small(
            r##"{ "cmd": "define", "name": "dot", "params": { "paint": "red" }, "commands": [
                    { "cmd": "ellipse", "center": [50, 50], "radius": 10, "fill": "$paint", "outline": "$paint" } ] },
                { "cmd": "use", "name": "dot", "with": { "paint": "#2020E0" } }"##,
        ));
        let by_hand = play(&small(
            r##"{ "cmd": "ellipse", "center": [50, 50], "radius": 10, "fill": "#2020E0", "outline": "#2020E0" }"##,
        ));
        assert_eq!(png(&used), png(&by_hand));
        let problem = error(&small(
            r##"{ "cmd": "define", "name": "dot", "params": { "paint": 5 }, "commands": [
                    { "cmd": "ellipse", "center": [50, 50], "radius": 10, "fill": "$paint" } ] },
                { "cmd": "use", "name": "dot" }"##,
        ));
        assert!(
            problem.contains("\"$paint\" must be a colour, not 5"),
            "{problem}"
        );
    }

    // ---- Clipping, the alpha lock and masks (phase 3, step 1) ----------------

    #[test]
    fn a_clipping_layer_shows_only_on_its_base() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "layer", "name": "Base" },
              { "cmd": "rect", "x": 20, "y": 20, "width": 40, "height": 40, "fill": "#0000FF" },
              { "cmd": "layer", "name": "Shade", "clip": true },
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#FF0000" },
              { "cmd": "layer", "name": "Light", "clip": true, "blend": "screen" },
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 50, "fill": "#00FF00" }
            ] }"##,
        );
        assert!(document.layers()[1].clip() && document.layers()[2].clip());
        assert!(!document.layers()[0].clip());
        // Inside the base: red, and lighter green up top; outside: paper.
        assert_eq!(seen(&document, 40, 55), Rgba8::new(255, 0, 0, 255));
        assert_eq!(seen(&document, 40, 30), Rgba8::new(255, 255, 0, 255));
        assert_eq!(seen(&document, 80, 80), Rgba8::WHITE);
        assert_eq!(seen(&document, 80, 30), Rgba8::WHITE);
        // The paint itself isn't limited: it's there, just not shown.
        assert_eq!(
            document.layers()[1].surface.pixel(80, 80),
            Rgba8::new(255, 0, 0, 255)
        );
    }

    #[test]
    fn a_locked_layer_only_recolours_what_is_there() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "layer", "name": "Duck" },
              { "cmd": "rect", "x": 20, "y": 20, "width": 40, "height": 40, "fill": "#0000FF" },
              { "cmd": "layer", "name": "Duck", "lock": true },
              { "cmd": "gradient", "from": [0, 0], "to": [100, 0], "colors": ["#FF0000", "#FF0000"] },
              { "cmd": "erase", "points": [[20, 40], [60, 40]], "size": 10 },
              { "cmd": "layer", "name": "Duck", "lock": false },
              { "cmd": "stroke", "points": [[0, 90], [100, 90]], "size": 4, "color": "#00FF00" }
            ] }"##,
        );
        assert_eq!(
            seen(&document, 40, 40),
            Rgba8::new(255, 0, 0, 255),
            "recoloured"
        );
        assert_eq!(seen(&document, 80, 80), Rgba8::WHITE, "no new paint");
        assert_eq!(
            seen(&document, 50, 90),
            Rgba8::new(0, 255, 0, 255),
            "unlocked again"
        );
        assert!(!document.layers()[0].lock());
    }

    #[test]
    fn a_group_mask_limits_painting_and_moves_with_the_group() {
        let masked = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "group", "name": "spot", "translate": [50, 0],
                "mask": { "circle": [0, 50, 20] }, "commands": [
                { "cmd": "rect", "x": -100, "y": 0, "width": 200, "height": 100, "fill": "#FF0000" },
                { "cmd": "stroke", "points": [[-50, 50], [50, 50]], "size": 6, "color": "#0000FF" },
                { "cmd": "fill", "at": [0, 50], "color": "#00FF00", "opacity": 0.5 }
              ] }
            ] }"##,
        );
        // The circle landed at (50, 50): painted inside, paper outside.
        let inside = seen(&masked, 50, 40);
        assert_eq!(inside, Rgba8::new(255, 0, 0, 255), "{inside:?}");
        let centre = seen(&masked, 50, 50);
        assert!(
            centre.b > 0 && centre.g > 0,
            "the stroke, then the half fill: {centre:?}"
        );
        assert_eq!(
            seen(&masked, 0, 50),
            Rgba8::WHITE,
            "where the mask was written"
        );
        assert_eq!(seen(&masked, 90, 50), Rgba8::WHITE);
        assert_eq!(seen(&masked, 50, 80), Rgba8::WHITE);
    }

    #[test]
    fn masks_nest_and_apply_on_whichever_layer_is_painted() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "layer", "name": "A" },
              { "cmd": "group", "mask": { "rect": [0, 0, 60, 100] }, "commands": [
                { "cmd": "group", "mask": { "rect": [40, 0, 60, 100] }, "commands": [
                  { "cmd": "layer", "name": "B" },
                  { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#FF0000" }
                ] },
                { "cmd": "rect", "x": 0, "y": 80, "width": 100, "height": 20, "fill": "#0000FF" }
              ] }
            ] }"##,
        );
        assert_eq!(
            seen(&document, 50, 50),
            Rgba8::new(255, 0, 0, 255),
            "in both"
        );
        assert_eq!(seen(&document, 20, 50), Rgba8::WHITE, "in the outer only");
        assert_eq!(seen(&document, 80, 50), Rgba8::WHITE, "in the inner only");
        assert_eq!(seen(&document, 20, 90), Rgba8::new(0, 0, 255, 255));
        assert_eq!(seen(&document, 80, 90), Rgba8::WHITE);
        assert_eq!(
            document.layers()[1].name(),
            "B",
            "the layer command still worked"
        );
    }

    #[test]
    fn a_mask_moves_with_keyframes_like_everything_else() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "blob", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": [20, 12], "fill": "#000000" } ] }
          ],
          "animations": { "go": { "frames": 5, "commands": [
            { "cmd": "group", "name": "shadow", "translate": { "keys": [[0, [20, 50]], [4, [80, 50]]] },
              "mask": { "symbol": "blob", "rotate": 30 }, "commands": [
              { "cmd": "gradient", "from": [-30, 0], "to": [30, 0], "colors": ["#FF0000", "#0000FF"] }
            ] }
          ] } }
        }"##;
        let frame = play_frame(script, Some("go"), 2).unwrap();
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "blob", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": [20, 12], "fill": "#000000" } ] },
            { "cmd": "group", "name": "shadow", "translate": [50, 50],
              "mask": { "symbol": "blob", "rotate": 30 }, "commands": [
              { "cmd": "gradient", "from": [-30, 0], "to": [30, 0], "colors": ["#FF0000", "#0000FF"] }
            ] }
          ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
        assert_ne!(seen(&by_hand, 50, 50), Rgba8::WHITE, "inside the blob");
        assert_eq!(seen(&by_hand, 50, 85), Rgba8::WHITE, "outside it");
        assert_eq!(by_hand.layers().len(), 1, "the silhouette made no layers");
    }

    #[test]
    fn a_symbols_silhouette_makes_a_mask_without_painting() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "define", "name": "duck", "params": { "size": 10 }, "commands": [
                { "cmd": "layer", "name": "Duck layer" },
                { "cmd": "group", "scale": "$size", "commands": [
                  { "cmd": "ellipse", "center": [0, 0], "radius": 1, "fill": "#FFFF00" } ] },
                { "cmd": "group", "mask": { "circle": [0, 0, 5] }, "commands": [
                  { "cmd": "rect", "x": -50, "y": -50, "width": 100, "height": 100, "fill": "#FF00FF" } ] }
              ] },
              { "cmd": "layer", "name": "Shadow" },
              { "cmd": "group", "mask": { "symbol": "duck", "translate": [50, 50], "with": { "size": 20 } }, "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#0000FF" }
              ] }
            ] }"##,
        );
        assert_eq!(document.layers().len(), 1, "no layer from the silhouette");
        assert_eq!(document.layers()[0].name(), "Shadow");
        assert_eq!(seen(&document, 50, 50), Rgba8::new(0, 0, 255, 255));
        assert_eq!(
            seen(&document, 65, 50),
            Rgba8::new(0, 0, 255, 255),
            "the size the use gave"
        );
        assert_eq!(seen(&document, 80, 50), Rgba8::WHITE);
        assert_eq!(
            seen(&document, 10, 10),
            Rgba8::WHITE,
            "nothing from the symbol itself"
        );
    }

    #[test]
    fn not_and_layer_masks() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "layer", "name": "Water" },
              { "cmd": "rect", "x": 0, "y": 50, "width": 100, "height": 50, "fill": "#0000FF" },
              { "cmd": "layer", "name": "Reflection" },
              { "cmd": "group", "mask": { "layer": "Water" }, "commands": [
                { "cmd": "rect", "x": 40, "y": 0, "width": 20, "height": 100, "fill": "#FF0000" } ] },
              { "cmd": "layer", "name": "Sky" },
              { "cmd": "group", "mask": { "not": { "circle": [20, 20, 10] } }, "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 40, "height": 40, "fill": "#00FF00" } ] }
            ] }"##,
        );
        assert_eq!(
            seen(&document, 50, 75),
            Rgba8::new(255, 0, 0, 255),
            "on the water"
        );
        assert_eq!(seen(&document, 50, 25), Rgba8::WHITE, "not above it");
        assert_eq!(seen(&document, 20, 20), Rgba8::WHITE, "the hole");
        assert_eq!(
            seen(&document, 5, 5),
            Rgba8::new(0, 255, 0, 255),
            "around it"
        );
    }

    #[test]
    fn a_use_takes_a_mask_and_mask_numbers_may_be_keyframes() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "wall", "commands": [
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#FF0000" } ] }
          ],
          "animations": { "grow": { "frames": 5, "commands": [
            { "cmd": "use", "name": "wall", "mask": { "circle": [50, 50, { "keys": [[0, 10], [4, 50]] }] } }
          ] } } }"##;
        let frame = play_frame(script, Some("grow"), 2).unwrap();
        assert_eq!(seen(&frame, 50, 50), Rgba8::new(255, 0, 0, 255));
        assert_eq!(
            seen(&frame, 50, 25),
            Rgba8::new(255, 0, 0, 255),
            "radius 30 by frame 2"
        );
        assert_eq!(seen(&frame, 50, 10), Rgba8::WHITE);
    }

    #[test]
    fn on_a_pixel_canvas_a_mask_has_hard_edges() {
        let document = play(
            r##"{ "palette": { "red": "#FF0000" }, "commands": [
              { "cmd": "canvas", "width": 16, "height": 16, "pixel": { "supersample": 1 } },
              { "cmd": "group", "mask": { "circle": [8, 8, 5] }, "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": 16, "height": 16, "fill": "red" } ] }
            ] }"##,
        );
        let (mut painted, mut partly) = (0, 0);
        for y in 0..16 {
            for x in 0..16 {
                match document.layers()[0].surface.pixel(x, y).a {
                    255 => painted += 1,
                    0 => {}
                    _ => partly += 1,
                }
            }
        }
        assert_eq!(partly, 0, "no half pixels");
        assert!((60..=90).contains(&painted), "{painted}");
    }

    #[test]
    fn mask_mistakes_are_explained() {
        let base = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100 },
            { "cmd": "define", "name": "blob", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "#000000" } ] },
            { "cmd": "group", "name": "g", "mask": MASK, "commands": [
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#FF0000" } ] }
          ] }"##;
        let error_with = |mask: &str| error(&base.replace("MASK", mask));
        let bad = error_with("5");
        assert!(
            bad.contains("(group \"g\")") && bad.contains("`mask`: a mask is an area"),
            "{bad}"
        );
        let unknown = error_with(r##"{ "symbol": "duck" }"##);
        assert!(
            unknown.contains("there's no symbol called \"duck\""),
            "{unknown}"
        );
        let field = error_with(r##"{ "symbol": "blob", "radius": 3 }"##);
        assert!(
            field.contains("unknown field `radius`") && field.contains("fields of a use"),
            "{field}"
        );
        let double = error_with(r##"{ "not": { "not": { "circle": [0, 0, 5] } } }"##);
        assert!(double.contains("`not` inside `not`"), "{double}");
        let layer = error_with(r##"{ "layer": "Nope" }"##);
        assert!(layer.contains("Nope"), "{layer}");
        // A mask that covers nothing is a warning, not an error.
        let script =
            Script::parse(&base.replace("MASK", r##"{ "circle": [500, 500, 5] }"##)).unwrap();
        let mut player = Player::new(script, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let warnings = player.report().warnings;
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("(group \"g\")") && w.contains("covers nothing")),
            "{warnings:?}"
        );
    }

    // ---- Brush tips, dynamics and stamp (phase 3, step 2) --------------------

    const TIP_SCRIPT: &str = r##"{ "seed": 2, "commands": [
        { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
        { "cmd": "brush", "tip": "TIP", "size": 20, "color": "#000000", "hardness": 1, "dynamics": DYNAMICS },
        { "cmd": "stroke", "points": [[20, 50], [180, 50]] }
      ] }"##;

    fn tip_script(tip: &str, dynamics: &str) -> String {
        TIP_SCRIPT.replace("TIP", tip).replace("DYNAMICS", dynamics)
    }

    fn painted_pixels(document: &Document) -> usize {
        let mut count = 0;
        for y in 0..100 {
            for x in 0..200 {
                if document.layers()[0].surface.pixel(x, y).a > 0 {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn every_tip_paints_and_each_looks_different() {
        let mut pictures = Vec::new();
        for tip in [
            "round", "square", "bristle", "chalk", "leaf", "grass", "sparkle",
        ] {
            let document = play(&tip_script(tip, "{}"));
            assert!(painted_pixels(&document) > 500, "{tip} paints");
            // Nothing lands outside the stroke's reach.
            assert_eq!(
                document.layers()[0].surface.pixel(100, 20),
                Rgba8::TRANSPARENT,
                "{tip}"
            );
            pictures.push((tip, png(&document)));
        }
        for (i, (tip, picture)) in pictures.iter().enumerate() {
            for (other, theirs) in &pictures[i + 1..] {
                assert_ne!(picture, theirs, "{tip} and {other} look the same");
            }
        }
        // The round tip is exactly the brush as it always was.
        let plain = play(&tip_script("round", "{}"));
        let before = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "brush", "size": 20, "color": "#000000", "hardness": 1 },
            { "cmd": "stroke", "points": [[20, 50], [180, 50]] } ] }"##,
        );
        assert_eq!(png(&plain), png(&before));
    }

    #[test]
    fn a_square_tip_has_corners_and_follows_the_stroke() {
        let square = play(&tip_script("square", "{}"));
        // A horizontal stroke with a square tip: a 20-pixel-high band
        // with square ends, solid right to the corner.
        let layer = &square.layers()[0].surface;
        assert_eq!(layer.pixel(100, 41).a, 255);
        assert_eq!(layer.pixel(100, 58).a, 255);
        assert_eq!(layer.pixel(100, 38).a, 0);
        assert_eq!(layer.pixel(11, 41).a, 255, "the start's corner is square");
        // Turned 45 degrees, the band gets wider (the square's diagonal).
        let turned = play(
            &tip_script("square", "{}")
                .replace(r#""hardness": 1,"#, r#""hardness": 1, "angle": 45,"#),
        );
        assert!(turned.layers()[0].surface.pixel(100, 38).a > 0);
        assert_ne!(png(&turned), png(&square));
    }

    #[test]
    fn dynamics_vary_the_dabs_from_the_seed_only() {
        let dynamics = r##"{ "size": 0.4, "angle": 30, "opacity": 0.5, "scatter": 6, "hue": 20, "lightness": 0.2, "saturation": 0.3 }"##;
        // (A colour, not black: black has no hue or lightness to vary.)
        let tip_script =
            |tip: &str, dynamics: &str| tip_script(tip, dynamics).replace("#000000", "#4080C0");
        let a = play(&tip_script("chalk", dynamics));
        let b = play(&tip_script("chalk", dynamics));
        assert_eq!(png(&a), png(&b), "the same seed: the same picture");
        let c = play(&tip_script("chalk", dynamics).replace("\"seed\": 2", "\"seed\": 3"));
        assert_ne!(png(&a), png(&c), "another seed: another picture");
        let plain = play(&tip_script("chalk", "{}"));
        assert_ne!(png(&a), png(&plain));
        // Scatter puts paint off the line; colour dynamics vary the colour.
        let layer = &a.layers()[0].surface;
        let off_line = (0..200).filter(|&x| layer.pixel(x, 36).a > 0).count();
        assert!(off_line > 0, "scatter");
        let mut colors = std::collections::HashSet::new();
        for x in 20..180 {
            let p = layer.pixel(x, 50);
            if p.a > 200 {
                colors.insert((p.r, p.g, p.b));
            }
        }
        assert!(colors.len() > 3, "colours vary: {colors:?}");
        // Round tips take dynamics too (size and scatter), with the same seed rule.
        let round = play(&tip_script("round", r##"{ "size": 0.5, "scatter": 5 }"##));
        assert_ne!(png(&round), png(&play(&tip_script("round", "{}"))));
    }

    #[test]
    fn dynamics_and_tips_are_checked() {
        let bad_tip = error(&tip_script("fluffy", "{}"));
        assert!(
            bad_tip.contains("unknown tip \"fluffy\"") && bad_tip.contains("chalk"),
            "{bad_tip}"
        );
        let bad_size = error(&tip_script("round", r##"{ "size": 3 }"##));
        assert!(
            bad_size.contains("`dynamics`: `size` is a fraction"),
            "{bad_size}"
        );
        let bad_field = error(&tip_script("round", r##"{ "wobble": 3 }"##));
        assert!(bad_field.contains("unknown field `wobble`"), "{bad_field}");
        let missing = error(
            &tip_script("round", "{}")
                .replace(r#""tip": "round""#, r#""tip": { "file": "nope.png" }"#),
        );
        assert!(
            missing.contains("can't load the tip \"nope.png\""),
            "{missing}"
        );
    }

    #[test]
    fn a_tip_from_a_file_is_its_lightness() {
        // A 8 x 8 PNG: white on the left half, black on the right.
        let folder = std::env::temp_dir().join(format!("quill-tip-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("half.png");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut encoder = png::Encoder::new(file, 8, 8);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            let mut data = vec![0u8; 64];
            for y in 0..8 {
                for x in 0..4 {
                    data[y * 8 + x] = 255;
                }
            }
            writer.write_image_data(&data).unwrap();
        }
        let script = Script::parse(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "brush", "tip": { "file": "half.png" }, "size": 40, "color": "#000000", "hardness": 1 },
            { "cmd": "stroke", "points": [[50, 50]] },
            { "cmd": "brush", "tip": { "file": "half.png" } } ] }"##,
        )
        .unwrap();
        let options = PlayOptions {
            folder: Some(folder.clone()),
            ..PlayOptions::default()
        };
        let mut player = Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        assert_eq!(player.tips.len(), 1, "loaded once");
        let layer = &document.layers()[0].surface;
        assert_eq!(layer.pixel(40, 50).a, 255, "the white half paints");
        assert_eq!(layer.pixel(60, 50).a, 0, "the black half doesn't");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn stamp_puts_dabs_along_a_path() {
        let document = play(
            r##"{ "commands": [
              { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
              { "cmd": "brush", "size": 10, "color": "#000000", "hardness": 1 },
              { "cmd": "stamp", "tip": "square", "points": [[20, 50], [180, 50]], "spacing": 40, "size": 10 }
            ] }"##,
        );
        let layer = &document.layers()[0].surface;
        // Copies at x = 20, 60, 100, 140, 180: solid there, nothing between.
        for x in [20, 60, 100, 140, 180] {
            assert_eq!(layer.pixel(x, 50).a, 255, "a dab at {x}");
        }
        assert_eq!(layer.pixel(40, 50).a, 0, "nothing between dabs");
        assert_eq!(layer.pixel(80, 50).a, 0);
        // Keyframed points: frame 2 of a moving path equals the path by hand.
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" } ],
          "animations": { "go": { "frames": 5, "commands": [
            { "cmd": "stamp", "tip": "leaf", "points": { "keys": [[0, [[20, 20], [180, 20]]], [4, [[20, 60], [180, 60]]]] },
              "spacing": 30, "size": 14, "color": "#008000", "seed": 1 } ] } } }"##;
        let frame = play_frame(script, Some("go"), 2).unwrap();
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "stamp", "tip": "leaf", "points": [[20, 40], [180, 40]], "spacing": 30, "size": 14, "color": "#008000", "seed": 1 }
            ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
    }

    #[test]
    fn stamp_copies_a_symbol_turned_scaled_and_coloured() {
        let script = r##"{ "seed": 7, "commands": [
            { "cmd": "canvas", "width": 400, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "dot", "params": { "color": "#FF0000", "r": 4 }, "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 4, "fill": "$color" },
              { "cmd": "rect", "x": -1, "y": -12, "width": 2, "height": 8, "fill": "$color" } ] },
            { "cmd": "stamp", "symbol": "dot", "points": [[20, 50], [380, 50]], "spacing": 18,
              "size": [0.7, 1.3], "colors": ["#FF0000", "#0000FF"], "with": { "r": 5 } }
          ] }"##;
        let document = play(script);
        let layer = &document.layers()[0].surface;
        let (mut red, mut blue) = (0, 0);
        for x in (20..=380).step_by(18) {
            let p = layer.pixel(x, 50);
            assert_eq!(p.a, 255, "a copy at {x}");
            if p.r > p.b {
                red += 1;
            } else {
                blue += 1;
            }
        }
        assert!(
            red > 0 && blue > 0,
            "both colours are picked: {red} red, {blue} blue"
        );
        // `follow` on a path going right turns the stick (drawn upwards)
        // to point right... which with angle 0 along a horizontal path is
        // a 0-degree turn: the stick still points up.
        assert!(layer.pixel(20, 42).a > 0, "the stick above the first dot");
        // The same seed, the same picture; and copies land where a use would.
        assert_eq!(png(&document), png(&play(script)));
        let one = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "box", "commands": [
              { "cmd": "rect", "x": -10, "y": -5, "width": 20, "height": 10, "fill": "#000000" } ] },
            { "cmd": "stamp", "symbol": "box", "points": [[50, 50]], "spacing": 10, "size": 2, "angle": 90 }
            ] }"##,
        );
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "box", "commands": [
              { "cmd": "rect", "x": -10, "y": -5, "width": 20, "height": 10, "fill": "#000000" } ] },
            { "cmd": "use", "name": "box", "translate": [50, 50], "scale": 2, "rotate": 90 }
            ] }"##,
        );
        assert_eq!(png(&one), png(&by_hand));
    }

    #[test]
    fn stamp_mistakes_are_explained() {
        let base = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100 },
            { "cmd": "define", "name": "plain", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 4, "fill": "#000000" } ] },
            STAMP
          ] }"##;
        let error_with = |stamp: &str| error(&base.replace("STAMP", stamp));
        let both = error_with(
            r##"{ "cmd": "stamp", "symbol": "plain", "tip": "leaf", "points": [[0, 0]], "spacing": 5 }"##,
        );
        assert!(
            both.contains("(stamp)") && both.contains("`symbol` or `tip`, not both"),
            "{both}"
        );
        let none = error_with(
            r##"{ "cmd": "stamp", "symbol": "nope", "points": [[0, 0]], "spacing": 5 }"##,
        );
        assert!(none.contains("no symbol called \"nope\""), "{none}");
        let color = error_with(
            r##"{ "cmd": "stamp", "symbol": "plain", "points": [[0, 0]], "spacing": 5, "colors": ["#FF0000"] }"##,
        );
        assert!(
            color.contains("`color` parameter") && color.contains("\"plain\" has none"),
            "{color}"
        );
        let spacing = error_with(
            r##"{ "cmd": "stamp", "tip": "leaf", "points": [[0, 0], [50, 0]], "spacing": 0 }"##,
        );
        assert!(
            spacing.contains("`spacing` must be more than 0"),
            "{spacing}"
        );
        let many = error_with(
            r##"{ "cmd": "stamp", "tip": "leaf", "points": [[0, 0], [100, 0]], "spacing": 0.001 }"##,
        );
        assert!(many.contains("at most 10000"), "{many}");
        let points =
            error_with(r##"{ "cmd": "stamp", "tip": "leaf", "points": 5, "spacing": 1 }"##);
        assert!(
            points.contains("`points` is a list of [x, y] points"),
            "{points}"
        );
    }

    // ---- Noise and textures (phase 3, step 3) ----------------------------------

    fn noise_script(fill: &str) -> String {
        format!(
            r##"{{ "seed": 4, "palette": {{ "a": "#2A5A8E", "b": "#6FA8D6" }}, "commands": [
            {{ "cmd": "canvas", "width": 120, "height": 80, "paper": "#FFFFFF" }},
            {{ "cmd": "rect", "x": 0, "y": 0, "width": 120, "height": 80, "fill": {fill} }} ] }}"##
        )
    }

    #[test]
    fn noise_fills_are_repeatable_and_vary_with_the_seed() {
        let noise =
            r##"{ "noise": { "scale": 20, "detail": 3, "colors": ["a", "b"], "seed": 1 } }"##;
        let a = play(&noise_script(noise));
        assert_eq!(png(&a), png(&play(&noise_script(noise))));
        let b = play(&noise_script(&noise.replace("\"seed\": 1", "\"seed\": 2")));
        assert_ne!(png(&a), png(&b));
        let c = play(&noise_script(noise).replace("\"seed\": 4,", "\"seed\": 5,"));
        assert_ne!(png(&a), png(&c), "the script's seed counts too");
        // Every pixel is between the two colours, and they're not all the same.
        let layer = &a.layers()[0].surface;
        let mut seen = std::collections::HashSet::new();
        for y in 0..80 {
            for x in 0..120 {
                let p = layer.pixel(x, y);
                assert_eq!(p.a, 255);
                assert!(p.r >= 0x2A && p.r <= 0x6F && p.b >= 0x8E, "{p:?}");
                seen.insert(p.r);
            }
        }
        assert!(seen.len() > 20, "{} shades", seen.len());
        // More contrast: more pixels near the two ends.
        let sharp = play(&noise_script(
            &noise.replace("\"detail\": 3", "\"detail\": 3, \"contrast\": 4"),
        ));
        let ends = |d: &Document| {
            let l = &d.layers()[0].surface;
            (0..80)
                .flat_map(|y| (0..120).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let r = l.pixel(x, y).r;
                    !(0x2A + 4..=0x6F - 4).contains(&r)
                })
                .count()
        };
        assert!(
            ends(&sharp) > ends(&a) * 2,
            "{} vs {}",
            ends(&sharp),
            ends(&a)
        );
    }

    #[test]
    fn a_pattern_lives_in_the_coordinates_of_its_group() {
        // A textured symbol moved, resized and turned by a group...
        let moved = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "rock", "commands": [
              { "cmd": "rect", "x": -30, "y": -20, "width": 60, "height": 40,
                "fill": { "noise": { "scale": 15, "detail": 2, "colors": ["a", "b"], "seed": 3 } } } ] },
            { "cmd": "group", "translate": [100, 100], "scale": 2, "rotate": 30, "commands": [
              { "cmd": "use", "name": "rock" } ] } ] }"##,
        );
        // ...is the same as the noise moved, resized and turned by hand.
        let by_hand = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "group", "translate": [100, 100], "scale": 2, "rotate": 30, "commands": [
              { "cmd": "rect", "x": -30, "y": -20, "width": 60, "height": 40,
                "fill": { "noise": { "scale": 15, "detail": 2, "colors": ["a", "b"], "seed": 3 } } } ] } ] }"##,
        );
        assert_eq!(png(&moved), png(&by_hand));
        // And differs from the same rock drawn without the group's transform
        // (the pattern is bigger and turned, not just the shape).
        let plain = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 40, "y": 60, "width": 120, "height": 80,
              "fill": { "noise": { "scale": 15, "detail": 2, "colors": ["a", "b"], "seed": 3 } } } ] }"##,
        );
        assert_ne!(png(&moved), png(&plain));
        // Moving by a group equals moving the pattern by its offset.
        let translated = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "group", "translate": [50, 20], "commands": [
              { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100,
                "fill": { "noise": { "scale": 15, "detail": 2, "colors": ["a", "b"], "seed": 3 } } } ] } ] }"##,
        );
        let offset = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 50, "y": 20, "width": 100, "height": 100,
              "fill": { "noise": { "scale": 15, "detail": 2, "colors": ["a", "b"], "seed": 3, "offset": [50, 20] } } } ] }"##,
        );
        assert_eq!(png(&translated), png(&offset));
    }

    #[test]
    fn noise_offset_and_scale_may_be_keyframes() {
        let script = r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 100, "height": 60, "paper": "#FFFFFF" } ],
          "animations": { "drift": { "frames": 5, "commands": [
            { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 60,
              "fill": { "noise": { "scale": { "keys": [[0, 10], [4, 30]] }, "detail": 2, "colors": ["a", "b"],
                                   "offset": { "keys": [[0, [0, 0]], [4, [40, 0]]] } } } } ] } } }"##;
        let frame = play_frame(script, Some("drift"), 2).unwrap();
        let by_hand = play(
            r##"{ "palette": { "a": "#2A5A8E", "b": "#6FA8D6" }, "commands": [
            { "cmd": "canvas", "width": 100, "height": 60, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 60,
              "fill": { "noise": { "scale": 20, "detail": 2, "colors": ["a", "b"], "offset": [20, 0] } } } ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
    }

    #[test]
    fn textures_tile_a_picture_and_paper_is_built_in() {
        let folder = std::env::temp_dir().join(format!("quill-texture-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        {
            // 2 x 2: white, black / black, white.
            let file = std::fs::File::create(folder.join("check.png")).unwrap();
            let mut encoder = png::Encoder::new(file, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[255, 0, 0, 255]).unwrap();
        }
        let script = Script::parse(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 40, "height": 40, "paper": "#808080" },
            { "cmd": "rect", "x": 0, "y": 0, "width": 40, "height": 40,
              "fill": { "texture": { "file": "check.png", "scale": 5 } } },
            { "cmd": "layer", "name": "Tinted" },
            { "cmd": "rect", "x": 0, "y": 0, "width": 40, "height": 20,
              "fill": { "texture": { "file": "check.png", "scale": 5, "colors": ["#FF0000", "#0000FF"] } } },
            { "cmd": "layer", "name": "Paper" },
            { "cmd": "rect", "x": 0, "y": 20, "width": 40, "height": 20, "fill": { "texture": "paper" } }
            ] }"##,
        )
        .unwrap();
        let options = PlayOptions {
            folder: Some(folder.clone()),
            ..PlayOptions::default()
        };
        let mut player = Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        assert_eq!(player.textures.len(), 1, "loaded once");
        let checks = &document.layers()[0].surface;
        // Each texture pixel is 5 canvas pixels; the picture repeats every 10.
        assert_eq!(checks.pixel(2, 2), Rgba8::WHITE);
        assert_eq!(checks.pixel(7, 2), Rgba8::BLACK);
        assert_eq!(checks.pixel(12, 2), Rgba8::WHITE, "repeats");
        assert_eq!(checks.pixel(2, 7), Rgba8::BLACK);
        let tinted = &document.layers()[1].surface;
        assert_eq!(
            tinted.pixel(2, 2),
            Rgba8::new(0, 0, 255, 255),
            "white -> the last colour"
        );
        assert_eq!(
            tinted.pixel(7, 2),
            Rgba8::new(255, 0, 0, 255),
            "black -> the first"
        );
        let paper = &document.layers()[2].surface;
        let p = paper.pixel(10, 30);
        assert!(p.a == 255 && p.r > 180 && p.r == p.g && p.g == p.b, "{p:?}");
        assert!((0..40).any(|x| paper.pixel(x, 30) != p), "it has grain");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_brush_can_paint_with_a_pattern_and_gradient_takes_a_fill() {
        let document = play(
            r##"{ "palette": { "a": "#FF0000", "b": "#0000FF" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "brush", "size": 20, "hardness": 1,
              "color": { "noise": { "scale": 10, "detail": 2, "colors": ["a", "b"], "seed": 1 } } },
            { "cmd": "stroke", "points": [[10, 30], [190, 30]] },
            { "cmd": "stroke", "points": [[10, 70], [190, 70]], "color": "#00FF00" },
            { "cmd": "layer", "name": "Fill" },
            { "cmd": "gradient", "area": { "rect": [0, 90, 200, 10] }, "fill": { "texture": "paper" } }
            ] }"##,
        );
        let layer = &document.layers()[0].surface;
        let mut shades = std::collections::HashSet::new();
        for x in 20..180 {
            let p = layer.pixel(x, 30);
            assert_eq!(p.a, 255);
            assert_eq!(p.g, 0, "between red and blue: {p:?}");
            shades.insert(p.r);
        }
        assert!(
            shades.len() > 10,
            "the stroke is patterned: {} shades",
            shades.len()
        );
        assert_eq!(
            layer.pixel(100, 70),
            Rgba8::new(0, 255, 0, 255),
            "a plain colour again"
        );
        let fill = &document.layers()[1].surface;
        assert!(fill.pixel(100, 95).a == 255 && fill.pixel(100, 85).a == 0);
    }

    #[test]
    fn pattern_mistakes_are_explained() {
        let bad = |fill: &str| error(&noise_script(fill));
        let e = bad(r##"{ "noise": { "scale": 0, "colors": ["a", "b"] } }"##);
        assert!(e.contains("noise: `scale`"), "{e}");
        let e = bad(r##"{ "noise": { "scale": 10, "detail": 9, "colors": ["a", "b"] } }"##);
        assert!(e.contains("`detail` is from 1 to 6"), "{e}");
        let e = bad(r##"{ "noise": { "scale": 10 } }"##);
        assert!(e.contains("needs `colors`"), "{e}");
        let e = bad(r##"{ "noise": { "scale": 10, "colors": ["a", "b"], "wobble": 1 } }"##);
        assert!(e.contains("unknown field `wobble`"), "{e}");
        let e = bad(r##"{ "texture": "linen" }"##);
        assert!(e.contains("no built-in texture called \"linen\""), "{e}");
        let e = bad(r##"{ "texture": { "file": "nope.png" } }"##);
        assert!(e.contains("can't load \"nope.png\""), "{e}");
        let e = bad(r##"{ "texture": { "scale": 2 } }"##);
        assert!(e.contains("`file` is missing"), "{e}");
        let e = error(
            r##"{ "commands": [ { "cmd": "canvas", "width": 10, "height": 10 },
              { "cmd": "gradient", "from": [0, 0], "fill": "#FF0000" } ] }"##,
        );
        assert!(e.contains("give `fill`, or `from`/`to`"), "{e}");
        let e = error(
            r##"{ "commands": [ { "cmd": "canvas", "width": 10, "height": 10 },
              { "cmd": "gradient", "colors": ["#FF0000", "#0000FF"] } ] }"##,
        );
        assert!(e.contains("needs `from` and `to`"), "{e}");
    }

    // ---- Filters (phase 3, step 4) ---------------------------------------------

    #[test]
    fn a_filter_changes_a_layer_in_place_and_can_be_undone() {
        let script = Script::parse(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "layer", "name": "A" },
            { "cmd": "rect", "x": 50, "y": 30, "width": 40, "height": 40, "fill": "#FF0000" },
            { "cmd": "layer", "name": "B" },
            { "cmd": "rect", "x": 150, "y": 30, "width": 40, "height": 40, "fill": "#FF0000" },
            { "cmd": "filter", "layer": "A", "blur": 6, "adjust": { "saturation": -1 } } ] }"##,
        )
        .unwrap();
        let options = PlayOptions {
            record_undo: true,
            ..PlayOptions::default()
        };
        let mut player = Player::new(script, options).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let undo = player.take_undo(&mut document);
        let a = &document.layers()[0].surface;
        assert!(a.pixel(70, 50).a > 200);
        assert_eq!(
            (a.pixel(70, 50).r, a.pixel(70, 50).g),
            (a.pixel(70, 50).a, a.pixel(70, 50).a),
            "grey now"
        );
        assert!(a.pixel(93, 50).a > 0, "blurred outwards");
        let b = &document.layers()[1].surface;
        assert_eq!(
            b.pixel(170, 50),
            Rgba8::new(255, 0, 0, 255),
            "the other layer is untouched"
        );
        assert_eq!(b.pixel(193, 50).a, 0);
        // The filter is one undo step that puts the layer back.
        let mut group = CommandGroup::new("script", undo);
        group.undo(&mut document);
        let empty = document.layers()[0].surface.tile_count() == 0;
        assert!(empty, "everything undone");
        // And the report says the filter painted on A.
        assert!(player.report().painted_by.values().any(|c| c.contains(&6)));
    }

    #[test]
    fn a_filtered_group_draws_on_its_own_then_lands_on_the_layers() {
        // A glow around a sun: the halo reaches beyond the shape, but a
        // dark shape on the same layer from before is untouched.
        let document = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#000000" },
            { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "#202020" },
            { "cmd": "group", "name": "sun", "filter": { "glow": { "radius": 10, "threshold": 0.5, "strength": 1 } }, "commands": [
              { "cmd": "ellipse", "center": [100, 50], "radius": 15, "fill": "#FFFFC0" } ] } ] }"##,
        );
        let layer = &document.layers()[0].surface;
        assert!(layer.pixel(118, 50).a > 0, "a halo outside the sun");
        assert_eq!(
            layer.pixel(20, 20),
            Rgba8::new(0x20, 0x20, 0x20, 255),
            "untouched"
        );
        assert_eq!(document.layers().len(), 1);
        // A use with a shadow on two layers: one shadow of the whole
        // silhouette, on the lower layer, under the drawing.
        let document = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "duck", "commands": [
              { "cmd": "layer", "name": "Body" },
              { "cmd": "rect", "x": -30, "y": -10, "width": 60, "height": 40, "fill": "#FFD000" },
              { "cmd": "layer", "name": "Head" },
              { "cmd": "rect", "x": 10, "y": -40, "width": 30, "height": 40, "fill": "#FFD000" } ] },
            { "cmd": "layer", "name": "Body" },
            { "cmd": "layer", "name": "Head" },
            { "cmd": "use", "name": "duck", "translate": [100, 100],
              "filter": { "shadow": { "offset": [20, 20], "blur": 0, "color": "#000000FF" } } } ] }"##,
        );
        let body = &document.layers()[0].surface;
        let head = &document.layers()[1].surface;
        assert_eq!(
            body.pixel(100, 100),
            Rgba8::new(255, 208, 0, 255),
            "the body"
        );
        assert_eq!(
            head.pixel(120, 80),
            Rgba8::new(255, 208, 0, 255),
            "the head"
        );
        assert_eq!(
            body.pixel(145, 120),
            Rgba8::BLACK,
            "the body's shadow, on the body layer"
        );
        assert_eq!(
            body.pixel(145, 85),
            Rgba8::BLACK,
            "the head's shadow too, on the body layer"
        );
        assert_eq!(
            head.pixel(145, 85).a,
            0,
            "nothing on the head layer but the head"
        );
        assert_eq!(
            body.pixel(125, 105),
            Rgba8::new(255, 208, 0, 255),
            "the shadow is under the body"
        );
        assert_eq!(
            seen(&document, 125, 75),
            Rgba8::new(255, 208, 0, 255),
            "and under the head"
        );
        // Without the filter, the same pixels, minus the shadow.
        let plain = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "duck", "commands": [
              { "cmd": "layer", "name": "Body" },
              { "cmd": "rect", "x": -30, "y": -10, "width": 60, "height": 40, "fill": "#FFD000" },
              { "cmd": "layer", "name": "Head" },
              { "cmd": "rect", "x": 10, "y": -40, "width": 30, "height": 40, "fill": "#FFD000" } ] },
            { "cmd": "layer", "name": "Body" },
            { "cmd": "layer", "name": "Head" },
            { "cmd": "use", "name": "duck", "translate": [100, 100] } ] }"##,
        );
        assert_eq!(
            plain.layers()[1].surface.pixel(120, 80),
            head.pixel(120, 80)
        );
        assert_eq!(seen(&plain, 145, 120), Rgba8::WHITE);
    }

    #[test]
    fn from_below_flattens_the_picture_onto_a_layer() {
        let document = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "layer", "name": "Sky" },
            { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 50, "fill": "#0000FF" },
            { "cmd": "layer", "name": "Hidden", "visible": false },
            { "cmd": "rect", "x": 0, "y": 0, "width": 100, "height": 100, "fill": "#00FF00" },
            { "cmd": "layer", "name": "Ground" },
            { "cmd": "rect", "x": 0, "y": 50, "width": 100, "height": 50, "fill": "#FF0000" },
            { "cmd": "layer", "name": "Mood", "from": "below", "opacity": 0.5 },
            { "cmd": "filter", "gradient_map": { "colors": ["#000000", "#FFFFFF"] } } ] }"##,
        );
        let mood = &document.layers()[3].surface;
        assert_eq!(mood.pixel(50, 25).a, 255, "copied from below");
        assert_eq!(mood.pixel(50, 75).a, 255);
        assert_eq!(mood.pixel(50, 25).g, mood.pixel(50, 25).r, "mapped to grey");
        assert!(
            mood.pixel(50, 75).r > mood.pixel(50, 25).r,
            "red is lighter than blue"
        );
        // Seen: half the original colour, half the grey.
        let sky = seen(&document, 50, 25);
        assert!(sky.b > sky.r && sky.r > 0, "{sky:?}");
        assert!(sky.b < 255 && sky.b > 100, "half way: {sky:?}");
        let bad = error(
            r##"{ "commands": [ { "cmd": "canvas", "width": 10, "height": 10 },
              { "cmd": "layer", "name": "A", "from": "above" } ] }"##,
        );
        assert!(bad.contains("`from` is \"below\""), "{bad}");
    }

    #[test]
    fn filter_numbers_may_be_keyframes_and_scale_with_the_group() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" } ],
          "animations": { "pulse": { "frames": 5, "commands": [
            { "cmd": "group", "filter": { "shadow": { "offset": { "keys": [[0, [0, 0]], [4, [40, 0]]] }, "blur": 0, "color": "#000000FF" } }, "commands": [
              { "cmd": "rect", "x": 50, "y": 30, "width": 40, "height": 40, "fill": "#FF0000" } ] } ] } } }"##;
        let frame = play_frame(script, Some("pulse"), 2).unwrap();
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "group", "filter": { "shadow": { "offset": [20, 0], "blur": 0, "color": "#000000FF" } }, "commands": [
              { "cmd": "rect", "x": 50, "y": 30, "width": 40, "height": 40, "fill": "#FF0000" } ] } ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
        assert_eq!(seen(&by_hand, 100, 50), Rgba8::BLACK, "offset 20");
        // In a group scaled by 2, the offset doubles.
        let scaled = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "group", "scale": 2, "filter": { "shadow": { "offset": [20, 0], "blur": 0, "color": "#000000FF" } }, "commands": [
              { "cmd": "rect", "x": 25, "y": 15, "width": 20, "height": 20, "fill": "#FF0000" } ] } ] }"##,
        );
        assert_eq!(seen(&scaled, 120, 50), Rgba8::BLACK, "offset 40");
        assert_eq!(seen(&scaled, 70, 50), Rgba8::new(255, 0, 0, 255));
    }

    #[test]
    fn filter_mistakes_are_explained() {
        let base = r##"{ "palette": { "k": "#000000" }, "commands": [
            { "cmd": "canvas", "width": 20, "height": 20 CANVAS },
            { "cmd": "rect", "x": 5, "y": 5, "width": 5, "height": 5, "fill": "k" },
            FILTER ] }"##;
        let plain = |filter: &str| error(&base.replace("CANVAS", "").replace("FILTER", filter));
        let e = plain(r##"{ "cmd": "filter" }"##);
        assert!(e.contains("give at least one of `blur`"), "{e}");
        let e = plain(r##"{ "cmd": "filter", "blur": -1 }"##);
        assert!(e.contains("`blur`"), "{e}");
        let e = plain(r##"{ "cmd": "filter", "adjust": { "saturation": 3 } }"##);
        assert!(e.contains("`saturation` is from -1 to 1"), "{e}");
        let e = plain(r##"{ "cmd": "filter", "glow": { "size": 3 } }"##);
        assert!(e.contains("unknown field `size`"), "{e}");
        let e = plain(r##"{ "cmd": "filter", "layer": "Nope", "blur": 2 }"##);
        assert!(e.contains("Nope"), "{e}");
        let e = plain(r##"{ "cmd": "group", "filter": { "wobble": 2 }, "commands": [] }"##);
        assert!(e.contains("`filter`: unknown field `wobble`"), "{e}");
        // A pixel canvas refuses anything that melts pixels.
        let pixel = |filter: &str| {
            error(
                &base
                    .replace("CANVAS", r##", "pixel": { "supersample": 1 }"##)
                    .replace("FILTER", filter),
            )
        };
        let e = pixel(r##"{ "cmd": "filter", "blur": 2 }"##);
        assert!(e.contains("a pixel canvas can't take blur"), "{e}");
        let e = pixel(r##"{ "cmd": "filter", "shadow": { "blur": 2 } }"##);
        assert!(e.contains("can't take a shadow with blur"), "{e}");
        let ok = base
            .replace("CANVAS", r##", "pixel": { "supersample": 1 }"##)
            .replace("FILTER", r##"{ "cmd": "filter", "shadow": { "offset": [2, 2], "blur": 0, "color": "k" }, "adjust": { "brightness": 0.1 } }"##);
        play(&ok);
    }

    // ---- Review fixes after phase 3, step 4 -----------------------------------

    #[test]
    fn a_symbol_looks_the_same_whatever_the_brush_was() {
        let script = |before: &str| {
            format!(
                r##"{{ "seed": 3, "commands": [
                {{ "cmd": "canvas", "width": 120, "height": 120, "paper": "#FFFFFF" }},
                {{ "cmd": "define", "name": "coin", "commands": [
                  {{ "cmd": "ellipse", "center": [0, 0], "radius": 20, "fill": "#F2C230", "outline": "#1A1410", "outline_size": 3 }},
                  {{ "cmd": "line", "from": [-10, 0], "to": [10, 0] }},
                  {{ "cmd": "stroke", "points": [[-8, 8], [8, 8]] }} ] }},
                {before}
                {{ "cmd": "use", "name": "coin", "translate": [60, 60] }} ] }}"##
            )
        };
        let plain = play(&script(""));
        let hairy = play(&script(
            r##"{ "cmd": "brush", "tip": "bristle", "size": 40, "color": "#FF0000", "opacity": 0.3,
                 "dynamics": { "follow": true, "size": 0.5, "scatter": 10, "hue": 90 } },"##,
        ));
        assert_eq!(
            png(&plain),
            png(&hairy),
            "the brush before a use doesn't reach into it"
        );
        let patterned = play(&script(
            r##"{ "cmd": "brush", "color": { "noise": { "scale": 5, "colors": ["#FF0000", "#0000FF"] } }, "hardness": 0 },"##,
        ));
        assert_eq!(png(&plain), png(&patterned));
        // A stamp's copies too.
        let stamped = |before: &str| {
            play(&format!(
                r##"{{ "commands": [
                {{ "cmd": "canvas", "width": 120, "height": 120, "paper": "#FFFFFF" }},
                {{ "cmd": "define", "name": "coin", "commands": [
                  {{ "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "#F2C230", "outline": "#1A1410", "outline_size": 2 }} ] }},
                {before}
                {{ "cmd": "stamp", "symbol": "coin", "points": [[20, 60], [100, 60]], "spacing": 40 }} ] }}"##
            ))
        };
        assert_eq!(
            png(&stamped("")),
            png(&stamped(
                r##"{ "cmd": "brush", "tip": "chalk", "size": 30, "dynamics": { "scatter": 8 } },"##
            ))
        );
        // After the use, the brush from before is back.
        let after = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 120, "height": 120, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "dot", "commands": [
              { "cmd": "brush", "size": 2, "color": "#00FF00" },
              { "cmd": "stroke", "points": [[0, 0]] } ] },
            { "cmd": "brush", "size": 30, "color": "#FF0000" },
            { "cmd": "use", "name": "dot", "translate": [30, 30] },
            { "cmd": "stroke", "points": [[90, 90]] } ] }"##,
        );
        assert_eq!(seen(&after, 90, 90), Rgba8::new(255, 0, 0, 255));
        assert_eq!(
            seen(&after, 100, 90),
            Rgba8::new(255, 0, 0, 255),
            "30 wide again"
        );
        let own = seen(&after, 30, 30);
        assert!(
            own.g == 255 && own.r < 80,
            "the symbol's own brush: {own:?}"
        );
    }

    #[test]
    fn brush_changes_stay_inside_a_group_and_reset_goes_back_to_the_plain_brush() {
        let document = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 120, "height": 120, "paper": "#FFFFFF" },
            { "cmd": "brush", "size": 10, "color": "#FF0000" },
            { "cmd": "group", "commands": [
              { "cmd": "brush", "size": 40, "color": "#0000FF", "tip": "square" },
              { "cmd": "stroke", "points": [[30, 30]] } ] },
            { "cmd": "stroke", "points": [[90, 30]] },
            { "cmd": "brush", "reset": true, "color": "#00FF00" },
            { "cmd": "stroke", "points": [[60, 90]] } ] }"##,
        );
        assert_eq!(seen(&document, 30, 30), Rgba8::new(0, 0, 255, 255));
        assert_eq!(
            seen(&document, 90, 30),
            Rgba8::new(255, 0, 0, 255),
            "red and small again"
        );
        assert_eq!(seen(&document, 98, 30), Rgba8::WHITE);
        // Reset: the plain brush (24 wide), then green.
        assert_eq!(seen(&document, 60, 90), Rgba8::new(0, 255, 0, 255));
        assert_eq!(
            seen(&document, 68, 90),
            Rgba8::new(0, 255, 0, 255),
            "24 wide"
        );
        assert_eq!(seen(&document, 74, 90), Rgba8::WHITE);
    }

    #[test]
    fn a_part_that_paints_most_of_the_canvas_with_small_shapes_is_a_warning() {
        let warnings_of = |text: &str| {
            let script = Script::parse(text).unwrap();
            let mut player = Player::new(script, PlayOptions::default()).unwrap();
            let mut document = player.create_document().unwrap();
            player.run(&mut document, |_, _| {}).unwrap();
            player.report().warnings
        };
        let warnings = warnings_of(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 400, "height": 400, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "pad", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 12, "fill": "#308030" },
              { "cmd": "group", "mask": { "not": { "circle": [8, 0, 4] } }, "commands": [
                { "cmd": "gradient", "from": [-12, -12], "to": [12, 12], "colors": ["#FFFFFF60", "transparent"] } ] } ] },
            { "cmd": "use", "name": "pad", "translate": [200, 200] } ] }"##,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.starts_with(
                    "command 3 (use \"pad\") painted 100% of the canvas, but its shapes cover 0%"
                ) && w.contains("without an area, inside a mask with \"not\"?")),
            "{warnings:?}"
        );
        // A whole-canvas gradient in a named group is fine (no small shapes
        // to mislead), and so is a part whose shapes are big.
        let fine = warnings_of(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 400, "height": 400, "paper": "#FFFFFF" },
            { "cmd": "group", "name": "sky", "commands": [
              { "cmd": "gradient", "from": [0, 0], "to": [0, 400], "colors": ["#2040A0", "#FFFFFF"] } ] },
            { "cmd": "group", "name": "ground", "commands": [
              { "cmd": "rect", "x": 0, "y": 200, "width": 400, "height": 200, "fill": "#308030" },
              { "cmd": "gradient", "from": [0, 200], "to": [0, 400], "colors": ["#00000040", "transparent"] } ] } ] }"##,
        );
        assert!(fine.iter().all(|w| !w.contains("painted")), "{fine:?}");
    }

    // ---- Outlines and rim light (phase 3, step 5) --------------------------------

    const TWO_LAYER_DUCK: &str = r##"{ "palette": { "ink": "#000000", "body": "#FFD000", "cream": "#FFFFFF" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 200, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "duck", "commands": [
              { "cmd": "layer", "name": "Body" },
              { "cmd": "rect", "x": -30, "y": -10, "width": 60, "height": 40, "fill": "body" },
              { "cmd": "layer", "name": "Head" },
              { "cmd": "rect", "x": 10, "y": -40, "width": 30, "height": 40, "fill": "body" } ] },
            { "cmd": "layer", "name": "Body" },
            { "cmd": "layer", "name": "Head" },
            USE ] }"##;

    #[test]
    fn an_outline_goes_around_the_whole_silhouette_behind_it() {
        let document = play(&TWO_LAYER_DUCK.replace(
            "USE",
            r##"{ "cmd": "use", "name": "duck", "translate": [100, 100], "outline": { "color": "ink", "size": 4 } }"##,
        ));
        let body = &document.layers()[0].surface;
        let head = &document.layers()[1].surface;
        assert_eq!(
            body.pixel(67, 100),
            Rgba8::BLACK,
            "around the body, on the body layer"
        );
        assert_eq!(
            body.pixel(125, 57),
            Rgba8::BLACK,
            "around the head, also on the body layer"
        );
        assert_eq!(head.pixel(125, 57).a, 0);
        assert_eq!(
            seen(&document, 100, 100),
            Rgba8::new(255, 208, 0, 255),
            "the body on top"
        );
        assert_eq!(
            seen(&document, 115, 85),
            Rgba8::new(255, 208, 0, 255),
            "no line where head meets body"
        );
        assert_eq!(seen(&document, 125, 75), Rgba8::new(255, 208, 0, 255));
        assert_eq!(seen(&document, 60, 100), Rgba8::WHITE, "not 10 pixels out");
        // With a rim from the right: light along the inside of the right
        // edges, on the top layer, over the drawing.
        let lit = play(&TWO_LAYER_DUCK.replace(
            "USE",
            r##"{ "cmd": "use", "name": "duck", "translate": [100, 100], "outline": { "color": "ink", "size": 2, "rim": { "color": "cream", "size": 2, "angle": 0 } } }"##,
        ));
        assert_eq!(
            seen(&lit, 129, 110),
            Rgba8::WHITE,
            "the body's right edge, inside"
        );
        assert_eq!(seen(&lit, 139, 80), Rgba8::WHITE, "the head's right edge");
        assert_eq!(
            seen(&lit, 71, 110),
            Rgba8::new(255, 208, 0, 255),
            "the left edge is not lit"
        );
        assert_eq!(
            lit.layers()[1].surface.pixel(129, 110),
            Rgba8::WHITE,
            "the rim is on the top layer"
        );
        // A mirrored duck is outlined where it is.
        let mirrored = play(&TWO_LAYER_DUCK.replace(
            "USE",
            r##"{ "cmd": "use", "name": "duck", "translate": [100, 100], "mirror": "x", "outline": { "color": "ink", "size": 4 } }"##,
        ));
        assert_eq!(seen(&mirrored, 75, 57), Rgba8::BLACK);
        assert_eq!(seen(&mirrored, 125, 57), Rgba8::WHITE);
    }

    #[test]
    fn outline_sizes_may_be_keyframes_and_pixel_outlines_are_whole_pixels() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" } ],
          "animations": { "grow": { "frames": 5, "commands": [
            { "cmd": "group", "outline": { "color": "#000000", "size": { "keys": [[0, 0], [4, 8]] } }, "commands": [
              { "cmd": "rect", "x": 40, "y": 40, "width": 20, "height": 20, "fill": "#FF0000" } ] } ] } } }"##;
        let frame = play_frame(script, Some("grow"), 2).unwrap();
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "group", "outline": { "color": "#000000", "size": 4 }, "commands": [
              { "cmd": "rect", "x": 40, "y": 40, "width": 20, "height": 20, "fill": "#FF0000" } ] } ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
        assert_eq!(seen(&by_hand, 37, 50), Rgba8::BLACK);
        let pixel = play(
            r##"{ "palette": { "k": "#000000", "r": "#FF0000" }, "commands": [
            { "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 1 } },
            { "cmd": "group", "outline": { "color": "k", "size": 2, "rim": { "color": "r", "size": 1, "angle": 0 } }, "commands": [
              { "cmd": "rect", "x": 10, "y": 10, "width": 10, "height": 10, "fill": "r" } ] } ] }"##,
        );
        let layer = &pixel.layers()[0].surface;
        for y in 0..32 {
            for x in 0..32 {
                let a = layer.pixel(x, y).a;
                assert!(a == 0 || a == 255, "({x}, {y}): {a}");
            }
        }
        assert_eq!(layer.pixel(8, 15), Rgba8::BLACK);
        assert_eq!(layer.pixel(7, 15).a, 0);
        // The filter command takes an outline too.
        let filtered = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 40, "y": 40, "width": 20, "height": 20, "fill": "#FF0000" },
            { "cmd": "filter", "outline": { "color": "#0000FF", "size": 3 } } ] }"##,
        );
        assert_eq!(seen(&filtered, 38, 50), Rgba8::new(0, 0, 255, 255));
        assert_eq!(seen(&filtered, 50, 50), Rgba8::new(255, 0, 0, 255));
        let bad = error(
            r##"{ "commands": [ { "cmd": "canvas", "width": 10, "height": 10 },
              { "cmd": "group", "outline": { "width": 3 }, "commands": [] } ] }"##,
        );
        assert!(bad.contains("`outline`: unknown field `width`"), "{bad}");
    }

    // ---- include (phase 3, step 6) -----------------------------------------------

    /// A folder with library files, for include tests.
    fn library() -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "quill-include-{}-{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("t")
                .replace("::", "-")
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("parts")).unwrap();
        std::fs::write(
            folder.join("parts/cloud.json"),
            r##"{ "palette": { "cloud": "#FFFFFF", "shade": "#C0C8D8" }, "commands": [
              { "cmd": "include", "file": "puff.json" },
              { "cmd": "define", "name": "cloud", "params": { "puff": 1 }, "commands": [
                { "cmd": "group", "scale": "$puff", "commands": [
                  { "cmd": "use", "name": "puff", "translate": [-20, 0] },
                  { "cmd": "use", "name": "puff", "translate": [20, 0] } ] } ] } ] }"##,
        )
        .unwrap();
        std::fs::write(
            folder.join("parts/puff.json"),
            r##"{ "palette": { "cloud": "#FFFFFF" }, "commands": [
              { "cmd": "define", "name": "puff", "commands": [
                { "cmd": "ellipse", "center": [0, 0], "radius": 24, "fill": "cloud" } ] } ] }"##,
        )
        .unwrap();
        std::fs::write(
            folder.join("parts/loop-a.json"),
            r##"{ "commands": [ { "cmd": "include", "file": "loop-b.json" } ] }"##,
        )
        .unwrap();
        std::fs::write(
            folder.join("parts/loop-b.json"),
            r##"{ "commands": [ { "cmd": "include", "file": "loop-a.json" } ] }"##,
        )
        .unwrap();
        std::fs::write(
            folder.join("parts/paints.json"),
            r##"{ "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, "fill": "#000000" } ] }"##,
        )
        .unwrap();
        std::fs::write(
            folder.join("parts/broken.json"),
            r##"{ "commands": [ { "cmd": "define", "name": "bad", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "nope" } ] } ] }"##,
        )
        .unwrap();
        folder
    }

    fn play_in(folder: &std::path::Path, text: &str) -> Result<(Document, Report), ScriptError> {
        let script = Script::parse(text)?;
        let options = PlayOptions {
            folder: Some(folder.to_path_buf()),
            ..PlayOptions::default()
        };
        let mut player = Player::new(script, options)?;
        let mut document = player.create_document()?;
        player.run(&mut document, |_, _| {})?;
        Ok((document, player.report()))
    }

    #[test]
    fn include_brings_in_defines_and_palette_from_another_file_and_its_includes() {
        let folder = library();
        let (document, report) = play_in(
            &folder,
            r##"{ "palette": { "sky": "#8EC1E6" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "sky" },
            { "cmd": "include", "file": "parts/cloud.json" },
            { "cmd": "use", "name": "cloud", "translate": [100, 50], "with": { "puff": 0.5 } },
            { "cmd": "rect", "x": 0, "y": 90, "width": 200, "height": 10, "fill": "shade" } ] }"##,
        )
        .unwrap();
        assert_eq!(
            seen(&document, 100, 50),
            Rgba8::WHITE,
            "the cloud, from the include"
        );
        assert_eq!(
            seen(&document, 100, 95),
            Rgba8::new(0xC0, 0xC8, 0xD8, 255),
            "its colour"
        );
        assert_eq!(seen(&document, 100, 20), Rgba8::new(0x8E, 0xC1, 0xE6, 255));
        // Included symbols and colours aren't "never used" (a library holds more).
        let unused = unused_warnings(&[&report]);
        assert!(unused.is_empty(), "{unused:?}");
        assert!(
            report.parts.iter().any(|p| p
                .path
                .contains("use \"cloud\" (from parts/cloud.json, command 2)")),
            "{:?}",
            report.parts
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn include_mistakes_name_the_files() {
        let folder = library();
        let error_in = |text: &str| play_in(&folder, text).unwrap_err().to_string();
        let script = |include: &str, rest: &str| {
            format!(
                r##"{{ "commands": [ {{ "cmd": "canvas", "width": 50, "height": 50 }},
                {{ "cmd": "include", "file": "{include}" }} {rest} ] }}"##
            )
        };
        let missing = error_in(&script("parts/nope.json", ""));
        assert!(
            missing.contains("can't read \"parts/nope.json\""),
            "{missing}"
        );
        let looped = error_in(&script("parts/loop-a.json", ""));
        assert!(looped.contains("includes itself"), "{looped}");
        let paints = error_in(&script("parts/paints.json", ""));
        assert!(
            paints.contains(
                "parts/paints.json: command 1 (rect): an included file holds only `define`"
            ),
            "{paints}"
        );
        let twice = error_in(&script(
            "parts/cloud.json",
            r##", { "cmd": "define", "name": "puff", "commands": [] }"##,
        ));
        assert!(
            twice.contains(
                "\"puff\" was already defined by parts/cloud.json > puff.json (command 1)"
            ),
            "{twice}"
        );
        let twice_included = error_in(&script(
            "parts/cloud.json",
            r##", { "cmd": "include", "file": "parts/puff.json" }"##,
        ));
        assert!(twice_included.contains("defines \"puff\", which is also defined by parts/cloud.json > puff.json (command 1)"), "{twice_included}");
        let clash = error_in(
            r##"{ "palette": { "cloud": "#000000" }, "commands": [ { "cmd": "canvas", "width": 50, "height": 50 },
            { "cmd": "include", "file": "parts/puff.json" } ] }"##,
        );
        assert!(
            clash.contains(
                "palette colour \"cloud\" is also defined, differently, by the script itself"
            ),
            "{clash}"
        );
        let inside = error_in(&script(
            "parts/broken.json",
            r##", { "cmd": "use", "name": "bad" }"##,
        ));
        assert!(
            inside.contains("(use \"bad\" (from parts/broken.json, command 1)) > 1 (ellipse)"),
            "{inside}"
        );
        let nested = Script::parse(
            r##"{ "commands": [ { "cmd": "canvas", "width": 50, "height": 50 },
            { "cmd": "group", "commands": [ { "cmd": "include", "file": "x.json" } ] } ] }"##,
        )
        .unwrap_err()
        .to_string();
        assert!(
            nested.contains("`include` only works at the top level"),
            "{nested}"
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    // ---- Phase 2 review fixes (phase 3, step 7) ---------------------------------

    #[test]
    fn keys_may_be_in_seconds_and_animations_keep_their_written_order() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" } ],
          "animations": {
            "zebra": { "frames": 24, "fps": 12, "commands": [
              { "cmd": "group", "translate": { "keys": [[0, [0, 0]], ["1.5s", [60, 0]]] }, "commands": [
                { "cmd": "rect", "x": 0, "y": 40, "width": 10, "height": 10, "fill": "#000000" } ] } ] },
            "apple": { "frames": 24, "fps": 12, "time": "seconds", "commands": [
              { "cmd": "group", "translate": { "keys": [[0, [0, 0]], [1.5, [60, 0]]], "shift": "0.5s" }, "commands": [
                { "cmd": "rect", "x": 0, "y": 40, "width": 10, "height": 10, "fill": "#000000" } ] } ] } } }"##;
        let parsed = Script::parse(script).unwrap();
        let names: Vec<&String> = parsed.animations.keys().collect();
        assert_eq!(names, ["zebra", "apple"], "as written, not alphabetical");
        assert!(parsed.animations["apple"].seconds);
        // "1.5s" at 12 fps is frame 18: half way at frame 9.
        let frame = play_frame(script, Some("zebra"), 9).unwrap();
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 30, "y": 40, "width": 10, "height": 10, "fill": "#000000" } ] }"##,
        );
        assert_eq!(png(&frame), png(&by_hand));
        // In seconds, with a shift of half a second (6 frames): frame 15 is half way.
        let shifted = play_frame(script, Some("apple"), 15).unwrap();
        assert_eq!(png(&shifted), png(&by_hand));
        // The first animation written is the one played by default.
        let default = play_with(
            script,
            PlayOptions {
                frame: 9,
                ..PlayOptions::default()
            },
        )
        .unwrap();
        assert_eq!(png(&default), png(&by_hand));
        let bad = Script::parse(&script.replace("\"time\": \"seconds\"", "\"time\": \"minutes\""))
            .unwrap_err();
        assert!(bad.to_string().contains("`time` is \"frames\""), "{bad}");
        let twice = Script::parse(&script.replace("\"apple\"", "\"zebra\"")).unwrap_err();
        assert!(twice.to_string().contains("written twice"), "{twice}");
        let bad_key = play_frame(&script.replace("\"1.5s\"", "\"soon\""), Some("zebra"), 0)
            .unwrap_err()
            .to_string();
        assert!(
            bad_key.contains("seconds like \"1.5s\", not \"soon\""),
            "{bad_key}"
        );
    }

    #[test]
    fn seed_keyframes_hold_until_the_next_key() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" } ],
          "animations": { "twinkle": { "frames": 6, "commands": [
            { "cmd": "scatter", "area": { "rect": [0, 0, 100, 100] }, "count": 30, "size": [2, 4], "color": "#000000",
              "seed": { "keys": [[0, 1], [3, 2]] } } ] } } }"##;
        let frames: Vec<Vec<u8>> = (0..6)
            .map(|f| png(&play_frame(script, Some("twinkle"), f).unwrap()))
            .collect();
        assert_eq!(frames[0], frames[1], "seed 1 holds");
        assert_eq!(frames[1], frames[2]);
        assert_ne!(frames[2], frames[3], "the seed changes at the key");
        assert_eq!(frames[3], frames[5], "seed 2 holds");
        let by_hand = play(
            r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "scatter", "area": { "rect": [0, 0, 100, 100] }, "count": 30, "size": [2, 4], "color": "#000000", "seed": 2 } ] }"##,
        );
        assert_eq!(frames[4], png(&by_hand));
    }

    #[test]
    fn silhouettes_record_where_each_named_part_painted() {
        let text = r##"{ "palette": { "ink": "#202020", "red": "#E02020" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "dot", "commands": [
              { "cmd": "ellipse", "center": [0, 0], "radius": 10, "fill": "red" } ] },
            { "cmd": "group", "name": "duck", "translate": [100, 50], "commands": [
              { "cmd": "rect", "x": -20, "y": -10, "width": 40, "height": 20, "fill": "ink" },
              { "cmd": "use", "name": "dot", "translate": [40, 0] } ] },
            { "cmd": "group", "name": "nothing", "commands": [] } ] }"##;
        let script = Script::parse(text).unwrap();
        let options = PlayOptions {
            silhouettes: true,
            ..PlayOptions::default()
        };
        let mut player = Player::new(script.clone(), options).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let report = player.report();
        let names: Vec<&str> = report.parts.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            names,
            [
                "command 3 (group \"duck\")",
                "command 3 (group \"duck\") > 2 (use \"dot\")",
                "command 4 (group \"nothing\")"
            ]
        );
        // The inner part finishes first; the empty group has no silhouette.
        let order: Vec<usize> = report.silhouettes.iter().map(|s| s.part).collect();
        assert_eq!(order, [1, 0]);
        let dot = &report.silhouettes[0].mask;
        let duck = &report.silhouettes[1].mask;
        // The duck's own paint: the rect in ink, the dot in red, nothing else.
        let own = &report.silhouettes[1].own;
        let rect = duck.rect();
        assert_eq!(
            own.pixel((100 - rect.left) as u32, (50 - rect.top) as u32),
            [0x20, 0x20, 0x20, 255]
        );
        assert_eq!(
            own.pixel((140 - rect.left) as u32, (50 - rect.top) as u32),
            [0xE0, 0x20, 0x20, 255]
        );
        assert_eq!(dot.get(140, 50), 255, "inside the dot");
        assert_eq!(dot.get(100, 50), 0, "the rect isn't the dot's");
        assert_eq!(duck.get(100, 50), 255, "the rect");
        assert_eq!(duck.get(140, 50), 255, "the dot is part of the duck");
        assert_eq!(duck.get(10, 10), 0, "paper");
        // Looking doesn't change the picture.
        let plain = play(text);
        assert_eq!(png(&plain), png(&document));
    }

    #[test]
    fn the_focal_point_is_found_from_the_named_part() {
        let text = r##"{ "intent": { "focal": "duck" }, "commands": [
            { "cmd": "canvas", "width": 200, "height": 100 },
            { "cmd": "group", "name": "duck", "translate": [100, 50], "commands": [
              { "cmd": "rect", "x": -20, "y": -10, "width": 40, "height": 20, "fill": "#000000" } ] } ] }"##;
        let script = Script::parse(text).unwrap();
        let mut player = Player::new(script.clone(), PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let report = player.report();
        let intent = script.intent.as_ref().unwrap();
        assert_eq!(
            focal_point(intent, &report, (200, 100)).unwrap(),
            Some((100.0, 50.0))
        );
        let missing = super::super::Intent {
            focal: Some(serde_json::json!("goose")),
            ..Default::default()
        };
        let e = focal_point(&missing, &report, (200, 100)).unwrap_err();
        assert!(
            e.contains("names \"goose\", but no group or use is called that"),
            "{e}"
        );
        let point = super::super::Intent {
            focal: Some(serde_json::json!([5, 6])),
            ..Default::default()
        };
        assert_eq!(
            focal_point(&point, &report, (200, 100)).unwrap(),
            Some((5.0, 6.0))
        );
    }

    // ---- Brush presets and the pencil (phase 5, step 5) ----------------------

    const PENCIL_SCRIPT: &str = r##"{ "commands": [
        { "cmd": "canvas", "width": 300, "height": 60, "paper": "#FFFFFF" },
        { "cmd": "brush", "preset": "Pencil (graphite)", "color": "#000000", "size": 12 },
        { "cmd": "stroke", "points": [[20, 30, PRESSURE], [280, 30, PRESSURE]] }
      ] }"##;

    /// Of the pixels in the middle of the pencil line, how many got no
    /// graphite at all, and the darkest one.
    fn holes_and_darkest(document: &Document) -> (usize, u8) {
        let layer = &document.layers()[0].surface;
        let mut holes = 0;
        let mut darkest = 0;
        for y in 27..=33 {
            for x in 40..260 {
                let a = layer.pixel(x, y).a;
                holes += usize::from(a == 0);
                darkest = darkest.max(a);
            }
        }
        (holes, darkest)
    }

    #[test]
    fn a_light_pencil_touch_shows_the_paper_grain_and_pressing_fills_it() {
        let light = play(&PENCIL_SCRIPT.replace("PRESSURE", "0.2"));
        let hard = play(&PENCIL_SCRIPT.replace("PRESSURE", "1"));
        let (light_holes, light_dark) = holes_and_darkest(&light);
        let (hard_holes, hard_dark) = holes_and_darkest(&hard);
        assert!(light_holes > 7 * 220 / 5, "light: grainy ({light_holes})");
        assert!(
            hard_holes < light_holes / 4,
            "hard: filled in ({hard_holes})"
        );
        assert!(light_dark < hard_dark / 2, "{light_dark} vs {hard_dark}");
        // The grain stays put on the canvas: the same stroke a little
        // further along leaves holes in exactly the same places.
        let moved = play(
            &PENCIL_SCRIPT
                .replace("PRESSURE", "0.2")
                .replace("[20, 30", "[10, 30"),
        );
        let hole = |d: &Document, x, y| d.layers()[0].surface.pixel(x, y).a == 0;
        for y in 28..=32 {
            for x in 60..240 {
                assert_eq!(hole(&light, x, y), hole(&moved, x, y), "({x}, {y})");
            }
        }
    }

    /// The share of pixels with no graphite in rows `ys`, along the line.
    fn hole_share(document: &Document, ys: impl Iterator<Item = i32> + Clone) -> f32 {
        let layer = &document.layers()[0].surface;
        let (mut holes, mut all) = (0, 0);
        for y in ys {
            for x in 40..260 {
                holes += usize::from(layer.pixel(x, y).a == 0);
                all += 1;
            }
        }
        holes as f32 / all as f32
    }

    #[test]
    fn the_pencils_edge_breaks_up_and_pressure_fills_the_middle() {
        let hard = play(&PENCIL_SCRIPT.replace("PRESSURE", "1"));
        let light = play(&PENCIL_SCRIPT.replace("PRESSURE", "0.2"));
        let middle = |d: &Document| hole_share(d, 29..=31);
        // The outer rows of a 12-wide line (its radius at full pressure
        // is 6): ragged, not a clean capsule edge.
        let edge = |d: &Document| hole_share(d, [25, 26, 34, 35].into_iter());
        assert!(
            middle(&hard) < 0.05,
            "hard: only a few specks ({})",
            middle(&hard)
        );
        assert!(edge(&hard) > 0.25, "hard: a broken edge ({})", edge(&hard));
        assert!(
            middle(&light) > 0.35,
            "light: most valleys white ({})",
            middle(&light)
        );
        assert!(
            edge(&light) > middle(&light),
            "the edge breaks more than the middle"
        );
    }

    #[test]
    fn going_over_pencil_again_fills_the_valleys() {
        let passes = |n: usize| {
            let stroke = r#"{ "cmd": "stroke", "points": [[20, 30, 0.55], [280, 30, 0.55]] }"#;
            let script = PENCIL_SCRIPT.replace(
                r#"{ "cmd": "stroke", "points": [[20, 30, PRESSURE], [280, 30, PRESSURE]] }"#,
                &vec![stroke; n].join(","),
            );
            hole_share(&play(&script), 29..=31)
        };
        let (one, eight) = (passes(1), passes(8));
        assert!(one > 0.15, "one pass shows the tooth ({one})");
        assert!(
            eight < one / 3.0,
            "eight passes fill most of it ({one} -> {eight})"
        );
    }

    #[test]
    fn going_over_pencil_again_darkens_it_only_so_far() {
        let again = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 60, "paper": "#FFFFFF" },
            { "cmd": "brush", "preset": "Pencil (graphite)", "color": "#000000", "size": 20, "grain": 0 },
            REPEATS
          ] }"##;
        let darkest = |times: usize| {
            let stroke = r#"{ "cmd": "stroke", "points": [[20, 30], [80, 30]] }"#;
            let document = play(&again.replace("REPEATS", &vec![stroke; times].join(",")));
            document.layers()[0].surface.pixel(50, 30).a
        };
        let limit = (find_preset("Pencil (graphite)").build_up * 255.0) as u8;
        assert!(darkest(1) < darkest(3), "it does build up");
        let many = darkest(12);
        assert!(many <= limit + 1 && many + 6 >= limit, "{many} vs {limit}");
        // Without a limit (the plain brush) it goes all the way.
        let plain = play(&again.replace("REPEATS", r#"{ "cmd": "brush", "build_up": 0, "opacity": 0.5 },
            { "cmd": "stroke", "points": [[20, 30], [80, 30]] }, { "cmd": "stroke", "points": [[20, 30], [80, 30]] },
            { "cmd": "stroke", "points": [[20, 30], [80, 30]] }, { "cmd": "stroke", "points": [[20, 30], [80, 30]] },
            { "cmd": "stroke", "points": [[20, 30], [80, 30]] }, { "cmd": "stroke", "points": [[20, 30], [80, 30]] },
            { "cmd": "stroke", "points": [[20, 30], [80, 30]] }, { "cmd": "stroke", "points": [[20, 30], [80, 30]] }"#));
        assert!(plain.layers()[0].surface.pixel(50, 30).a > limit + 20);
    }

    fn find_preset(name: &str) -> BrushSettings {
        crate::presets::find(name).unwrap().settings().unwrap()
    }

    #[test]
    fn a_preset_in_a_script_is_the_same_as_its_settings_written_out() {
        for preset in crate::presets::built_in() {
            let with_preset = format!(
                r##"{{ "commands": [
                    {{ "cmd": "canvas", "width": 200, "height": 80, "paper": "#FFFFFF" }},
                    {{ "cmd": "brush", "color": "#3050A0" }},
                    {{ "cmd": "brush", "preset": "{}" }},
                    {{ "cmd": "stroke", "points": [[20, 40, 0.3], [100, 30, 0.7], [180, 40, 1]] }}
                  ] }}"##,
                preset.name
            );
            let mut written = preset.brush.clone();
            written.color = Some(Value::String("#3050A0".into()));
            let settings = serde_json::to_string(&Command::Brush(written)).unwrap();
            let reset = if preset.eraser {
                // The eraser preset's own starting point (the `erase`
                // command makes any brush erase).
                r#"{ "cmd": "brush", "reset": true, "size": 40, "hardness": 0.6, "pressure_opacity": false },"#
            } else {
                r#"{ "cmd": "brush", "reset": true },"#
            };
            let by_hand = with_preset
                .replace(r##"{ "cmd": "brush", "color": "#3050A0" },"##, reset)
                .replace(
                    &format!(r#"{{ "cmd": "brush", "preset": "{}" }}"#, preset.name),
                    &settings,
                );
            assert_eq!(
                png(&play(&with_preset)),
                png(&play(&by_hand)),
                "{}: the colour stays, the rest is the preset's",
                preset.name
            );
        }
        // Fields after the preset change it; capitals don't matter.
        let bigger = play(
            &PENCIL_SCRIPT
                .replace("PRESSURE", "1")
                .replace("\"size\": 12", "\"size\": 30"),
        );
        let pencil = play(
            &PENCIL_SCRIPT
                .replace("PRESSURE", "1")
                .replace("Pencil (graphite)", "PENCIL (GRAPHITE)"),
        );
        assert_ne!(png(&bigger), png(&pencil));
    }

    #[test]
    fn an_unknown_preset_lists_the_ones_there_are() {
        let problem = error(
            &PENCIL_SCRIPT
                .replace("PRESSURE", "1")
                .replace("Pencil (graphite)", "Biro"),
        );
        assert!(problem.contains("unknown preset \"Biro\""), "{problem}");
        assert!(problem.contains("Pencil (graphite), Ink pen"), "{problem}");
        let problem = error(
            &PENCIL_SCRIPT
                .replace("PRESSURE", "1")
                .replace("\"size\": 12", "\"grain\": 2"),
        );
        assert!(problem.contains("`grain` must be from 0 to 1"), "{problem}");
    }

    #[test]
    fn a_recorded_brush_after_the_pencil_loses_its_grain() {
        // The app records every brush change; after the pencil, a plain
        // brush must come out plain when the recording is played.
        let pencil = find_preset("Pencil (graphite)");
        let plain = BrushSettings::default();
        let line = |settings: &BrushSettings, y: f32| {
            crate::script::record::stroke(
                "Ink",
                settings,
                &[
                    StrokePoint::new(20.0, y, 0.5),
                    StrokePoint::new(180.0, y, 0.5),
                ],
                None,
            )
        };
        let mut both = vec![crate::script::record::brush(&pencil, true)];
        both.extend(line(&pencil, 10.0));
        both.push(crate::script::record::brush(&plain, true));
        both.extend(line(&plain, 40.0));
        let mut only_plain = vec![crate::script::record::brush(&plain, true)];
        only_plain.extend(line(&plain, 40.0));
        let render = |commands: Vec<Command>| {
            let script = crate::script::record::script(200, 80, Rgba8::WHITE, commands, None, None);
            let text = script.to_json();
            play(&text)
        };
        let both = render(both);
        let plain_only = render(only_plain);
        // The second line (drawn plain) is the same in both.
        for x in 30..170 {
            for y in 30..50 {
                assert_eq!(
                    both.layers()[0].surface.pixel(x, y),
                    plain_only.layers()[0].surface.pixel(x, y)
                );
            }
        }
    }

    // ---- Symmetry (phase 5, step 6) ------------------------------------------

    fn mirror_script(symmetry: &str, points: &str) -> String {
        format!(
            r##"{{ "commands": [
                {{ "cmd": "canvas", "width": 200, "height": 120, "paper": "#FFFFFF" }},
                {{ "cmd": "brush", "size": 14, "color": "#000000", "opacity": 0.5, "hardness": 0.7 }},
                {{ "cmd": "stroke", "points": {points}, "symmetry": {symmetry} }}
              ] }}"##
        )
    }

    #[test]
    fn the_mirror_copies_land_across_the_lines() {
        let both = Symmetry {
            centre: (100.0, 40.0),
            vertical: true,
            horizontal: true,
            angle: 0.0,
        };
        let copies = both.copies((90.0, 50.0), 30.0);
        let close = |(x, y): (f32, f32), (ex, ey): (f32, f32)| {
            (x - ex).abs() < 1e-3 && (y - ey).abs() < 1e-3
        };
        assert!(close(copies[0].0, (110.0, 50.0)) && (copies[0].1 - 150.0).abs() < 1e-3);
        assert!(close(copies[1].0, (90.0, 30.0)) && (copies[1].1 + 30.0).abs() < 1e-3);
        assert!(close(copies[2].0, (110.0, 30.0)));
        // Turned a quarter: the "down" line now goes across.
        let turned = Symmetry {
            angle: 90.0,
            horizontal: false,
            ..both
        };
        assert!(close(turned.copies((90.0, 50.0), 0.0)[0].0, (90.0, 30.0)));
    }

    #[test]
    fn a_mirrored_stroke_is_its_own_mirror_image_and_never_doubles_up() {
        let layer = |d: &Document| d.layers()[0].surface.clone();
        let one = play(&mirror_script(
            r#"{ "x": 100 }"#,
            "[[30, 30], [120, 60], [60, 100]]",
        ));
        let surface = layer(&one);
        for y in 0..120 {
            for x in 0..200 {
                assert_eq!(surface.pixel(x, y), surface.pixel(199 - x, y), "({x}, {y})");
            }
        }
        // The stroke crosses the line, where the copies overlap: still no
        // darker than the brush's opacity.
        let darkest = (0..200)
            .flat_map(|x| (0..120).map(move |y| (x, y)))
            .map(|(x, y)| surface.pixel(x, y).a)
            .max()
            .unwrap();
        assert!(darkest <= 128, "{darkest}");
        // Both lines: four copies, mirrored both ways.
        let four = play(&mirror_script(
            r#"{ "x": 100, "y": 60 }"#,
            "[[30, 20], [80, 40]]",
        ));
        let surface = layer(&four);
        for y in 0..120 {
            for x in 0..200 {
                let p = surface.pixel(x, y);
                assert_eq!(p, surface.pixel(199 - x, y));
                assert_eq!(p, surface.pixel(x, 119 - y));
            }
        }
        assert!(surface.pixel(55, 30).a > 0 && surface.pixel(144, 89).a > 0);
        // Erasing is mirrored too.
        let erased = play(&mirror_script(r#"{ "x": 100 }"#, "[[20, 60], [60, 60]]").replace(
            r#"{ "cmd": "stroke", "points": [[20, 60], [60, 60]]"#,
            r##"{ "cmd": "rect", "x": 0, "y": 0, "width": 200, "height": 120, "fill": "#000000" },
               { "cmd": "erase", "points": [[20, 60], [60, 60]]"##,
        ));
        let surface = layer(&erased);
        assert!(surface.pixel(40, 60).a < 255 && surface.pixel(159, 60).a < 255);
        assert_eq!(surface.pixel(100, 60).a, 255, "the middle is left alone");
    }

    #[test]
    fn symmetry_moves_with_the_group_and_says_what_it_needs() {
        // In a group moved 50 to the right, the line at x 50 is at 100.
        let moved = play(
            &mirror_script(r#"{ "x": 50 }"#, "[[0, 30], [20, 60]]")
                .replace(
                    r#"{ "cmd": "stroke""#,
                    r#"{ "cmd": "group", "translate": [50, 0], "commands": [ { "cmd": "stroke""#,
                )
                .replace(
                    r#""symmetry": { "x": 50 } }"#,
                    r#""symmetry": { "x": 50 } } ] }"#,
                ),
        );
        let surface = &moved.layers()[0].surface;
        assert!(surface.pixel(60, 45).a > 0);
        for y in 0..120 {
            for x in 0..200 {
                assert_eq!(surface.pixel(x, y), surface.pixel(199 - x, y));
            }
        }
        let problem = error(&mirror_script("{}", "[[0, 30], [20, 60]]"));
        assert!(problem.contains("`symmetry` needs"), "{problem}");
    }

    #[test]
    fn the_apps_mirrored_strokes_replay_exactly() {
        let symmetry = Symmetry {
            centre: (100.0, 60.0),
            vertical: true,
            horizontal: true,
            angle: 0.0,
        };
        let brush = BrushSettings {
            size: 12.0,
            ..BrushSettings::default()
        };
        let points = [
            StrokePoint::new(20.0, 20.0, 0.4),
            StrokePoint::new(70.0, 50.0, 1.0),
            StrokePoint::new(95.0, 90.0, 0.6),
        ];
        // As the app paints it.
        let mut document = Document::new(200, 120);
        let mut stroke = Stroke::new(brush, document.bounds()).mirrored(symmetry);
        let surface = &mut document.active_layer_mut().surface;
        for point in points {
            stroke.add_point(surface, point);
        }
        stroke.finish(surface);
        // As Export strokes writes it, played.
        let name = document.active_layer().name().to_owned();
        let mut commands = vec![crate::script::record::brush(&brush, true)];
        let mut stroke = crate::script::record::stroke(&name, &brush, &points, None);
        crate::script::record::mirror(&mut stroke, Some(&symmetry));
        commands.extend(stroke);
        let script = crate::script::record::script(200, 120, Rgba8::WHITE, commands, None, None);
        assert!(
            script
                .to_json()
                .contains(r#""symmetry":{"x":100.0,"y":60.0}"#),
            "{}",
            script.to_json()
        );
        let played = play(&script.to_json());
        for y in 0..120 {
            for x in 0..200 {
                assert_eq!(
                    played.layers()[0].surface.pixel(x, y),
                    document.layers()[0].surface.pixel(x, y),
                    "({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_shadow_layer_past_the_edge_stays_out_of_a_transparent_export() {
        // A character, and a multiply shadow painted a bit too big: with no
        // paper, the shadow mustn't leave a dark band beside the character.
        let document = play(
            r##"{ "commands": [
                { "cmd": "canvas", "width": 60, "height": 40 },
                { "cmd": "layer", "name": "Body" },
                { "cmd": "rect", "x": 10, "y": 10, "width": 20, "height": 20, "fill": "#E0C060" },
                { "cmd": "layer", "name": "Shade", "blend": "multiply", "opacity": 0.6 },
                { "cmd": "rect", "x": 20, "y": 5, "width": 30, "height": 30, "fill": "#403060" }
            ] }"##,
        );
        let transparent = |x, y| document.composite_pixel(x, y, Background::Transparent);
        assert_eq!(transparent(40, 20).a, 0, "beside the body: still empty");
        assert_eq!(transparent(15, 20).a, 255, "the body, unshaded");
        let shaded = transparent(25, 20);
        assert_eq!(shaded.a, 255);
        assert!(shaded.r < transparent(15, 20).r, "the body, shaded");
        // On paper, the shadow darkens the paper as it always did.
        let paper = |x, y| document.composite_pixel(x, y, Background::Paper);
        assert!(paper(40, 20).r < 255);
    }

    #[test]
    fn uneven_scales_keep_lines_unstretched_unless_asked_and_check_warns() {
        let line_width = |extra: &str| {
            let script = format!(
                r##"{{ "commands": [
                    {{ "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" }},
                    {{ "cmd": "group", "name": "wide", "scale": [3, 1], {extra} "commands": [
                        {{ "cmd": "stroke", "points": [[10, 20], [50, 20]], "size": 6, "hardness": 1, "color": "#000000" }}
                    ] }}
                  ] }}"##
            );
            let script = Script::parse(&script).unwrap();
            let mut player = Player::new(script, PlayOptions::default()).unwrap();
            let mut document = player.create_document().unwrap();
            player.run(&mut document, |_, _| {}).unwrap();
            let layer = &document.layers()[0].surface;
            let width = (0..100).filter(|&y| layer.pixel(90, y).a > 128).count();
            (width, player.report().warnings)
        };
        let (plain, warnings) = line_width("");
        assert!(
            (5..=7).contains(&plain),
            "the line stays about 6 wide ({plain})"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("scaled unevenly") && w.contains("stretch_brush")),
            "{warnings:?}"
        );
        let (stretched, _) = line_width(r#""stretch_brush": true,"#);
        assert!(
            (9..=12).contains(&stretched),
            "the old way: about 6 x 1.73 ({stretched})"
        );
        // An even scale still scales lines, and doesn't warn.
        let even = play(
            r##"{ "commands": [
                { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
                { "cmd": "group", "scale": 2, "commands": [
                    { "cmd": "stroke", "points": [[10, 20], [50, 20]], "size": 6, "hardness": 1, "color": "#000000" }
                ] }
              ] }"##,
        );
        let width = (0..100)
            .filter(|&y| even.layers()[0].surface.pixel(60, y).a > 128)
            .count();
        assert!((11..=13).contains(&width), "{width}");
    }

    #[test]
    fn scale_zero_shrinks_a_part_to_nothing_without_an_error() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 100, "height": 60, "paper": "#FFFFFF" },
            { "cmd": "group", "name": "gone", "scale": 0, "origin": [50, 30], "commands": [
                { "cmd": "layer", "name": "Made inside" },
                { "cmd": "ellipse", "center": [50, 30], "radius": 20, "fill": "#F0C040", "outline": "#202020" },
                { "cmd": "stroke", "points": [[30, 30], [70, 30]], "size": 8 }
            ] },
            { "cmd": "group", "name": "flat", "scale": [1, 0], "origin": [50, 30], "commands": [
                { "cmd": "rect", "x": 10, "y": 10, "width": 30, "height": 30, "fill": "#F0C040" }
            ] },
            { "cmd": "layer", "name": "Made inside" }
          ] }"##;
        let parsed = Script::parse(script).unwrap();
        let mut player = Player::new(parsed, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        for layer in document.layers() {
            assert_eq!(
                layer.surface.tile_count(),
                0,
                "{} has nothing on it",
                layer.name()
            );
        }
        assert!(
            document.layers().iter().any(|l| l.name() == "Made inside"),
            "its layer was made"
        );
        let report = player.report();
        assert!(
            report.parts.iter().all(|p| p.area.is_none()),
            "it drew nothing"
        );
        // Negative scales still say to use mirror.
        let problem = error(&script.replace(r#""scale": 0,"#, r#""scale": -1,"#));
        assert!(problem.contains("mirror"), "{problem}");
    }

    #[test]
    fn symbol_parameters_work_in_points_centres_and_sizes() {
        let script = r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "define", "name": "post", "params": { "top": [0, 0], "w": 4, "path": [[0, 0], [0, 40]] }, "commands": [
                { "cmd": "rect", "x": 0, "y": 0, "width": "$w", "height": 10, "fill": "#000000" },
                { "cmd": "ellipse", "center": "$top", "radius": 3, "fill": "#FF0000" },
                { "cmd": "stroke", "points": "$path", "size": "$w", "color": "#0000FF" }
            ] },
            { "cmd": "use", "name": "post", "translate": [20, 20] },
            { "cmd": "use", "name": "post", "translate": [120, 20], "with": { "w": 12, "top": [30, 5], "path": [[30, 20], [60, 20]] } }
          ] }"##;
        let by_hand = r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 100, "paper": "#FFFFFF" },
            { "cmd": "rect", "x": 20, "y": 20, "width": 4, "height": 10, "fill": "#000000" },
            { "cmd": "ellipse", "center": [20, 20], "radius": 3, "fill": "#FF0000" },
            { "cmd": "stroke", "points": [[20, 20], [20, 60]], "size": 4, "color": "#0000FF" },
            { "cmd": "rect", "x": 120, "y": 20, "width": 12, "height": 10, "fill": "#000000" },
            { "cmd": "ellipse", "center": [150, 25], "radius": 3, "fill": "#FF0000" },
            { "cmd": "stroke", "points": [[150, 40], [180, 40]], "size": 12, "color": "#0000FF" }
          ] }"##;
        assert_eq!(png(&play(script)), png(&play(by_hand)));
        // Written out again (Export strokes, tidy), the "$name"s stay.
        let parsed = Script::parse(script).unwrap();
        let json = parsed.to_json();
        assert!(
            json.contains(r#""width": "$w""#) || json.contains(r#""width":"$w""#),
            "{json}"
        );
        assert_eq!(png(&play(&json)), png(&play(by_hand)));
        // A "$name" that isn't a parameter says so.
        let problem = error(&script.replace(r#""width": "$w""#, r#""width": "$wide""#));
        assert!(
            problem.contains("\"$wide\" would stand for a symbol parameter"),
            "{problem}"
        );
        // A value that doesn't fit says which use gave it.
        let problem = error(&script.replace(r#""w": 12"#, r#""w": "big""#));
        assert!(problem.contains("with these `with` values"), "{problem}");
    }

    #[test]
    fn a_single_shape_can_have_a_name_and_paints_the_same() {
        let named = r##"{ "commands": [
            { "cmd": "canvas", "width": 200, "height": 120, "paper": "#FFFFFF" },
            { "cmd": "rect", "name": "door", "x": 20, "y": 30, "width": 40, "height": 60, "fill": "#804020", "jitter": 3 },
            { "cmd": "group", "name": "house", "commands": [
                { "cmd": "ellipse", "name": "window", "center": [120, 50], "radius": 12, "fill": "#80C0FF" },
                { "cmd": "stroke", "name": "smoke", "points": [[150, 20], [170, 5]], "size": 6, "jitter": 2 }
            ] }
          ] }"##;
        let plain = named
            .replace(r#""name": "door", "#, "")
            .replace(r#""name": "window", "#, "")
            .replace(r#""name": "smoke", "#, "");
        assert_eq!(
            png(&play(named)),
            png(&play(&plain)),
            "a name changes nothing painted"
        );
        let parsed = Script::parse(named).unwrap();
        let mut player = Player::new(parsed, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let report = player.report();
        let paths: Vec<&str> = report.parts.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"command 2 (rect \"door\")"), "{paths:?}");
        assert!(
            paths
                .iter()
                .any(|p| p.ends_with("(ellipse \"window\")") && p.contains("group \"house\"")),
            "{paths:?}"
        );
        let door = report
            .parts
            .iter()
            .find(|p| p.path.contains("door"))
            .unwrap();
        let area = door.area.unwrap();
        assert!(
            area.left <= 20 && area.right >= 60 && area.top <= 30,
            "{area:?}"
        );
        // Mistakes say its name.
        let problem = error(&named.replace(r##""fill": "#80C0FF""##, r#""fill": "nope""#));
        assert!(problem.contains("(ellipse \"window\")"), "{problem}");
    }

    #[test]
    fn outline_colours_count_as_used() {
        let script = r##"{ "palette": { "ink": "#202020", "body": "#F0C040", "rim": "#FFFFFF", "edge": "#101010", "spare": "#00FF00" },
          "commands": [
            { "cmd": "canvas", "width": 32, "height": 32, "pixel": { "outline": "ink" } },
            { "cmd": "group", "name": "duck", "outline": { "color": "rim", "size": 1 }, "commands": [
                { "cmd": "ellipse", "center": [16, 16], "radius": 8, "fill": "body", "outline": "edge" }
            ] }
          ] }"##;
        let parsed = Script::parse(script).unwrap();
        let mut player = Player::new(parsed, PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let report = player.report();
        let warnings = unused_warnings(&[&report]);
        assert_eq!(
            warnings,
            ["palette colour \"spare\" is never used"],
            "only the spare one"
        );
    }

    #[test]
    fn pixel_art_follows_the_centre_rule_and_thin_lines_never_break() {
        // Tiny circles: a pixel is painted when its centre is inside
        // (on the edge counts as outside), so the counts can be worked
        // out in advance.
        let mut commands =
            vec![r#"{ "cmd": "canvas", "width": 56, "height": 16, "pixel": true }"#.to_owned()];
        let radii = [0.5, 1.0, 1.5, 2.0, 2.5];
        for (i, r) in radii.iter().enumerate() {
            let x = 3 + 10 * i;
            commands.push(format!(
                r#"{{ "cmd": "ellipse", "center": [{}, 3.5], "radius": {r}, "fill": "r" }}"#,
                x as f32 + 0.5
            ));
            commands.push(format!(
                r#"{{ "cmd": "ellipse", "center": [{}, 11], "radius": {r}, "fill": "b" }}"#,
                x + 1
            ));
        }
        let script = format!(
            r##"{{ "palette": {{ "r": "#C03030", "b": "#3050C0" }}, "commands": [{}] }}"##,
            commands.join(",")
        );
        let document = play(&script);
        let layer = &document.layers()[0].surface;
        let count = |x0: i32, y0: i32, y1: i32| {
            (x0 - 3..x0 + 4)
                .flat_map(|x| (y0..y1).map(move |y| (x, y)))
                .filter(|&(x, y)| layer.pixel(x, y).a > 0)
                .count()
        };
        let on_centres: Vec<usize> = (0..5).map(|i| count(3 + 10 * i, 0, 8)).collect();
        let on_corners: Vec<usize> = (0..5).map(|i| count(4 + 10 * i, 8, 16)).collect();
        assert_eq!(on_centres, [1, 1, 9, 9, 21], "centred on a pixel's centre");
        assert_eq!(on_corners, [0, 4, 4, 12, 16], "centred on a pixel's corner");

        // Two thin lines crossing at a shallow slant: every column has
        // exactly one pixel of each (no gaps, no doubling), apart from
        // where they cross.
        let lines = play(
            r##"{ "palette": { "k": "#202020", "b": "#3050C0" }, "commands": [
                { "cmd": "canvas", "width": 40, "height": 16, "pixel": { "supersample": 8 } },
                { "cmd": "line", "from": [0.5, 14.5], "to": [39.5, 2.5], "size": 1, "color": "k", "taper": 0 },
                { "cmd": "stroke", "points": [[0.5, 2.5], [39.5, 13.5]], "size": 1, "color": "b" }
            ] }"##,
        );
        let layer = &lines.layers()[0].surface;
        let colour_in = |x: i32, rgb: [u8; 3]| {
            (0..16)
                .filter(|&y| {
                    let p = layer.pixel(x, y);
                    p.a > 0 && [p.r, p.g, p.b] == rgb
                })
                .count()
        };
        let mut both = 0;
        for x in 0..40 {
            let (k, b) = (
                colour_in(x, [0x20, 0x20, 0x20]),
                colour_in(x, [0x30, 0x50, 0xC0]),
            );
            assert!(k <= 1 && b <= 1, "column {x}: doubled ({k}, {b})");
            if k == 1 && b == 1 {
                both += 1;
            }
        }
        assert!(both >= 38, "only {both} of 40 columns have both lines");
    }
}
