//! The Layers panel: the stack of layers, buttons to add, delete and move
//! them, a show/hide switch for each, and the selected layer's opacity
//! and blend mode.
//!
//! Every change here goes into the undo history (with what it did as
//! script commands, for "Export strokes"). The layer you click is the one
//! you paint on (choosing a layer isn't undoable, like in other painting
//! apps).

use art_engine::document::Document;
use art_engine::history::History;
use art_engine::layer::{BlendMode, LayerId, LayerProps};
use art_engine::layer_commands::{LayerPresenceCommand, LayerPropsCommand, MoveLayerCommand};
use art_engine::script::record;
use eframe::egui::{self, Button, Key, TextEdit, vec2};

use crate::brush_panel::percent_slider;

/// How tall each layer row is: comfortable to tap on a tablet.
const ROW_HEIGHT: f32 = 30.0;

#[derive(Default)]
pub struct LayersPanel {
    /// The layer being renamed, and the name typed so far.
    renaming: Option<(LayerId, String)>,
    /// Put the keyboard focus in the rename box on the next frame.
    focus_rename_box: bool,
    /// While the opacity slider is being used: which layer, and how it was
    /// before, so the whole drag becomes one undo step.
    opacity_edit: Option<(LayerId, LayerProps)>,
}

impl LayersPanel {
    pub fn ui(&mut self, ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
        ui.add_space(8.0);
        ui.heading("Layers");
        ui.add_space(4.0);

        self.buttons(ui, document, history);
        ui.add_space(4.0);
        self.opacity_slider(ui, document, history);
        ui.add_space(4.0);
        blend_mode_menu(ui, document, history);
        ui.separator();
        self.layer_list(ui, document, history);
    }

    /// Add, Delete, Up, Down.
    fn buttons(&mut self, ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
        let count = document.layers().len();
        let index = document.layer_index(document.active_id()).unwrap_or(0);
        let name = document.active_layer().name().to_owned();
        let size = vec2(44.0, 28.0);

        ui.horizontal_wrapped(|ui| {
            if ui
                .add(Button::new("Add").min_size(size))
                .on_hover_text("New layer above the selected one")
                .clicked()
            {
                let command = LayerPresenceCommand::add_layer(document);
                let script = record::add_layer(document.active_layer().name(), Some(&name));
                history.push_recorded(Box::new(command), script);
            }
            if ui
                .add_enabled(count > 1, Button::new("Delete").min_size(size))
                .on_hover_text("Delete the selected layer")
                .on_disabled_hover_text("A picture needs at least one layer")
                .clicked()
                && let Some(command) = LayerPresenceCommand::delete_active_layer(document)
            {
                history.push_recorded(Box::new(command), record::delete_layer(&name));
            }
            if ui
                .add_enabled(index + 1 < count, Button::new("Up").min_size(size))
                .on_hover_text("Move the selected layer up (over the one above)")
                .clicked()
                && let Some(command) = MoveLayerCommand::move_active_layer(document, true)
            {
                history.push_recorded(Box::new(command), record::move_layer(&name, index + 1));
            }
            if ui
                .add_enabled(index > 0, Button::new("Down").min_size(size))
                .on_hover_text("Move the selected layer down (under the one below)")
                .clicked()
                && let Some(command) = MoveLayerCommand::move_active_layer(document, false)
            {
                history.push_recorded(Box::new(command), record::move_layer(&name, index - 1));
            }
        });
    }

    /// The selected layer's opacity. The layer changes live while you
    /// drag; the undo step is recorded when you let go.
    fn opacity_slider(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        history: &mut History,
    ) {
        let id = document.active_id();
        let before = document.active_layer().props();
        let mut opacity = before.opacity;
        ui.label("Opacity");
        // Leave room for the value box next to the slider.
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().slider_width = (ui.available_width() - 64.0).max(60.0);
                ui.add(percent_slider(&mut opacity, 0.0..=1.0, ""))
            })
            .inner;

        if response.changed() {
            if self.opacity_edit.is_none() {
                self.opacity_edit = Some((id, before));
            }
            document.active_layer_mut().set_opacity(opacity);
        }

        let still_editing = response.dragged() || response.has_focus();
        if !still_editing
            && let Some((edited, start)) = self.opacity_edit.take()
            && let Some(layer) = document.layer(edited)
        {
            let after = layer.props();
            let script = record::layer_changes(&start, &after);
            if let Some(command) = LayerPropsCommand::already_applied(edited, start, after) {
                history.push_recorded(Box::new(command), script);
            }
        }
    }

    /// One row per layer, top layer first (like the stack itself).
    fn layer_list(&mut self, ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
        // Copy what we show first, so we can change the document while drawing.
        let rows: Vec<(LayerId, String, bool)> = document
            .layers()
            .iter()
            .rev()
            .map(|layer| (layer.id(), layer.name().to_owned(), layer.visible()))
            .collect();
        let active = document.active_id();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (id, name, visible) in rows {
                ui.horizontal(|ui| {
                    ui.set_min_height(ROW_HEIGHT);

                    // Show / hide.
                    let mut shown = visible;
                    let tip = if visible {
                        "Hide this layer"
                    } else {
                        "Show this layer"
                    };
                    if ui.checkbox(&mut shown, "").on_hover_text(tip).changed()
                        && let Some(layer) = document.layer(id)
                    {
                        let before = layer.props();
                        let after = LayerProps {
                            visible: shown,
                            ..before.clone()
                        };
                        let script = record::layer_changes(&before, &after);
                        if let Some(command) = LayerPropsCommand::apply(document, id, after) {
                            history.push_recorded(Box::new(command), script);
                        }
                    }

                    // The name: click to choose, double-click to rename.
                    if self
                        .renaming
                        .as_ref()
                        .is_some_and(|(renaming, _)| *renaming == id)
                    {
                        self.rename_box(ui, document, history);
                    } else {
                        let row_size = vec2(ui.available_width(), ROW_HEIGHT);
                        let response =
                            ui.add_sized(row_size, Button::selectable(id == active, &name));
                        if response.clicked() {
                            document.set_active(id);
                        }
                        if response.double_clicked() {
                            self.renaming = Some((id, name.clone()));
                            self.focus_rename_box = true;
                        }
                    }
                });
            }
        });

        ui.add_space(4.0);
        ui.weak("Double-click a name to rename it.");
    }

    /// The text box shown while renaming. Enter (or clicking elsewhere)
    /// keeps the new name; Escape cancels.
    fn rename_box(&mut self, ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
        let Some((id, text)) = &mut self.renaming else {
            return;
        };
        let id = *id;
        let response = ui.add(TextEdit::singleline(text).desired_width(ui.available_width()));
        if self.focus_rename_box {
            response.request_focus();
            self.focus_rename_box = false;
        }
        if response.lost_focus() {
            let cancelled = ui.input(|i| i.key_pressed(Key::Escape));
            let new_name = text.trim().to_owned();
            self.renaming = None;
            if !cancelled
                && !new_name.is_empty()
                && let Some(layer) = document.layer(id)
            {
                let before = layer.props();
                let after = LayerProps {
                    name: new_name,
                    ..before.clone()
                };
                let script = record::layer_changes(&before, &after);
                if let Some(command) = LayerPropsCommand::apply(document, id, after) {
                    history.push_recorded(Box::new(command), script);
                }
            }
        }
    }
}

/// The selected layer's blend mode: how it mixes with the layers below.
fn blend_mode_menu(ui: &mut egui::Ui, document: &mut Document, history: &mut History) {
    let id = document.active_id();
    let before = document.active_layer().props();
    let mut blend = before.blend;
    ui.label("Blend mode");
    egui::ComboBox::from_id_salt("blend_mode")
        .selected_text(blend.label())
        .width(ui.available_width())
        .height(500.0) // tall enough to show every mode without scrolling
        .show_ui(ui, |ui| {
            for mode in BlendMode::ALL {
                ui.selectable_value(&mut blend, mode, mode.label());
            }
        });
    if blend != before.blend {
        let after = LayerProps {
            blend,
            ..before.clone()
        };
        let script = record::layer_changes(&before, &after);
        if let Some(command) = LayerPropsCommand::apply(document, id, after) {
            history.push_recorded(Box::new(command), script);
        }
    }
}
