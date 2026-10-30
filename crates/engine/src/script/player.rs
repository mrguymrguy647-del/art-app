//! Plays a painting script on a document.
//!
//! Each command is turned into simple jobs: brush strokes (a list of
//! points), areas to fill (a shape and a colour or gradient) and paint
//! bucket fills. The jobs are then carried out one by one. That split
//! lets the app show a script being painted bit by bit (`advance` with a
//! small budget), while the command-line renderer just plays everything
//! at once (`run`).
//!
//! Everything a command needs is checked before anything is painted, and
//! problems are reported with the command's number.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use super::geometry::{self, Pt};
use super::{
    BrushCmd, Command, EllipseCmd, FillCmd, GradientCmd, HatchCmd, LayerCmd, LineCmd, MoveLayerCmd,
    PathCmd, Point, PolygonCmd, RectCmd, ScatterCmd, Script, ScriptError, StrokeCmd, parse_hex,
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
use crate::random::Random;
use crate::stroke::{Stroke, StrokePoint, TilesBefore};
use crate::tile::Rgba8;

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
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            folder: None,
            record_undo: false,
            dry_run: false,
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
/// Random wobble changes direction about this often (in pixels at full size).
const WOBBLE_WAVELENGTH: f32 = 40.0;

/// One simple job a command was turned into.
enum Job {
    Stroke {
        layer: LayerId,
        brush: BrushSettings,
        points: Vec<StrokePoint>,
    },
    /// Fills a shape (or the whole layer, if `shape` is `None`).
    Area {
        layer: LayerId,
        shape: Option<Vec<Pt>>,
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

/// Plays a script, start to finish.
pub struct Player {
    commands: Vec<Command>,
    palette: HashMap<String, Rgba>,
    seed: u64,
    options: PlayOptions,
    /// The next command to start.
    next: usize,
    /// The command being worked on (for error messages).
    current: usize,
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
}

impl Player {
    /// Gets ready to play `script`, which must start with `canvas`.
    pub fn new(script: Script, options: PlayOptions) -> Result<Self, ScriptError> {
        match script.commands.first() {
            Some(Command::Canvas(_)) => {}
            Some(other) => {
                return Err(ScriptError::in_command(
                    0,
                    Some(other.kind()),
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
        let palette = script
            .palette
            .iter()
            .map(|(name, color)| {
                Ok((
                    name.clone(),
                    parse_hex(color).map_err(ScriptError::general)?,
                ))
            })
            .collect::<Result<_, ScriptError>>()?;
        Ok(Self {
            commands: script.commands,
            palette,
            seed: script.seed,
            options,
            next: 0,
            current: 0,
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
        })
    }

    fn canvas(&self) -> &super::CanvasCmd {
        match &self.commands[0] {
            Command::Canvas(canvas) => canvas,
            _ => unreachable!("checked in `new`"),
        }
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
                let scaled = |side: u32| ((side as f32 * self.options.scale).round() as u32).max(1);
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
        self.next = 1;
        Ok(document)
    }

    /// Gets ready to play on a picture that's already open (in the app).
    /// The canvas's paper colour is applied; its size isn't checked here
    /// (see `canvas_size`) and `open` is ignored.
    pub fn start_on(&mut self, document: &mut Document) -> Result<(), ScriptError> {
        if let Some(paper) = self.canvas().paper.clone() {
            let color = self
                .paper_color(&paper)
                .map_err(|m| ScriptError::in_command(0, Some("canvas"), m))?;
            if let Some(step) = PaperCommand::apply(document, color) {
                self.record(step);
            }
        }
        self.next = 1;
        Ok(())
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
            } else if self.next < self.commands.len() {
                self.current = self.next;
                self.next += 1;
                let command = self.commands[self.current].clone();
                self.start_command(document, &command).map_err(|message| {
                    ScriptError::in_command(self.current, Some(command.kind()), message)
                })?;
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
                Progress::Finished => return Ok(()),
                Progress::Working => {}
            }
        }
    }

    /// How far along: (commands started, all commands).
    pub fn position(&self) -> (usize, usize) {
        (self.next, self.commands.len())
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
                    Some(points) => {
                        let mask = geometry::polygon_mask(&points, limits);
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
                if !self.options.dry_run {
                    let before = document.clear_layer(id);
                    if !before.is_empty() {
                        self.record(TileCommand::new("Clear layer", id, before));
                    }
                }
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
            Command::Snapshot(snapshot) => {
                self.painted = None; // the snapshot already shows that layer
                self.events
                    .push_back(Event::Snapshot(snapshot.name.clone()));
                Ok(())
            }
        }
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
    /// changes, at the playing scale.
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
        brush.opacity *= f32::from(alpha) / 255.0;
        brush.size = (brush.size * self.options.scale).max(0.25);
        brush.mode = mode;
        Ok(brush)
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
        let ends = [self.pt(command.from)?, self.pt(command.to)?];
        let mut points = to_stroke_points(&geometry::densify(&ends, false));
        let (start, end) = taper(command.taper.as_ref(), 0.25)?;
        if start > 0.0 || end > 0.0 {
            brush.pressure_size = true; // so the taper shows
            geometry::apply_taper(&mut points, start, end);
        }
        self.jitter(&mut points, command.jitter, command.seed, false)?;
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
        let mut points = geometry::smooth_path(&given, closed);
        let (start, end) = taper(command.taper.as_ref(), 0.0)?;
        if start > 0.0 || end > 0.0 {
            brush.pressure_size = true;
            geometry::apply_taper(&mut points, start, end);
        }
        self.jitter(&mut points, command.jitter, command.seed, closed)?;

        let layer = document.active_id();
        if let Some(fill) = &command.fill {
            if !closed {
                return Err("only a `closed` path can be filled: add \"closed\": true".into());
            }
            let paint = self.paint(fill)?;
            let opacity = unit("opacity", command.opacity.unwrap_or(1.0))?;
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(points.iter().map(|p| (p.x, p.y)).collect()),
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
        let s = self.options.scale;
        let outline = geometry::rect_outline(
            command.x * s,
            command.y * s,
            command.width * s,
            command.height * s,
            radius * s,
            command.rotation.unwrap_or(0.0),
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
        let s = self.options.scale;
        let centre = self.pt(command.center)?;
        let outline =
            geometry::ellipse_outline(centre, rx * s, ry * s, command.rotation.unwrap_or(0.0));
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
            .map(|&p| self.pt(p))
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
    /// then the outline on top.
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
        let mut points = to_stroke_points(&geometry::densify(&outline, true));
        self.jitter(&mut points, style.jitter, style.seed, true)?;
        let layer = document.active_id();

        if let Some(fill) = style.fill {
            let paint = self.paint(fill)?;
            self.jobs.push_back(Job::Area {
                layer,
                shape: Some(points.iter().map(|p| (p.x, p.y)).collect()),
                paint,
                opacity,
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

    fn bucket(&mut self, document: &Document, command: &FillCmd) -> Result<(), String> {
        let (x, y) = self.pt(command.at)?;
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
                expand: (grow as f32 * self.options.scale).round() as u32,
            },
            paint,
            opacity: unit("opacity", command.opacity.unwrap_or(1.0))?,
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
            Some(area) => Some(self.area(area)?),
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
            opacity: unit("opacity", command.opacity.unwrap_or(1.0))?,
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
        let spacing = match command.spacing {
            Some(spacing) if spacing > 0.0 => spacing,
            Some(_) => return Err("`spacing` must be more than 0".into()),
            None => (brush.size / self.options.scale * 1.5).max(2.0),
        } * self.options.scale;
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
        let jitter = not_negative("jitter", command.jitter.unwrap_or(0.0))? * self.options.scale;
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
            if (b.0 - a.0).hypot(b.1 - a.1) < 1.0 {
                continue;
            }
            let mut points = to_stroke_points(&geometry::densify(&[a, b], false));
            geometry::apply_taper(&mut points, start, end);
            if jitter > 0.0 {
                geometry::wobble(
                    &mut points,
                    jitter / 2.0,
                    WOBBLE_WAVELENGTH * self.options.scale,
                    false,
                    &mut random,
                );
            }
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

        let s = self.options.scale;
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
            let brush = BrushSettings {
                color: [r, g, b],
                size: (random.range(size.0, size.1.max(size.0 + f32::EPSILON)) * s).max(0.25),
                opacity: random
                    .range(opacity.0, opacity.1.max(opacity.0 + f32::EPSILON))
                    .min(1.0)
                    * f32::from(a)
                    / 255.0,
                hardness,
                mode: BrushMode::Paint,
                pressure_size: true,
                ..self.brush
            };
            let points = if strokes {
                let half = random.range(length.0, length.1.max(length.0 + f32::EPSILON)) * s / 2.0;
                let (sin, cos) = random
                    .range(angle.0, angle.1.max(angle.0 + f32::EPSILON))
                    .to_radians()
                    .sin_cos();
                let ends = [
                    (x - cos * half, y - sin * half),
                    (x + cos * half, y + sin * half),
                ];
                let mut points = to_stroke_points(&geometry::densify(&ends, false));
                geometry::apply_taper(&mut points, 0.3, 0.3);
                points
            } else {
                vec![StrokePoint::new(x, y, 1.0)]
            };
            self.jobs.push_back(Job::Stroke {
                layer,
                brush,
                points,
            });
        }
        Ok(())
    }

    // ---- Reading values -----------------------------------------------------------------

    /// A colour: "#RRGGBB", "#RRGGBBAA", a palette name, or "transparent".
    fn color(&self, text: &str) -> Result<Rgba, String> {
        if text == "transparent" {
            return Ok([0, 0, 0, 0]);
        }
        if text.starts_with('#') {
            return parse_hex(text);
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
        let (start, end) = (self.pt(from)?, self.pt(to)?);
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
    /// {"circle": [cx, cy, r]} or {"polygon": [[x, y], ...]}, as a polygon.
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
        let s = self.options.scale;
        let polygon = match kind.as_str() {
            "rect" => {
                let v = flat(4)?;
                geometry::rect_outline(v[0] * s, v[1] * s, v[2] * s, v[3] * s, 0.0, 0.0)
            }
            "ellipse" => {
                let v = flat(4)?;
                geometry::ellipse_outline((v[0] * s, v[1] * s), v[2] * s, v[3] * s, 0.0)
            }
            "circle" => {
                let v = flat(3)?;
                geometry::ellipse_outline((v[0] * s, v[1] * s), v[2] * s, v[2] * s, 0.0)
            }
            "polygon" => {
                let points: Vec<[f32; 2]> =
                    serde_json::from_value(numbers.clone()).map_err(|_| help.to_owned())?;
                points
                    .into_iter()
                    .map(|p| self.pt(p))
                    .collect::<Result<_, _>>()?
            }
            _ => return Err(help.into()),
        };
        if polygon.len() < 3 {
            return Err("an area needs at least 3 points".into());
        }
        Ok(polygon)
    }

    /// A position, at the playing scale.
    fn pt(&self, [x, y]: [f32; 2]) -> Result<Pt, String> {
        if !(x.is_finite() && y.is_finite()) {
            return Err("positions must be numbers".into());
        }
        Ok((x * self.options.scale, y * self.options.scale))
    }

    fn points(&self, points: &[Point]) -> Result<Vec<StrokePoint>, String> {
        points
            .iter()
            .map(|p| {
                let (x, y) = self.pt([p.x, p.y])?;
                let pressure = unit("pressure", p.pressure.unwrap_or(1.0))?;
                Ok(StrokePoint::new(x, y, pressure))
            })
            .collect()
    }

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
        let s = self.options.scale;
        geometry::wobble(
            points,
            amount * s,
            WOBBLE_WAVELENGTH * s,
            closed,
            &mut self.random(seed),
        );
        Ok(())
    }

    /// Random numbers for the current command: from the script's seed and
    /// the command's own `seed` (or, without one, its number).
    fn random(&self, seed: Option<u64>) -> Random {
        Random::new(Random::mix(
            self.seed,
            seed.unwrap_or(self.current as u64 + 1),
        ))
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
}
