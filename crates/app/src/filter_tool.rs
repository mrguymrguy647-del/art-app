//! The Filters menu: blur, glow, drop shadow, adjust and gradient map
//! for the selected layer, each with a small window for its numbers.
//!
//! A filter is first written as a `filter` script command, and that
//! command is what's applied, so "Export strokes" replays exactly what
//! you saw. Applying is one undo step.

use eframe::egui::{self, Button, Id, Modal, Slider, vec2};
use quill_engine::color::{from_hex, to_hex};
use quill_engine::document::Document;
use quill_engine::filters;
use quill_engine::history::{History, TileCommand};
use quill_engine::script::{FilterCmd, record};
use serde_json::{Value, json};

/// Which filter the window is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Blur,
    Glow,
    Shadow,
    Adjust,
    GradientMap,
}

impl FilterKind {
    const ALL: [FilterKind; 5] = [
        FilterKind::Blur,
        FilterKind::Glow,
        FilterKind::Shadow,
        FilterKind::Adjust,
        FilterKind::GradientMap,
    ];

    fn label(self) -> &'static str {
        match self {
            FilterKind::Blur => "Blur…",
            FilterKind::Glow => "Glow…",
            FilterKind::Shadow => "Drop shadow…",
            FilterKind::Adjust => "Adjust colours…",
            FilterKind::GradientMap => "Gradient map…",
        }
    }

    fn tip(self) -> &'static str {
        match self {
            FilterKind::Blur => "Soften everything on the selected layer",
            FilterKind::Glow => "Spread the light parts of the selected layer into a glow",
            FilterKind::Shadow => "A soft shadow behind everything on the selected layer",
            FilterKind::Adjust => "Hue, saturation, brightness and contrast of the selected layer",
            FilterKind::GradientMap => "Recolour the selected layer by lightness, dark to light",
        }
    }
}

/// The numbers in the window, kept between uses.
pub struct FilterTool {
    open: Option<FilterKind>,
    blur: f32,
    glow_radius: f32,
    glow_threshold: f32,
    glow_strength: f32,
    shadow_x: f32,
    shadow_y: f32,
    shadow_blur: f32,
    shadow_color: [u8; 3],
    shadow_opacity: f32,
    hue: f32,
    saturation: f32,
    brightness: f32,
    contrast: f32,
    map_dark: [u8; 3],
    map_light: [u8; 3],
    /// A short message under the canvas, like "nothing on this layer".
    notice: Option<&'static str>,
}

impl Default for FilterTool {
    fn default() -> Self {
        Self {
            open: None,
            blur: 8.0,
            glow_radius: 24.0,
            glow_threshold: 0.7,
            glow_strength: 1.0,
            shadow_x: 6.0,
            shadow_y: 8.0,
            shadow_blur: 6.0,
            shadow_color: [0, 0, 0],
            shadow_opacity: 0.5,
            hue: 0.0,
            saturation: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            map_dark: [0x1B, 0x24, 0x33],
            map_light: [0xFF, 0xF3, 0xDC],
            notice: None,
        }
    }
}

impl FilterTool {
    /// The Filters button in the top bar, with its menu. Greyed out
    /// while `busy`.
    pub fn menu(&mut self, ui: &mut egui::Ui, busy: bool) {
        let button = Button::new("Filters").min_size(vec2(64.0, 26.0));
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(200.0);
            for kind in FilterKind::ALL {
                let entry = Button::new(kind.label()).min_size(vec2(0.0, 28.0)); // easy to tap
                if ui
                    .add_enabled(!busy, entry)
                    .on_hover_text(kind.tip())
                    .clicked()
                {
                    self.open = Some(kind);
                }
            }
        });
    }

    /// True while the filter window is open (shortcuts should wait).
    pub fn dialog_open(&self) -> bool {
        self.open.is_some()
    }

    /// A message to show once, if a filter couldn't be applied.
    pub fn take_notice(&mut self) -> Option<&'static str> {
        self.notice.take()
    }

    /// Shows the window for the chosen filter, and applies it on Apply.
    pub fn update(&mut self, ctx: &egui::Context, document: &mut Document, history: &mut History) {
        let Some(kind) = self.open else {
            return;
        };
        let modal = Modal::new(Id::unique("filter")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(kind.label().trim_end_matches('…'));
            ui.add_space(6.0);
            ui.label(format!(
                "On the layer \"{}\".",
                document.active_layer().name()
            ));
            ui.add_space(8.0);
            self.fields(ui, kind);
            ui.add_space(12.0);
            let size = vec2(110.0, 30.0);
            let mut apply = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                apply = ui.add(Button::new("Apply").min_size(size)).clicked();
                cancel = ui.add(Button::new("Cancel").min_size(size)).clicked();
            });
            (apply, cancel)
        });
        let (apply, cancel) = modal.inner;
        if apply {
            let command = self.command(kind);
            if !apply_filter(document, history, &command) {
                self.notice = Some("There's nothing on this layer to filter.");
            }
            self.open = None;
        } else if cancel || modal.should_close() {
            self.open = None;
        }
    }

    fn fields(&mut self, ui: &mut egui::Ui, kind: FilterKind) {
        match kind {
            FilterKind::Blur => {
                ui.add(Slider::new(&mut self.blur, 0.5..=200.0).text("pixels"));
            }
            FilterKind::Glow => {
                ui.add(Slider::new(&mut self.glow_radius, 1.0..=200.0).text("reach (pixels)"));
                ui.add(percent(&mut self.glow_threshold, "lighter than"));
                ui.add(Slider::new(&mut self.glow_strength, 0.1..=3.0).text("strength"));
            }
            FilterKind::Shadow => {
                ui.add(Slider::new(&mut self.shadow_x, -100.0..=100.0).text("across"));
                ui.add(Slider::new(&mut self.shadow_y, -100.0..=100.0).text("down"));
                ui.add(Slider::new(&mut self.shadow_blur, 0.0..=100.0).text("softness"));
                ui.add(percent(&mut self.shadow_opacity, "darkness"));
                ui.horizontal(|ui| {
                    ui.label("colour");
                    ui.color_edit_button_srgb(&mut self.shadow_color);
                });
            }
            FilterKind::Adjust => {
                ui.add(Slider::new(&mut self.hue, -180.0..=180.0).text("hue (degrees)"));
                ui.add(Slider::new(&mut self.saturation, -1.0..=1.0).text("saturation"));
                ui.add(Slider::new(&mut self.brightness, -1.0..=1.0).text("brightness"));
                ui.add(Slider::new(&mut self.contrast, -1.0..=1.0).text("contrast"));
            }
            FilterKind::GradientMap => {
                ui.horizontal(|ui| {
                    ui.label("dark parts become");
                    ui.color_edit_button_srgb(&mut self.map_dark);
                });
                ui.horizontal(|ui| {
                    ui.label("light parts become");
                    ui.color_edit_button_srgb(&mut self.map_light);
                });
            }
        }
    }

    /// The window's numbers as a `filter` command.
    fn command(&self, kind: FilterKind) -> FilterCmd {
        let round = |v: f32| (v * 100.0).round() / 100.0;
        let mut command = FilterCmd::default();
        match kind {
            FilterKind::Blur => command.blur = Some(json!(round(self.blur))),
            FilterKind::Glow => {
                command.glow = Some(json!({
                    "radius": round(self.glow_radius),
                    "threshold": round(self.glow_threshold),
                    "strength": round(self.glow_strength),
                }))
            }
            FilterKind::Shadow => {
                let [r, g, b] = self.shadow_color;
                let a = (self.shadow_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                command.shadow = Some(json!({
                    "offset": [round(self.shadow_x), round(self.shadow_y)],
                    "blur": round(self.shadow_blur),
                    "color": record::hex([r, g, b, a]),
                }))
            }
            FilterKind::Adjust => {
                command.adjust = Some(json!({
                    "hue": round(self.hue),
                    "saturation": round(self.saturation),
                    "brightness": round(self.brightness),
                    "contrast": round(self.contrast),
                }))
            }
            FilterKind::GradientMap => {
                command.gradient_map = Some(json!({
                    "colors": [to_hex(self.map_dark), to_hex(self.map_light)],
                }))
            }
        }
        command
    }
}

/// A slider for a 0..1 value, shown as a percentage.
fn percent<'a>(value: &'a mut f32, name: &str) -> Slider<'a> {
    Slider::new(value, 0.0..=1.0)
        .text(name)
        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
        .custom_parser(|text| {
            let number = text
                .trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f64>()
                .ok()?;
            Some(number / 100.0)
        })
}

/// Applies `command` to the selected layer as one undo step (with the
/// command, for "Export strokes"). False if nothing changed.
pub fn apply_filter(document: &mut Document, history: &mut History, command: &FilterCmd) -> bool {
    let Ok(filters) = read(command) else {
        return false;
    };
    let layer = document.active_id();
    let name = document.active_layer().name().to_owned();
    let limits = document.bounds();
    // Inside the selection, if there is one (the script command does the same).
    let clip = document.clip();
    let before = filters::apply_within(
        &mut document.active_layer_mut().surface,
        &filters,
        limits,
        clip.as_deref(),
    );
    if before.is_empty() {
        return false;
    }
    history.push_recorded(
        Box::new(TileCommand::new("Filter", layer, before)),
        record::filter(&name, command),
    );
    true
}

/// The window's command as engine filters (the same numbers the script
/// player would read, at full size).
fn read(command: &FilterCmd) -> Result<Vec<filters::Filter>, String> {
    let number = |value: &Value, what: &str| -> Result<f32, String> {
        value
            .as_f64()
            .map(|v| v as f32)
            .ok_or_else(|| format!("{what} must be a number"))
    };
    let field = |object: &Value, name: &str, default: f32| -> Result<f32, String> {
        match object.get(name) {
            Some(value) => number(value, name),
            None => Ok(default),
        }
    };
    let mut out = Vec::new();
    if let Some(adjust) = &command.adjust {
        out.push(filters::Filter::Adjust {
            hue: field(adjust, "hue", 0.0)?,
            saturation: field(adjust, "saturation", 0.0)?,
            brightness: field(adjust, "brightness", 0.0)?,
            contrast: field(adjust, "contrast", 0.0)?,
        });
    }
    if let Some(map) = &command.gradient_map {
        let colors = map
            .get("colors")
            .and_then(Value::as_array)
            .ok_or("colours")?;
        let stops = colors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let [r, g, b] = c.as_str().and_then(from_hex).ok_or("a colour")?;
                Ok(quill_engine::fill::GradientStop {
                    position: i as f32 / (colors.len() - 1).max(1) as f32,
                    color: [r, g, b, 255],
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        out.push(filters::Filter::GradientMap(stops));
    }
    if let Some(blur) = &command.blur {
        out.push(filters::Filter::Blur(number(blur, "blur")?));
    }
    if let Some(glow) = &command.glow {
        out.push(filters::Filter::Glow {
            radius: field(glow, "radius", 16.0)?,
            threshold: field(glow, "threshold", 0.7)?,
            strength: field(glow, "strength", 1.0)?,
        });
    }
    if let Some(shadow) = &command.shadow {
        let offset = shadow
            .get("offset")
            .and_then(Value::as_array)
            .filter(|o| o.len() == 2)
            .ok_or("offset")?;
        let color = shadow
            .get("color")
            .and_then(Value::as_str)
            .and_then(|hex| quill_engine::script::parse_hex(hex).ok())
            .unwrap_or([0, 0, 0, 128]);
        out.push(filters::Filter::Shadow {
            offset: (number(&offset[0], "offset")?, number(&offset[1], "offset")?),
            blur: field(shadow, "blur", 4.0)?,
            color,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_engine::tile::Rgba8;

    #[test]
    fn the_window_writes_a_filter_command_and_applying_it_can_be_undone() {
        let mut tool = FilterTool {
            blur: 6.0,
            ..FilterTool::default()
        };
        let command = tool.command(FilterKind::Blur);
        assert_eq!(serde_json::to_string(&command).unwrap(), r#"{"blur":6.0}"#);
        let shadow = tool.command(FilterKind::Shadow);
        assert_eq!(
            shadow.shadow.unwrap()["color"],
            Value::String("#00000080".into())
        );
        let mut document = Document::new(100, 100);
        let mut history = History::default();
        assert!(
            !apply_filter(&mut document, &mut history, &command),
            "nothing to blur"
        );
        for y in 40..60 {
            for x in 40..60 {
                document
                    .active_layer_mut()
                    .surface
                    .set_pixel(x, y, Rgba8::new(255, 0, 0, 255));
            }
        }
        assert!(apply_filter(&mut document, &mut history, &command));
        assert!(
            document.active_layer().surface.pixel(62, 50).a > 0,
            "blurred"
        );
        assert_eq!(history.undo_name(), Some("Filter"));
        let recorded = history.recorded_script();
        assert!(
            recorded.iter().any(
                |c| matches!(c, quill_engine::script::Command::Filter(f) if f.layer.is_some())
            ),
            "{recorded:?}"
        );
        history.undo(&mut document);
        assert_eq!(document.active_layer().surface.pixel(62, 50).a, 0);
        tool.open = Some(FilterKind::Adjust);
        assert!(tool.dialog_open());
    }
}
