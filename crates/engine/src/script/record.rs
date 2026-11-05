//! Writing down what was painted, as painting-script commands.
//!
//! The app records each change (a stroke, a fill, a new layer, ...) as
//! script commands together with its undo step (see
//! `History::push_recorded`). "Export strokes" collects the steps that
//! are still in the picture and tidies them up with [`tidy`]: a stroke is
//! recorded with its layer and brush, but the exported script only says
//! "change layer" or "change brush" when they actually change.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::{
    BrushCmd, CanvasCmd, ClearLayerCmd, Command, DeleteLayerCmd, FillCmd, GradientCmd, LayerCmd,
    MoveLayerCmd, Point, RenameLayerCmd, Script, StrokeCmd, TransformLayerCmd,
};
use crate::brush::{BrushMode, BrushSettings};
use crate::fill::{FloodFill, Gradient, GradientShape, Rgba, SampleFrom};
use crate::layer::LayerProps;
use crate::stroke::StrokePoint;
use crate::tile::Rgba8;

/// Rounds to `places` decimals: plenty for pixels, and far easier to read.
fn round(value: f32, places: i32) -> f32 {
    let factor = 10f32.powi(places);
    (value * factor).round() / factor
}

/// "#RRGGBB", or "#RRGGBBAA" when the colour is see-through.
pub fn hex([r, g, b, a]: Rgba) -> String {
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// Selects (or creates) the layer called `name`.
pub fn select_layer(name: &str) -> Command {
    Command::Layer(LayerCmd {
        name: name.to_owned(),
        ..LayerCmd::default()
    })
}

/// Every brush setting, as a `brush` command (the colour only if wanted).
pub fn brush(settings: &BrushSettings, with_color: bool) -> Command {
    let [r, g, b] = settings.color;
    Command::Brush(BrushCmd {
        color: with_color.then(|| hex([r, g, b, 255])),
        size: Some(round(settings.size, 2)),
        opacity: Some(round(settings.opacity, 3)),
        flow: Some(round(settings.flow, 3)),
        hardness: Some(round(settings.hardness, 3)),
        spacing: Some(round(settings.spacing, 3)),
        pressure_size: Some(settings.pressure_size),
        pressure_opacity: Some(settings.pressure_opacity),
        pressure_flow: Some(settings.pressure_flow),
    })
}

/// A brush or eraser stroke on `layer`, through `points` exactly as
/// they went into the stroke (after smoothing).
pub fn stroke(layer: &str, settings: &BrushSettings, points: &[StrokePoint]) -> Vec<Command> {
    let erase = settings.mode == BrushMode::Erase;
    let points = points
        .iter()
        .map(|p| {
            let pressure = (p.pressure < 1.0).then(|| round(p.pressure, 3));
            Point::new(round(p.x, 2), round(p.y, 2), pressure)
        })
        .collect();
    let stroke = StrokeCmd {
        points,
        ..StrokeCmd::default()
    };
    vec![
        select_layer(layer),
        // The eraser's colour doesn't matter, so leave the brush colour alone.
        brush(settings, !erase),
        if erase {
            Command::Erase(stroke)
        } else {
            Command::Stroke(stroke)
        },
    ]
}

/// A paint bucket fill on `layer`.
pub fn bucket(layer: &str, fill: &FloodFill, color: Rgba, opacity: f32) -> Vec<Command> {
    vec![
        select_layer(layer),
        Command::Fill(FillCmd {
            at: [round(fill.x, 2), round(fill.y, 2)],
            color: Some(hex(color)),
            tolerance: Some(round(fill.tolerance, 3)),
            sample: Some(
                match fill.sample {
                    SampleFrom::Visible => "visible",
                    SampleFrom::Layer => "layer",
                }
                .to_owned(),
            ),
            grow: Some(fill.expand),
            opacity: Some(round(opacity, 3)),
        }),
    ]
}

/// A gradient across the whole of `layer`.
pub fn gradient(layer: &str, gradient: &Gradient, opacity: f32) -> Vec<Command> {
    let point = |(x, y): (f32, f32)| [round(x, 2), round(y, 2)];
    vec![
        select_layer(layer),
        Command::Gradient(GradientCmd {
            kind: Some(
                match gradient.shape {
                    GradientShape::Linear => "linear",
                    GradientShape::Radial => "radial",
                }
                .to_owned(),
            ),
            from: point(gradient.start),
            to: point(gradient.end),
            colors: None,
            stops: Some(
                gradient
                    .stops()
                    .iter()
                    .map(|stop| (round(stop.position, 4), hex(stop.color)))
                    .collect(),
            ),
            opacity: Some(round(opacity, 3)),
            area: None,
            erase: None,
        }),
    ]
}

pub fn clear_layer(name: &str) -> Vec<Command> {
    vec![Command::ClearLayer(ClearLayerCmd {
        name: Some(name.to_owned()),
    })]
}

/// A new layer called `name`, just above the layer `above` (or on top).
pub fn add_layer(name: &str, above: Option<&str>) -> Vec<Command> {
    vec![Command::Layer(LayerCmd {
        name: name.to_owned(),
        above: above.map(str::to_owned),
        ..LayerCmd::default()
    })]
}

pub fn delete_layer(name: &str) -> Vec<Command> {
    vec![Command::DeleteLayer(DeleteLayerCmd {
        name: name.to_owned(),
    })]
}

/// Moving a layer to position `index` (0 = bottom).
pub fn move_layer(name: &str, index: usize) -> Vec<Command> {
    vec![Command::MoveLayer(MoveLayerCmd {
        name: name.to_owned(),
        above: None,
        below: None,
        index: Some(index),
    })]
}

/// A `scale` field: nothing for no change, one number when it's the
/// same across and down, otherwise `[across, down]`.
pub fn scale(across: f64, down: f64) -> Option<Value> {
    if across == 1.0 && down == 1.0 {
        None
    } else if across == down {
        Some(Value::from(across))
    } else {
        Some(Value::from(vec![across, down]))
    }
}

/// Moving, turning, resizing or mirroring everything on a layer: the
/// command as given (its `name` set to `name`).
pub fn transform_layer(name: &str, command: &TransformLayerCmd) -> Vec<Command> {
    vec![Command::TransformLayer(TransformLayerCmd {
        name: Some(name.to_owned()),
        ..command.clone()
    })]
}

/// Changing a layer's name, visibility, opacity or blend mode.
pub fn layer_changes(before: &LayerProps, after: &LayerProps) -> Vec<Command> {
    let mut commands = Vec::new();
    if before.name != after.name {
        commands.push(Command::RenameLayer(RenameLayerCmd {
            name: before.name.clone(),
            to: after.name.clone(),
        }));
    }
    let settings = LayerCmd {
        name: after.name.clone(),
        opacity: (before.opacity != after.opacity).then(|| round(after.opacity, 3)),
        blend: (before.blend != after.blend).then(|| after.blend.name().to_owned()),
        visible: (before.visible != after.visible).then_some(after.visible),
        ..LayerCmd::default()
    };
    if settings.opacity.is_some() || settings.blend.is_some() || settings.visible.is_some() {
        commands.push(Command::Layer(settings));
    }
    commands
}

/// The commands of a played script, with palette colour names replaced by
/// the colours themselves, so they still work in another script.
pub fn without_palette(commands: &[Command], palette: &BTreeMap<String, String>) -> Vec<Command> {
    let resolve = |name: &mut String| {
        if let Some(color) = palette.get(name.as_str()) {
            *name = color.clone();
        }
    };
    commands
        .iter()
        .cloned()
        .map(|mut command| {
            for_each_color(&mut command, &mut |c| resolve(c));
            command
        })
        .collect()
}

/// Calls `f` on a colour written as a string, or on each key's colour
/// in keyframes.
fn colors_in_value(value: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match value {
        Value::String(color) => f(color),
        Value::Object(keyed) => {
            if let Some(Value::Array(keys)) = keyed.get_mut("keys") {
                for key in keys {
                    if let Some(Value::String(color)) = key.get_mut(1) {
                        f(color);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Calls `f` on every colour written in `command`.
fn for_each_color(command: &mut Command, f: &mut dyn FnMut(&mut String)) {
    let paint = |value: &mut Value, f: &mut dyn FnMut(&mut String)| match value {
        Value::String(color) => f(color),
        Value::Object(object) => {
            if let Some(Value::Array(colors)) = object.get_mut("colors") {
                for color in colors.iter_mut() {
                    if let Value::String(c) = color {
                        f(c);
                    }
                }
            }
            if let Some(Value::Array(stops)) = object.get_mut("stops") {
                for stop in stops.iter_mut() {
                    if let Some(Value::String(c)) = stop.get_mut(1) {
                        f(c);
                    }
                }
            }
        }
        _ => {}
    };
    let option = |color: &mut Option<String>, f: &mut dyn FnMut(&mut String)| {
        if let Some(c) = color {
            f(c);
        }
    };
    match command {
        Command::Canvas(c) => option(&mut c.paper, f),
        Command::Paper(c) => f(&mut c.color),
        Command::Brush(c) => option(&mut c.color, f),
        Command::Stroke(c) | Command::Erase(c) => option(&mut c.color, f),
        Command::Line(c) => option(&mut c.color, f),
        Command::Path(c) => {
            option(&mut c.color, f);
            if let Some(fill) = &mut c.fill {
                paint(fill, f);
            }
        }
        Command::Rect(c) => {
            option(&mut c.outline, f);
            if let Some(fill) = &mut c.fill {
                paint(fill, f);
            }
        }
        Command::Ellipse(c) => {
            option(&mut c.outline, f);
            if let Some(fill) = &mut c.fill {
                paint(fill, f);
            }
        }
        Command::Polygon(c) => {
            option(&mut c.outline, f);
            if let Some(fill) = &mut c.fill {
                paint(fill, f);
            }
        }
        Command::Fill(c) => option(&mut c.color, f),
        Command::Gradient(c) => {
            for color in c.colors.iter_mut().flatten() {
                f(color);
            }
            for (_, color) in c.stops.iter_mut().flatten() {
                f(color);
            }
        }
        Command::Hatch(c) => option(&mut c.color, f),
        Command::Scatter(c) => {
            option(&mut c.color, f);
            for color in c.colors.iter_mut().flatten() {
                f(color);
            }
        }
        Command::Text(c) => {
            option(&mut c.color, f);
            option(&mut c.outline, f);
        }
        Command::Pixels(c) => {
            for color in c.colors.values_mut() {
                f(color);
            }
        }
        Command::Group(c) => {
            for inner in &mut c.commands {
                for_each_color(inner, f);
            }
        }
        Command::Define(c) => {
            for inner in &mut c.commands {
                for_each_color(inner, f);
            }
        }
        Command::Repeat(c) => {
            for inner in &mut c.commands {
                for_each_color(inner, f);
            }
        }
        Command::Use(c) => {
            // Both sides of a colour swap are colours (the new one may be
            // keyframes, with a colour in each key).
            if let Some(swaps) = &mut c.colors {
                *swaps = std::mem::take(swaps)
                    .into_iter()
                    .map(|(mut from, mut to)| {
                        f(&mut from);
                        colors_in_value(&mut to, f);
                        (from, to)
                    })
                    .collect();
            }
        }
        Command::Layer(_)
        | Command::DeleteLayer(_)
        | Command::MoveLayer(_)
        | Command::RenameLayer(_)
        | Command::ClearLayer(_)
        | Command::TransformLayer(_)
        | Command::Snapshot(_) => {}
    }
}

/// Removes what doesn't change anything: selecting the layer that's
/// already selected, and brush settings that are already set.
pub fn tidy(commands: Vec<Command>) -> Vec<Command> {
    let mut out = Vec::with_capacity(commands.len());
    let mut layer: Option<String> = None;
    // The brush settings known so far (None = not known yet).
    let mut known = BrushCmd::default();
    for command in commands {
        match command {
            Command::Layer(select)
                if is_plain(&select) && layer.as_deref() == Some(select.name.as_str()) => {}
            Command::Layer(select) => {
                layer = Some(select.name.clone());
                out.push(Command::Layer(select));
            }
            Command::RenameLayer(rename) => {
                if layer.as_deref() == Some(rename.name.as_str()) {
                    layer = Some(rename.to.clone());
                }
                out.push(Command::RenameLayer(rename));
            }
            Command::DeleteLayer(delete) => {
                if layer.as_deref() == Some(delete.name.as_str()) {
                    layer = None; // another layer gets selected
                }
                out.push(Command::DeleteLayer(delete));
            }
            Command::Brush(settings) => {
                let changes = changed_brush_settings(&mut known, settings);
                if changes != BrushCmd::default() {
                    out.push(Command::Brush(changes));
                }
            }
            Command::Canvas(canvas) => {
                layer = None;
                known = BrushCmd::default();
                out.push(Command::Canvas(canvas));
            }
            other => out.push(other),
        }
    }
    out
}

/// A `layer` command that only selects (changes no settings).
fn is_plain(select: &LayerCmd) -> bool {
    select.opacity.is_none()
        && select.blend.is_none()
        && select.visible.is_none()
        && select.above.is_none()
        && select.below.is_none()
}

/// The settings in `new` that differ from `known`; `known` is updated.
fn changed_brush_settings(known: &mut BrushCmd, new: BrushCmd) -> BrushCmd {
    fn keep<T: PartialEq + Clone>(known: &mut Option<T>, new: Option<T>) -> Option<T> {
        match new {
            Some(value) if known.as_ref() != Some(&value) => {
                *known = Some(value.clone());
                Some(value)
            }
            _ => None,
        }
    }
    BrushCmd {
        color: keep(&mut known.color, new.color),
        size: keep(&mut known.size, new.size),
        opacity: keep(&mut known.opacity, new.opacity),
        flow: keep(&mut known.flow, new.flow),
        hardness: keep(&mut known.hardness, new.hardness),
        spacing: keep(&mut known.spacing, new.spacing),
        pressure_size: keep(&mut known.pressure_size, new.pressure_size),
        pressure_opacity: keep(&mut known.pressure_opacity, new.pressure_opacity),
        pressure_flow: keep(&mut known.pressure_flow, new.pressure_flow),
    }
}

/// Played scripts can each define a symbol with the same name (playing
/// one script twice, say), but a script defines each name only once. So
/// a definition the same as the one before it is left out, and a
/// different one gets a new name ("duck 2"), used by the `use`s after it.
pub fn unique_symbols(commands: Vec<Command>) -> Vec<Command> {
    // Each name as written, and what it's called from here on.
    let mut called: HashMap<String, String> = HashMap::new();
    // What each name in the output stands for.
    let mut bodies: HashMap<String, Vec<Command>> = HashMap::new();
    let mut out = Vec::with_capacity(commands.len());
    for mut command in commands {
        rename_uses(&mut command, &called);
        if let Command::Define(define) = &mut command {
            let written = define.name.clone();
            if let Some(now) = called.get(&written) {
                if bodies.get(now) == Some(&define.commands) {
                    continue; // the same again: the first one will do
                }
                let new_name = (2..)
                    .map(|n| format!("{written} {n}"))
                    .find(|name| !bodies.contains_key(name))
                    .expect("some number is free");
                define.name = new_name;
            }
            called.insert(written, define.name.clone());
            bodies.insert(define.name.clone(), define.commands.clone());
        }
        out.push(command);
    }
    out
}

/// Gives every `use` in `command` (also inside others) its new name.
fn rename_uses(command: &mut Command, called: &HashMap<String, String>) {
    match command {
        Command::Use(using) => {
            if let Some(name) = called.get(&using.name) {
                using.name = name.clone();
            }
        }
        Command::Group(group) => group
            .commands
            .iter_mut()
            .for_each(|c| rename_uses(c, called)),
        Command::Define(define) => define
            .commands
            .iter_mut()
            .for_each(|c| rename_uses(c, called)),
        Command::Repeat(repeat) => repeat
            .commands
            .iter_mut()
            .for_each(|c| rename_uses(c, called)),
        _ => {}
    }
}

/// A whole script from recorded commands: a canvas like the picture's,
/// then everything recorded, tidied.
pub fn script(
    width: u32,
    height: u32,
    paper: Rgba8,
    recorded: Vec<Command>,
    title: Option<String>,
    notes: Option<String>,
) -> Script {
    let mut commands = vec![Command::Canvas(CanvasCmd {
        width: Some(width),
        height: Some(height),
        paper: Some(hex([paper.r, paper.g, paper.b, 255])),
        open: None,
        pixel: None,
    })];
    commands.extend(unique_symbols(tidy(recorded)));
    Script {
        title,
        notes: notes.map(Value::String),
        seed: 1,
        palette: BTreeMap::new(),
        commands,
        animations: BTreeMap::new(),
        pivot: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Background, Document};
    use crate::file;
    use crate::fill::{Paint, fill_layer, flood_fill};
    use crate::history::{History, TileCommand};
    use crate::layer::BlendMode;
    use crate::layer_commands::{LayerPresenceCommand, LayerPropsCommand, MoveLayerCommand};
    use crate::script::{PlayOptions, Player};
    use crate::stroke::Stroke;

    fn png(document: &Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        file::write_png(document, Background::Paper, &mut bytes).unwrap();
        bytes
    }

    /// Paints a stroke the way the app does, and records it.
    fn paint(
        document: &mut Document,
        history: &mut History,
        settings: BrushSettings,
        points: &[(f32, f32, f32)],
    ) {
        let layer = document.active_id();
        let name = document.active_layer().name().to_owned();
        let points: Vec<StrokePoint> = points
            .iter()
            .map(|&(x, y, p)| StrokePoint::new(x, y, p))
            .collect();
        let mut stroke = Stroke::new(settings, document.bounds());
        let surface = &mut document.active_layer_mut().surface;
        for &point in &points {
            stroke.add_point(surface, point);
        }
        let before = stroke.finish(surface);
        history.push_recorded(
            Box::new(TileCommand::new("Stroke", layer, before)),
            stroke_commands(&name, &settings, &points),
        );
    }

    fn stroke_commands(
        name: &str,
        settings: &BrushSettings,
        points: &[StrokePoint],
    ) -> Vec<Command> {
        stroke(name, settings, points)
    }

    #[test]
    fn exported_strokes_replay_to_the_same_picture() {
        let mut document = Document::new(400, 300);
        let mut history = History::default();
        let red = BrushSettings {
            color: [200, 30, 30],
            size: 12.0,
            ..BrushSettings::default()
        };
        paint(
            &mut document,
            &mut history,
            red,
            &[(20.0, 20.0, 1.0), (200.0, 120.0, 0.5), (380.0, 40.0, 0.25)],
        );

        // A new layer with a gradient, a fill, and changed settings.
        let first = document.active_layer().name().to_owned();
        history.push_recorded(
            Box::new(LayerPresenceCommand::add_layer(&mut document)),
            add_layer(document.active_layer().name(), Some(&first)),
        );
        let sky = document.active_id();
        let gradient = Gradient::new(
            GradientShape::Linear,
            (0.0, 0.0),
            (0.0, 300.0),
            &[[40, 80, 200, 255], [40, 80, 200, 0]],
        );
        let before = fill_layer(&mut document, sky, &Paint::Gradient(gradient.clone()), 0.8);
        history.push_recorded(
            Box::new(TileCommand::new("Gradient", sky, before)),
            super::gradient("Layer 2", &gradient, 0.8),
        );
        let props_before = document.active_layer().props();
        let props_after = LayerProps {
            name: "Sky".into(),
            blend: BlendMode::Multiply,
            opacity: 0.7,
            ..props_before.clone()
        };
        let step = LayerPropsCommand::apply(&mut document, sky, props_after.clone()).unwrap();
        history.push_recorded(Box::new(step), layer_changes(&props_before, &props_after));
        let step = MoveLayerCommand::move_layer(&mut document, sky, 0).unwrap();
        history.push_recorded(Box::new(step), move_layer("Sky", 0));

        // Back on the first layer: an eraser stroke and a fill.
        let id = document.layers()[1].id();
        document.set_active(id);
        let eraser = BrushSettings {
            size: 20.0,
            ..BrushSettings::eraser()
        };
        paint(
            &mut document,
            &mut history,
            eraser,
            &[(100.0, 0.0, 1.0), (100.0, 300.0, 1.0)],
        );
        let fill = FloodFill {
            x: 300.0,
            y: 250.0,
            ..FloodFill::default()
        };
        let before = flood_fill(
            &mut document,
            id,
            &fill,
            &Paint::Color([250, 200, 60, 255]),
            1.0,
        );
        history.push_recorded(
            Box::new(TileCommand::new("Fill", id, before)),
            bucket(&first, &fill, [250, 200, 60, 255], 1.0),
        );

        // An undone stroke must not be exported.
        paint(
            &mut document,
            &mut history,
            red,
            &[(10.0, 290.0, 1.0), (390.0, 290.0, 1.0)],
        );
        history.undo(&mut document);

        let exported = script(
            400,
            300,
            document.paper(),
            history.recorded_script(),
            None,
            None,
        );
        let text = exported.to_json();
        assert_eq!(
            text.matches("\"cmd\":\"stroke\"").count(),
            1,
            "the undone stroke is left out:\n{text}"
        );

        let mut player =
            Player::new(Script::parse(&text).unwrap(), PlayOptions::default()).unwrap();
        let mut replayed = player.create_document().unwrap();
        player.run(&mut replayed, |_, _| {}).unwrap();
        assert_eq!(png(&replayed), png(&document));
        let names: Vec<&str> = replayed.layers().iter().map(|l| l.name()).collect();
        assert_eq!(names, ["Sky", "Layer 1"]);
    }

    #[test]
    fn tidying_drops_repeated_layers_and_brush_settings() {
        let settings = BrushSettings::default();
        let point = [StrokePoint::new(1.0, 2.0, 1.0)];
        let mut commands = stroke("A", &settings, &point);
        commands.extend(stroke("A", &settings, &point));
        let bigger = BrushSettings {
            size: 50.0,
            ..settings
        };
        commands.extend(stroke("A", &bigger, &point));
        let tidy = tidy(commands);
        let kinds: Vec<&str> = tidy.iter().map(Command::kind).collect();
        assert_eq!(
            kinds,
            ["layer", "brush", "stroke", "stroke", "brush", "stroke"]
        );
        let Command::Brush(change) = &tidy[4] else {
            panic!()
        };
        assert_eq!(
            *change,
            BrushCmd {
                size: Some(50.0),
                ..BrushCmd::default()
            },
            "only the size changed"
        );
    }

    #[test]
    fn palette_names_are_replaced_by_their_colours() {
        let script = Script::parse(
            r##"{ "palette": { "sun": "#FFCC00" }, "commands": [
                { "cmd": "canvas", "width": 10, "height": 10 },
                { "cmd": "ellipse", "center": [5, 5], "radius": 3, "fill": { "gradient": "radial", "from": [5, 5], "to": [5, 8], "stops": [[0, "sun"], [1, "#000"]] }, "outline": "sun" },
                { "cmd": "scatter", "area": { "circle": [5, 5, 4] }, "count": 3, "colors": ["sun", "#123"] }
            ] }"##,
        )
        .unwrap();
        let resolved = without_palette(&script.commands, &script.palette);
        let text = serde_json::to_string(&resolved).unwrap();
        assert!(!text.contains("\"sun\""), "{text}");
        assert_eq!(text.matches("#FFCC00").count(), 3, "{text}");
    }

    #[test]
    fn numbers_are_rounded_for_reading() {
        let commands = stroke(
            "A",
            &BrushSettings::default(),
            &[StrokePoint::new(1.23456, 7.891, 0.33333)],
        );
        let Command::Stroke(stroke) = &commands[2] else {
            panic!()
        };
        assert_eq!(stroke.points[0], Point::new(1.23, 7.89, Some(0.333)));
        assert_eq!(hex([255, 0, 16, 255]), "#FF0010");
        assert_eq!(hex([255, 0, 16, 128]), "#FF001080");
    }

    /// A script with groups, symbols, a repeat and text, played the way
    /// the app plays it, twice.
    const PLAYED: &str = r##"{ "palette": { "yellow": "#F2C230", "ink": "#202020" }, "commands": [
        { "cmd": "canvas", "width": 300, "height": 120 },
        { "cmd": "define", "name": "duck", "commands": [
            { "cmd": "layer", "name": "Ducks" },
            { "cmd": "ellipse", "center": [0, 0], "radius": [20, 14], "fill": "yellow", "outline": "ink" } ] },
        { "cmd": "repeat", "count": 3, "step": { "translate": [90, 0] }, "commands": [
            { "cmd": "use", "name": "duck", "translate": [50, 50] },
            { "cmd": "text", "text": "duck {i}", "at": [50, 80], "size": 14, "color": "ink", "align": "center" } ] },
        { "cmd": "group", "name": "big", "mirror": "x", "origin": [150, 0], "commands": [
            { "cmd": "use", "name": "duck", "translate": [40, 100], "colors": { "yellow": "#C04040" } } ] }
    ] }"##;

    #[test]
    fn played_scripts_with_symbols_export_and_replay_the_same() {
        // As the app does it: the first time onto a new picture, the
        // second onto the open picture, starting on its selected layer.
        let script = Script::parse(PLAYED).unwrap();
        let mut player = Player::new(script.clone(), PlayOptions::default()).unwrap();
        let mut document = player.create_document().unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        let mut recorded = without_palette(&script.commands[1..], &script.palette);
        recorded.push(select_layer(document.active_layer().name()));
        let mut player = Player::new(script.clone(), PlayOptions::default()).unwrap();
        player.start_on(&mut document).unwrap();
        player.run(&mut document, |_, _| {}).unwrap();
        recorded.extend(without_palette(&script.commands[1..], &script.palette));
        let exported = super::script(300, 120, document.paper(), recorded, None, None);
        // Read back from the file, it paints the same picture.
        let text = exported.to_json();
        assert_eq!(text.matches("\"define\"").count(), 1, "{text}");
        let again = Script::parse(&text).unwrap();
        let mut player = Player::new(again, PlayOptions::default()).unwrap();
        let mut replayed = player.create_document().unwrap();
        player.run(&mut replayed, |_, _| {}).unwrap();
        assert_eq!(png(&replayed), png(&document));
    }

    #[test]
    fn symbols_defined_again_differently_get_new_names() {
        let script = Script::parse(
            r##"{ "commands": [
                { "cmd": "canvas", "width": 10, "height": 10 },
                { "cmd": "define", "name": "dot", "commands": [ { "cmd": "rect", "x": 0, "y": 0, "width": 1, "height": 1, "fill": "#000" } ] },
                { "cmd": "use", "name": "dot" },
                { "cmd": "group", "commands": [ { "cmd": "use", "name": "dot" } ] }
            ] }"##,
        )
        .unwrap();
        let mut other = script.commands[1..].to_vec();
        let Command::Define(define) = &mut other[0] else {
            panic!()
        };
        define.commands.clear(); // a different "dot"
        let mut commands = script.commands[1..].to_vec();
        commands.extend(other);
        commands.extend(script.commands[1..].to_vec());
        let names: Vec<String> = unique_symbols(commands)
            .iter()
            .map(|command| match command {
                Command::Define(define) => format!("define {}", define.name),
                Command::Use(using) => format!("use {}", using.name),
                Command::Group(group) => match &group.commands[0] {
                    Command::Use(using) => format!("group > use {}", using.name),
                    _ => panic!(),
                },
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            names,
            [
                "define dot",
                "use dot",
                "group > use dot",
                "define dot 2",
                "use dot 2",
                "group > use dot 2",
                // The first "dot" again: a new name too, since "dot" now means "dot 2".
                "define dot 3",
                "use dot 3",
                "group > use dot 3",
            ]
        );
    }

    /// Plays an exported script, read back from its JSON text.
    fn replay(script: &Script) -> Document {
        let again = Script::parse(&script.to_json()).unwrap();
        let mut player = Player::new(again, PlayOptions::default()).unwrap();
        let mut replayed = player.create_document().unwrap();
        player.run(&mut replayed, |_, _| {}).unwrap();
        replayed
    }

    #[test]
    fn layer_transforms_export_and_replay_exactly() {
        let mut document = Document::new(400, 300);
        let mut history = History::default();
        let red = BrushSettings {
            color: [200, 30, 30],
            size: 18.0,
            ..BrushSettings::default()
        };
        paint(
            &mut document,
            &mut history,
            red,
            &[(40.0, 40.0, 1.0), (200.0, 140.0, 0.6), (300.0, 60.0, 0.9)],
        );
        // As the app does it: a flip, then a transform with rounded
        // numbers, each through the command's own `transform`.
        let flip = TransformLayerCmd {
            mirror: Some("x".into()),
            origin: Some([200.0, 150.0]),
            ..TransformLayerCmd::default()
        };
        let turn = TransformLayerCmd {
            translate: Some([12.35, -7.5]),
            scale: Some(serde_json::json!([1.2346, 0.8])),
            rotate: Some(17.25),
            origin: Some([171.5, 95.0]),
            ..TransformLayerCmd::default()
        };
        for command in [flip, turn] {
            let layer = document.active_id();
            let name = document.active_layer().name().to_owned();
            let before = crate::layer_transform::transform_layer(
                &mut document,
                layer,
                command.transform().unwrap(),
            );
            history.push_recorded(
                Box::new(TileCommand::new("Transform layer", layer, before)),
                transform_layer(&name, &command),
            );
        }
        let exported = super::script(
            400,
            300,
            document.paper(),
            history.recorded_script(),
            None,
            None,
        );
        assert!(exported.to_json().contains("\"transform_layer\""));
        assert_eq!(png(&replay(&exported)), png(&document));
    }
}
