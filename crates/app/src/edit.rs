//! The Edit and Select menus: cut, copy, paste, delete and fill inside
//! the selection; select all, deselect, invert, select a layer's contents,
//! feather. Copy also puts the picture on the Windows clipboard, so other
//! programs can paste it, and Paste takes a picture from there.
//!
//! Selection changes, Delete and Fill are written down as script commands
//! for Export strokes. Paste and moving a selection can't be (scripts
//! have no "paste these pixels"), so Export strokes says they're missing.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use quill_engine::document::Document;
use quill_engine::history::{CommandGroup, History, TileCommand};
use quill_engine::image::Image;
use quill_engine::layer_commands::LayerPresenceCommand;
use quill_engine::script::{Command, SelectCmd, record};
use quill_engine::selection::{self, Clip, Selection};
use quill_engine::view::Point;
use serde_json::json;

use crate::select_tool;

/// What was chosen in the Edit or Select menu (or with its shortcut).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAction {
    Cut,
    Copy,
    Paste,
    /// Wipe what's selected.
    Delete,
    /// Fill what's selected with the brush colour.
    Fill,
    /// Move, resize, turn or flip what's selected (with the transform tool).
    Transform,
    SelectAll,
    Deselect,
    Invert,
    /// Select where the selected layer has paint.
    SelectLayer,
    /// Open the Feather window.
    Feather,
}

const CUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::X);
const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
const TRANSFORM: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
const SELECT_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
const DESELECT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
const INVERT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::I);
const DELETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);

/// One menu entry with its shortcut written beside it. True if clicked.
fn item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<&KeyboardShortcut>,
    enabled: bool,
) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
    }
    ui.add_enabled(enabled, button).clicked()
}

/// The Edit menu. `busy`: something must be finished first.
pub fn edit_menu(ui: &mut egui::Ui, busy: bool, selected: bool) -> Option<EditAction> {
    let mut chosen = None;
    ui.add_enabled_ui(!busy, |ui| {
        ui.menu_button("Edit", |ui| {
            let entries = [
                ("Cut", Some(&CUT), EditAction::Cut, selected),
                ("Copy", Some(&COPY), EditAction::Copy, true),
                ("Paste", Some(&PASTE), EditAction::Paste, true),
                (
                    "Delete inside the selection",
                    Some(&DELETE),
                    EditAction::Delete,
                    selected,
                ),
                (
                    "Transform selection…",
                    Some(&TRANSFORM),
                    EditAction::Transform,
                    selected,
                ),
                (
                    "Fill with the brush colour",
                    None,
                    EditAction::Fill,
                    selected,
                ),
            ];
            for (i, (label, shortcut, action, enabled)) in entries.into_iter().enumerate() {
                if i == 3 || i == 5 {
                    ui.separator();
                }
                if item(ui, label, shortcut, enabled) {
                    chosen = Some(action);
                }
            }
        });
    });
    chosen
}

/// The Select menu.
pub fn select_menu(ui: &mut egui::Ui, busy: bool, selected: bool) -> Option<EditAction> {
    let mut chosen = None;
    ui.add_enabled_ui(!busy, |ui| {
        ui.menu_button("Select", |ui| {
            let entries = [
                ("All", Some(&SELECT_ALL), EditAction::SelectAll, true),
                ("Deselect", Some(&DESELECT), EditAction::Deselect, selected),
                ("Invert", Some(&INVERT), EditAction::Invert, true),
                ("Layer contents", None, EditAction::SelectLayer, true),
                ("Feather…", None, EditAction::Feather, selected),
            ];
            for (label, shortcut, action, enabled) in entries {
                if item(ui, label, shortcut, enabled) {
                    chosen = Some(action);
                }
            }
        });
    });
    chosen
}

/// The Edit and Select shortcuts, unless a text box has the keyboard.
pub fn shortcuts(ctx: &egui::Context) -> Option<EditAction> {
    if ctx.egui_wants_keyboard_input() {
        return None;
    }
    ctx.input_mut(|i| {
        // Ctrl+X, Ctrl+C and Ctrl+V never arrive as keys: the window
        // library turns them into Cut, Copy and Paste events (Paste with
        // the clipboard's text or picture, which we read ourselves).
        let mut found = None;
        i.events.retain(|event| {
            let action = match event {
                egui::Event::Cut => Some(EditAction::Cut),
                egui::Event::Copy => Some(EditAction::Copy),
                egui::Event::Paste(_) | egui::Event::PasteImage(_) => Some(EditAction::Paste),
                _ => None,
            };
            match action {
                Some(action) if found.is_none() => {
                    found = Some(action);
                    false // used up
                }
                _ => true,
            }
        });
        if found.is_some() {
            return found;
        }
        // With Shift first: egui ignores an extra Shift when matching.
        [
            (INVERT, EditAction::Invert),
            (TRANSFORM, EditAction::Transform),
            (SELECT_ALL, EditAction::SelectAll),
            (DESELECT, EditAction::Deselect),
            (DELETE, EditAction::Delete),
        ]
        .into_iter()
        .find(|(shortcut, _)| i.consume_shortcut(shortcut))
        .map(|(_, action)| action)
    })
}

/// The selection commands that need no shape.
pub fn select_all(document: &mut Document, history: &mut History) {
    let command = SelectCmd {
        all: Some(true),
        ..SelectCmd::default()
    };
    select_tool::change(document, history, command, "Select all");
}

pub fn invert(document: &mut Document, history: &mut History) {
    let command = SelectCmd {
        invert: Some(true),
        ..SelectCmd::default()
    };
    select_tool::change(document, history, command, "Invert selection");
}

pub fn select_layer_contents(document: &mut Document, history: &mut History) {
    let command = SelectCmd {
        layer: Some(document.active_layer().name().to_owned()),
        ..SelectCmd::default()
    };
    select_tool::change(document, history, command, "Select layer contents");
}

pub fn feather(document: &mut Document, history: &mut History, pixels: f32) {
    let command = SelectCmd {
        feather: Some(pixels),
        ..SelectCmd::default()
    };
    select_tool::change(document, history, command, "Feather selection");
}

/// A rectangle over the whole canvas on the selected layer, written as a
/// script command: with the selection in place, that's exactly what
/// Delete (erase) or Fill (a colour) does.
fn over_canvas(document: &Document, fill: Option<[u8; 3]>) -> Vec<Command> {
    let mut rect = json!({
        "cmd": "rect", "x": 0, "y": 0,
        "width": document.width(), "height": document.height(),
    });
    match fill {
        Some(color) => rect["fill"] = json!(record::hex([color[0], color[1], color[2], 255])),
        // An erasing shape still names a colour; it only uses its opacity.
        None => {
            rect["fill"] = json!("#000000");
            rect["erase"] = json!(true);
        }
    }
    let mut commands = vec![record::select_layer(document.active_layer().name())];
    commands.extend(serde_json::from_value::<Command>(rect));
    commands
}

/// Wipes what's selected on the selected layer (one undo step).
/// Returns false if nothing was there to wipe.
pub fn delete_selected(document: &mut Document, history: &mut History) -> bool {
    let Some(selection) = document.selection().cloned() else {
        return false;
    };
    let layer = document.active_id();
    let script = over_canvas(document, None);
    let before = selection::delete_inside(document, layer, &selection);
    if before.is_empty() {
        return false;
    }
    history.push_recorded(Box::new(TileCommand::new("Delete", layer, before)), script);
    true
}

/// Fills what's selected with `color` (one undo step).
pub fn fill_selected(document: &mut Document, history: &mut History, color: [u8; 3]) {
    use quill_engine::brush::BrushMode;
    use quill_engine::fill::{Paint, paint_mask};
    let Some(selection) = document.selection().cloned() else {
        return;
    };
    let layer = document.active_id();
    let limits = document.bounds();
    let script = over_canvas(document, Some(color));
    let [r, g, b] = color;
    let before = paint_mask(
        &mut document.active_layer_mut().surface,
        selection.mask(),
        &Paint::Color([r, g, b, 255]),
        1.0,
        BrushMode::Paint,
        limits,
    );
    if !before.is_empty() {
        history.push_recorded(
            Box::new(TileCommand::new("Fill selection", layer, before)),
            script,
        );
    }
}

/// What was copied last, and a fingerprint of the picture put on the
/// system clipboard with it: when Paste finds the same picture there, it
/// goes back to the place it was copied from.
#[derive(Default)]
pub struct Clipboard {
    copied: Option<(Clip, u64)>,
}

impl Clipboard {
    /// Copies what's selected on the selected layer (all of it if nothing
    /// is selected). False if there's nothing to copy.
    pub fn copy(&mut self, document: &Document) -> bool {
        let selection = match document.selection() {
            Some(selection) => selection.clone(),
            None => match Selection::new(selection::all(document.bounds())) {
                Some(all) => all,
                None => return false,
            },
        };
        let Some(clip) = selection::copy(&document.active_layer().surface, &selection) else {
            return false;
        };
        let straight = straight_rgba(&clip.image);
        let print = fingerprint(clip.image.width(), clip.image.height(), &straight);
        // Other programs paste it too. If the clipboard can't be reached
        // (rare), copying still works inside Duck Quill.
        if let Ok(mut system) = arboard::Clipboard::new() {
            let _ = system.set_image(arboard::ImageData {
                width: clip.image.width() as usize,
                height: clip.image.height() as usize,
                bytes: straight.into(),
            });
        }
        self.copied = Some((clip, print));
        true
    }

    /// What Paste should put down: a picture from another program (placed
    /// in the middle of `centre`'s view), or what was copied here (at its
    /// own place).
    fn to_paste(&self, centre: Point) -> Option<Clip> {
        let from_system = arboard::Clipboard::new()
            .ok()
            .and_then(|mut system| system.get_image().ok());
        match (from_system, &self.copied) {
            (Some(image), Some((clip, print)))
                if fingerprint(image.width as u32, image.height as u32, &image.bytes) == *print =>
            {
                Some(clip.clone())
            }
            (Some(image), _) => {
                let picture = from_straight(image.width as u32, image.height as u32, &image.bytes)?;
                Some(Clip {
                    left: (centre.x - picture.width() as f32 / 2.0).round() as i32,
                    top: (centre.y - picture.height() as f32 / 2.0).round() as i32,
                    image: picture,
                })
            }
            (None, Some((clip, _))) => Some(clip.clone()),
            (None, None) => None,
        }
    }

    /// Pastes onto a new layer above the selected one (one undo step).
    /// False if there was nothing to paste.
    pub fn paste(&self, document: &mut Document, history: &mut History, centre: Point) -> bool {
        let Some(clip) = self.to_paste(centre) else {
            return false;
        };
        paste_clip(document, history, &clip);
        true
    }
}

/// Puts `clip` on a new layer called "Pasted" above the selected one.
pub fn paste_clip(document: &mut Document, history: &mut History, clip: &Clip) {
    let index = document
        .layer_index(document.active_id())
        .map_or(0, |i| i + 1);
    let add = LayerPresenceCommand::insert_layer(document, "Pasted", index);
    let layer = document.active_id();
    let limits = document.bounds();
    let before = selection::paste_into(&mut document.active_layer_mut().surface, clip, limits);
    let group = CommandGroup::new(
        "Paste",
        vec![
            Box::new(add),
            Box::new(TileCommand::new("Paste", layer, before)),
        ],
    );
    history.push_unrecordable(Box::new(group));
}

/// The engine keeps colours premultiplied; other programs want them
/// straight (not multiplied by their opacity).
fn straight_rgba(image: &Image) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((image.width() * image.height() * 4) as usize);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let [r, g, b, a] = image.pixel(x, y);
            let un = |c: u8| {
                if a == 0 {
                    0
                } else {
                    ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                }
            };
            bytes.extend_from_slice(&[un(r), un(g), un(b), a]);
        }
    }
    bytes
}

/// A picture from straight RGBA bytes, premultiplied as the engine wants.
fn from_straight(width: u32, height: u32, bytes: &[u8]) -> Option<Image> {
    if width == 0 || height == 0 || bytes.len() < (width * height * 4) as usize {
        return None;
    }
    let mut image = Image::new(width, height, [0, 0, 0, 0]);
    for (i, pixel) in bytes
        .chunks_exact(4)
        .take((width * height) as usize)
        .enumerate()
    {
        let p = quill_engine::tile::Rgba8::from_straight(pixel[0], pixel[1], pixel[2], pixel[3]);
        image.set_pixel(
            (i as u32 % width) as i32,
            (i as u32 / width) as i32,
            [p.r, p.g, p.b, p.a],
        );
    }
    Some(image)
}

/// A quick fingerprint (FNV-1a) of a picture's size and bytes.
fn fingerprint(width: u32, height: u32, bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in width
        .to_le_bytes()
        .iter()
        .chain(&height.to_le_bytes())
        .chain(bytes)
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The Feather window: how many pixels the selection's edge should fade over.
pub struct FeatherWindow {
    pub pixels: f32,
}

impl FeatherWindow {
    /// Shows the window. `Some(pixels)` when OK is pressed, `Some(0)`-free:
    /// `None` while it's open, and `close` is set when it should go.
    pub fn ui(&mut self, ctx: &egui::Context, close: &mut bool) -> Option<f32> {
        let mut result = None;
        egui::Window::new("Feather selection")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Soften the selection's edge over this many pixels:");
                ui.add(egui::Slider::new(&mut self.pixels, 1.0..=100.0).suffix(" px"));
                ui.horizontal(|ui| {
                    if ui.add(crate::theme::primary_button("OK")).clicked()
                        || ui.input(|i| i.key_pressed(Key::Enter))
                    {
                        result = Some(self.pixels);
                        *close = true;
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                        *close = true;
                    }
                });
            });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_engine::script::{PlayOptions, Player, Script};

    fn red_document() -> Document {
        let mut document = Document::new(120, 80);
        let mut history = History::default();
        select_all(&mut document, &mut history);
        fill_selected(&mut document, &mut history, [255, 0, 0]);
        document
    }

    #[test]
    fn delete_and_fill_only_change_the_selection_and_replay_in_a_script() {
        let mut document = Document::new(120, 80);
        let mut history = History::default();
        select_all(&mut document, &mut history);
        fill_selected(&mut document, &mut history, [255, 0, 0]);
        let command = SelectCmd {
            rect: Some(json!([10, 10, 30, 20])),
            ..SelectCmd::default()
        };
        select_tool::change(&mut document, &mut history, command, "Select");
        assert!(delete_selected(&mut document, &mut history));
        let surface = &document.active_layer().surface;
        assert_eq!(surface.pixel(20, 20).a, 0);
        assert_eq!(surface.pixel(60, 60).a, 255);
        // Export strokes replays it to the same picture.
        let script = record::script(
            120,
            80,
            document.paper(),
            history.recorded_script(),
            None,
            None,
        );
        let mut player = Player::new(
            Script::parse(&script.to_json()).unwrap(),
            PlayOptions::default(),
        )
        .unwrap();
        let mut replayed = player.create_document().unwrap();
        player.run(&mut replayed, |_, _| {}).unwrap();
        let a = &replayed.active_layer().surface;
        for (x, y) in [(20, 20), (60, 60), (10, 10), (39, 29)] {
            assert_eq!(a.pixel(x, y), surface.pixel(x, y), "at {x}, {y}");
        }
        assert_eq!(history.unrecordable_steps(), 0);
    }

    #[test]
    fn paste_puts_the_copy_on_a_new_layer_at_its_place_and_is_marked_missing_from_scripts() {
        let mut document = red_document();
        let mut history = History::default();
        let selection = Selection::new(selection::rect(10.0, 20.0, 30.0, 10.0, document.bounds()));
        document.set_selection(selection.clone());
        let clip = selection::copy(&document.active_layer().surface, &selection.unwrap()).unwrap();
        let layers = document.layers().len();
        paste_clip(&mut document, &mut history, &clip);
        assert_eq!(document.layers().len(), layers + 1);
        assert_eq!(document.active_layer().name(), "Pasted");
        assert_eq!(
            document
                .active_layer()
                .surface
                .painted_area()
                .map(|r| (r.left, r.top)),
            Some((10, 20))
        );
        assert_eq!(history.unrecordable_steps(), 1);
        history.undo(&mut document);
        assert_eq!(document.layers().len(), layers);
        assert_eq!(history.unrecordable_steps(), 0);
    }

    #[test]
    fn straight_colours_round_trip_for_other_programs() {
        let mut image = Image::new(2, 1, [0, 0, 0, 0]);
        image.set_pixel(0, 0, [100, 50, 0, 200]); // premultiplied
        let bytes = straight_rgba(&image);
        assert_eq!(&bytes[..4], &[128, 64, 0, 200]);
        let back = from_straight(2, 1, &bytes).unwrap();
        assert_eq!(back.pixel(0, 0), [100, 50, 0, 200]);
        assert_eq!(fingerprint(2, 1, &bytes), fingerprint(2, 1, &bytes));
        assert_ne!(fingerprint(2, 1, &bytes), fingerprint(1, 2, &bytes));
    }
}
