//! Help → Keyboard shortcuts (or the ? key): every shortcut on one sheet.

use eframe::egui::{self, Key, RichText};

/// Each group: its title, then (keys, what they do).
pub const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Tools",
        &[
            ("B", "Brush"),
            ("E", "Eraser (or turn the pen round)"),
            ("I", "Pick a colour (or hold Alt, or the pen's side button)"),
            ("F", "Fill (paint bucket)"),
            ("G", "Gradient"),
            ("T", "Text"),
            ("M", "Select a rectangle; again for an ellipse"),
            ("L", "Lasso; again for a polygon"),
            ("[  ]", "Brush or eraser smaller, bigger"),
        ],
    ),
    (
        "Selecting",
        &[
            ("Shift + drag", "Add to the selection"),
            ("Alt + drag", "Take away from the selection"),
            ("Ctrl+A", "Select all"),
            ("Ctrl+D", "Deselect"),
            ("Ctrl+Shift+I", "Invert the selection"),
            ("Enter", "Close a polygon; put down a transform"),
            ("Backspace", "Take back a polygon corner"),
            ("Esc", "Give up a polygon; put a transform back"),
        ],
    ),
    (
        "Edit",
        &[
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y or Ctrl+Shift+Z", "Redo"),
            ("Ctrl+X", "Cut"),
            ("Ctrl+C", "Copy"),
            ("Ctrl+V", "Paste onto a new layer"),
            ("Delete", "Delete what's selected"),
            ("Ctrl+T", "Transform what's selected"),
        ],
    ),
    (
        "File",
        &[
            ("Ctrl+N", "New picture"),
            ("Ctrl+O", "Open"),
            ("Ctrl+S", "Save"),
            ("Ctrl+Shift+S", "Save as"),
            ("Ctrl+E", "Export PNG"),
            ("Ctrl+Shift+E", "Export PNG with no paper"),
        ],
    ),
    (
        "View",
        &[
            ("Wheel, + and −", "Zoom"),
            ("Space + drag", "Move around (or the middle mouse button)"),
            ("Shift + Space + drag", "Turn the canvas"),
            ("4  6", "Turn 15° left, right"),
            ("5", "Turn back upright"),
            ("0", "Fit the picture in the window"),
            ("?", "This sheet"),
        ],
    ),
];

#[derive(Default)]
pub struct ShortcutSheet {
    pub open: bool,
}

impl ShortcutSheet {
    /// `?` opens and closes it (not while typing).
    pub fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Questionmark)) {
            self.open = !self.open;
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.columns(2, |columns| {
                    for (index, (group, entries)) in SHORTCUTS.iter().enumerate() {
                        // Tools and View on the left, the rest on the right.
                        let column = &mut columns[usize::from(matches!(index, 1..=3))];
                        column.label(RichText::new(*group).strong());
                        egui::Grid::new(*group)
                            .num_columns(2)
                            .spacing([12.0, 3.0])
                            .show(column, |ui| {
                                for (keys, what) in *entries {
                                    ui.label(RichText::new(*keys).monospace());
                                    ui.label(*what);
                                    ui.end_row();
                                }
                            });
                        column.add_space(8.0);
                    }
                });
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.open = false;
                }
            });
        self.open &= open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_lists_every_tool_key_and_file_shortcut_once() {
        let all: Vec<&str> = SHORTCUTS
            .iter()
            .flat_map(|(_, entries)| entries.iter().map(|(keys, _)| *keys))
            .collect();
        for key in [
            "B", "E", "I", "F", "G", "T", "M", "L", "Ctrl+Z", "Ctrl+S", "Ctrl+V", "Ctrl+T", "?",
        ] {
            assert_eq!(all.iter().filter(|k| **k == key).count(), 1, "{key}");
        }
    }
}
