//! Art App: program entry point.
//!
//! Opens the window, sets up pen input, and shows the current screen:
//! the canvas, or the pen test.

// In release builds on Windows, don't open a black console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas_view;
mod pen_input;
mod pen_test;

use art_engine::document::Document;
use eframe::egui;

use canvas_view::CanvasView;
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

/// The screens you can switch between in the top bar.
#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Canvas,
    PenTest,
}

/// The whole app. eframe calls `ui` every time the window needs redrawing.
struct ArtApp {
    pen_input: PenInput,
    document: Document,
    canvas: CanvasView,
    pen_test: PenTestScreen,
    screen: Screen,
}

impl ArtApp {
    fn new(pen_input: PenInput) -> Self {
        // A 4000 x 3000 canvas with a test picture, until we have a brush.
        let mut document = Document::new(4000, 3000);
        document.draw_test_pattern();

        Self {
            pen_input,
            document,
            canvas: CanvasView::default(),
            pen_test: PenTestScreen::default(),
            screen: Screen::Canvas,
        }
    }
}

impl eframe::App for ArtApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Pen and touch events since the last frame. Only the pen test uses
        // them for now; the brush (Step 3) will too.
        let ink_events = self.pen_input.take_events();

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Art App");
                ui.separator();
                ui.selectable_value(&mut self.screen, Screen::Canvas, "Canvas");
                ui.selectable_value(&mut self.screen, Screen::PenTest, "Pen test");
            });
        });

        match self.screen {
            Screen::Canvas => self.canvas.ui(ui, &mut self.document),
            Screen::PenTest => self.pen_test.ui(ui, ink_events),
        }
    }
}
