//! The Text tool (T): click on the canvas and type.
//!
//! The text is painted with the engine's own `text` command (the built-in
//! font), on the selected layer, every time it changes, so what you see
//! while typing is exactly what you get. Until it's finished it stays
//! editable, with a box around it and a line where the next letter goes:
//! type, Backspace, Enter for a new line, and change the size, colour,
//! alignment and outline in the side panel.
//!
//! Enter on an empty line, or a click outside the box, finishes it: it
//! becomes one undo step, written down for "Export strokes" as the same
//! `text` command. Escape takes it away again.

use std::collections::BTreeMap;

use art_engine::document::Document;
use art_engine::history::{Command as UndoStep, CommandGroup, History};
use art_engine::layer::LayerId;
use art_engine::script::{CanvasCmd, Command, PlayOptions, Player, Script, TextCmd, record};
use art_engine::text::{self, Align};
use art_engine::view::Point;
use eframe::egui::{self, Key};

/// The Text tool's settings (the colour is the brush colour).
pub struct TextSettings {
    /// The font size in pixels.
    pub size: f32,
    pub align: Align,
    /// Draw a border around the letters.
    pub outline: bool,
    pub outline_color: [u8; 3],
    /// How wide the border is, in pixels.
    pub outline_size: f32,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self {
            size: 48.0,
            align: Align::Left,
            outline: false,
            outline_color: [255, 255, 255],
            outline_size: 3.0,
        }
    }
}

/// What the keys typed mean for the text.
#[derive(Debug, PartialEq, Eq)]
pub enum Typing {
    /// Still typing.
    Editing,
    /// Enter on an empty line: done.
    Finish,
    /// Escape: take it away.
    Cancel,
}

/// Text being typed.
pub struct TextEdit {
    /// The layer it's painted on.
    layer: LayerId,
    /// Where it was clicked: the top of the first line (its left end, middle
    /// or right end, by the alignment).
    at: Point,
    text: String,
    /// What's painted now, and the undo steps that take it off again.
    painted: Option<(TextCmd, Vec<Box<dyn UndoStep>>)>,
}

impl TextEdit {
    pub fn new(layer: LayerId, at: Point) -> Self {
        Self {
            layer,
            at,
            text: String::new(),
            painted: None,
        }
    }

    /// The `text` command for what's typed, or `None` if there's nothing
    /// to paint yet. Its numbers are rounded, so the exported script is
    /// easy to read; it's also exactly what's painted.
    pub fn command(&self, settings: &TextSettings, color: [u8; 3]) -> Option<TextCmd> {
        if self.text.trim().is_empty() {
            return None;
        }
        let [r, g, b] = settings.outline_color;
        Some(TextCmd {
            text: self.text.clone(),
            at: [round(self.at.x), round(self.at.y)],
            size: Some((settings.size * 10.0).round() / 10.0),
            color: Some(record::hex([color[0], color[1], color[2], 255])),
            align: match settings.align {
                Align::Left => None,
                Align::Center => Some("center".to_owned()),
                Align::Right => Some("right".to_owned()),
            },
            outline: settings.outline.then(|| record::hex([r, g, b, 255])),
            outline_size: settings
                .outline
                .then(|| (settings.outline_size * 10.0).round() / 10.0),
            rotate: None,
            opacity: None,
            fit: None,
        })
    }

    /// Takes in the keys typed this frame.
    pub fn type_keys(&mut self, events: &[egui::Event]) -> Typing {
        for event in events {
            match event {
                egui::Event::Text(typed) | egui::Event::Paste(typed) => {
                    // Line breaks in pasted text count; other control
                    // characters (tabs become spaces when painted) too.
                    self.text.extend(
                        typed
                            .chars()
                            .filter(|&c| c == '\n' || c == '\t' || !c.is_control()),
                    );
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command => match key {
                    Key::Backspace => {
                        self.text.pop();
                    }
                    Key::Enter => {
                        if self.text.is_empty() || self.text.ends_with('\n') {
                            while self.text.ends_with('\n') {
                                self.text.pop();
                            }
                            return Typing::Finish;
                        }
                        self.text.push('\n');
                    }
                    Key::Escape => return Typing::Cancel,
                    _ => {}
                },
                _ => {}
            }
        }
        Typing::Editing
    }

    /// Paints the text again if what's typed or the settings changed.
    pub fn update(&mut self, document: &mut Document, settings: &TextSettings, color: [u8; 3]) {
        let wanted = self.command(settings, color);
        if self.painted.as_ref().map(|(command, _)| command) == wanted.as_ref() {
            return;
        }
        self.unpaint(document);
        if let Some(command) = wanted {
            let steps = paint(document, self.layer, &command);
            self.painted = Some((command, steps));
        }
    }

    /// Takes the painted text off the layer again.
    fn unpaint(&mut self, document: &mut Document) {
        if let Some((_, mut steps)) = self.painted.take() {
            for step in steps.iter_mut().rev() {
                step.undo(document);
            }
        }
    }

    /// Done: what's painted becomes one undo step, written down as its
    /// `text` command. Returns true if there was any text.
    pub fn finish(mut self, document: &mut Document, history: &mut History) -> bool {
        let Some((command, steps)) = self.painted.take() else {
            return false;
        };
        let Some(name) = document.layer(self.layer).map(|l| l.name().to_owned()) else {
            return false;
        };
        history.push_recorded(
            Box::new(CommandGroup::new("Text", steps)),
            vec![record::select_layer(&name), Command::Text(command)],
        );
        true
    }

    /// Takes the text away: the layer is as it was.
    pub fn cancel(mut self, document: &mut Document) {
        self.unpaint(document);
    }

    /// The box around the text and the line where the next letter goes,
    /// in canvas pixels: (top left, bottom right, caret top, caret bottom).
    pub fn frame(&self, settings: &TextSettings) -> (Point, Point, Point, Point) {
        let size = settings.size;
        let shape = text::layout(&self.text, size, settings.align, 1.0);
        let lines = self.text.split('\n').count().max(1);
        let line_height = shape.height / lines as f32;
        // At least a little room to click in, even before typing.
        let width = shape.width.max(size * 0.6);
        let left = match settings.align {
            Align::Left => self.at.x,
            Align::Center => self.at.x - width / 2.0,
            Align::Right => self.at.x - width,
        };
        let last = self.text.rsplit('\n').next().unwrap_or("");
        let last_width = text::layout(last, size, settings.align, 1.0).width;
        let caret_x = match settings.align {
            Align::Left => self.at.x + last_width,
            Align::Center => self.at.x + last_width / 2.0,
            Align::Right => self.at.x,
        };
        let caret_top = self.at.y + (lines - 1) as f32 * line_height;
        // Room for the outline around the letters.
        let pad = if settings.outline {
            settings.outline_size + 4.0
        } else {
            4.0
        };
        (
            Point::new(left - pad, self.at.y - pad),
            Point::new(left + width + pad, self.at.y + shape.height + pad),
            Point::new(caret_x, caret_top),
            Point::new(caret_x, caret_top + line_height),
        )
    }

    /// True if `point` is inside the box.
    pub fn contains(&self, settings: &TextSettings, point: Point) -> bool {
        let (top_left, bottom_right, ..) = self.frame(settings);
        (top_left.x..=bottom_right.x).contains(&point.x)
            && (top_left.y..=bottom_right.y).contains(&point.y)
    }
}

/// Paints `command` on `layer` with the script player (exactly as a script
/// would), and returns the undo steps that take it off again.
fn paint(document: &mut Document, layer: LayerId, command: &TextCmd) -> Vec<Box<dyn UndoStep>> {
    if document.active_id() != layer {
        document.set_active(layer);
    }
    let script = Script {
        title: None,
        notes: None,
        seed: 1,
        palette: BTreeMap::new(),
        animations: art_engine::script::Animations::default(),
        pivot: None,
        intent: None,
        commands: vec![
            Command::Canvas(CanvasCmd::default()),
            Command::Text(command.clone()),
        ],
    };
    let options = PlayOptions {
        record_undo: true,
        ..PlayOptions::default()
    };
    let Ok(mut player) = Player::new(script, options) else {
        return Vec::new();
    };
    if player.start_on(document).is_err() || player.run(document, |_, _| {}).is_err() {
        // Can't happen with the tool's settings; leave nothing half-done.
        let mut steps = player.take_undo(document);
        for step in steps.iter_mut().rev() {
            step.undo(document);
        }
        return Vec::new();
    }
    player.take_undo(document)
}

/// Rounded to 2 decimals: plenty for a position, and easy to read.
fn round(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use art_engine::document::Background;
    use art_engine::file;
    use art_engine::script::record;

    fn png(document: &Document) -> Vec<u8> {
        let mut bytes = Vec::new();
        file::write_png(document, Background::Paper, &mut bytes).unwrap();
        bytes
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn typed(text: &str) -> egui::Event {
        egui::Event::Text(text.to_owned())
    }

    const INK: [u8; 3] = [30, 30, 30];

    #[test]
    fn typing_backspace_and_enter() {
        let layer = Document::new(10, 10).active_id();
        let mut edit = TextEdit::new(layer, Point::new(10.0, 10.0));
        assert_eq!(
            edit.type_keys(&[typed("Hi"), typed("!"), key(Key::Backspace)]),
            Typing::Editing
        );
        assert_eq!(edit.text, "Hi");
        // Enter at the end of a line starts a new one...
        assert_eq!(
            edit.type_keys(&[key(Key::Enter), typed("there")]),
            Typing::Editing
        );
        assert_eq!(edit.text, "Hi\nthere");
        // ...and Enter on an empty line finishes (without the empty line).
        assert_eq!(
            edit.type_keys(&[key(Key::Enter), key(Key::Enter)]),
            Typing::Finish
        );
        assert_eq!(edit.text, "Hi\nthere");
        assert_eq!(edit.type_keys(&[key(Key::Escape)]), Typing::Cancel);
    }

    #[test]
    fn finishing_makes_one_undo_step_that_exports_as_a_text_command() {
        let mut document = Document::new(400, 200);
        let mut history = History::default();
        let blank = png(&document);
        let mut settings = TextSettings::default();
        let mut edit = TextEdit::new(document.active_id(), Point::new(20.333, 30.0));
        edit.type_keys(&[typed("Duck")]);
        edit.update(&mut document, &settings, INK);
        let plain = png(&document);
        assert_ne!(plain, blank, "painted while typing");
        // Changing a setting paints it again.
        settings.outline = true;
        settings.align = Align::Center;
        edit.update(&mut document, &settings, INK);
        assert_ne!(png(&document), plain);
        assert_eq!(
            history.undo_steps(),
            0,
            "nothing in the history until it's finished"
        );

        assert!(edit.finish(&mut document, &mut history));
        assert_eq!(history.undo_steps(), 1);
        assert_eq!(history.undo_name(), Some("Text"));
        let finished = png(&document);

        // Exported and replayed: the same picture.
        let script = record::script(
            400,
            200,
            document.paper(),
            history.recorded_script(),
            None,
            None,
        );
        let again = Script::parse(&script.to_json()).unwrap();
        let written: Vec<&TextCmd> = again
            .commands
            .iter()
            .filter_map(|c| match c {
                Command::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].text, "Duck");
        assert_eq!(written[0].at, [20.33, 30.0], "rounded");
        assert_eq!(written[0].align.as_deref(), Some("center"));
        assert_eq!(written[0].outline.as_deref(), Some("#FFFFFF"));
        let mut player = Player::new(again, PlayOptions::default()).unwrap();
        let mut replayed = player.create_document().unwrap();
        player.run(&mut replayed, |_, _| {}).unwrap();
        assert_eq!(png(&replayed), finished);

        // And undo takes it all away.
        history.undo(&mut document);
        assert_eq!(png(&document), blank);
    }

    #[test]
    fn cancelling_or_typing_nothing_leaves_the_layer_alone() {
        let mut document = Document::new(200, 100);
        let mut history = History::default();
        let blank = png(&document);
        let settings = TextSettings::default();
        let mut edit = TextEdit::new(document.active_id(), Point::new(5.0, 5.0));
        edit.type_keys(&[typed("Gone")]);
        edit.update(&mut document, &settings, INK);
        edit.cancel(&mut document);
        assert_eq!(png(&document), blank);

        let mut empty = TextEdit::new(document.active_id(), Point::new(5.0, 5.0));
        empty.type_keys(&[typed("  ")]);
        empty.update(&mut document, &settings, INK);
        assert!(!empty.finish(&mut document, &mut history));
        assert_eq!(history.undo_steps(), 0);
    }

    #[test]
    fn the_box_grows_with_the_text_and_follows_the_alignment() {
        let mut settings = TextSettings::default();
        let layer = Document::new(10, 10).active_id();
        let mut edit = TextEdit::new(layer, Point::new(200.0, 50.0));
        let (empty_left, empty_right, ..) = edit.frame(&settings);
        edit.type_keys(&[typed("Hello there")]);
        let (left, right, caret_top, caret_bottom) = edit.frame(&settings);
        assert!(right.x - left.x > empty_right.x - empty_left.x);
        assert!((left.x - (200.0 - 4.0)).abs() < 1e-3, "starts at the click");
        assert!(
            (caret_top.x - (right.x - 4.0)).abs() < 1.0,
            "caret after the last letter"
        );
        assert!(caret_bottom.y > caret_top.y);
        assert!(edit.contains(&settings, Point::new(250.0, 70.0)));
        assert!(!edit.contains(&settings, Point::new(150.0, 70.0)));
        settings.align = Align::Right;
        let (_, right, ..) = edit.frame(&settings);
        assert!((right.x - (200.0 + 4.0)).abs() < 1e-3, "ends at the click");
        assert!(edit.contains(&settings, Point::new(150.0, 70.0)));
    }
}
