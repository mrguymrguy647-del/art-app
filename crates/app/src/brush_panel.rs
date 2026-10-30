//! The tool panel: pick brush, eraser or eyedropper, choose a colour, and
//! adjust the brush settings.

use art_engine::brush::{MAX_SIZE, MIN_SIZE};
use eframe::egui::{self, Slider};

use crate::color_panel::ColorPicker;
use crate::paint_tool::{PaintTool, Tool};

/// What the user asked for in the panel this frame.
#[derive(Default)]
pub struct PanelActions {
    pub clear_layer: bool,
}

/// Shows the panel for the painting tools.
pub fn brush_panel(
    ui: &mut egui::Ui,
    tools: &mut PaintTool,
    colors: &mut ColorPicker,
) -> PanelActions {
    let mut actions = PanelActions::default();

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut tools.tool, Tool::Brush, "Brush (B)");
        ui.selectable_value(&mut tools.tool, Tool::Eraser, "Eraser (E)");
        ui.selectable_value(&mut tools.tool, Tool::Eyedropper, "Pick (I)")
            .on_hover_text(
                "Pick a colour from the canvas. You can also hold Alt, or the pen's side button.",
            );
    });
    ui.add_space(4.0);

    // Scrolls if the window is too short to show everything.
    egui::ScrollArea::vertical().show(ui, |ui| {
        let is_eraser = tools.tool == Tool::Eraser;
        if !is_eraser {
            let recent = tools.recent_colors.clone();
            colors.ui(ui, &mut tools.brush.color, &recent);
            ui.separator();
        }

        let settings = tools.settings_mut();
        ui.add(
            Slider::new(&mut settings.size, MIN_SIZE..=MAX_SIZE)
                .logarithmic(true)
                .text("Size")
                .suffix(" px"),
        );
        // For the eraser, "opacity" is how strongly it rubs out.
        let strength_name = if is_eraser { "Strength" } else { "Opacity" };
        ui.add(percent_slider(&mut settings.opacity, 0.01..=1.0, strength_name));
        ui.add(percent_slider(&mut settings.flow, 0.01..=1.0, "Flow"));
        ui.add(percent_slider(&mut settings.hardness, 0.0..=1.0, "Hardness"));
        ui.add(percent_slider(&mut settings.spacing, 0.02..=1.0, "Spacing"));

        ui.separator();
        ui.label("Pen pressure changes:");
        ui.checkbox(&mut settings.pressure_size, "Size");
        ui.checkbox(&mut settings.pressure_opacity, strength_name);
        ui.checkbox(&mut settings.pressure_flow, "Flow");

        ui.separator();
        // Smoothing is shared by the brush and the eraser.
        ui.add(percent_slider(&mut tools.smoothing, 0.0..=1.0, "Smoothing"));

        ui.separator();
        // Clearing can be undone like any other change.
        if ui.button("Clear layer").clicked() {
            actions.clear_layer = true;
        }

        ui.add_space(8.0);
        ui.weak("Tips: [ and ] change the size. Turn the pen round to erase. Hold Alt to pick a colour.");
    });

    actions
}

/// A slider for a 0..1 value, shown as a percentage.
pub fn percent_slider<'a>(
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
