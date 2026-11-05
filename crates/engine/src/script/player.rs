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
    BrushCmd, Command, EllipseCmd, FillCmd, GradientCmd, GroupCmd, HatchCmd, LayerCmd, LineCmd,
    MoveLayerCmd, PathCmd, PixelsCmd, Point, PolygonCmd, RectCmd, RepeatCmd, ScatterCmd, Script,
    ScriptError, StrokeCmd, TextCmd, TransformLayerCmd, UseCmd, parse_hex,
};
use crate::brush::{BrushMode, BrushSettings, MAX_SIZE};
use crate::document::{Document, MAX_CANVAS_SIDE};
use crate::file;
use crate::fill::{
    self, FloodFill, Gradient, GradientShape, GradientStop, Paint, Rgba, SampleFrom, paint_mask,
    paint_rect,
};
use crate::history::{Command as UndoStep, TileCommand};
use crate::layer::{BlendMode, LayerId, LayerProps};
use crate::layer_commands::{
    LayerPresenceCommand, LayerPropsCommand, MoveLayerCommand, PaperCommand,
};
use crate::layer_transform::transform_layer;
use crate::pixels::{MAX_SUPERSAMPLE, PixelMode};
use crate::random::Random;
use crate::stroke::{Stroke, StrokePoint, TilesBefore};
use crate::surface::PixelRect;
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
    /// Every symbol defined, with its command number, and the symbols
    /// and palette colours used in this frame: see [`unused_warnings`].
    pub defined: Vec<(String, usize)>,
    pub used_symbols: BTreeSet<String>,
    pub used_colors: BTreeSet<String>,
    pub palette: BTreeSet<String>,
}

/// Warnings about symbols defined but never used and palette colours
/// never used, counted over all of `reports` (every frame of every
/// animation): a symbol swapped in at frame 9 is used.
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
    },
    /// Fills a shape (or the whole layer, if `shape` is `None`). A shape
    /// is one or more outlines filled together (a letter "o" has two).
    Area {
        layer: LayerId,
        shape: Option<Vec<Vec<Pt>>>,
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

/// A stroke being painted, a few points at a time.
struct RunningStroke {
    layer: LayerId,
    stroke: Stroke,
    points: Vec<StrokePoint>,
    next: usize,
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
}

/// A symbol: what `define` stored.
struct Symbol {
    commands: Arc<Vec<Command>>,
    /// Its parameters and their defaults.
    params: Arc<BTreeMap<String, Value>>,
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
}

/// The brush and selected layer from before a group, put back after it.
struct Saved {
    layer: LayerId,
    brush: BrushSettings,
    brush_alpha: u8,
}

/// A group's transform and settings, checked.
struct Placement {
    transform: Transform,
    opacity: f32,
    scale_brush: bool,
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
    warnings: Vec<String>,
    defined: Vec<(String, usize)>,
    used_symbols: HashSet<String>,
    used_colors: RefCell<HashSet<String>>,
    painted_by: HashMap<LayerId, BTreeSet<usize>>,
    /// The animation being played: its name and commands.
    animation: Option<(String, Arc<Vec<Command>>)>,
    /// Those commands, until they're started (after the script's own).
    pending_animation: Option<Arc<Vec<Command>>>,
    /// The frame being painted, counting from 0.
    frame: u32,
    /// The frame and the animation's length, for keyframes.
    clock: Clock,
    /// A pixel canvas: painted this many times bigger, then shrunk.
    pixel: Option<PixelMode>,
    /// How many painted pixels one canvas pixel is: the preview scale
    /// times the pixel canvas's supersample.
    paint_scale: f32,
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
            warnings: Vec::new(),
            defined: Vec::new(),
            used_symbols: HashSet::new(),
            used_colors: RefCell::new(HashSet::new()),
            painted_by: HashMap::new(),
            frame: options.frame,
            clock,
            pixel,
            paint_scale,
            animation,
            pending_animation: None,
            options,
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
            restore: None,
            symbol: None,
            swaps: Vec::new(),
            copies: None,
            path: None,
            drawn: None,
            outside: Vec::new(),
            part: None,
            animation: None,
            params: None,
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
                    restore: None,
                    symbol: None,
                    swaps: Vec::new(),
                    copies: None,
                    path: None,
                    drawn: None,
                    outside: Vec::new(),
                    part: None,
                    animation: name,
                    params: None,
                });
            } else {
                self.flush_tiles();
                if let Some(name) = self.painted.take() {
                    self.events.push_back(Event::LayerDone(name));
                }
                self.finished = true;
            }
        }
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
        match self.frames.last().and_then(|frame| frame.seed_base) {
            None => index as u64 + 1,
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
        let frame = Frame {
            commands: new.commands,
            next: 0,
            label: Some((self.current + 1, new.label)),
            place,
            detail: parent.detail * own.size_factor(),
            brush_scale: if new.placement.scale_brush {
                parent.brush_scale * own.size_factor()
            } else {
                parent.brush_scale
            },
            opacity: parent.opacity * new.placement.opacity,
            seed_base: Some(new.seed_base),
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
            outside: Vec::new(),
            part,
            animation: None,
            params: new.params,
        };
        self.frames.push(frame);
        Ok(())
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
        copies.so_far = copies.so_far.then(copies.step);
        frame.next = 0;
        frame.place = copies.so_far.then(copies.base_place);
        frame.detail = copies.base_detail * copies.so_far.size_factor();
        frame.brush_scale = copies.base_brush_scale * copies.so_far.size_factor();
        frame.seed_base = Some(Random::mix(copies.seed, copies.current as u64 + 1));
        if let Some((_, label)) = &mut frame.label {
            *label = format!("repeat, copy {}", copies.current + 1);
        }
    }

    /// A frame has played all its commands: put back the brush and the
    /// selected layer from before it.
    fn end_frame(&mut self, document: &mut Document, frame: Frame) {
        if let Some(saved) = frame.restore {
            self.restore(document, saved.layer, saved.brush, saved.brush_alpha);
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
        match self.frames.last_mut() {
            Some(parent) => {
                if let Some(drawn) = frame.drawn {
                    parent.drawn = Some(union(parent.drawn, drawn));
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
        Report {
            parts: self.parts.clone(),
            warnings: self.warnings.clone(),
            defined: self.defined.clone(),
            used_symbols: self.used_symbols.iter().cloned().collect(),
            used_colors: self.used_colors.borrow().iter().cloned().collect(),
            palette: self.palette.keys().cloned().collect(),
            painted_by: self
                .painted_by
                .iter()
                .map(|(&layer, numbers)| (layer, numbers.iter().copied().collect()))
                .collect(),
        }
    }

    /// After a command has started: adds the box around what it queued to
    /// its frame's, and warns if all of it is outside the canvas.
    fn note_drawn(&mut self, document: &Document, index: usize, command: &Command, queued: usize) {
        let mut drawn = None;
        // The number of the script's own command this is part of.
        let top = self.frames.first().map_or(0, |script| script.next);
        for job in self.jobs.iter().skip(queued) {
            drawn = Some(union(drawn, job_extent(job, document)));
            let (Job::Stroke { layer, .. } | Job::Area { layer, .. } | Job::Bucket { layer, .. }) =
                job;
            self.painted_by.entry(*layer).or_default().insert(top);
        }
        let Some(drawn) = drawn else {
            return;
        };
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
    fn continue_stroke(&mut self, document: &mut Document, mut budget: f32) -> f32 {
        let Some(mut running) = self.running.take() else {
            return budget;
        };
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
        let RunningStroke { layer, stroke, .. } = running;
        if let Some(target) = document.layer_mut(layer) {
            let before = stroke.finish(&mut target.surface);
            self.remember_tiles(layer, before);
        }
        self.mark_painted(document, layer);
    }

    /// Starts or does one job. Returns how much of the budget it used.
    fn start_job(&mut self, document: &mut Document, job: Job) -> f32 {
        if self.options.dry_run {
            return 0.0;
        }
        match job {
            Job::Stroke {
                layer,
                brush,
                points,
            } => {
                self.running = Some(RunningStroke {
                    layer,
                    stroke: Stroke::new(brush, document.bounds()),
                    points,
                    next: 0,
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
                let before = match shape {
                    Some(rings) => {
                        let mask = geometry::polygons_mask(&rings, limits);
                        paint_mask(&mut target.surface, &mask, &paint, opacity, mode, limits)
                    }
                    None => paint_rect(&mut target.surface, limits, &paint, opacity, mode),
                };
                self.remember_tiles(layer, before);
                self.mark_painted(document, layer);
                AREA_COST
            }
            Job::Bucket {
                layer,
                fill,
                paint,
                opacity,
            } => {
                let before = fill::flood_fill(document, layer, &fill, &paint, opacity);
                self.remember_tiles(layer, before);
                self.mark_painted(document, layer);
                AREA_COST
            }
        }
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
                let top = self.frames.first().map_or(0, |script| script.next);
                self.painted_by.entry(id).or_default().insert(top);
                Ok(())
            }
            Command::Brush(brush) => self.set_brush(brush),
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
            Command::Group(group) => self.group(document, group),
            Command::Define(define) => {
                // Duplicate names and `define` inside other commands are
                // refused when the script is read.
                self.symbols.insert(
                    define.name.clone(),
                    Symbol {
                        commands: Arc::new(define.commands.clone()),
                        params: Arc::new(define.params.clone().unwrap_or_default()),
                    },
                );
                self.defined.push((define.name.clone(), self.current + 1));
                Ok(())
            }
            Command::Use(using) => self.use_symbol(document, using),
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
        };
        self.push_frame(document, new)
    }

    /// `use`: plays a symbol's commands, placed like a group, with some
    /// colours swapped.
    fn use_symbol(&mut self, document: &Document, command: &UseCmd) -> Result<(), String> {
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
        let commands = Arc::clone(&symbol.commands);
        let defaults = Arc::clone(&symbol.params);
        let placement = self.placement_of(
            command.translate.as_ref(),
            command.scale.as_ref(),
            command.rotate.as_ref(),
            command.mirror.as_ref(),
            command.origin.as_ref(),
            command.opacity.as_ref(),
            command.scale_brush,
        )?;
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
        let new = NewFrame {
            commands,
            label: format!("use \"{name}\""),
            placement,
            // Every use of a symbol has the same random patterns.
            seed_base: name_seed(name),
            symbol: Some(name.clone()),
            swaps,
            part: true,
            params: Some(Arc::new(params)),
            upright: command.upright.unwrap_or(false),
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
                origin: (0.0, 0.0),
            },
            seed_base: Random::mix(self.current_seed, 1),
            symbol: None,
            swaps: Vec::new(),
            part: false,
            params: None,
            upright: false,
        };
        self.push_frame(document, new)?;
        let frame = self.frames.last_mut().expect("just pushed");
        // Its commands are numbered by copy in messages.
        if let Some((_, label)) = &mut frame.label {
            *label = "repeat, copy 1".to_owned();
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
        });
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
        if let Some(step) = LayerPropsCommand::apply(document, id, props) {
            self.record(step);
        }
        Ok(())
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
        let mut brush = self.brush;
        if let Some(color) = &command.color {
            let [r, g, b, a] = self.color(color)?;
            brush.color = [r, g, b];
            self.brush_alpha = a;
        }
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
        self.brush = brush;
        Ok(())
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
        self.jitter(&mut points, command.jitter, command.seed, false)?;
        self.place_all(&mut points);
        self.jobs.push_back(Job::Stroke {
            layer: document.active_id(),
            brush,
            points,
        });
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
        self.jitter(&mut points, command.jitter, command.seed, false)?;
        self.place_all(&mut points);
        self.jobs.push_back(Job::Stroke {
            layer: document.active_id(),
            brush,
            points,
        });
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
        self.jitter(&mut points, command.jitter, command.seed, closed)?;
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
                shape: Some(vec![points.iter().map(|p| (p.x, p.y)).collect()]),
                paint,
                opacity,
                mode: BrushMode::Paint,
            });
        }
        if command.line.unwrap_or(true) {
            self.jobs.push_back(Job::Stroke {
                layer,
                brush,
                points,
            });
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
            seed: command.seed,
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
            seed: command.seed,
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
            seed: command.seed,
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
                shape: Some(vec![points.iter().map(|p| (p.x, p.y)).collect()]),
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
            self.jobs.push_back(Job::Stroke {
                layer,
                brush,
                points,
            });
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

        let shape = text::layout(&words, size, align, self.detail());
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
                self.jobs.push_back(Job::Stroke {
                    layer,
                    brush,
                    points,
                });
            }
        } else if command.outline_size.is_some() {
            return Err("`outline_size` needs an `outline` colour too".into());
        }
        if !rings.is_empty() {
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(rings),
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
                    shape: Some(vec![self.place_polygon(ring)]),
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
        let gradient = self.make_gradient(
            command.kind.as_deref(),
            command.from,
            command.to,
            command.colors.as_deref(),
            command.stops.as_deref(),
        )?;
        let shape = match &command.area {
            Some(area) => Some(vec![self.place_polygon(self.area(area)?)]),
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
            paint: Paint::Gradient(gradient),
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
        let mut random = self.random(command.seed);
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
            self.jobs.push_back(Job::Stroke {
                layer,
                brush,
                points,
            });
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
        let mut random = self.random(command.seed);
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
            let brush = BrushSettings {
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
            self.jobs.push_back(Job::Stroke {
                layer,
                brush,
                points,
            });
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
    fn paint(&self, value: &Value) -> Result<Paint, String> {
        match value {
            Value::String(color) => Ok(Paint::Color(self.color(color)?)),
            Value::Object(_) => {
                let spec: GradientFill = serde_json::from_value(value.clone())
                    .map_err(|e| format!("fill: {e}"))?;
                let gradient = self.make_gradient(
                    Some(&spec.gradient),
                    spec.from,
                    spec.to,
                    spec.colors.as_deref(),
                    spec.stops.as_deref(),
                )?;
                Ok(Paint::Gradient(gradient))
            }
            _ => Err("`fill` is a colour (like \"#FF8800\" or a palette name) or a gradient like \
                 {\"gradient\": \"linear\", \"from\": [0, 0], \"to\": [0, 100], \"colors\": [\"a\", \"b\"]}"
                .into()),
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
    let scale_help = "`scale` is a number above 0 (like 1.5), or [across, down] like [2, 1]";
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
    if !(scale.0 > 0.0 && scale.1 > 0.0 && scale.0.is_finite() && scale.1.is_finite()) {
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

fn layer_props(document: &Document, id: LayerId) -> LayerProps {
    document
        .layer(id)
        .map(|layer| layer.props())
        .unwrap_or_else(|| LayerProps {
            name: String::new(),
            visible: true,
            opacity: 1.0,
            blend: BlendMode::Normal,
        })
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
            shape: Some(rings), ..
        } => around(&mut rings.iter().flatten().copied(), 0.0),
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
        file::save(&base, &folder.join("base.artapp")).unwrap();

        let options = PlayOptions {
            folder: Some(folder.clone()),
            ..PlayOptions::default()
        };
        let document = play_with(
            r#"{ "commands": [
                { "cmd": "canvas", "open": "base.artapp" },
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
        let docs = include_str!("../../../../docs/script-format.md");
        let start = docs.find("```json\n{").expect("an example") + "```json\n".len();
        let end = start + docs[start..].find("```").expect("its end");
        let document = play(&docs[start..end]);
        let names: Vec<&str> = document.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Sky", "Sun", "Sea"]);
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
        let message = error(&small(r#"{ "cmd": "group", "scale": 0, "commands": [] }"#));
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
}
