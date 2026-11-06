//! Painting scripts: a picture described as text, step by step.
//!
//! A script is a JSON file: a list of commands like "make a layer called
//! Sky", "fill it with a gradient", "draw an ellipse here", "paint a
//! stroke through these points". Playing it gives the picture. It's made
//! to be written by hand (or by an AI assistant) and read back, so:
//!
//! - **It's predictable.** Playing a script always gives exactly the same
//!   picture. Even "random" commands use a seed (see `random.rs`).
//! - **Mistakes are explained.** Every error names the command's number
//!   and what's wrong, e.g. "command 12 (stroke): unknown field `sise`".
//! - **Big shapes are one command.** Rectangles, ellipses, polygons,
//!   smooth paths, tapered lines, hatching and scattering save writing
//!   hundreds of strokes.
//!
//! This file describes the format and reads it. `script/player.rs` plays
//! it; `script/geometry.rs` works out the shapes. The full description,
//! with examples, is in `docs/script-format.md`.

pub mod animate;
mod geometry;
mod player;
pub mod record;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use player::{
    Event, Part, PlayOptions, Player, Progress, Report, layer_named, unused_warnings,
};

/// Written at the top of every script, so it can be recognised.
pub const FORMAT_NAME: &str = "artapp-script";
/// The script format version this app writes, and the newest it plays.
pub const FORMAT_VERSION: u32 = 1;

/// A script that has been read and checked for typos (but not yet played).
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    /// A title for people; not used when playing.
    pub title: Option<String>,
    /// Anything for people (text, a list, ...); not used when playing.
    pub notes: Option<Value>,
    /// Where randomness starts (see `random.rs`). Default 1.
    pub seed: u64,
    /// Colour names defined once and used by name in commands.
    pub palette: BTreeMap<String, String>,
    pub commands: Vec<Command>,
    /// Animations, by name (in alphabetical order). Each plays `commands`
    /// and then its own, once per frame. Empty for a single picture.
    pub animations: BTreeMap<String, Animation>,
    /// The character's anchor point in canvas pixels, for sprite sheets.
    /// Default: the bottom centre of the canvas.
    pub pivot: Option<[f32; 2]>,
}

/// One animation: `frames` frames at `fps`, looping or not, made of the
/// script's commands followed by these. See `docs/animation-design.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub frames: u32,
    pub fps: f32,
    #[serde(rename = "loop", default)]
    pub looping: bool,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// The most frames one animation may have.
pub const MAX_FRAMES: u32 = 100_000;

/// A point: `[x, y]`, or `[x, y, pressure]` with pressure from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Vec<f32>", into = "Vec<f32>")]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub pressure: Option<f32>,
}

impl Point {
    pub fn new(x: f32, y: f32, pressure: Option<f32>) -> Self {
        Self { x, y, pressure }
    }
}

impl TryFrom<Vec<f32>> for Point {
    type Error = String;

    fn try_from(numbers: Vec<f32>) -> Result<Self, String> {
        match numbers[..] {
            [x, y] => Ok(Point::new(x, y, None)),
            [x, y, pressure] => Ok(Point::new(x, y, Some(pressure))),
            _ => Err(format!(
                "a point is [x, y] or [x, y, pressure], not {} number(s)",
                numbers.len()
            )),
        }
    }
}

impl From<Point> for Vec<f32> {
    fn from(point: Point) -> Self {
        match point.pressure {
            Some(pressure) => vec![point.x, point.y, pressure],
            None => vec![point.x, point.y],
        }
    }
}

// ---- The commands ------------------------------------------------------------
//
// Every command is a JSON object with a "cmd" field saying which command it
// is, e.g. {"cmd": "stroke", "points": [[10, 10], [200, 50]]}. Fields marked
// Option can be left out. Misspelt fields are errors, not silently ignored.
// Any command may also have a "note" field: a comment for people.

/// One step of a script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Canvas(CanvasCmd),
    Paper(PaperCmd),
    Layer(LayerCmd),
    DeleteLayer(DeleteLayerCmd),
    MoveLayer(MoveLayerCmd),
    RenameLayer(RenameLayerCmd),
    ClearLayer(ClearLayerCmd),
    TransformLayer(TransformLayerCmd),
    Brush(BrushCmd),
    Stroke(StrokeCmd),
    Erase(StrokeCmd),
    Line(LineCmd),
    Path(PathCmd),
    Rect(RectCmd),
    Ellipse(EllipseCmd),
    Polygon(PolygonCmd),
    Fill(FillCmd),
    Gradient(GradientCmd),
    Hatch(HatchCmd),
    Scatter(ScatterCmd),
    Text(TextCmd),
    Pixels(PixelsCmd),
    Snapshot(SnapshotCmd),
    Stamp(StampCmd),
    Filter(FilterCmd),
    Group(GroupCmd),
    Define(DefineCmd),
    Use(UseCmd),
    Repeat(RepeatCmd),
}

impl Command {
    /// The command's name as written in scripts, e.g. "stroke".
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Canvas(_) => "canvas",
            Command::Paper(_) => "paper",
            Command::Layer(_) => "layer",
            Command::DeleteLayer(_) => "delete_layer",
            Command::MoveLayer(_) => "move_layer",
            Command::RenameLayer(_) => "rename_layer",
            Command::ClearLayer(_) => "clear_layer",
            Command::TransformLayer(_) => "transform_layer",
            Command::Brush(_) => "brush",
            Command::Stroke(_) => "stroke",
            Command::Erase(_) => "erase",
            Command::Line(_) => "line",
            Command::Path(_) => "path",
            Command::Rect(_) => "rect",
            Command::Ellipse(_) => "ellipse",
            Command::Polygon(_) => "polygon",
            Command::Fill(_) => "fill",
            Command::Gradient(_) => "gradient",
            Command::Hatch(_) => "hatch",
            Command::Scatter(_) => "scatter",
            Command::Text(_) => "text",
            Command::Pixels(_) => "pixels",
            Command::Snapshot(_) => "snapshot",
            Command::Stamp(_) => "stamp",
            Command::Filter(_) => "filter",
            Command::Group(_) => "group",
            Command::Define(_) => "define",
            Command::Use(_) => "use",
            Command::Repeat(_) => "repeat",
        }
    }

    /// The command's name for messages: its kind, and its own name if it
    /// has one, like `group "duck"`.
    pub fn label(&self) -> String {
        match self {
            Command::Group(GroupCmd {
                name: Some(name), ..
            }) => format!("group \"{name}\""),
            Command::Define(define) => format!("define \"{}\"", define.name),
            Command::Use(using) => format!("use \"{}\"", using.name),
            other => other.kind().to_owned(),
        }
    }
}

/// `canvas`: the first command. Either a new canvas (`width`, `height`,
/// `paper`) or an existing picture (`open`: an .artapp file, relative to
/// the script's folder).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// A pixel canvas: `true`, or `{"supersample": 8, "outline": "ink"}`.
    /// Everything is painted bigger and shrunk to crisp whole pixels in
    /// the palette's colours (see `pixels.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel: Option<Value>,
}

/// `paper`: changes the paper colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperCmd {
    pub color: String,
}

/// `layer`: selects the layer called `name`, creating it if there's none
/// (on top, or `above`/`below` another layer), and applies any settings given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerCmd {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// `true`: shown only where the nearest unclipped layer below has paint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<bool>,
    /// `true`: the alpha lock, so painting only recolours what's there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<bool>,
    /// `"below"`: fill the layer with everything below it, flattened
    /// (for a filter over the whole picture).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<String>,
}

/// `delete_layer`: deletes the layer called `name`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteLayerCmd {
    pub name: String,
}

/// `move_layer`: moves a layer `above` or `below` another one, or to
/// position `index` (0 = bottom).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveLayerCmd {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

/// `rename_layer`: renames layer `name` to `to`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameLayerCmd {
    pub name: String,
    pub to: String,
}

/// `clear_layer`: empties a layer (`name`, or the selected one).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearLayerCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// `transform_layer`: moves (`translate`), resizes (`scale`), turns
/// (`rotate`, degrees clockwise) and/or mirrors (`mirror`: "x", "y" or
/// "xy") everything already on a layer (`name`, or the selected one),
/// around `origin`, in canvas pixels. What ends up off the canvas is cut
/// off. The app's Layer menu (flip, transform) writes these.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformLayerCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
}

/// `brush`: changes the brush. Only the settings given change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushCmd {
    /// `true`: back to the plain brush first (then any settings given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset: Option<bool>,
    /// A colour, or a pattern to paint with: `{"noise": {...}}` or
    /// `{"texture": ...}` (see `fill`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_size: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_opacity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_flow: Option<bool>,
    /// The dab's shape: `"round"`, `"square"`, `"bristle"`, `"chalk"`,
    /// `"leaf"`, `"grass"`, `"sparkle"`, or `{"file": "tips/splat.png"}`
    /// (a greyscale picture, relative to the script).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Value>,
    /// How far the tip is turned, degrees clockwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    /// How dabs vary along a stroke; replaces the dynamics as a whole
    /// (`{}` turns them all off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<DynamicsCmd>,
}

/// A brush's dynamics, as written: see `brush::Dynamics`. Anything left
/// out is off.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicsCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scatter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
}

/// `stamp`: puts copies of a symbol, or single dabs of a tip, along a
/// smooth path through `points`, one every `spacing` units, each turned
/// to follow the path (unless `follow` is false) and varied at random
/// (from `seed`) within `size`, `angle` and `colors`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StampCmd {
    /// The symbol to copy (placed like a `use` at each spot)...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// ...or the tip to stamp (as on `brush`; default: the brush's own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Value>,
    /// `[[x, y], ...]`; may be keyframes.
    pub points: Value,
    /// Distance between copies, in drawing units; may be keyframes.
    pub spacing: Value,
    /// For a symbol, a scale (default 1); for a tip, pixels across
    /// (default: the brush size). A number, or `[min, max]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Value>,
    /// Degrees added to each copy's turn: a number, or `[min, max]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Value>,
    /// Turn each copy along the path (default true).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<bool>,
    /// Copies land up to this far off the path (drawing units).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scatter: Option<f32>,
    /// One colour, or colours to pick from per copy. For a symbol, the
    /// colour goes to its `color` parameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<String>>,
    /// For tip dabs: their opacity (default: the brush's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    /// Values for the symbol's other parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `filter`: changes everything on a layer (`layer`, or the selected
/// one) in place: `adjust`, then `gradient_map`, `blur`, `glow` and
/// `shadow`, whichever are given. Every number may be keyframes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Pixels of softening.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blur: Option<Value>,
    /// `{"radius": 24, "threshold": 0.75, "strength": 0.8}`: light
    /// pixels spread and added on top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<Value>,
    /// `{"offset": [6, 8], "blur": 6, "color": "#00000070"}`: a soft
    /// copy of the silhouette behind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Value>,
    /// `{"hue": 0, "saturation": 0, "brightness": 0, "contrast": 0}`:
    /// degrees, then -1 to 1 each.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adjust: Option<Value>,
    /// `{"stops": [[0, "dark"], [1, "light"]]}` or `{"colors": [...]}`:
    /// every pixel recoloured by its lightness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient_map: Option<Value>,
}

impl FilterCmd {
    /// The filter fields as one object (as a group's `filter` is written).
    pub fn fields(&self) -> Value {
        let mut object = serde_json::Map::new();
        for (name, value) in [
            ("blur", &self.blur),
            ("glow", &self.glow),
            ("shadow", &self.shadow),
            ("adjust", &self.adjust),
            ("gradient_map", &self.gradient_map),
        ] {
            if let Some(value) = value {
                object.insert(name.to_owned(), value.clone());
            }
        }
        Value::Object(object)
    }
}

/// `stroke` / `erase`: paints (or erases) through the points with the
/// brush, exactly as if a pen had moved through them. `color`, `size`,
/// `opacity`, `hardness` and `flow` change the brush for this stroke only.
/// `jitter` wobbles the line by up to that many pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeCmd {
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `line`: a straight line from `from` to `to`, thinning towards its ends
/// with `taper`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineCmd {
    pub from: [f32; 2],
    pub to: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `path`: a smooth curve through the points, drawn with the brush
/// (`line: false` to skip that), optionally `closed` and filled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathCmd {
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `rect`: a rectangle, `fill`ed and/or `outline`d. `radius` rounds the
/// corners; `rotation` turns it (degrees, clockwise, around its middle).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RectCmd {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erase: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `ellipse`: an ellipse (or circle) around `center`, `fill`ed and/or
/// `outline`d. `radius` is one number (a circle) or `[rx, ry]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EllipseCmd {
    pub center: [f32; 2],
    pub radius: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erase: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `polygon`: straight edges through the points (at least 3), `fill`ed
/// and/or `outline`d.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolygonCmd {
    pub points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erase: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `fill`: the paint bucket, starting `at` a point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillCmd {
    pub at: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f32>,
    /// "visible" (everything you see, the default) or "layer" (only this layer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grow: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
}

/// `gradient`: a gradient from `from` to `to` through `colors` (or
/// `stops`: `[[position, colour], ...]`), over the whole layer or inside
/// an `area`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientCmd {
    /// "linear" (the default) or "radial".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<[f32; 2]>,
    /// Instead of `from`/`to`/`colors`: any fill (a colour, a gradient
    /// object, `{"noise": ...}` or `{"texture": ...}`) over the area or
    /// the whole layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stops: Option<Vec<(f32, String)>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erase: Option<bool>,
}

/// `hatch`: fills an `area` with parallel brush strokes at an `angle`,
/// `spacing` pixels apart (`cross` adds a second set across them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HatchCmd {
    pub area: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `scatter`: `count` dabs (or short strokes) at random places in an
/// `area`, with sizes, colours and opacities picked from the ranges given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterCmd {
    pub area: Value,
    pub count: u32,
    /// "dab" (the default) or "stroke".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

/// `text`: letters in the built-in font (see `text.rs`). `at` is the
/// top of the first line: its left end, middle or right end, depending on
/// `align` ("left", "center" or "right"). `size` is the font size in
/// pixels (32 if not given); "\n" starts a new line. `color` is the
/// letters' colour (the brush colour if not given); an `outline` colour
/// draws a border `outline_size` pixels wide (2 if not given) around
/// them. `rotate` turns the text around `at`, in degrees clockwise.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextCmd {
    pub text: String,
    pub at: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    /// Shrinks the text (never enlarges it) so its widest line is at
    /// most this many pixels wide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit: Option<f32>,
}

/// `group`: draws its `commands` moved (`translate`), resized (`scale`),
/// turned (`rotate`, degrees clockwise) and/or mirrored (`mirror`: "x"
/// swaps left and right, "y" up and down, "xy" both), all around
/// `origin`. `opacity` makes everything inside fainter. With
/// `scale_brush` false, brush and outline sizes stay as they are.
/// Groups can hold groups. Afterwards, the brush and the selected layer
/// are back to what they were before the group.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `[x, y]`. This and the fields below may also be keyframes
    /// (`{"keys": ...}`) or a symbol parameter (`"$name"`): see
    /// `animate.rs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<Value>,
    /// One number, or `[sx, sy]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_brush: Option<bool>,
    /// `true`: not turned by the groups around it (a foot stays flat
    /// while the leg turns). It still moves and mirrors with them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    /// Paint only inside this: an area, `{"symbol": ..}` (a symbol's
    /// silhouette, placed like a use), `{"layer": ..}` (where a layer has
    /// paint) or `{"not": ..}` (everything but one of those).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Value>,
    /// Filters applied to what the group draws, on its own, before it's
    /// laid on the layers: the fields of `filter` (see `FilterCmd`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// `define`: stores `commands` as a symbol called `name`, without
/// painting anything. Only at the top level of a script.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefineCmd {
    pub name: String,
    /// Parameters and their defaults: `{"leg": 0, "eye": "eye_open"}`.
    /// Inside the symbol, `"$leg"` stands for the value a `use` gives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// `use`: paints the symbol `name`, placed like a group (the same
/// transform fields), with some of its colours swapped: `colors` maps a
/// colour the symbol uses (a palette name or "#hex") to the colour to
/// paint instead.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseCmd {
    /// The symbol's name (or a parameter, `"$mouth"`).
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_brush: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    /// As on a group: paint only inside this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Value>,
    /// As on a group: filters on what it draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// Colours to paint instead of the symbol's: `{"yellow": "#C04040"}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<BTreeMap<String, Value>>,
    /// Values for the symbol's parameters: `{"leg": 20}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<BTreeMap<String, Value>>,
}

/// `repeat`: draws `commands` `count` times, each copy with `step`
/// applied once more than the one before (the first copy as written).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatCmd {
    pub count: u32,
    pub step: StepCmd,
    /// Values that differ per copy: `{"color": ["red", "green", "blue"]}`
    /// gives copy 1 `"$color"` = red, copy 2 green, ... (round and round
    /// if there are more copies than values).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub each: Option<BTreeMap<String, Vec<Value>>>,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// How far each copy of a `repeat` moves, turns and resizes, around
/// `origin`, compared with the copy before.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
}

/// `pixels`: art written as rows of characters, each a palette colour
/// from `colors` (`.` is transparent), with the top-left pixel at `at`:
/// `"rows": ["..kk..", ".kyyk."]`. Every character is one canvas pixel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelsCmd {
    pub at: [f32; 2],
    pub rows: Vec<String>,
    pub colors: BTreeMap<String, String>,
}

/// `snapshot`: a named checkpoint. The command-line renderer can save a
/// picture here, to check progress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCmd {
    pub name: String,
}

// ---- Errors ------------------------------------------------------------------

/// Something wrong with a script: where, and what.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptError {
    /// The command's number, counting from 1, if the problem is in one.
    pub command: Option<usize>,
    /// The command's name ("stroke", or `group "duck"`), if known.
    pub kind: Option<String>,
    /// Inside a group (or symbol, or repeat): the commands leading to the
    /// mistake, like `3 (ellipse)`, outermost first.
    pub inside: Vec<String>,
    /// The line in the file, for JSON mistakes (like a missing comma).
    pub line: Option<usize>,
    /// The animation the command is in, if any.
    pub animation: Option<String>,
    pub message: String,
}

impl ScriptError {
    /// A problem with the script as a whole.
    pub fn general(message: impl Into<String>) -> Self {
        Self {
            command: None,
            kind: None,
            inside: Vec::new(),
            line: None,
            animation: None,
            message: message.into(),
        }
    }

    /// A problem in command number `index + 1`.
    pub fn in_command(index: usize, kind: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            command: Some(index + 1),
            kind: kind.map(str::to_owned),
            inside: Vec::new(),
            line: None,
            animation: None,
            message: message.into(),
        }
    }

    /// The same mistake, inside the animation called `name`.
    pub fn in_animation(mut self, name: &str) -> Self {
        self.animation = Some(name.to_owned());
        self
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = &self.animation {
            write!(f, "animation \"{name}\" > ")?;
        }
        match (self.command, &self.kind, self.line) {
            (Some(number), Some(kind), _) => {
                write!(f, "command {number} ({kind})")?;
                for step in &self.inside {
                    write!(f, " > {step}")?;
                }
                write!(f, ": {}", self.message)
            }
            (Some(number), None, _) => write!(f, "command {number}: {}", self.message),
            (None, _, Some(line)) => write!(f, "line {line}: {}", self.message),
            _ => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for ScriptError {}

// ---- Reading and writing ------------------------------------------------------

/// The file as written, before each command is looked at.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScript {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    title: Option<String>,
    /// Comments for people, ignored when playing.
    #[serde(default)]
    notes: Option<Value>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    palette: BTreeMap<String, String>,
    commands: Vec<Value>,
    #[serde(default)]
    animations: BTreeMap<String, RawAnimation>,
    #[serde(default)]
    pivot: Option<[f32; 2]>,
}

/// An animation as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAnimation {
    frames: u32,
    #[serde(default = "default_fps")]
    fps: f32,
    #[serde(rename = "loop", default)]
    looping: bool,
    #[serde(default)]
    commands: Vec<Value>,
}

fn default_fps() -> f32 {
    12.0
}

impl Script {
    /// Reads a script from JSON text, checking every command for typos
    /// and missing fields. (Whether the values make sense, like colour
    /// names or layer names, is checked when it's played: see
    /// `Player::check`.)
    pub fn parse(text: &str) -> Result<Script, ScriptError> {
        let raw: RawScript = serde_json::from_str(text).map_err(|error| ScriptError {
            command: None,
            kind: None,
            inside: Vec::new(),
            line: Some(error.line()),
            animation: None,
            message: describe_json_error(&error),
        })?;
        if let Some(format) = &raw.format
            && format != FORMAT_NAME
        {
            return Err(ScriptError::general(format!(
                "\"format\" should be \"{FORMAT_NAME}\", not \"{format}\""
            )));
        }
        if let Some(version) = raw.version
            && version > FORMAT_VERSION
        {
            return Err(ScriptError::general(format!(
                "this script is version {version}, but this Art App only plays up to version \
                 {FORMAT_VERSION}. Please update Art App."
            )));
        }
        for (name, color) in &raw.palette {
            if name.starts_with('#') || name.is_empty() || name == "transparent" {
                return Err(ScriptError::general(format!(
                    "palette: \"{name}\" can't be a colour name (names can't be empty, \
                     start with # or be \"transparent\")"
                )));
            }
            parse_hex(color).map_err(|problem| {
                ScriptError::general(format!("palette: \"{name}\": {problem}"))
            })?;
        }

        let mut commands = Vec::with_capacity(raw.commands.len());
        let mut symbols: BTreeMap<String, usize> = BTreeMap::new();
        for (index, value) in raw.commands.into_iter().enumerate() {
            let label = label_of(&value);
            let command = parse_command(value, false).map_err(|problem| ScriptError {
                command: Some(index + 1),
                kind: label.clone(),
                inside: problem.inside,
                line: None,
                animation: None,
                message: problem.message,
            })?;
            if let Command::Define(define) = &command
                && let Some(first) = symbols.insert(define.name.clone(), index + 1)
            {
                return Err(ScriptError::in_command(
                    index,
                    label.as_deref(),
                    format!(
                        "a symbol called \"{}\" was already defined by command {first}",
                        define.name
                    ),
                ));
            }
            commands.push(command);
        }
        if commands.is_empty() {
            return Err(ScriptError::general("the script has no commands"));
        }

        let mut animations = BTreeMap::new();
        for (name, raw) in raw.animations {
            let problem = |message: String| ScriptError::general(message).in_animation(&name);
            if name.trim().is_empty() {
                return Err(ScriptError::general("an animation needs a name"));
            }
            if !(1..=MAX_FRAMES).contains(&raw.frames) {
                return Err(problem(format!(
                    "`frames` must be from 1 to {MAX_FRAMES}, not {}",
                    raw.frames
                )));
            }
            if !(raw.fps > 0.0 && raw.fps <= 1000.0) {
                return Err(problem(format!(
                    "`fps` must be above 0 and at most 1000, not {}",
                    raw.fps
                )));
            }
            let mut inner = Vec::with_capacity(raw.commands.len());
            for (index, value) in raw.commands.into_iter().enumerate() {
                let label = label_of(&value);
                let command = parse_command(value, true).map_err(|problem| ScriptError {
                    command: Some(index + 1),
                    kind: label.clone(),
                    inside: problem.inside,
                    line: None,
                    animation: Some(name.clone()),
                    message: problem.message,
                })?;
                inner.push(command);
            }
            animations.insert(
                name,
                Animation {
                    frames: raw.frames,
                    fps: raw.fps,
                    looping: raw.looping,
                    commands: inner,
                },
            );
        }
        if let Some([x, y]) = raw.pivot
            && !(x.is_finite() && y.is_finite())
        {
            return Err(ScriptError::general("`pivot` must be [x, y] in pixels"));
        }

        Ok(Script {
            title: raw.title,
            notes: raw.notes,
            seed: raw.seed.unwrap_or(1),
            palette: raw.palette,
            commands,
            animations,
            pivot: raw.pivot,
        })
    }

    /// Writes the script as JSON text: readable, one command per line.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{\n");
        out += &format!("  \"format\": \"{FORMAT_NAME}\",\n");
        out += &format!("  \"version\": {FORMAT_VERSION},\n");
        if let Some(title) = &self.title {
            out += &format!("  \"title\": {},\n", json(title));
        }
        if let Some(notes) = &self.notes {
            out += &format!("  \"notes\": {},\n", json(notes));
        }
        out += &format!("  \"seed\": {},\n", self.seed);
        out += &format!("  \"palette\": {},\n", json(&self.palette));
        if let Some(pivot) = &self.pivot {
            out += &format!("  \"pivot\": {},\n", json(pivot));
        }
        out += "  \"commands\": [\n";
        for (i, command) in self.commands.iter().enumerate() {
            let comma = if i + 1 < self.commands.len() { "," } else { "" };
            out += &format!("    {}{comma}\n", json(command));
        }
        if self.animations.is_empty() {
            out += "  ]\n}\n";
            return out;
        }
        out += "  ],\n  \"animations\": {\n";
        let count = self.animations.len();
        for (i, (name, animation)) in self.animations.iter().enumerate() {
            out += &format!(
                "    {}: {{ \"frames\": {}, \"fps\": {}, \"loop\": {}, \"commands\": [\n",
                json(name),
                animation.frames,
                animation.fps,
                animation.looping
            );
            for (j, command) in animation.commands.iter().enumerate() {
                let comma = if j + 1 < animation.commands.len() {
                    ","
                } else {
                    ""
                };
                out += &format!("      {}{comma}\n", json(command));
            }
            out += &format!("    ] }}{}\n", if i + 1 < count { "," } else { "" });
        }
        out += "  }\n}\n";
        out
    }
}

/// A mistake found while reading a command: where inside it (for commands
/// that hold others), and what.
struct Problem {
    inside: Vec<String>,
    message: String,
}

impl Problem {
    fn here(message: impl Into<String>) -> Self {
        Self {
            inside: Vec::new(),
            message: message.into(),
        }
    }
}

/// Reads one command, and the commands inside it for those that hold
/// others (`group`, `define`, `repeat`), so mistakes in those can say
/// exactly where they are. `nested` is true inside one of those.
fn parse_command(mut value: Value, nested: bool) -> Result<Command, Problem> {
    let Some(object) = value.as_object_mut() else {
        return Err(Problem::here(
            "each command must be an object like {\"cmd\": \"stroke\", ...}",
        ));
    };
    object.remove("note"); // a comment for people
    let kind = object
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if kind == "define" && nested {
        return Err(Problem::here(
            "`define` only works at the top level of a script, not inside a group, symbol, \
             repeat or animation",
        ));
    }
    let mut inner = Vec::new();
    if matches!(kind, "group" | "define" | "repeat") {
        match object.remove("commands") {
            Some(Value::Array(list)) => {
                for (index, child) in list.into_iter().enumerate() {
                    let label = label_of(&child).unwrap_or_else(|| "?".into());
                    let command = parse_command(child, true).map_err(|mut problem| {
                        problem.inside.insert(0, format!("{} ({label})", index + 1));
                        problem
                    })?;
                    inner.push(command);
                }
            }
            Some(_) => {
                return Err(Problem::here(
                    "`commands` must be a list of commands, like [{\"cmd\": \"ellipse\", ...}]",
                ));
            }
            None => {
                return Err(Problem::here(
                    "`commands` is missing: the list of commands to draw inside it",
                ));
            }
        }
    }
    let mut command: Command =
        serde_json::from_value(value).map_err(|error| Problem::here(error.to_string()))?;
    match &mut command {
        Command::Group(group) => group.commands = inner,
        Command::Define(define) => define.commands = inner,
        Command::Repeat(repeat) => repeat.commands = inner,
        _ => {}
    }
    Ok(command)
}

/// How a command (still as JSON) is named in messages, like `stroke` or
/// `group "duck"`.
fn label_of(value: &Value) -> Option<String> {
    let kind = value.get("cmd")?.as_str()?;
    match value.get("name").and_then(Value::as_str) {
        Some(name) if matches!(kind, "group" | "define" | "use") => {
            Some(format!("{kind} \"{name}\""))
        }
        _ => Some(kind.to_owned()),
    }
}

fn json(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

/// A JSON reading error in plain words, with where it is.
fn describe_json_error(error: &serde_json::Error) -> String {
    let what = error.to_string();
    // serde_json adds " at line X column Y" itself; we report the line
    // separately, so keep the column only.
    match what.rsplit_once(" at line ") {
        Some((message, place)) => {
            let column = place.split_once(" column ").map_or("", |(_, c)| c);
            format!("{message} (column {column})")
        }
        None => what,
    }
}

/// Reads "#RGB", "#RGBA", "#RRGGBB" or "#RRGGBBAA" (opacity last).
pub fn parse_hex(text: &str) -> Result<[u8; 4], String> {
    let problem = || {
        format!("\"{text}\" isn't a colour: use \"#RRGGBB\" (like \"#FF8800\") or \"#RRGGBBAA\"")
    };
    let digits = text.strip_prefix('#').ok_or_else(problem)?;
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(problem());
    }
    let pair = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).unwrap_or(0);
    let single = |i: usize| u8::from_str_radix(&digits[i..=i], 16).unwrap_or(0) * 17;
    match digits.len() {
        3 => Ok([single(0), single(1), single(2), 255]),
        4 => Ok([single(0), single(1), single(2), single(3)]),
        6 => Ok([pair(0), pair(2), pair(4), 255]),
        8 => Ok([pair(0), pair(2), pair(4), pair(6)]),
        _ => Err(problem()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Script, ScriptError> {
        Script::parse(text)
    }

    #[test]
    fn a_small_script_is_read() {
        let script = parse(
            r##"{
              "format": "artapp-script", "version": 1, "seed": 7,
              "palette": { "sky": "#88BBFF" },
              "commands": [
                { "cmd": "canvas", "width": 800, "height": 600 },
                { "cmd": "layer", "name": "Sky", "note": "a comment" },
                { "cmd": "stroke", "points": [[10, 10], [200, 50, 0.5]], "color": "sky" }
              ]
            }"##,
        )
        .unwrap();
        assert_eq!(script.seed, 7);
        assert_eq!(script.commands.len(), 3);
        let Command::Stroke(stroke) = &script.commands[2] else {
            panic!("not a stroke");
        };
        assert_eq!(stroke.points[1], Point::new(200.0, 50.0, Some(0.5)));
    }

    #[test]
    fn errors_name_the_command_and_the_problem() {
        let error = parse(
            r#"{ "commands": [
                { "cmd": "canvas", "width": 10, "height": 10 },
                { "cmd": "stroke", "points": [[1, 2]], "sise": 3 }
            ] }"#,
        )
        .unwrap_err();
        assert_eq!(error.command, Some(2));
        let text = error.to_string();
        assert!(
            text.starts_with("command 2 (stroke): unknown field `sise`"),
            "{text}"
        );

        let error = parse(r#"{ "commands": [ { "cmd": "strok" } ] }"#).unwrap_err();
        assert!(
            error.to_string().contains("unknown variant `strok`"),
            "{error}"
        );

        let error =
            parse(r#"{ "commands": [ { "cmd": "stroke", "points": [[1]] } ] }"#).unwrap_err();
        assert!(error.to_string().contains("a point is [x, y]"), "{error}");
    }

    #[test]
    fn json_mistakes_give_the_line() {
        let error =
            parse("{\n  \"commands\": [\n    { \"cmd\": \"snapshot\" \"name\": \"a\" }\n  ]\n}")
                .unwrap_err();
        assert_eq!(error.line, Some(3));
        assert!(
            error.to_string().starts_with("line 3: expected `,`"),
            "{error}"
        );
    }

    #[test]
    fn newer_scripts_and_wrong_formats_are_refused() {
        let error = parse(r#"{ "version": 2, "commands": [] }"#).unwrap_err();
        assert!(error.to_string().contains("version 2"), "{error}");
        let error = parse(r#"{ "format": "svg", "commands": [] }"#).unwrap_err();
        assert!(error.to_string().contains("artapp-script"), "{error}");
    }

    #[test]
    fn palette_colours_are_checked() {
        let error = parse(r##"{ "palette": { "sky": "blue" }, "commands": [] }"##).unwrap_err();
        assert!(error.to_string().contains("palette: \"sky\""), "{error}");
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#F80"), Ok([255, 136, 0, 255]));
        assert_eq!(parse_hex("#F808"), Ok([255, 136, 0, 136]));
        assert_eq!(parse_hex("#102030"), Ok([16, 32, 48, 255]));
        assert_eq!(parse_hex("#10203040"), Ok([16, 32, 48, 64]));
        assert!(parse_hex("102030").is_err());
        assert!(parse_hex("#12345").is_err());
        assert!(parse_hex("#GG0000").is_err());
    }

    #[test]
    fn writing_and_reading_back_gives_the_same_script() {
        let script = parse(
            r##"{ "title": "Test", "palette": { "a": "#123456" }, "commands": [
                { "cmd": "canvas", "width": 100, "height": 50, "paper": "#FFFFFF" },
                { "cmd": "brush", "size": 12.5, "color": "a" },
                { "cmd": "stroke", "points": [[1, 2], [3.25, 4, 0.5]] },
                { "cmd": "ellipse", "center": [50, 25], "radius": [20, 10], "fill": "a" },
                { "cmd": "gradient", "from": [0, 0], "to": [0, 50], "stops": [[0, "a"], [1, "#FFF0"]] },
                { "cmd": "snapshot", "name": "done" }
            ] }"##,
        )
        .unwrap();
        let text = script.to_json();
        assert!(
            text.contains("\n    {\"cmd\":\"stroke\""),
            "one command per line:\n{text}"
        );
        assert_eq!(parse(&text).unwrap(), script);
    }

    /// Every example command in the documentation must be valid, so a
    /// typo there can't teach anyone (or any AI) the wrong thing.
    #[test]
    fn the_examples_in_the_documentation_are_valid() {
        let quick_start = include_str!("../../../docs/QUICKSTART.md");
        let docs = [
            include_str!("../../../docs/script-format.md"),
            include_str!("../../../docs/AI_GUIDE.md"),
            quick_start,
        ];
        // The quick start's first script is the example file, word for word.
        let example = include_str!("../../../examples/two-ducks.json");
        assert!(
            quick_start.contains(example.trim()),
            "QUICKSTART.md and two-ducks.json differ"
        );
        let mut checked = 0;
        for doc in docs {
            for block in doc.split("```json\n").skip(1) {
                let block = block.split("```").next().unwrap_or("").trim();
                if block.contains('…') || block.contains("...") {
                    continue; // has placeholders, like [x, y] ...
                }
                if block.starts_with("{\n") && block.contains("\"commands\"") {
                    // A whole script: it must also play without mistakes.
                    let script = parse(block).unwrap_or_else(|e| panic!("{e}\nin:\n{block}"));
                    let options = PlayOptions {
                        dry_run: true,
                        ..PlayOptions::default()
                    };
                    let mut player = Player::new(script, options).unwrap();
                    let mut document = player.create_document().unwrap();
                    player
                        .run(&mut document, |_, _| {})
                        .unwrap_or_else(|e| panic!("{e}\nin:\n{block}"));
                    checked += 1;
                    continue;
                }
                if block.starts_with('"') {
                    // One field, like "fill": {...}: check it inside a rectangle.
                    let script = format!(
                        r#"{{ "commands": [ {{ "cmd": "canvas", "width": 10, "height": 10 }},
                            {{ "cmd": "rect", "x": 0, "y": 0, "width": 5, "height": 5, {block} }} ] }}"#
                    );
                    let options = PlayOptions {
                        dry_run: true,
                        ..PlayOptions::default()
                    };
                    let mut player = Player::new(parse(&script).unwrap(), options).unwrap();
                    let mut document = player.create_document().unwrap();
                    player
                        .run(&mut document, |_, _| {})
                        .unwrap_or_else(|e| panic!("{e}\nin:\n{block}"));
                    checked += 1;
                    continue;
                }
                // One or more commands, one after another (with or without commas).
                let list = format!("[{}]", block.trim_end_matches(',').replace("}\n{", "},\n{"));
                let Ok(values) = serde_json::from_str::<Vec<Value>>(&list) else {
                    panic!("not valid JSON:\n{block}");
                };
                for value in values {
                    if value.get("cmd").is_none() {
                        continue; // an area or a fill, not a whole command
                    }
                    let script = format!(
                        r#"{{ "commands": [ {{ "cmd": "canvas", "width": 10, "height": 10 }}, {value} ] }}"#
                    );
                    parse(&script).unwrap_or_else(|e| panic!("{e}\nin:\n{value}"));
                    checked += 1;
                }
            }
        }
        assert!(checked >= 30, "only {checked} examples found");
    }
}
