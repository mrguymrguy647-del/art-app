//! Brush presets: a named set of brush settings, kept as a small JSON
//! file. The built-in ones ship inside the program, so a script that asks
//! for `{"cmd": "brush", "preset": "Pencil (graphite)"}` paints the same
//! on every machine; the app keeps people's own presets in a folder.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::brush::{BrushSettings, Tip};
use crate::script::BrushCmd;
use crate::script::player::apply_brush_fields;
use crate::script::record;

/// What a preset file says it is in its `format` field.
pub const FORMAT_NAME: &str = "duckquill-preset";

/// The file ending for a preset.
pub const FILE_EXTENSION: &str = "json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Always `duckquill-preset` (it may be left out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    pub name: String,
    /// A sentence on what it's for, shown in the app and by `quill presets`.
    #[serde(default)]
    pub description: String,
    /// `true`: it's an eraser (it removes paint).
    #[serde(default, skip_serializing_if = "is_false")]
    pub eraser: bool,
    /// How much the app smooths the line, 0..1 (scripts ignore it: their
    /// points are already where they should be).
    #[serde(default)]
    pub smoothing: f32,
    /// The settings, as in a script's `brush` command (no colour: a
    /// preset keeps the colour you had).
    pub brush: BrushCmd,
}

fn is_false(value: &bool) -> bool {
    !*value
}

static BUILT_IN: LazyLock<Vec<Preset>> = LazyLock::new(|| {
    [
        include_str!("../presets/pencil-graphite.json"),
        include_str!("../presets/ink-pen.json"),
        include_str!("../presets/marker.json"),
        include_str!("../presets/soft-airbrush.json"),
        include_str!("../presets/chalk.json"),
        include_str!("../presets/soft-eraser.json"),
    ]
    .into_iter()
    .map(|text| Preset::read(text).expect("the built-in presets are checked by a test"))
    .collect()
});

/// The presets that come with Duck Quill, in the order they're shown.
pub fn built_in() -> &'static [Preset] {
    &BUILT_IN
}

/// A built-in preset by name (capital letters don't matter).
pub fn find(name: &str) -> Option<&'static Preset> {
    let name = name.trim();
    built_in()
        .iter()
        .find(|preset| preset.name.eq_ignore_ascii_case(name))
}

/// Whether `name` is taken by a built-in preset.
pub fn is_built_in(name: &str) -> bool {
    find(name).is_some()
}

impl Preset {
    /// A preset from a file's text, checked: every setting must make sense.
    pub fn read(text: &str) -> Result<Preset, String> {
        let preset: Preset =
            serde_json::from_str(text).map_err(|e| format!("not a brush preset: {e}"))?;
        if let Some(format) = &preset.format
            && format != FORMAT_NAME
        {
            return Err(format!(
                "not a brush preset: \"format\" is \"{format}\", not \"{FORMAT_NAME}\""
            ));
        }
        if preset.name.trim().is_empty() {
            return Err("a preset needs a name".into());
        }
        if preset.brush.color.is_some() {
            return Err("a preset has no colour (it keeps the colour you have)".into());
        }
        if preset.brush.preset.is_some() || preset.brush.reset.is_some() {
            return Err("a preset's brush can't name another preset or reset".into());
        }
        if !(0.0..=1.0).contains(&preset.smoothing) {
            return Err(format!(
                "`smoothing` must be from 0 to 1, not {}",
                preset.smoothing
            ));
        }
        preset.settings()?;
        Ok(preset)
    }

    /// The file's text, as written by `read`'s opposite.
    pub fn to_json(&self) -> String {
        let mut preset = self.clone();
        preset.format = Some(FORMAT_NAME.into());
        serde_json::to_string_pretty(&preset).expect("a preset can always be written") + "\n"
    }

    /// The brush this preset gives (black, until a colour is chosen).
    pub fn settings(&self) -> Result<BrushSettings, String> {
        let mut brush = if self.eraser {
            BrushSettings::eraser()
        } else {
            BrushSettings::default()
        };
        apply_brush_fields(&mut brush, &self.brush)
            .map_err(|p| format!("\"{}\": {p}", self.name))?;
        brush.tip = match &self.brush.tip {
            None => Tip::Round,
            Some(Value::String(name)) => Tip::from_name(name)
                .ok_or_else(|| format!("\"{}\": unknown tip \"{name}\"", self.name))?,
            // A tip from a picture would make the preset depend on a file.
            Some(_) => {
                return Err(format!(
                    "\"{}\": a preset's tip is a built-in name",
                    self.name
                ));
            }
        };
        Ok(brush)
    }

    /// A preset holding these settings (the colour isn't kept). A tip
    /// from a file can't go in a preset, so it becomes round.
    pub fn from_settings(name: &str, settings: &BrushSettings, smoothing: f32) -> Preset {
        let mut settings = *settings;
        if settings.tip.name().is_none() {
            settings.tip = Tip::Round;
        }
        let crate::script::Command::Brush(mut brush) = record::brush(&settings, false) else {
            unreachable!("record::brush makes a brush command")
        };
        // Only built-in tips, so the source and colour never matter.
        brush.color = None;
        Preset {
            format: Some(FORMAT_NAME.into()),
            name: name.trim().to_owned(),
            description: String::new(),
            eraser: settings.mode == crate::brush::BrushMode::Erase,
            smoothing: smoothing.clamp(0.0, 1.0),
            brush,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::BrushMode;

    #[test]
    fn the_built_in_presets_are_all_there_and_make_sense() {
        let names: Vec<&str> = built_in().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Pencil (graphite)",
                "Ink pen",
                "Marker",
                "Soft airbrush",
                "Chalk",
                "Soft eraser"
            ]
        );
        for preset in built_in() {
            assert!(
                !preset.description.is_empty(),
                "{} needs a description",
                preset.name
            );
            preset.settings().unwrap();
        }
        assert!(find("pencil (GRAPHITE)").is_some(), "any capitals");
        assert!(find("Biro").is_none());
        assert_eq!(
            find("Soft eraser").unwrap().settings().unwrap().mode,
            BrushMode::Erase
        );
        assert_eq!(find("Chalk").unwrap().settings().unwrap().tip, Tip::Chalk);
    }

    #[test]
    fn the_pencil_is_a_pencil() {
        let pencil = find("Pencil (graphite)").unwrap().settings().unwrap();
        let light = pencil.dab(0.2);
        let hard = pencil.dab(1.0);
        // Pressure changes the darkness far more than the width.
        let width_change = hard.radius / light.radius;
        let darkness_change = hard.opacity / light.opacity;
        assert!(width_change < 1.2, "{width_change}");
        assert!(darkness_change > 3.0, "{darkness_change}");
        assert!(pencil.grain > 0.0 && pencil.build_up > 0.0 && pencil.build_up < 1.0);
    }

    #[test]
    fn a_preset_goes_to_a_file_and_back_unchanged() {
        let mut settings = find("Chalk").unwrap().settings().unwrap();
        settings.size = 42.0;
        settings.color = [200, 10, 10];
        let preset = Preset::from_settings("  My chalk ", &settings, 0.3);
        assert_eq!(preset.name, "My chalk");
        let back = Preset::read(&preset.to_json()).unwrap();
        assert_eq!(back, preset);
        let mut expected = settings;
        expected.color = BrushSettings::default().color; // not kept
        assert_eq!(back.settings().unwrap(), expected);
        // An eraser stays an eraser.
        let eraser = Preset::from_settings("Rubber", &BrushSettings::eraser(), 0.0);
        assert!(Preset::read(&eraser.to_json()).unwrap().eraser);
        // Every built-in one too.
        for preset in built_in() {
            assert_eq!(&Preset::read(&preset.to_json()).unwrap(), preset);
        }
    }

    #[test]
    fn a_bad_preset_file_says_what_is_wrong() {
        let problem = |text: &str| Preset::read(text).unwrap_err();
        assert!(problem("{").starts_with("not a brush preset"));
        assert!(
            problem(r#"{"format": "duckquill-script", "name": "x", "brush": {}}"#)
                .contains("format")
        );
        assert!(problem(r#"{"name": " ", "brush": {}}"#).contains("needs a name"));
        assert!(
            problem(r#"{"name": "x", "brush": {"size": 10, "opacity": 3}}"#).contains("opacity")
        );
        assert!(problem(r##"{"name": "x", "brush": {"color": "#ff0000"}}"##).contains("colour"));
        assert!(problem(r#"{"name": "x", "brush": {"tip": "blob"}}"#).contains("unknown tip"));
        assert!(problem(r#"{"name": "x", "smoothing": 2, "brush": {}}"#).contains("smoothing"));
        // The least a preset needs: a name and a brush.
        assert!(Preset::read(r#"{"name": "x", "brush": {}}"#).is_ok());
    }
}
