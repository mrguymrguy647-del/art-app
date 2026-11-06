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
pub(crate) mod geometry;
pub(crate) mod player;
pub mod record;
pub mod reuse;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use player::{
    Event, Part, PlayOptions, Player, Progress, Report, Silhouette, focal_point, layer_named,
    unused_warnings,
};
pub use reuse::{Reuse, Sequence};

/// Written at the top of every script, so it can be recognised.
pub const FORMAT_NAME: &str = "duckquill-script";
/// The name scripts had before the app was renamed Duck Quill. Still
/// played, with no warning: only `quill check` mentions it.
pub const OLD_FORMAT_NAME: &str = "artapp-script";
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
    /// Animations, by name, in the order written. Each plays `commands`
    /// and then its own, once per frame. Empty for a single picture.
    pub animations: Animations,
    /// The character's anchor point in canvas pixels, for sprite sheets.
    /// Default: the bottom centre of the canvas.
    pub pivot: Option<[f32; 2]>,
    /// What the painter means (see `Intent`). Paints nothing.
    pub intent: Option<Intent>,
}

/// What the painter means: an optional block at the top of a script
/// that paints nothing, read by `check`, `views` and `critique`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    /// What the picture is of, in a short sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Where the eye should go: `[x, y]`, or the name of a named group
    /// or `use` (the middle of its box).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focal: Option<Value>,
    /// Where the light comes from: a direction word (`"top right"`),
    /// degrees clockwise (0 = right), `[x, y]` (a sun, a lamp),
    /// `"behind"` or `"front"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Value>,
    /// A few words, for people and for the blind-read comparison.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood: Option<String>,
    /// Palette names (2 to 6) that set the picture's key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_colors: Vec<String>,
}

/// Where the eye should go, as the intent says it.
#[derive(Clone, Debug, PartialEq)]
pub enum Focal {
    Point([f32; 2]),
    /// A named group or `use`.
    Part(String),
}

/// Where the light comes from, as the intent says it.
#[derive(Clone, Debug, PartialEq)]
pub enum Light {
    /// Degrees clockwise, 0 = from the right, 270 = from above.
    Direction(f32),
    /// A point on the canvas (a sun, a lamp).
    Point([f32; 2]),
    /// From behind the subject (backlit) or from the viewer.
    Behind,
    Front,
}

impl Light {
    /// The direction words a script may use, and their degrees.
    pub const WORDS: [(&'static str, f32); 10] = [
        ("right", 0.0),
        ("bottom right", 45.0),
        ("bottom", 90.0),
        ("below", 90.0),
        ("bottom left", 135.0),
        ("left", 180.0),
        ("top left", 225.0),
        ("top", 270.0),
        ("above", 270.0),
        ("top right", 315.0),
    ];

    /// Degrees clockwise the light comes from, if it has a direction.
    pub fn degrees(&self) -> Option<f32> {
        match self {
            Light::Direction(d) => Some(*d),
            Light::Point(_) | Light::Behind | Light::Front => None,
        }
    }

    /// In words, for reports.
    pub fn describe(&self) -> String {
        match self {
            Light::Direction(d) => {
                let d = d.rem_euclid(360.0);
                let word = Light::WORDS
                    .iter()
                    .filter(|(w, _)| !matches!(*w, "below" | "above"))
                    .min_by(|(_, a), (_, b)| {
                        let da = (a - d).abs().min(360.0 - (a - d).abs());
                        let db = (b - d).abs().min(360.0 - (b - d).abs());
                        da.total_cmp(&db)
                    })
                    .map_or("?", |(w, _)| *w);
                format!("from the {word} ({d:.0} degrees)")
            }
            Light::Point([x, y]) => format!("from the point ({x:.0}, {y:.0})"),
            Light::Behind => "from behind the subject".to_owned(),
            Light::Front => "from the front".to_owned(),
        }
    }
}

impl Intent {
    /// The focal point as written, checked.
    pub fn focal(&self) -> Result<Option<Focal>, String> {
        match &self.focal {
            None => Ok(None),
            Some(Value::String(name)) if !name.trim().is_empty() => {
                Ok(Some(Focal::Part(name.clone())))
            }
            Some(Value::Array(pair)) if pair.len() == 2 && pair.iter().all(Value::is_number) => {
                let n = |i: usize| pair[i].as_f64().unwrap_or(0.0) as f32;
                Ok(Some(Focal::Point([n(0), n(1)])))
            }
            Some(other) => Err(format!(
                "`focal` is [x, y] or the name of a named group or use, not {other}"
            )),
        }
    }

    /// The light as written, checked.
    pub fn light(&self) -> Result<Option<Light>, String> {
        let help = || {
            let words: Vec<&str> = Light::WORDS.iter().map(|(w, _)| *w).collect();
            format!(
                "`light` is a direction ({}), degrees clockwise (0 = right), [x, y] (a sun, a \
                 lamp), \"behind\" or \"front\"",
                words.join(", ")
            )
        };
        match &self.light {
            None => Ok(None),
            Some(Value::Number(n)) => Ok(Some(Light::Direction(n.as_f64().unwrap_or(0.0) as f32))),
            Some(Value::Array(pair)) if pair.len() == 2 && pair.iter().all(Value::is_number) => {
                let n = |i: usize| pair[i].as_f64().unwrap_or(0.0) as f32;
                Ok(Some(Light::Point([n(0), n(1)])))
            }
            Some(Value::String(word)) => {
                let word = word.trim().to_lowercase().replace('-', " ");
                let word = word.split_whitespace().collect::<Vec<_>>().join(" ");
                match word.as_str() {
                    "behind" | "backlit" => Ok(Some(Light::Behind)),
                    "front" => Ok(Some(Light::Front)),
                    _ => Light::WORDS
                        .iter()
                        .find(|(w, _)| *w == word)
                        .map(|(_, d)| Some(Light::Direction(*d)))
                        .ok_or_else(|| format!("unknown light \"{word}\"; {}", help())),
                }
            }
            Some(_) => Err(help()),
        }
    }

    /// Checks every field against the palette: used when the script is read.
    fn check(&self, palette: &BTreeMap<String, String>) -> Result<(), String> {
        self.focal()?;
        self.light()?;
        if !self.key_colors.is_empty() && !(2..=6).contains(&self.key_colors.len()) {
            return Err(format!(
                "`key_colors` names 2 to 6 palette colours, not {}",
                self.key_colors.len()
            ));
        }
        for name in &self.key_colors {
            if !palette.contains_key(name) {
                return Err(format!("`key_colors`: \"{name}\" isn't in the palette"));
            }
        }
        if let Some(subject) = &self.subject
            && subject.trim().is_empty()
        {
            return Err("`subject` is empty: say what the picture is of".into());
        }
        Ok(())
    }

    /// The intent in a few lines, for `check` and the gallery.
    pub fn describe(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(subject) = &self.subject {
            lines.push(format!("subject: {subject}"));
        }
        match self.focal() {
            Ok(Some(Focal::Part(name))) => lines.push(format!("focal point: \"{name}\"")),
            Ok(Some(Focal::Point([x, y]))) => lines.push(format!("focal point: ({x:.0}, {y:.0})")),
            _ => {}
        }
        if let Ok(Some(light)) = self.light() {
            lines.push(format!("light: {}", light.describe()));
        }
        if let Some(mood) = &self.mood {
            lines.push(format!("mood: {mood}"));
        }
        if !self.key_colors.is_empty() {
            lines.push(format!("key colours: {}", self.key_colors.join(", ")));
        }
        lines
    }
}

/// One animation: `frames` frames at `fps`, looping or not, made of the
/// script's commands followed by these. See `docs/animation-design.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub frames: u32,
    pub fps: f32,
    #[serde(rename = "loop", default)]
    pub looping: bool,
    /// `true` (`"time": "seconds"`): plain numbers in its keyframes are
    /// seconds, not frames.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub seconds: bool,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// The animations of a script, by name, in the order they were written
/// (so the first one is the one `render` shows by default).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animations(Vec<(String, Animation)>);

impl Animations {
    pub fn get(&self, name: &str) -> Option<&Animation> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Animation)> {
        self.0.iter().map(|(n, a)| (n, a))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(n, _)| n)
    }

    pub fn values(&self) -> impl Iterator<Item = &Animation> {
        self.0.iter().map(|(_, a)| a)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Adds one at the end. Returns false (and changes nothing) if the
    /// name is taken.
    pub fn insert(&mut self, name: String, animation: Animation) -> bool {
        if self.contains_key(&name) {
            return false;
        }
        self.0.push((name, animation));
        true
    }
}

impl<'a> IntoIterator for &'a Animations {
    type Item = (&'a String, &'a Animation);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, Animation)>,
        fn(&'a (String, Animation)) -> (&'a String, &'a Animation),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter().map(|(n, a)| (n, a))
    }
}

impl std::ops::Index<&str> for Animations {
    type Output = Animation;

    fn index(&self, name: &str) -> &Animation {
        self.get(name)
            .unwrap_or_else(|| panic!("no animation called \"{name}\""))
    }
}

impl Serialize for Animations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, animation) in &self.0 {
            map.serialize_entry(name, animation)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Animations {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pairs: Vec<(String, Animation)> = ordered_pairs(deserializer)?;
        Ok(Animations(pairs))
    }
}

/// Reads a JSON object as (name, value) pairs in the order written.
fn ordered_pairs<'de, D, T>(deserializer: D) -> Result<Vec<(String, T)>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Pairs<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Pairs<T> {
        type Value = Vec<(String, T)>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object of animations by name")
        }

        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            let mut pairs = Vec::new();
            while let Some((name, value)) = map.next_entry::<String, T>()? {
                pairs.push((name, value));
            }
            Ok(pairs)
        }
    }
    deserializer.deserialize_map(Pairs(std::marker::PhantomData))
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
    Select(SelectCmd),
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
    Include(IncludeCmd),
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
            Command::Select(_) => "select",
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
            Command::Include(_) => "include",
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
            other => match other.shape_name() {
                Some(name) => format!("{} \"{name}\"", other.kind()),
                None => other.kind().to_owned(),
            },
        }
    }

    /// A single shape's (or stroke's) `name`, if it has one.
    pub fn shape_name(&self) -> Option<&str> {
        match self {
            Command::Stroke(c) | Command::Erase(c) => c.name.as_deref(),
            Command::Line(c) => c.name.as_deref(),
            Command::Path(c) => c.name.as_deref(),
            Command::Rect(c) => c.name.as_deref(),
            Command::Ellipse(c) => c.name.as_deref(),
            Command::Polygon(c) => c.name.as_deref(),
            Command::Text(c) => c.name.as_deref(),
            Command::Hatch(c) => c.name.as_deref(),
            Command::Scatter(c) => c.name.as_deref(),
            Command::Stamp(c) => c.name.as_deref(),
            Command::Pixels(c) => c.name.as_deref(),
            _ => None,
        }
    }

    /// The same command without its shape `name`.
    pub fn without_shape_name(&self) -> Command {
        let mut command = self.clone();
        match &mut command {
            Command::Stroke(c) | Command::Erase(c) => c.name = None,
            Command::Line(c) => c.name = None,
            Command::Path(c) => c.name = None,
            Command::Rect(c) => c.name = None,
            Command::Ellipse(c) => c.name = None,
            Command::Polygon(c) => c.name = None,
            Command::Text(c) => c.name = None,
            Command::Hatch(c) => c.name = None,
            Command::Scatter(c) => c.name = None,
            Command::Stamp(c) => c.name = None,
            Command::Pixels(c) => c.name = None,
            _ => {}
        }
        command
    }
}

/// `canvas`: the first command. Either a new canvas (`width`, `height`,
/// `paper`) or an existing picture (`open`: a .quill file, relative to
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
    /// A built-in preset by name, e.g. "Pencil (graphite)": all its
    /// settings (not the colour), then any others given here on top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
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
    /// What the lightest touch still gives, 0..1, of the size and of the
    /// opacity (see `brush::BrushSettings`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_size_min: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_opacity_min: Option<f32>,
    /// Above 1: press harder for the full effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_curve: Option<f32>,
    /// Paper grain, 0..1, and how coarse it is (1: fine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain_scale: Option<f32>,
    /// The most a layer darkens from going over it again, 0..1 (0: no limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_up: Option<f32>,
}

/// `select`: from here on, every painting command stays inside the
/// selection (as inside a group's `mask`, and as with a selection in the
/// app). One shape (`rect`, `ellipse`, `circle`, `polygon`, written like
/// an area), or `layer` (where a layer has paint), or `all`, `none` or
/// `invert`. `mode` "add" or "subtract" combines the shape with the
/// selection there is; `feather` softens the shape's edge (or, alone,
/// the selection's).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ellipse: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polygon: Option<Value>,
    /// Where this layer has paint, as much as each pixel is opaque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    /// `true`: no selection any more; everything can be painted again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invert: Option<bool>,
    /// "replace" (the default), "add" or "subtract".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// How many pixels the edge fades over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feather: Option<f32>,
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
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
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
    /// `{"color": "ink", "size": 3, "rim": {...}}`: a line around
    /// everything on the layer, behind it (and rim light on top).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Value>,
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
            ("outline", &self.outline),
        ] {
            if let Some(value) = value {
                object.insert(name.to_owned(), value.clone());
            }
        }
        Value::Object(object)
    }
}

/// `include`: reads another script's `define`s and palette (not its
/// commands or animations), relative to this script. Only at the top
/// level. An included file may include others.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncludeCmd {
    pub file: String,
}

/// `stroke` / `erase`: paints (or erases) through the points with the
/// brush, exactly as if a pen had moved through them. `color`, `size`,
/// `opacity`, `hardness` and `flow` change the brush for this stroke only.
/// `jitter` wobbles the line by up to that many pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrokeCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
    /// Mirror lines: the stroke is painted again mirrored across them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symmetry: Option<SymmetryCmd>,
}

/// A stroke's mirror lines: `x` is a line going down at that x (left and
/// right mirror each other), `y` a line going across at that y; both
/// give four copies. Inside a group they move and turn with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymmetryCmd {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
}

/// `line`: a straight line from `from` to `to`, thinning towards its ends
/// with `taper`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
}

/// `path`: a smooth curve through the points, drawn with the brush
/// (`line: false` to skip that), optionally `closed` and filled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
}

/// `rect`: a rectangle, `fill`ed and/or `outline`d. `radius` rounds the
/// corners; `rotation` turns it (degrees, clockwise, around its middle).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RectCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
}

/// `ellipse`: an ellipse (or circle) around `center`, `fill`ed and/or
/// `outline`d. `radius` is one number (a circle) or `[rx, ry]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EllipseCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
}

/// `polygon`: straight edges through the points (at least 3), `fill`ed
/// and/or `outline`d.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolygonCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
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
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
}

/// `scatter`: `count` dabs (or short strokes) at random places in an
/// `area`, with sizes, colours and opacities picked from the ranges given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterCmd {
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// Which random numbers (a whole number; may be keyframes or a parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<Value>,
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
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    /// With a scale different across and down: `true` resizes brush and
    /// outline sizes by the average of the two (lines look stretched);
    /// normally they follow the smaller one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch_brush: Option<bool>,
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
    /// `{"color": "ink", "size": 3, "rim": {"color": "cream", "size": 2,
    /// "angle": -45}}`: a line around the silhouette of everything the
    /// group draws, behind it, and rim light along the inside edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Value>,
    #[serde(default)]
    pub commands: Vec<Command>,
}

/// `define`: stores `commands` as a symbol called `name`, without
/// painting anything. Only at the top level of a script.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefineCmd {
    pub name: String,
    /// Parameters and their defaults: `{"leg": 0, "eye": "eye_open"}`.
    /// Inside the symbol, `"$leg"` stands for the value a `use` gives,
    /// in any field (points, sizes, centres...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, Value>>,
    /// The commands, read with the parameters' defaults put in.
    #[serde(default)]
    pub commands: Vec<Command>,
    /// With parameters: the commands as written, `"$name"`s and all. Each
    /// `use` reads them again with its own values (see
    /// `symbol_commands`), and the script is written out from these.
    #[serde(skip)]
    pub raw: Option<std::sync::Arc<Vec<Value>>>,
}

impl Serialize for DefineCmd {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut out = serializer.serialize_struct("DefineCmd", 3)?;
        out.serialize_field("name", &self.name)?;
        if let Some(params) = &self.params {
            out.serialize_field("params", params)?;
        }
        // As written, so the parameters stay parameters.
        match &self.raw {
            Some(raw) => out.serialize_field("commands", raw.as_ref())?,
            None => out.serialize_field("commands", &self.commands)?,
        }
        out.end()
    }
}

/// `value` with every `"$name"` (a whole string) that names one of
/// `params` replaced by its value.
fn substitute(value: &Value, params: &BTreeMap<String, Value>) -> Value {
    match value {
        Value::String(text) => match text.strip_prefix('$').and_then(|name| params.get(name)) {
            Some(given) => given.clone(),
            None => value.clone(),
        },
        Value::Array(items) => Value::Array(items.iter().map(|v| substitute(v, params)).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, v)| (key.clone(), substitute(v, params)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A command inside a define, read as written if it can be (then its
/// `"$name"`s are worked out as it plays, with the usual messages), else
/// with `params` put in (a `"$name"` standing for a point or a size).
/// Says whether the values were needed.
fn parse_in_define(
    child: Value,
    params: &BTreeMap<String, Value>,
) -> Result<(Command, bool), Problem> {
    if params.is_empty() {
        return parse_command(child, true).map(|c| (c, false));
    }
    match parse_command(child.clone(), true) {
        Ok(command) => Ok((command, false)),
        Err(_) => parse_command(substitute(&child, params), true).map(|c| (c, true)),
    }
}

/// A symbol's commands, read with these parameter values put in where
/// they're needed (see `parse_in_define`).
pub fn symbol_commands(
    raw: &[Value],
    params: &BTreeMap<String, Value>,
) -> Result<Vec<Command>, String> {
    raw.iter()
        .enumerate()
        .map(|(index, child)| {
            let label = label_of(child).unwrap_or_else(|| "?".into());
            parse_in_define(child.clone(), params)
                .map(|(command, _)| command)
                .map_err(|problem| {
                    let mut path = vec![format!("{} ({label})", index + 1)];
                    path.extend(problem.inside);
                    format!("{}: {}", path.join(" > "), problem.message)
                })
        })
        .collect()
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
    /// With a scale different across and down: `true` resizes brush and
    /// outline sizes by the average of the two (lines look stretched);
    /// normally they follow the smaller one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stretch_brush: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    /// As on a group: paint only inside this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Value>,
    /// As on a group: filters on what it draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Value>,
    /// As on a group: an outline around everything it draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Value>,
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
    /// A name, as on a group: `check`, `critique` and `views` list it as
    /// a part (`rect "door"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
    #[serde(default, deserialize_with = "ordered_pairs")]
    animations: Vec<(String, RawAnimation)>,
    #[serde(default)]
    pivot: Option<[f32; 2]>,
    #[serde(default)]
    intent: Option<Intent>,
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
    /// `"seconds"`: plain numbers in keyframes are seconds; `"frames"`
    /// (the default) frames.
    #[serde(default)]
    time: Option<String>,
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
            && format != OLD_FORMAT_NAME
        {
            return Err(ScriptError::general(format!(
                "\"format\" should be \"{FORMAT_NAME}\", not \"{format}\""
            )));
        }
        if let Some(version) = raw.version
            && version > FORMAT_VERSION
        {
            return Err(ScriptError::general(format!(
                "this script is version {version}, but this Duck Quill only plays up to version \
                 {FORMAT_VERSION}. Please update Duck Quill."
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

        let mut animations = Animations::default();
        for (name, raw) in raw.animations {
            let problem = |message: String| ScriptError::general(message).in_animation(&name);
            if name.trim().is_empty() {
                return Err(ScriptError::general("an animation needs a name"));
            }
            if animations.contains_key(&name) {
                return Err(ScriptError::general(format!(
                    "the animation \"{name}\" is written twice"
                )));
            }
            let seconds = match raw.time.as_deref() {
                None | Some("frames") => false,
                Some("seconds") => true,
                Some(other) => {
                    return Err(problem(format!(
                        "`time` is \"frames\" (the default) or \"seconds\", not \"{other}\""
                    )));
                }
            };
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
                    seconds,
                    commands: inner,
                },
            );
        }
        if let Some([x, y]) = raw.pivot
            && !(x.is_finite() && y.is_finite())
        {
            return Err(ScriptError::general("`pivot` must be [x, y] in pixels"));
        }

        if let Some(intent) = &raw.intent {
            intent
                .check(&raw.palette)
                .map_err(|message| ScriptError::general(format!("intent: {message}")))?;
        }
        Ok(Script {
            title: raw.title,
            notes: raw.notes,
            seed: raw.seed.unwrap_or(1),
            palette: raw.palette,
            commands,
            animations,
            pivot: raw.pivot,
            intent: raw.intent,
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
        if let Some(intent) = &self.intent {
            out += &format!("  \"intent\": {},\n", json(intent));
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
        .unwrap_or_default()
        .to_owned();
    let kind = kind.as_str();
    if matches!(kind, "define" | "include") && nested {
        return Err(Problem::here(format!(
            "`{kind}` only works at the top level of a script, not inside a group, symbol, \
             repeat or animation"
        )));
    }
    let mut inner = Vec::new();
    // A define's parameters: their defaults are put in while reading (so
    // "$x" can stand for a point or a size), and the commands as written
    // are kept for each use to read with its own values.
    let defaults: BTreeMap<String, Value> = match (kind, object.get("params")) {
        ("define", Some(Value::Object(params))) => {
            params.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }
        _ => BTreeMap::new(),
    };
    let mut raw = None;
    if matches!(kind, "group" | "define" | "repeat") {
        match object.remove("commands") {
            Some(Value::Array(list)) => {
                let written = list.clone();
                let mut needs_values = false;
                for (index, child) in list.into_iter().enumerate() {
                    let label = label_of(&child).unwrap_or_else(|| "?".into());
                    let parsed = if kind == "define" {
                        parse_in_define(child, &defaults).map(|(command, needed)| {
                            needs_values |= needed;
                            command
                        })
                    } else {
                        parse_command(child, true)
                    };
                    let command = parsed.map_err(|mut problem| {
                        problem.inside.insert(0, format!("{} ({label})", index + 1));
                        problem
                    })?;
                    inner.push(command);
                }
                if needs_values {
                    raw = Some(std::sync::Arc::new(written));
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
    let mut command: Command = serde_json::from_value(value).map_err(|error| {
        let mut message = error.to_string();
        // A "$name" where a number or point was expected.
        if let Some(start) = message.find("string \"$") {
            let name: String = message[start + 8..]
                .chars()
                .take_while(|&c| c != '"')
                .collect();
            message.push_str(&format!(
                ". \"{name}\" would stand for a symbol parameter, but only inside a define \
                 that has \"{}\" in its \"params\"",
                name.trim_start_matches('$')
            ));
        }
        Problem::here(message)
    })?;
    match &mut command {
        Command::Group(group) => group.commands = inner,
        Command::Define(define) => {
            define.commands = inner;
            define.raw = raw;
        }
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
        Some(name)
            if matches!(
                kind,
                "group"
                    | "define"
                    | "use"
                    | "stroke"
                    | "erase"
                    | "line"
                    | "path"
                    | "rect"
                    | "ellipse"
                    | "polygon"
                    | "text"
                    | "hatch"
                    | "scatter"
                    | "stamp"
                    | "pixels"
            ) =>
        {
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
              "format": "duckquill-script", "version": 1, "seed": 7,
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
        assert!(error.to_string().contains("duckquill-script"), "{error}");
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
        // On Windows, Git may check the files out with "\r\n" line endings;
        // the examples are found by "\n", so read them with Unix endings.
        let unix = |text: &str| text.replace("\r\n", "\n");
        let quick_start = unix(include_str!("../../../docs/QUICKSTART.md"));
        let docs = [
            unix(include_str!("../../../docs/script-format.md")),
            unix(include_str!("../../../docs/AI_GUIDE.md")),
            quick_start.clone(),
        ];
        // The quick start's first script is the example file, word for word.
        let example = unix(include_str!("../../../examples/two-ducks.json"));
        assert!(
            quick_start.contains(example.trim()),
            "QUICKSTART.md and two-ducks.json differ"
        );
        let mut checked = 0;
        for doc in &docs {
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
                if block.starts_with("\"intent\"") {
                    // A top-level block: check it in a script.
                    let script = format!(
                        r##"{{ "palette": {{ "sky_low": "#F08040", "sun": "#FFF0C0", "water_dark": "#204060" }},
                            {block}, "commands": [ {{ "cmd": "canvas", "width": 10, "height": 10 }} ] }}"##
                    );
                    parse(&script).unwrap_or_else(|e| panic!("{e}\nin:\n{block}"));
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

    #[test]
    fn the_intent_is_read_checked_and_written_back() {
        let script = parse(
            r##"{ "palette": { "sky": "#8EC1E6", "sun": "#FFF1C0" },
              "intent": { "subject": "a duck at sunset", "focal": "duck", "light": "top right",
                          "mood": "calm", "key_colors": ["sky", "sun"] },
              "commands": [ { "cmd": "canvas", "width": 10, "height": 10 } ] }"##,
        )
        .unwrap();
        let intent = script.intent.as_ref().unwrap();
        assert_eq!(intent.focal().unwrap(), Some(Focal::Part("duck".into())));
        assert_eq!(intent.light().unwrap(), Some(Light::Direction(315.0)));
        assert_eq!(
            intent.describe(),
            [
                "subject: a duck at sunset",
                "focal point: \"duck\"",
                "light: from the top right (315 degrees)",
                "mood: calm",
                "key colours: sky, sun"
            ]
        );
        assert_eq!(
            parse(&script.to_json()).unwrap(),
            script,
            "written back the same"
        );
        // Other ways to say the light and the focal point.
        let other = parse(
            r##"{ "intent": { "focal": [30, 40], "light": [100, 0] },
              "commands": [ { "cmd": "canvas", "width": 10, "height": 10 } ] }"##,
        )
        .unwrap();
        let intent = other.intent.unwrap();
        assert_eq!(intent.focal().unwrap(), Some(Focal::Point([30.0, 40.0])));
        assert_eq!(intent.light().unwrap(), Some(Light::Point([100.0, 0.0])));
        let behind = parse(
            r##"{ "intent": { "light": "behind" }, "commands": [ { "cmd": "canvas", "width": 10, "height": 10 } ] }"##,
        )
        .unwrap();
        assert_eq!(behind.intent.unwrap().light().unwrap(), Some(Light::Behind));
        assert_eq!(
            Light::Direction(100.0).describe(),
            "from the bottom (100 degrees)"
        );
        // Mistakes.
        let bad = |intent: &str| {
            parse(&format!(
                r##"{{ "palette": {{ "sky": "#8EC1E6" }}, "intent": {intent},
                  "commands": [ {{ "cmd": "canvas", "width": 10, "height": 10 }} ] }}"##
            ))
            .unwrap_err()
            .to_string()
        };
        let e = bad(r##"{ "light": "sideways" }"##);
        assert!(e.contains("intent: unknown light \"sideways\""), "{e}");
        let e = bad(r##"{ "key_colors": ["sky", "sea"] }"##);
        assert!(e.contains("\"sea\" isn't in the palette"), "{e}");
        let e = bad(r##"{ "key_colors": ["sky"] }"##);
        assert!(e.contains("2 to 6 palette colours"), "{e}");
        let e = bad(r##"{ "focal": true }"##);
        assert!(e.contains("`focal` is [x, y] or the name"), "{e}");
        let e = bad(r##"{ "topic": "ducks" }"##);
        assert!(e.contains("unknown field `topic`"), "{e}");
    }
}
