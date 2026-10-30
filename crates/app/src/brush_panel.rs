//! The brush panel: sliders and switches for the brush.

use art_engine::brush::{BrushSettings, MAX_SIZE, MIN_SIZE};
use eframe::egui::{self, Slider};

/// What the user asked for in the panel this frame.
#[derive(Default)]
pub struct PanelActions {
    pub clear_canvas: bool,
}

/// Shows the panel. `smoothing` is 0..1 (see `smoothing.rs` in the engine).
pub fn brush_panel(
    ui: &mut egui::Ui,
    brush: &mut BrushSettings,
    smoothing: &mut f32,
) -> PanelActions {
    let mut actions = PanelActions::default();

    ui.add_space(8.0);
    ui.heading("Brush");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Colour");
        // A simple colour button for now; Step 6 adds a colour wheel and eyedropper.
        ui.color_edit_button_srgb(&mut brush.color);
    });
    ui.add(
        Slider::new(&mut brush.size, MIN_SIZE..=MAX_SIZE)
            .logarithmic(true)
            .text("Size")
            .suffix(" px"),
    );
    ui.add(percent_slider(&mut brush.opacity, 0.01..=1.0, "Opacity"));
    ui.add(percent_slider(&mut brush.flow, 0.01..=1.0, "Flow"));
    ui.add(percent_slider(&mut brush.hardness, 0.0..=1.0, "Hardness"));
    ui.add(percent_slider(&mut brush.spacing, 0.02..=1.0, "Spacing"));
    ui.add(percent_slider(smoothing, 0.0..=1.0, "Smoothing"));

    ui.separator();
    ui.label("Pen pressure changes:");
    ui.checkbox(&mut brush.pressure_size, "Size");
    ui.checkbox(&mut brush.pressure_opacity, "Opacity");
    ui.checkbox(&mut brush.pressure_flow, "Flow");

    ui.separator();
    // Until undo arrives (Step 5), this is the way to start over.
    if ui.button("Clear canvas").clicked() {
        actions.clear_canvas = true;
    }

    ui.add_space(8.0);
    ui.weak("Tip: [ and ] change the brush size.");

    actions
}

/// A slider for a 0..1 value, shown as a percentage.
fn percent_slider<'a>(
    value: &'a mut f32,
    range: std::ops::RangeInclusive<f32>,
    name: &str,
) -> Slider<'a> {
    Slider::new(value, range)
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
