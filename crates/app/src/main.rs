//! Art App: program entry point.
//!
//! Opens the window, sets up pen input, and shows the top bar (File menu,
//! screens, undo/redo) and the current screen: the canvas, or the pen test.

// In release builds on Windows, don't open a black console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod brush_panel;
mod canvas_view;
mod color_panel;
mod files;
mod layers_panel;
mod paint_tool;
mod pen_input;
mod pen_test;

use art_engine::document::Document;
use art_engine::history::History;
use eframe::egui::{self, Button, Key, KeyboardShortcut, Modifiers};

use canvas_view::CanvasView;
use files::{DEFAULT_SIZE, Files};
use pen_input::PenInput;
use pen_test::PenTestScreen;

fn main() -> eframe::Result {
    let pen_input = PenInput::default();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png should be a valid PNG file");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Art App")
            // Painting needs room: start maximized. This is the size
            // used when you un-maximize the window.
            .with_inner_size([1280.0, 800.0])
            .with_maximized(true)
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
    /// Undo and redo.
    history: History,
    /// New, open, save, export, and the window title.
    files: Files,
    canvas: CanvasView,
    pen_test: PenTestScreen,
    screen: Screen,
}

impl ArtApp {
    fn new(pen_input: PenInput) -> Self {
        // `ArtApp.exe picture.artapp` opens that picture. That's also how
        // Windows starts us for "Open with". Otherwise (or if it can't be
        // opened) start with a blank picture of the standard size.
        let mut files = Files::default();
        let document = std::env::args_os()
            .nth(1)
            .and_then(|path| files.open_path(path.into()))
            .unwrap_or_else(|| Document::new(DEFAULT_SIZE[0], DEFAULT_SIZE[1]));
        Self {
            pen_input,
            document,
            history: History::default(),
            files,
            canvas: CanvasView::default(),
            pen_test: PenTestScreen::default(),
            screen: Screen::Canvas,
        }
    }

    /// Checks the keyboard for Ctrl+Z (undo) and Ctrl+Y or Ctrl+Shift+Z
    /// (redo). Returns (undo, redo).
    fn undo_redo_shortcuts(&self, ctx: &egui::Context) -> (bool, bool) {
        if ctx.egui_wants_keyboard_input() {
            return (false, false); // typing in a text box: Ctrl+Z is for the text
        }
        ctx.input_mut(|i| {
            // egui ignores an extra Shift when matching, so check
            // Ctrl+Shift+Z first, or it would count as Ctrl+Z.
            let redo = i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::Z,
            )) | i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Y));
            let undo = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::Z));
            (undo, redo)
        })
    }
}

impl eframe::App for ArtApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Pen and touch events since the last frame, for whichever screen is showing.
        let ink_events = self.pen_input.take_events();

        // While a small window like "Save changes?" is open, shortcuts
        // would act on the picture behind it, so they're ignored.
        let (mut undo, mut redo, mut file_action) = if self.files.dialog_open() {
            (false, false, None)
        } else {
            let (undo, redo) = self.undo_redo_shortcuts(ui.ctx());
            (undo, redo, self.files.shortcuts(ui.ctx()))
        };

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                file_action = file_action.or(self.files.menu(ui));
                ui.separator();
                ui.selectable_value(&mut self.screen, Screen::Canvas, "Canvas");
                ui.selectable_value(&mut self.screen, Screen::PenTest, "Pen test");
                ui.separator();

                // Big enough to tap easily on a tablet.
                let size = egui::vec2(72.0, 26.0);
                let undo_tip = match self.history.undo_name() {
                    Some(name) => format!("Undo {name} (Ctrl+Z)"),
                    None => "Nothing to undo".to_owned(),
                };
                let redo_tip = match self.history.redo_name() {
                    Some(name) => format!("Redo {name} (Ctrl+Y)"),
                    None => "Nothing to redo".to_owned(),
                };
                let can_undo = self.history.undo_name().is_some();
                let can_redo = self.history.redo_name().is_some();
                undo |= ui
                    .add_enabled(can_undo, Button::new("Undo").min_size(size))
                    .on_hover_text(undo_tip)
                    .on_disabled_hover_text("Nothing to undo")
                    .clicked();
                redo |= ui
                    .add_enabled(can_redo, Button::new("Redo").min_size(size))
                    .on_hover_text(redo_tip)
                    .on_disabled_hover_text("Nothing to redo")
                    .clicked();
                ui.separator();
                self.files.notice_ui(ui);
            });
        });

        // Not in the middle of a stroke: wait until it's finished.
        if self.canvas.is_painting() {
            file_action = None;
        } else {
            if undo {
                self.history.undo(&mut self.document);
            }
            if redo {
                self.history.redo(&mut self.document);
            }
        }

        // New, open, save, export; asks "Save changes?" when needed.
        let new_document = self.files.update(
            ui.ctx(),
            frame,
            file_action,
            &self.document,
            &mut self.history,
        );
        if let Some(document) = new_document {
            self.document = document;
            self.history = History::default();
            self.canvas.reset();
        }

        match self.screen {
            Screen::Canvas => self
                .canvas
                .ui(ui, &mut self.document, &mut self.history, ink_events),
            Screen::PenTest => self.pen_test.ui(ui, ink_events),
        }
    }
}
