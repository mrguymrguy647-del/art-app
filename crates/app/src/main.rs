//! Art App: program entry point.
//!
//! Opens the window, sets up pen input, and shows the current screen
//! (for now: the pen test).

// In release builds on Windows, don't open a black console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pen_input;
mod pen_test;

use eframe::egui;

use pen_input::PenInput;
use pen_test::PenTestScreen;

fn main() -> eframe::Result {
    let pen_input = PenInput::default();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png should be a valid PNG file");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Art App {}", art_engine::VERSION))
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0])
            .with_icon(icon),
        // Lets pen_input see Windows' pen messages (does nothing elsewhere).
        event_loop_builder: pen_input.event_loop_hook(),
        ..Default::default()
    };

    eframe::run_native(
        "Art App",
        options,
        Box::new(|creation_context| {
            let ctx = &creation_context.egui_ctx;
            // Dark grey, like most painting apps, so the colours you paint
            // with stand out.
            ctx.set_theme(egui::Theme::Dark);
            pen_input.set_egui_context(ctx);
            Ok(Box::new(ArtApp::new(pen_input)))
        }),
    )
}

/// The whole app. eframe calls `ui` every time the window needs redrawing.
struct ArtApp {
    pen_test: PenTestScreen,
}

impl ArtApp {
    fn new(pen_input: PenInput) -> Self {
        Self {
            pen_test: PenTestScreen::new(pen_input),
        }
    }
}

impl eframe::App for ArtApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Art App");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak("Step 1: pen test");
                });
            });
        });

        self.pen_test.ui(ui);
    }
}
