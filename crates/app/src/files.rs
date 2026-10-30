//! The File menu: new, open, save, save as and export PNG.
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
}

/// Something that throws the current picture away, so it first asks
/// "Save changes?" when there's unsaved work.
#[derive(Clone, Copy, PartialEq)]
enum Discard {
    New,
    Open,
    CloseWindow,
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
}

impl Files {
    /// The File button in the top bar, with its menu.
    /// Returns what was chosen, if anything.
    pub fn menu(&self, ui: &mut egui::Ui) -> Option<FileAction> {
        let mut chosen = None;
        let entries = [
            ("New…", NEW, FileAction::New),
            ("Open…", OPEN, FileAction::Open),
            ("Save", SAVE, FileAction::Save),
            ("Save as…", SAVE_AS, FileAction::SaveAs),
            ("Export PNG…", EXPORT, FileAction::Export(Background::Paper)),
            (
                "Export PNG, transparent…",
                EXPORT_TRANSPARENT,
                FileAction::Export(Background::Transparent),
            ),
        ];
        let button = Button::new("File").min_size(vec2(56.0, 26.0));
        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
            ui.set_min_width(260.0);
            for (label, shortcut, action) in entries {
                // The menu closes by itself after a click.
                let shortcut_text = ui.ctx().format_shortcut(&shortcut);
                let entry = Button::new(label)
                    .shortcut_text(shortcut_text)
                    .min_size(vec2(0.0, 28.0)); // easy to tap
                if ui.add(entry).clicked() {
                    chosen = Some(action);
                }
                // A line between saving and exporting.
                if action == FileAction::SaveAs {
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
        self.asking.is_some() || self.new_size.is_some() || self.error.is_some()
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
            }
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
            Answer::Cancel => None,
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
        self.error = Some(ErrorMessage {
            title,
            text: format!("{}\n\n{error}", path.display()),
        });
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
}
