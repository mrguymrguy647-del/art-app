//! Writing down what was painted, as painting-script commands.
//!
//! The app records each change (a stroke, a fill, a new layer, ...) as
//! script commands together with its undo step (see
//! `History::push_recorded`). "Export strokes" collects the steps that
//! are still in the picture and tidies them up with [`tidy`]: a stroke is
//! recorded with its layer and brush, but the exported script only says
//! "change layer" or "change brush" when they actually change.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    BrushCmd, CanvasCmd, ClearLayerCmd, Command, DeleteLayerCmd, FillCmd, GradientCmd, LayerCmd,
    MoveLayerCmd, Point, RenameLayerCmd, Script, StrokeCmd,
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
        Command::Layer(_)
        | Command::DeleteLayer(_)
        | Command::MoveLayer(_)
        | Command::RenameLayer(_)
        | Command::ClearLayer(_)
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
    })];
    commands.extend(tidy(recorded));
    Script {
        title,
        notes: notes.map(Value::String),
        seed: 1,
        palette: BTreeMap::new(),
        commands,
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
}
