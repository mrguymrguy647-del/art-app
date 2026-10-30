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

mod geometry;
mod player;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use player::{Event, PlayOptions, Player, Progress, layer_named};

/// Written at the top of every script, so it can be recognised.
pub const FORMAT_NAME: &str = "artapp-script";
/// The script format version this app writes, and the newest it plays.
pub const FORMAT_VERSION: u32 = 1;

/// A script that has been read and checked for typos (but not yet played).
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    /// A title for people; not used when playing.
    pub title: Option<String>,
    /// Where randomness starts (see `random.rs`). Default 1.
    pub seed: u64,
    /// Colour names defined once and used by name in commands.
    pub palette: BTreeMap<String, String>,
    pub commands: Vec<Command>,
}

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
    Snapshot(SnapshotCmd),
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
            Command::Snapshot(_) => "snapshot",
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

/// `brush`: changes the brush. Only the settings given change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrushCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
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
    pub from: [f32; 2],
    pub to: [f32; 2],
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
    /// The command's name ("stroke"), if known.
    pub kind: Option<String>,
    /// The line in the file, for JSON mistakes (like a missing comma).
    pub line: Option<usize>,
    pub message: String,
}

impl ScriptError {
    /// A problem with the script as a whole.
    pub fn general(message: impl Into<String>) -> Self {
        Self {
            command: None,
            kind: None,
            line: None,
            message: message.into(),
        }
    }

    /// A problem in command number `index + 1`.
    pub fn in_command(index: usize, kind: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            command: Some(index + 1),
            kind: kind.map(str::to_owned),
            line: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.command, &self.kind, self.line) {
            (Some(number), Some(kind), _) => {
                write!(f, "command {number} ({kind}): {}", self.message)
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
    /// Comments for people, ignored.
    #[serde(default)]
    #[allow(dead_code)]
    notes: Option<Value>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    palette: BTreeMap<String, String>,
    commands: Vec<Value>,
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
            line: Some(error.line()),
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
        for (index, mut value) in raw.commands.into_iter().enumerate() {
            let kind = value.get("cmd").and_then(Value::as_str).map(str::to_owned);
            if let Some(object) = value.as_object_mut() {
                object.remove("note"); // a comment for people
            } else {
                return Err(ScriptError::in_command(
                    index,
                    None,
                    "each command must be an object like {\"cmd\": \"stroke\", ...}",
                ));
            }
            let command: Command = serde_json::from_value(value).map_err(|error| {
                ScriptError::in_command(index, kind.as_deref(), error.to_string())
            })?;
            commands.push(command);
        }
        if commands.is_empty() {
            return Err(ScriptError::general("the script has no commands"));
        }

        Ok(Script {
            title: raw.title,
            seed: raw.seed.unwrap_or(1),
            palette: raw.palette,
            commands,
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
        out += &format!("  \"seed\": {},\n", self.seed);
        out += &format!("  \"palette\": {},\n", json(&self.palette));
        out += "  \"commands\": [\n";
        for (i, command) in self.commands.iter().enumerate() {
            let comma = if i + 1 < self.commands.len() { "," } else { "" };
            out += &format!("    {}{comma}\n", json(command));
        }
        out += "  ]\n}\n";
        out
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
}
