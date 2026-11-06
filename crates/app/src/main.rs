//! Duck Quill: program entry point.
//!
//! Opens the window, sets up pen input, and shows the top bar (File menu,
//! screens, undo/redo) and the current screen: the canvas, or the pen test.

// In release builds on Windows, don't open a black console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about;
mod animation_view;
mod autosave;
mod brush_panel;
mod canvas_view;
mod color_panel;
mod edit;
mod files;
mod filter_tool;
mod layers_panel;
mod paint_tool;
mod pen_input;
mod pen_test;
mod playback;
mod presets_panel;
mod references_view;
mod select_tool;
mod settings;
mod settings_folder;
mod shortcut_sheet;
mod symmetry_view;
mod text_tool;
mod theme;
mod thumbnails;
mod transform_tool;

use eframe::egui::{self, Button, Key, KeyboardShortcut, Modifiers};
use quill_engine::document::Document;
use quill_engine::history::History;

use animation_view::{AnimationView, PanelAction};
use canvas_view::CanvasView;
use files::{AnimationExport, DEFAULT_SIZE, Files};
use filter_tool::FilterTool;
use pen_input::PenInput;
use pen_test::PenTestScreen;
use playback::{BarAction, Playback};
use transform_tool::layer_menu;

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use autosave::{Autosave, LeftBehind};
use settings::{Settings, WindowPlace};
use shortcut_sheet::ShortcutSheet;

/// The settings are saved at most this often while Duck Quill runs (and
/// when it closes).
const SETTINGS_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

fn main() -> eframe::Result {
    let pen_input = PenInput::default();

    // The window and taskbar icon. (DuckQuill.exe's own icon, the one File
    // Explorer shows, is put in by build.rs.)
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../brand/duck-quill-icon-256.png"))
            .expect("brand/duck-quill-icon-256.png should be a valid PNG file");

    // What was remembered from last time (or the defaults).
    let folder = settings_folder::settings_folder();
    let settings = Settings::load(folder.as_deref());
    // Painting needs room: start maximized, unless it was left smaller.
    let place = settings.window.unwrap_or(WindowPlace {
        size: [1280.0, 800.0],
        maximized: true,
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Duck Quill")
            // The size used when the window isn't maximized.
            .with_inner_size(place.size)
            .with_maximized(place.maximized)
            .with_min_inner_size([640.0, 400.0])
            .with_icon(icon),
        // Lets pen_input see Windows' pen messages (does nothing elsewhere).
        event_loop_builder: pen_input.event_loop_hook(),
        ..Default::default()
    };

    eframe::run_native(
        "Duck Quill",
        options,
        Box::new(|creation_context| {
            let ctx = &creation_context.egui_ctx;
            // Dark grey, like most painting apps, so the colours you paint
            // with stand out. View → Theme can change it (and it's remembered).
            ctx.set_theme(theme::from_name(settings.theme.as_deref()));
            theme::apply_brand(ctx);
            pen_input.set_egui_context(ctx);
            Ok(Box::new(DuckQuill::new(pen_input, settings, folder)))
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
struct DuckQuill {
    pen_input: PenInput,
    document: Document,
    /// Undo and redo.
    history: History,
    /// New, open, save, export, and the window title.
    files: Files,
    canvas: CanvasView,
    /// The Filters menu and its window.
    filters: FilterTool,
    pen_test: PenTestScreen,
    screen: Screen,
    /// A painting script being played on the canvas ("Play strokes").
    playback: Option<Playback>,
    /// A script's animations, shown frame by frame (the Animation panel).
    animation: Option<AnimationView>,
    /// Which animation and frame the canvas shows, so it's replaced only
    /// when that changes.
    frame_shown: Option<(String, usize)>,
    onion_shown: bool,
    /// Help → About Duck Quill.
    about: about::About,
    /// Help → Keyboard shortcuts.
    shortcut_sheet: ShortcutSheet,
    /// What's remembered between runs, and where it's kept.
    settings: Settings,
    settings_folder: Option<PathBuf>,
    /// The window's size when not maximized, and whether it is.
    window: Option<WindowPlace>,
    autosave: Autosave,
    /// Copies of unsaved work left behind last time, offered back.
    restore_offer: Vec<LeftBehind>,
    /// When the last key, click or pen movement came (autosave waits
    /// for a quiet moment).
    last_input: Instant,
    /// For saving the theme when the window closes.
    ctx: Option<egui::Context>,
    /// When the settings were last saved (they're saved now and then, so
    /// a crash doesn't forget the brush).
    settings_saved: Instant,
}

impl DuckQuill {
    fn new(pen_input: PenInput, settings: Settings, settings_folder: Option<PathBuf>) -> Self {
        // `DuckQuill.exe picture.quill` opens that picture, and
        // `DuckQuill.exe script.json` plays that painting script on a new
        // picture. That's also how Windows starts us for "Open with".
        // Otherwise (or if it can't be opened) start with a blank picture
        // of the standard size.
        let mut files = Files::default();
        files.set_recent(settings.recent_files.clone());
        let mut playback = None;
        let mut animation = None;
        let argument = std::env::args_os().nth(1).map(std::path::PathBuf::from);
        let document = match argument {
            Some(path)
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json")) =>
            {
                match files.script_from_file(&path) {
                    Some((mut player, script)) => {
                        let name = path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                        let folder = path.parent().map(std::path::Path::to_path_buf);
                        if !script.animations.is_empty() {
                            match AnimationView::new(&script, folder, name) {
                                Ok(view) => {
                                    let document = view.document().clone();
                                    animation = Some(view);
                                    Some(document)
                                }
                                Err(error) => {
                                    files.show_message("Couldn't play the script", error);
                                    None
                                }
                            }
                        } else {
                            match player.create_document() {
                                Ok(document) => {
                                    let active = document.active_layer().name().to_owned();
                                    playback =
                                        Some(Playback::new(player, &script, name, true, &active));
                                    Some(document)
                                }
                                Err(error) => {
                                    files.show_message(
                                        "Couldn't play the script",
                                        error.to_string(),
                                    );
                                    None
                                }
                            }
                        }
                    }
                    None => None,
                }
            }
            Some(path) => files.open_path(path),
            None => None,
        }
        .unwrap_or_else(|| Document::new(DEFAULT_SIZE[0], DEFAULT_SIZE[1]));
        let mut canvas = CanvasView::default();
        canvas.fit_text_size(document.height());
        canvas.apply_settings(&settings);
        let now = SystemTime::now();
        let autosave = Autosave::new(settings_folder.as_ref().map(|f| f.join("recovery")), now);
        let restore_offer = autosave.left_behind();
        Self {
            pen_input,
            document,
            history: History::default(),
            files,
            canvas,
            filters: FilterTool::default(),
            pen_test: PenTestScreen::default(),
            screen: Screen::Canvas,
            playback,
            animation,
            frame_shown: None,
            onion_shown: false,
            about: about::About::default(),
            shortcut_sheet: ShortcutSheet::default(),
            window: settings.window,
            settings,
            settings_folder,
            autosave,
            restore_offer,
            last_input: Instant::now(),
            ctx: None,
            settings_saved: Instant::now(),
        }
    }

    /// Saves what's remembered between runs (quietly: if it can't, the
    /// defaults come back next time, nothing worse).
    fn save_settings(&mut self, ctx: &egui::Context) {
        self.settings_saved = Instant::now();
        let Some(folder) = &self.settings_folder else {
            return;
        };
        let before = self.settings.clone();
        self.canvas.store_settings(&mut self.settings);
        self.settings.recent_files = self.files.recent().to_vec();
        self.settings.theme = Some(theme::name(ctx.options(|o| o.theme_preference)).to_owned());
        self.settings.window = self.window;
        if self.settings != before || !folder.join("settings.json").exists() {
            let _ = self.settings.save(folder);
        }
    }

    /// Notes the window's size (when it isn't maximized) for next time.
    fn watch_window(&mut self, ctx: &egui::Context) {
        let (rect, maximized) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().maximized));
        let maximized = maximized.unwrap_or(false);
        let size = match (maximized, rect, self.window) {
            (false, Some(rect), _) => [rect.width(), rect.height()],
            (_, _, Some(place)) => place.size,
            _ => [1280.0, 800.0],
        };
        self.window = Some(WindowPlace { size, maximized });
    }

    /// "Restore the picture that wasn't saved?" for each copy left behind.
    fn restore_window(&mut self, ctx: &egui::Context) {
        let Some(left) = self.restore_offer.first().cloned() else {
            return;
        };
        let now = SystemTime::now();
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("restore")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Restore unsaved work?");
            ui.label(format!(
                "Duck Quill closed before \"{}\" was saved. A copy was kept {}.",
                left.note.name,
                autosave::how_long_ago(left.note.copied, now)
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Restore")).clicked() {
                    choice = Some(true);
                }
                if ui
                    .button("Throw it away")
                    .on_hover_text("Delete the copy for good")
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => match quill_engine::file::open(&left.picture) {
                Ok(document) => {
                    self.document = document;
                    self.history = History::default();
                    // Never saved by you, so it counts as unsaved.
                    self.history.mark_unsaved();
                    self.files.restored(left.note.original.clone());
                    self.canvas.reset();
                    self.canvas.fit_text_size(self.document.height());
                    self.animation = None;
                    self.autosave.adopt(&left);
                    self.restore_offer.clear(); // one picture at a time
                }
                Err(error) => {
                    self.files
                        .show_message("Couldn't restore it", error.to_string());
                    self.restore_offer.remove(0);
                }
            },
            Some(false) => {
                Autosave::throw_away(&left);
                self.restore_offer.remove(0);
            }
            None => {}
        }
    }

    /// Shows the Animation panel, plays, and puts the frame shown on the
    /// canvas (with the onion skin over it).
    fn update_animation(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &mut self.animation else {
            self.canvas.set_overlay(None);
            return;
        };
        let seconds = ui.input(|i| i.stable_dt).min(0.25);
        let mut changed = view.tick(seconds);
        let action = egui::Panel::bottom("animation")
            .show(ui, |ui| view.panel(ui))
            .inner;
        if view.is_playing() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f32(
                    (1.0 / view.fps.max(1.0)).min(0.1),
                ));
        }
        // The frame shown is the picture: when it changes, so does the
        // canvas (and the onion skin with it).
        let now = (view.current().name.clone(), view.frame());
        changed |= self.frame_shown.as_ref() != Some(&now);
        if changed {
            self.document = view.document().clone();
            self.frame_shown = Some(now);
        }
        if changed || self.onion_shown != view.onion {
            self.canvas.set_overlay(view.onion_skin());
            self.onion_shown = view.onion;
        }
        if action == PanelAction::Close {
            self.animation = None;
            self.frame_shown = None;
            self.canvas.set_overlay(None);
        }
    }

    /// Carries out File → Export animation's choice.
    fn export_animation(&mut self, frame: &eframe::Frame, choice: AnimationExport) {
        let Some(view) = &self.animation else {
            return;
        };
        let result = match choice {
            AnimationExport::Sheet => self.files.ask_sheet_path(frame).map(|path| {
                view.export_sheet(&path)
                    .map(|json| format!("{} and {}", path.display(), json.display()))
            }),
            AnimationExport::Frames => self.files.ask_frames_folder(frame).map(|folder| {
                view.export_frames(&folder)
                    .map(|count| format!("{count} frames in {}", folder.display()))
            }),
            AnimationExport::Gif => self
                .files
                .ask_gif_path(frame, &view.current().name)
                .map(|path| view.export_gif(&path).map(|()| path.display().to_string())),
        };
        match result {
            Some(Ok(what)) => self.files.show_message("Exported", what),
            Some(Err(error)) => self.files.show_message("Couldn't export", error),
            None => {} // cancelled
        }
    }

    /// Stops playing a script: everything it painted becomes one undo
    /// step. `problem` is shown if it stopped because of a mistake.
    fn finish_playback(&mut self, problem: Option<String>) {
        let Some(playback) = self.playback.take() else {
            return;
        };
        let (group, script) = playback.finish(&mut self.document);
        if let Some(group) = group {
            self.history.push_recorded(Box::new(group), script);
        }
        if let Some(problem) = problem {
            self.files.show_message("The script stopped", problem);
        }
    }

    /// Plays the script a little further, and shows its bar.
    fn update_playback(&mut self, ui: &mut egui::Ui) {
        let Some(playback) = &mut self.playback else {
            return;
        };
        let action = egui::Panel::bottom("playback")
            .show(ui, |ui| playback.bar(ui))
            .inner;
        let ended = match action {
            BarAction::Stop => Some(Ok(())),
            BarAction::SkipToEnd => Some(playback.skip_to_end(&mut self.document)),
            BarAction::None => playback.update(ui.ctx(), &mut self.document),
        };
        if let Some(result) = ended {
            self.finish_playback(result.err());
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

impl eframe::App for DuckQuill {
    fn on_exit(&mut self) {
        // Closing normally: either nothing was unsaved, or you chose not
        // to save it. Either way the autosaved copy isn't needed.
        self.autosave.finish();
        if let Some(ctx) = self.ctx.clone() {
            self.save_settings(&ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Pen and touch events since the last frame, for whichever screen is showing.
        let ink_events = self.pen_input.take_events();
        let had_ink = !ink_events.is_empty();
        self.ctx = Some(ui.ctx().clone());

        // Closing the window while a script plays: keep what it painted
        // (as an undo step), so "Save changes?" knows about it.
        if ui.input(|i| i.viewport().close_requested()) {
            self.finish_playback(None);
            self.canvas
                .finish_editing(&mut self.document, &mut self.history);
        }
        let playing = self.playback.is_some();
        // A transform (or other edit on the canvas) must be finished first.
        let busy = playing || self.canvas.is_editing();
        let watching = self.animation.is_some();

        // While a small window like "Save changes?" is open, or a script
        // is playing, shortcuts would act on the picture, so they're ignored.
        let dialog =
            self.files.dialog_open() || self.filters.dialog_open() || self.canvas.dialog_open();
        let (mut undo, mut redo, mut file_action) = if dialog || busy {
            (false, false, None)
        } else {
            let (undo, redo) = self.undo_redo_shortcuts(ui.ctx());
            (undo, redo, self.files.shortcuts(ui.ctx()))
        };
        // Edit and Select shortcuts: on the canvas, not while typing text.
        let mut edit_action = if dialog
            || busy
            || watching
            || self.screen != Screen::Canvas
            || self.canvas.is_typing()
        {
            None
        } else {
            edit::shortcuts(ui.ctx())
        };

        let mut layer_action = None;
        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                file_action = file_action.or(self.files.menu(ui, busy));
                let not_here = busy || watching || self.screen != Screen::Canvas;
                let selected = self.document.selection().is_some();
                edit_action = edit_action
                    .or(edit::edit_menu(ui, not_here, selected))
                    .or(edit::select_menu(ui, not_here, selected));
                layer_action = layer_menu(ui, busy || self.screen != Screen::Canvas);
                self.filters.menu(ui, busy || self.screen != Screen::Canvas);
                let mut add_reference = false;
                ui.menu_button("View", |ui| {
                    theme::theme_menu(ui);
                    ui.separator();
                    let here = !busy && self.screen == Screen::Canvas;
                    if ui
                        .add_enabled(here, egui::Button::new("Add reference picture…"))
                        .on_hover_text("Pin a photo or drawing beside the painting, to look at and pick colours from")
                        .clicked()
                    {
                        add_reference = true;
                        ui.close();
                    }
                    ui.checkbox(self.canvas.references_hidden(), "Hide reference pictures");
                });
                if add_reference
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Pictures", &["png", "jpg", "jpeg"])
                        .pick_file()
                    && let Err(problem) =
                        self.canvas
                            .add_reference(&mut self.document, &mut self.history, &path)
                {
                    self.files.show_message(
                        "Can't pin that picture",
                        format!("{}\n\n{problem}", path.display()),
                    );
                }
                ui.menu_button("Help", |ui| {
                    if ui
                        .add(Button::new("Keyboard shortcuts").shortcut_text("?"))
                        .clicked()
                    {
                        self.shortcut_sheet.open = true;
                    }
                    if ui.button("About Duck Quill").clicked() {
                        self.about.open = true;
                    }
                });
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
                let can_undo = self.history.undo_name().is_some() && !busy;
                let can_redo = self.history.redo_name().is_some() && !busy;
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

        // Text being typed is finished first (it becomes an undo step, and
        // what's saved or played includes it).
        if undo || redo || file_action.is_some() || layer_action.is_some() || edit_action.is_some()
        {
            self.canvas
                .finish_typing(&mut self.document, &mut self.history);
        }

        // Not in the middle of a stroke: wait until it's finished.
        if self.canvas.is_painting() {
            file_action = None;
        } else {
            if let Some(action) = layer_action {
                self.canvas
                    .layer_action(action, &mut self.document, &mut self.history);
            }
            if let Some(action) = edit_action {
                self.canvas
                    .edit_action(action, &mut self.document, &mut self.history);
            }
            if undo {
                self.history.undo(&mut self.document);
            }
            if redo {
                self.history.redo(&mut self.document);
            }
        }

        // The Filters window, if one is open.
        if !self.canvas.is_painting() {
            self.filters
                .update(ui.ctx(), &mut self.document, &mut self.history);
            if let Some(text) = self.filters.take_notice() {
                self.canvas.show_notice(text);
            }
        }

        // New, open, save, export; asks "Save changes?" when needed.
        let animation_name = self.animation.as_ref().map(|v| v.current().name.clone());
        let new_document = self.files.update(
            ui.ctx(),
            frame,
            file_action,
            &self.document,
            &mut self.history,
            animation_name.as_deref(),
        );
        if let Some(document) = new_document {
            self.document = document;
            self.history = History::default();
            self.canvas.reset();
            self.canvas.fit_text_size(self.document.height());
            self.animation = None;
            self.frame_shown = None;
        }
        if let Some(choice) = self.files.take_animation_export() {
            self.export_animation(frame, choice);
        }

        // "Play strokes": start playing a script that's ready.
        if let Some(request) = self.files.take_play_request() {
            let mut player = request.player;
            let started = if request.fresh {
                Ok(())
            } else {
                player.start_on(&mut self.document)
            };
            if !request.script.animations.is_empty() {
                // Frames, not strokes: the Animation panel shows them.
                match AnimationView::new(&request.script, request.folder, request.name) {
                    Ok(view) => {
                        self.document = view.document().clone();
                        self.history = History::default();
                        self.canvas.reset();
                        self.animation = Some(view);
                        self.frame_shown = None;
                        self.screen = Screen::Canvas;
                    }
                    Err(error) => self.files.show_message("Couldn't play the script", error),
                }
            } else {
                match started {
                    Ok(()) => {
                        let active = self.document.active_layer().name().to_owned();
                        self.playback = Some(Playback::new(
                            player,
                            &request.script,
                            request.name,
                            request.fresh,
                            &active,
                        ));
                        self.screen = Screen::Canvas;
                    }
                    Err(error) => self
                        .files
                        .show_message("Couldn't play the script", error.to_string()),
                }
            }
        }
        self.update_playback(ui);
        self.update_animation(ui);
        self.canvas.locked = self.playback.is_some() || watching || self.animation.is_some();

        self.about.ui(ui.ctx());
        if !dialog && !busy {
            self.shortcut_sheet.keys(ui.ctx());
        }
        self.shortcut_sheet.ui(ui.ctx());
        self.restore_window(ui.ctx());
        if self.files.take_recent_changed() {
            self.save_settings(ui.ctx());
        }
        self.watch_window(ui.ctx());

        // Autosave, in a quiet moment.
        let input = ui.input(|i| !i.events.is_empty() || i.pointer.any_down());
        if input || had_ink {
            self.last_input = Instant::now();
        }
        let idle = self.last_input.elapsed() >= autosave::IDLE
            && !self.canvas.is_painting()
            && self.playback.is_none();
        let name = self.files.picture_name();
        let original = self.files.path().map(Path::to_path_buf);
        self.autosave.tick(
            &self.document,
            &self.history,
            &name,
            original.as_deref(),
            idle,
            SystemTime::now(),
        );
        if idle && self.settings_saved.elapsed() >= SETTINGS_EVERY {
            self.save_settings(ui.ctx());
        }
        // Wake up for these even if nothing else happens (egui only
        // redraws when something does).
        if self.history.is_modified() {
            ui.ctx().request_repaint_after(autosave::IDLE);
        }
        ui.ctx()
            .request_repaint_after(SETTINGS_EVERY.saturating_sub(self.settings_saved.elapsed()));

        match self.screen {
            Screen::Canvas => self
                .canvas
                .ui(ui, &mut self.document, &mut self.history, ink_events),
            Screen::PenTest => self.pen_test.ui(ui, ink_events),
        }
    }
}
