//! The File menu: new, open, save, save as, export PNG, and painting
//! scripts: play one on the canvas, or export what you painted as one.
//!
//! It also keeps your work safe: before starting a new picture, opening
//! another one or closing the window, it asks "Save changes?" if there is
//! anything unsaved. The window title shows the picture's name, with a `*`
//! when there are unsaved changes.
//!
//! The actual reading and writing is done by the engine (`file.rs`).
//! Opening and saving use the standard Windows file windows (the `rfd`
//! library). While one of those is open, the app waits for it, like any
//! Windows program.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use art_engine::document::{Document, MAX_CANVAS_SIDE};
use art_engine::file::{self, Background, FILE_EXTENSION, FileError};
use art_engine::history::History;
use art_engine::script::{PlayOptions, Player, Script, record};
use eframe::egui::{
    self, Button, DragValue, Id, Key, KeyboardShortcut, Modal, Modifiers, ViewportCommand, vec2,
};

/// Keyboard shortcuts. Each one is also shown next to its menu entry.
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const EXPORT_TRANSPARENT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E);

/// The size a new picture gets unless you choose another one.
pub const DEFAULT_SIZE: [u32; 2] = [4000, 3000];

/// Common picture sizes offered in the New window.
const PRESETS: [(&str, [u32; 2]); 5] = [
    ("Standard", DEFAULT_SIZE),
    ("Full HD screen", [1920, 1080]),
    ("4K screen", [3840, 2160]),
    ("Square", [2048, 2048]),
    ("A4 page, 300 dpi", [2480, 3508]),
];

/// How long "Saved ..." stays in the top bar.
const NOTICE_TIME: Duration = Duration::from_secs(3);

/// Something you can ask the File menu to do.
#[derive(Clone, Copy, PartialEq)]
pub enum FileAction {
    New,
    Open,
    Save,
    SaveAs,
    Export(Background),
    PlayStrokes,
    ExportStrokes,
}

/// A script to start playing, handed over to the app.
pub struct PlayRequest {
    /// Ready to play, except that `start_on` must be called first unless
    /// `fresh` (then it was played onto a new picture made from its canvas).
    pub player: Player,
    pub script: Script,
    /// The script file's name.
    pub name: String,
    pub fresh: bool,
}

/// A script waiting for an answer: its canvas size isn't the picture's.
struct PendingPlay {
    player: Player,
    script: Script,
    name: String,
    size: (u32, u32),
}

/// Something that throws the current picture away, so it first asks
/// "Save changes?" when there's unsaved work.
#[derive(Clone, Copy, PartialEq)]
enum Discard {
    New,
    Open,
    CloseWindow,
    /// Play a script on a new picture of the script's canvas size.
    PlayNew,
}

/// An error message shown in a small window until you click OK.
struct ErrorMessage {
    title: &'static str,
    text: String,
}

#[derive(Default)]
pub struct Files {
    /// The file the picture was opened from or last saved to.
    /// `None` until it has been saved once.
    path: Option<PathBuf>,
    /// Waiting for the answer to "Save changes?" before doing this.
    asking: Option<Discard>,
    /// The New picture window, while it's open: the size typed in so far.
    new_size: Option<[u32; 2]>,
    /// The size chosen last time, offered again in the New window.
    last_new_size: Option<[u32; 2]>,
    error: Option<ErrorMessage>,
    /// A short message in the top bar ("Saved ...") and when it appeared.
    notice: Option<(String, Instant)>,
    /// You chose to close the window without saving: let it close.
    closing: bool,
    /// The window title we set last, so we only send it when it changes.
    title: String,
    /// The name of the file the picture was opened from (for the notes
    /// in exported strokes). `None` for a new picture.
    opened_from: Option<String>,
    /// A script waiting for "Play it on a new picture or this one?".
    pending_play: Option<PendingPlay>,
    asking_play_size: bool,
    /// A script ready to start playing.
    ready_play: Option<PlayRequest>,
}

impl Files {
    /// The File button in the top bar, with its menu. While `busy` (a
    /// script is playing), the entries are greyed out.
    /// Returns what was chosen, if anything.
    pub fn menu(&self, ui: &mut egui::Ui, busy: bool) -> Option<FileAction> {
        let mut chosen = None;
        let entries = [
            ("New…", Some(NEW), FileAction::New),
            ("Open…", Some(OPEN), FileAction::Open),
            ("Save", Some(SAVE), FileAction::Save),
            ("Save as…", Some(SAVE_AS), FileAction::SaveAs),
            (
                "Export PNG…",
                Some(EXPORT),
                FileAction::Export(Background::Paper),
            ),
            (
                "Export PNG, transparent…",
                Some(EXPORT_TRANSPARENT),
                FileAction::Export(Background::Transparent),
            ),
            ("Play strokes…", None, FileAction::PlayStrokes),
            ("Export strokes…", None, FileAction::ExportStrokes),
        ];
        let button = Button::new("File").min_size(vec2(56.0, 26.0));
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(260.0);
            for (label, shortcut, action) in entries {
                // The menu closes by itself after a click.
                let shortcut_text =
                    shortcut.map_or_else(String::new, |s| ui.ctx().format_shortcut(&s));
                let entry = Button::new(label)
                    .shortcut_text(shortcut_text)
                    .min_size(vec2(0.0, 28.0)); // easy to tap
                if ui.add_enabled(!busy, entry).clicked() {
                    chosen = Some(action);
                }
                // Lines between saving, exporting and scripts.
                if matches!(
                    action,
                    FileAction::SaveAs | FileAction::Export(Background::Transparent)
                ) {
                    ui.separator();
                }
            }
        });
        chosen
    }

    /// Checks the keyboard for the File shortcuts (Ctrl+N, Ctrl+O, ...).
    pub fn shortcuts(&self, ctx: &egui::Context) -> Option<FileAction> {
        if ctx.egui_wants_keyboard_input() {
            return None; // typing in a text box
        }
        ctx.input_mut(|i| {
            // egui ignores an extra Shift when matching, so the shortcuts
            // with Shift must be checked first.
            let in_order = [
                (SAVE_AS, FileAction::SaveAs),
                (SAVE, FileAction::Save),
                (
                    EXPORT_TRANSPARENT,
                    FileAction::Export(Background::Transparent),
                ),
                (EXPORT, FileAction::Export(Background::Paper)),
                (NEW, FileAction::New),
                (OPEN, FileAction::Open),
            ];
            in_order
                .into_iter()
                .find(|(shortcut, _)| i.consume_shortcut(shortcut))
                .map(|(_, action)| action)
        })
    }

    /// True while one of our small windows ("Save changes?", New, an
    /// error) is open. The rest of the app should then ignore shortcuts.
    pub fn dialog_open(&self) -> bool {
        self.asking.is_some()
            || self.new_size.is_some()
            || self.error.is_some()
            || self.asking_play_size
    }

    /// The short "Saved ..." message, shown in the top bar for a few seconds.
    pub fn notice_ui(&mut self, ui: &mut egui::Ui) {
        let Some((text, shown_at)) = &self.notice else {
            return;
        };
        let elapsed = shown_at.elapsed();
        if elapsed >= NOTICE_TIME {
            self.notice = None;
            return;
        }
        ui.weak(text.as_str());
        // Redraw when it's time for the message to go.
        ui.ctx().request_repaint_after(NOTICE_TIME - elapsed);
    }

    /// Runs every frame: carries out `requested`, shows our small windows,
    /// watches for the window being closed and keeps the title up to date.
    ///
    /// Returns a new document when one was created or opened. The caller
    /// then switches to it and starts a fresh undo history.
    pub fn update(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        requested: Option<FileAction>,
        document: &Document,
        history: &mut History,
    ) -> Option<Document> {
        let mut replacement = None;

        // The window's close button (or Alt+F4): stop and ask first.
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.closing && history.is_modified() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.new_size = None;
            self.asking = Some(Discard::CloseWindow);
        }

        if let Some(action) = requested
            && !self.dialog_open()
        {
            match action {
                FileAction::New => replacement = self.discard(Discard::New, ctx, frame, history),
                FileAction::Open => replacement = self.discard(Discard::Open, ctx, frame, history),
                FileAction::Save => {
                    self.save(frame, document, history, false);
                }
                FileAction::SaveAs => {
                    self.save(frame, document, history, true);
                }
                FileAction::Export(background) => self.export(frame, document, background),
                FileAction::PlayStrokes => self.play_strokes(frame, document),
                FileAction::ExportStrokes => self.export_strokes(frame, document, history),
            }
        }
        if self.asking_play_size {
            replacement = replacement.or(self.play_size_dialog(ctx, frame, document, history));
        }

        if let Some(next) = self.asking {
            replacement =
                replacement.or(self.save_changes_dialog(ctx, frame, next, document, history));
        }
        if self.new_size.is_some() {
            replacement = replacement.or(self.new_dialog(ctx));
        }
        self.error_dialog(ctx);

        if replacement.is_some() {
            // Opening or starting a picture clears the undo history, so
            // there is nothing unsaved in it.
            self.update_title(ctx, false);
        } else {
            self.update_title(ctx, history.is_modified());
        }
        replacement
    }

    /// Starts `next`, after asking "Save changes?" if there is unsaved work.
    fn discard(
        &mut self,
        next: Discard,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        history: &History,
    ) -> Option<Document> {
        if history.is_modified() {
            self.asking = Some(next);
            None
        } else {
            self.carry_out(next, ctx, frame)
        }
    }

    /// Does what was waiting for "Save changes?".
    fn carry_out(
        &mut self,
        next: Discard,
        ctx: &egui::Context,
        frame: &eframe::Frame,
    ) -> Option<Document> {
        match next {
            Discard::New => {
                self.new_size = Some(self.last_new_size.unwrap_or(DEFAULT_SIZE));
                None
            }
            Discard::Open => self.open(frame),
            Discard::CloseWindow => {
                self.closing = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
                None
            }
            Discard::PlayNew => {
                let PendingPlay {
                    mut player,
                    script,
                    name,
                    ..
                } = self.pending_play.take()?;
                match player.create_document() {
                    Ok(document) => {
                        self.path = None;
                        self.opened_from = None;
                        self.ready_play = Some(PlayRequest {
                            player,
                            script,
                            name,
                            fresh: true,
                        });
                        Some(document)
                    }
                    Err(error) => {
                        self.show_message("Couldn't play the script", error.to_string());
                        None
                    }
                }
            }
        }
    }

    /// The script picked with "Play strokes…", once it's ready to start.
    pub fn take_play_request(&mut self) -> Option<PlayRequest> {
        self.ready_play.take()
    }

    /// Asks for a painting script and gets it ready to play. If it was
    /// made for a different canvas size, asks what to do first.
    fn play_strokes(&mut self, frame: &eframe::Frame, document: &Document) {
        let Some(path) = self
            .file_window(frame)
            .set_title("Play strokes")
            .add_filter("Painting script", &["json"])
            .pick_file()
        else {
            return;
        };
        let Some((player, script)) = self.script_from_file(&path) else {
            return;
        };
        let name = file_name(&path);
        match player.canvas_size() {
            Some(size) if size != (document.width(), document.height()) => {
                self.pending_play = Some(PendingPlay {
                    player,
                    script,
                    name,
                    size,
                });
                self.asking_play_size = true;
            }
            _ => {
                self.ready_play = Some(PlayRequest {
                    player,
                    script,
                    name,
                    fresh: false,
                })
            }
        }
    }

    /// Reads a painting script and gets a player ready for it (recording
    /// undo steps). Problems are shown in a message, and give `None`.
    pub fn script_from_file(&mut self, path: &Path) -> Option<(Player, Script)> {
        let problem = |error: &dyn std::fmt::Display| format!("{}\n\n{error}", path.display());
        let title = "Couldn't play the script";
        let script = match std::fs::read_to_string(path) {
            Ok(text) => match Script::parse(&text) {
                Ok(script) => script,
                Err(error) => {
                    self.show_message(title, problem(&error));
                    return None;
                }
            },
            Err(error) => {
                self.show_message(title, problem(&error));
                return None;
            }
        };
        let options = PlayOptions {
            folder: path.parent().map(Path::to_path_buf),
            record_undo: true,
            ..PlayOptions::default()
        };
        match Player::new(script.clone(), options) {
            Ok(player) => Some((player, script)),
            Err(error) => {
                self.show_message(title, problem(&error));
                None
            }
        }
    }

    /// "This script was made for another canvas size": play it on a new
    /// picture of that size, or on this picture anyway?
    fn play_size_dialog(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        document: &Document,
        history: &History,
    ) -> Option<Document> {
        let (width, height) = self.pending_play.as_ref()?.size;
        let new_label = format!("New {width} × {height} picture");
        let modal = Modal::new(Id::unique("play_size")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading("Play strokes");
            ui.add_space(6.0);
            ui.label(format!(
                "This script was made for a {width} × {height} canvas, but this picture is {} × {}.",
                document.width(),
                document.height()
            ));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let size = vec2(0.0, 30.0);
                let new = ui.add(Button::new(new_label.as_str()).min_size(size)).clicked();
                let here = ui.add(Button::new("Play here anyway").min_size(size)).clicked();
                let cancel = ui.add(Button::new("Cancel").min_size(size)).clicked();
                (new, here, cancel)
            })
            .inner
        });
        let (new, here, cancel) = modal.inner;
        if cancel || modal.should_close() {
            self.asking_play_size = false;
            self.pending_play = None;
            None
        } else if here {
            self.asking_play_size = false;
            let pending = self.pending_play.take()?;
            self.ready_play = Some(PlayRequest {
                player: pending.player,
                script: pending.script,
                name: pending.name,
                fresh: false,
            });
            None
        } else if new {
            self.asking_play_size = false;
            self.discard(Discard::PlayNew, ctx, frame, history)
        } else {
            None
        }
    }

    /// Everything painted since the picture was started or opened (and
    /// not undone), as a painting script. `None` if nothing was.
    fn strokes_script(&self, document: &Document, history: &History) -> Option<Script> {
        let recorded = history.recorded_script();
        if recorded.is_empty() {
            return None;
        }
        let notes = self
            .opened_from
            .as_ref()
            .map(|name| format!("Painted on top of {name}, as it was when it was opened."));
        Some(record::script(
            document.width(),
            document.height(),
            document.paper(),
            recorded,
            Some(format!("Strokes from {}", self.name())),
            notes,
        ))
    }

    /// Saves everything painted since the picture was started or opened
    /// (and not undone) as a painting script.
    fn export_strokes(&mut self, frame: &eframe::Frame, document: &Document, history: &History) {
        let Some(script) = self.strokes_script(document, history) else {
            return self.show_message(
                "Nothing to export",
                "Nothing has been painted since this picture was started or opened.".into(),
            );
        };
        let Some(chosen) = self
            .file_window(frame)
            .set_title("Export strokes")
            .set_file_name(format!("{}-strokes.json", self.stem()))
            .add_filter("Painting script", &["json"])
            .save_file()
        else {
            return;
        };
        let path = with_extension(chosen, "json");
        match std::fs::write(&path, script.to_json()) {
            Ok(()) => self.show_notice(format!("Exported strokes to {}", file_name(&path))),
            Err(error) => self.show_message(
                "Couldn't export the strokes",
                format!("{}\n\n{error}", path.display()),
            ),
        }
    }

    /// "Save changes to ...?" with Save, Don't save and Cancel.
    fn save_changes_dialog(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        next: Discard,
        document: &Document,
        history: &mut History,
    ) -> Option<Document> {
        #[derive(PartialEq)]
        enum Answer {
            Save,
            DontSave,
            Cancel,
        }

        let question = match next {
            Discard::New => "before starting a new picture?",
            Discard::Open => "before opening another picture?",
            Discard::CloseWindow => "before closing?",
            Discard::PlayNew => "before playing the script on a new picture?",
        };
        let name = self.name();
        let modal = Modal::new(Id::unique("save_changes")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading("Save changes?");
            ui.add_space(6.0);
            ui.label(format!(
                "Do you want to save the changes to \"{name}\" {question}"
            ));
            ui.add_space(12.0);
            let size = vec2(90.0, 30.0);
            ui.horizontal(|ui| {
                if ui.add(Button::new("Save").min_size(size)).clicked() {
                    return Some(Answer::Save);
                }
                if ui.add(Button::new("Don't save").min_size(size)).clicked() {
                    return Some(Answer::DontSave);
                }
                if ui.add(Button::new("Cancel").min_size(size)).clicked() {
                    return Some(Answer::Cancel);
                }
                None
            })
            .inner
        });
        // Escape or a click outside the window means Cancel.
        let answer = if modal.should_close() {
            Some(Answer::Cancel)
        } else {
            modal.inner
        };

        let answer = answer?;
        self.asking = None;
        match answer {
            Answer::Save => {
                // Only carry on if it really was saved (not cancelled or failed).
                if self.save(frame, document, history, false) {
                    self.carry_out(next, ctx, frame)
                } else {
                    None
                }
            }
            Answer::DontSave => self.carry_out(next, ctx, frame),
            Answer::Cancel => {
                self.pending_play = None;
                None
            }
        }
    }

    /// The New picture window: choose a size, then Create.
    fn new_dialog(&mut self, ctx: &egui::Context) -> Option<Document> {
        let [mut width, mut height] = self.new_size?;
        let modal = Modal::new(Id::unique("new_picture")).show(ctx, |ui| {
            ui.set_width(300.0);
            ui.heading("New picture");
            ui.add_space(8.0);

            egui::Grid::new("new_picture_size")
                .num_columns(2)
                .spacing(vec2(12.0, 8.0))
                .show(ui, |ui| {
                    ui.label("Width");
                    ui.add(size_field(&mut width));
                    ui.end_row();
                    ui.label("Height");
                    ui.add(size_field(&mut height));
                    ui.end_row();
                });
            if ui
                .button("Swap width and height")
                .on_hover_text("Turn the page: portrait ↔ landscape")
                .clicked()
            {
                std::mem::swap(&mut width, &mut height);
            }

            ui.add_space(8.0);
            ui.label("Common sizes:");
            for (name, size) in PRESETS {
                let text = format!("{name}: {} × {}", size[0], size[1]);
                if ui.selectable_label([width, height] == size, text).clicked() {
                    [width, height] = size;
                }
            }

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let size = vec2(90.0, 30.0);
                let create = ui.add(Button::new("Create").min_size(size)).clicked();
                let cancel = ui.add(Button::new("Cancel").min_size(size)).clicked();
                (create, cancel)
            })
            .inner
        });
        self.new_size = Some([width, height]);

        let (create, cancel) = modal.inner;
        if cancel || modal.should_close() {
            self.new_size = None;
            return None;
        }
        if !create {
            return None;
        }
        self.new_size = None;
        self.last_new_size = Some([width, height]);
        self.path = None; // not saved yet
        self.opened_from = None;
        Some(Document::new(width, height))
    }

    /// Shows the current error message, if any, until OK is clicked.
    fn error_dialog(&mut self, ctx: &egui::Context) {
        let Some(error) = &self.error else {
            return;
        };
        let modal = Modal::new(Id::unique("file_error")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(error.title);
            ui.add_space(6.0);
            ui.label(&error.text);
            ui.add_space(12.0);
            ui.add(Button::new("OK").min_size(vec2(90.0, 30.0)))
                .clicked()
        });
        if modal.inner || modal.should_close() {
            self.error = None;
        }
    }

    /// Asks which picture to open, then opens it.
    fn open(&mut self, frame: &eframe::Frame) -> Option<Document> {
        let path = self
            .file_window(frame)
            .set_title("Open picture")
            .add_filter("Art App picture", &[FILE_EXTENSION])
            .pick_file()?;
        self.open_path(path)
    }

    /// Opens the picture at `path`. If that fails, the error is shown
    /// and `None` returned.
    pub fn open_path(&mut self, path: PathBuf) -> Option<Document> {
        match file::open(&path) {
            Ok(document) => {
                self.opened_from = Some(file_name(&path));
                self.path = Some(path);
                Some(document)
            }
            Err(error) => {
                self.show_error("Couldn't open the picture", &path, &error);
                None
            }
        }
    }

    /// Saves to the picture's file. Asks for a file name first when
    /// `choose_file` is set or the picture was never saved.
    /// Returns true if the picture was saved.
    fn save(
        &mut self,
        frame: &eframe::Frame,
        document: &Document,
        history: &mut History,
        choose_file: bool,
    ) -> bool {
        let path = match &self.path {
            Some(path) if !choose_file => path.clone(),
            _ => {
                let Some(chosen) = self
                    .file_window(frame)
                    .set_title("Save picture as")
                    .set_file_name(format!("{}.{FILE_EXTENSION}", self.stem()))
                    .add_filter("Art App picture", &[FILE_EXTENSION])
                    .save_file()
                else {
                    return false; // cancelled
                };
                with_extension(chosen, FILE_EXTENSION)
            }
        };

        match file::save(document, &path) {
            Ok(()) => {
                history.mark_saved();
                self.path = Some(path);
                self.show_notice(format!("Saved {}", self.name()));
                true
            }
            Err(error) => {
                self.show_error("Couldn't save the picture", &path, &error);
                false
            }
        }
    }

    /// Asks for a file name, then saves the picture as a PNG image.
    /// Exporting doesn't count as saving: the picture stays unsaved.
    fn export(&mut self, frame: &eframe::Frame, document: &Document, background: Background) {
        let title = match background {
            Background::Paper => "Export PNG",
            Background::Transparent => "Export PNG with a transparent background",
        };
        let Some(chosen) = self
            .file_window(frame)
            .set_title(title)
            .set_file_name(format!("{}.png", self.stem()))
            .add_filter("PNG image", &["png"])
            .save_file()
        else {
            return; // cancelled
        };
        let path = with_extension(chosen, "png");
        match file::export_png(document, &path, background) {
            Ok(()) => self.show_notice(format!("Exported {}", file_name(&path))),
            Err(error) => self.show_error("Couldn't export the picture", &path, &error),
        }
    }

    /// A Windows "Open" or "Save As" window, in front of our window and
    /// starting in the picture's folder.
    fn file_window(&self, frame: &eframe::Frame) -> rfd::FileDialog {
        let mut dialog = rfd::FileDialog::new().set_parent(frame);
        if let Some(folder) = self.path.as_deref().and_then(Path::parent) {
            dialog = dialog.set_directory(folder);
        }
        dialog
    }

    fn show_error(&mut self, title: &'static str, path: &Path, error: &FileError) {
        self.show_message(title, format!("{}\n\n{error}", path.display()));
    }

    /// Shows a message in a small window until OK is clicked.
    pub fn show_message(&mut self, title: &'static str, text: String) {
        self.error = Some(ErrorMessage { title, text });
    }

    fn show_notice(&mut self, text: String) {
        self.notice = Some((text, Instant::now()));
    }

    /// The picture's file name, e.g. "cat.artapp", or "Untitled".
    fn name(&self) -> String {
        self.path
            .as_deref()
            .map_or_else(|| "Untitled".to_owned(), file_name)
    }

    /// The file name without its ending, e.g. "cat", or "Untitled".
    fn stem(&self) -> String {
        self.path.as_deref().and_then(Path::file_stem).map_or_else(
            || "Untitled".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        )
    }

    /// "cat.artapp* - Art App": the `*` means unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context, modified: bool) {
        let star = if modified { "*" } else { "" };
        let title = format!("{}{star} - Art App", self.name());
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

/// A box for typing a width or height in pixels (you can also drag it).
fn size_field(value: &mut u32) -> DragValue<'_> {
    DragValue::new(value)
        .range(1..=MAX_CANVAS_SIDE)
        .speed(4.0)
        .suffix(" px")
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Adds `.extension` to the file name unless it already ends with it
/// (in any mix of capitals). "cat" becomes "cat.artapp", and
/// "my.cat" becomes "my.cat.artapp" rather than losing its ".cat".
fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let has_it = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension));
    if has_it {
        return path;
    }
    let mut path = path.into_os_string();
    path.push(".");
    path.push(extension);
    path.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_ending_is_added_only_when_missing() {
        let check = |given: &str, expected: &str| {
            assert_eq!(
                with_extension(PathBuf::from(given), "artapp"),
                PathBuf::from(expected)
            );
        };
        check("cat", "cat.artapp");
        check("cat.artapp", "cat.artapp");
        check("cat.ARTAPP", "cat.ARTAPP");
        check("my.cat", "my.cat.artapp");
    }

    #[test]
    fn untitled_until_saved() {
        let mut files = Files::default();
        assert_eq!(files.name(), "Untitled");
        assert_eq!(files.stem(), "Untitled");
        files.path = Some(PathBuf::from("pictures").join("cat.artapp"));
        assert_eq!(files.name(), "cat.artapp");
        assert_eq!(files.stem(), "cat");
    }

    #[test]
    fn exported_strokes_say_where_they_were_painted() {
        use art_engine::history::TileCommand;
        use art_engine::stroke::TilesBefore;

        let mut files = Files::default();
        let document = Document::new(300, 200);
        let mut history = History::default();
        assert!(
            files.strokes_script(&document, &history).is_none(),
            "nothing painted yet"
        );

        let layer = document.active_id();
        history.push_recorded(
            Box::new(TileCommand::new("Clear layer", layer, TilesBefore::new())),
            record::clear_layer("Layer 1"),
        );
        files.path = Some(PathBuf::from("cat.artapp"));
        files.opened_from = Some("cat.artapp".into());
        let script = files.strokes_script(&document, &history).unwrap();
        assert_eq!(script.title.as_deref(), Some("Strokes from cat.artapp"));
        let text = script.to_json();
        assert!(text.contains("Painted on top of cat.artapp"), "{text}");
        assert!(
            text.contains("{\"cmd\":\"canvas\",\"width\":300,\"height\":200"),
            "{text}"
        );
        assert_eq!(Script::parse(&text).unwrap(), script);
    }
}
