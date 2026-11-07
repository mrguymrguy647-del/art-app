//! The Layers panel: the stack of layers, buttons to add, delete and move
//! them, a show/hide switch for each, and the selected layer's opacity
//! and blend mode.
//!
//! Every change here goes into the undo history (with what it did as
//! script commands, for "Export strokes"). The layer you click is the one
//! you paint on (choosing a layer isn't undoable, like in other painting
//! apps).

use eframe::egui::{self, Button, Color32, Key, TextEdit, vec2};
use quill_engine::document::Document;
use quill_engine::history::History;
use quill_engine::layer::{BlendMode, LayerId, LayerProps};
use quill_engine::layer_commands::{LayerPresenceCommand, LayerPropsCommand, MoveLayerCommand};
use quill_engine::script::record;

use crate::brush_panel::percent_slider;
use crate::thumbnails::Thumbnails;

/// How tall each layer row is: comfortable to tap on a tablet.
const ROW_HEIGHT: f32 = 30.0;

#[derive(Default)]
pub struct LayersPanel {
    /// Each layer's little picture.
    thumbnails: Thumbnails,
    /// The layer being renamed, and the name typed so far.
    renaming: Option<(LayerId, String)>,
    /// Put the keyboard focus in the rename box on the next frame.
    focus_rename_box: bool,
    /// While the opacity slider is being used: which layer, and how it was
    /// before, so the whole drag becomes one undo step.
    opacity_edit: Option<(LayerId, LayerProps)>,
}

impl LayersPanel {
    /// `painting`: a stroke is in progress (the thumbnails wait for it).
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        history: &mut History,
        painting: bool,
    ) {
        self.thumbnails.update(ui.ctx(), document, painting);
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
        let rows: Vec<(LayerId, String, bool, bool, bool)> = document
            .layers()
            .iter()
            .rev()
            .map(|layer| {
                (
                    layer.id(),
                    layer.name().to_owned(),
                    layer.visible(),
                    layer.clip(),
                    layer.lock(),
                )
            })
            .collect();
        let active = document.active_id();
        // A row dropped on another: (the dragged layer, the row, above it?).
        let mut dropped: Option<(LayerId, LayerId, bool)> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (id, name, visible, clip, lock) in rows {
                let row = ui.horizontal(|ui| {
                    ui.set_min_height(ROW_HEIGHT);

                    // A handle to drag the row up or down the stack.
                    let handle =
                        drag_handle(ui).on_hover_text("Drag to move this layer up or down");
                    handle.dnd_set_drag_payload(id);

                    // Show / hide, and the alpha lock: drawn icons.
                    let tip = if visible {
                        "Hide this layer"
                    } else {
                        "Show this layer"
                    };
                    if icon_toggle(ui, visible, draw_eye)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        change_props(document, history, id, |p| p.visible = !visible);
                    }
                    let lock_tip = "Lock: painting only recolours what's already on this layer";
                    if icon_toggle(ui, lock, draw_padlock)
                        .on_hover_text(lock_tip)
                        .clicked()
                    {
                        change_props(document, history, id, |p| p.lock = !lock);
                    }
                    // Clip to the layer below.
                    let mut clipped = clip;
                    let clip_tip = "Clip: show this layer only where the layer below has paint";
                    if ui
                        .toggle_value(&mut clipped, "⤵")
                        .on_hover_text(clip_tip)
                        .changed()
                    {
                        change_props(document, history, id, |p| p.clip = clipped);
                    }

                    // The layer's picture, on a checkerboard for what's see-through.
                    let (rect, _) =
                        ui.allocate_exact_size(self.thumbnails.size(), egui::Sense::hover());
                    draw_checkerboard(ui.painter(), rect);
                    if let Some(texture) = self.thumbnails.texture(id) {
                        ui.painter().image(
                            texture.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
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
                // Dropping a dragged row here: above or below this one,
                // by which half of the row the pen is over.
                let rect = row.response.rect;
                let zone = ui.interact(
                    rect,
                    egui::Id::unique(("layer-drop", id)),
                    egui::Sense::hover(),
                );
                if let Some(dragged) = zone.dnd_hover_payload::<LayerId>()
                    && *dragged != id
                    && let Some(pointer) = ui.ctx().pointer_interact_pos()
                {
                    let above = pointer.y < rect.center().y;
                    let y = if above { rect.top() } else { rect.bottom() };
                    let stroke = egui::Stroke::new(2.0, crate::theme::NIGHT_INDIGO);
                    ui.painter().hline(rect.x_range(), y, stroke);
                    if let Some(released) = zone.dnd_release_payload::<LayerId>() {
                        dropped = Some((*released, id, above));
                    }
                }
            }
        });

        if let Some((dragged, onto, above)) = dropped {
            move_next_to(document, history, dragged, onto, above);
        }

        ui.add_space(4.0);
        ui.weak("Double-click a name to rename it. Drag the dots to reorder.");
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

/// Changes one layer's settings (visible, lock, clip) as one undo step,
/// recorded for Export strokes.
fn change_props(
    document: &mut Document,
    history: &mut History,
    id: LayerId,
    change: impl FnOnce(&mut LayerProps),
) {
    let Some(layer) = document.layer(id) else {
        return;
    };
    let before = layer.props();
    let mut after = before.clone();
    change(&mut after);
    let script = record::layer_changes(&before, &after);
    if let Some(command) = LayerPropsCommand::apply(document, id, after) {
        history.push_recorded(Box::new(command), script);
    }
}

/// Moves `dragged` just above or below the layer `onto` (one undo step).
fn move_next_to(
    document: &mut Document,
    history: &mut History,
    dragged: LayerId,
    onto: LayerId,
    above: bool,
) {
    // The stack without the dragged layer, then it goes back in beside `onto`.
    let mut order: Vec<LayerId> = document.layers().iter().map(|l| l.id()).collect();
    order.retain(|&id| id != dragged);
    let Some(at) = order.iter().position(|&id| id == onto) else {
        return;
    };
    let to = if above { at + 1 } else { at };
    let Some(name) = document.layer(dragged).map(|l| l.name().to_owned()) else {
        return;
    };
    if let Some(command) = MoveLayerCommand::move_layer(document, dragged, to) {
        history.push_recorded(Box::new(command), record::move_layer(&name, to));
    }
}

/// A grip of dots: drag it to move the row.
fn drag_handle(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(10.0, ROW_HEIGHT), egui::Sense::drag());
    let color = if response.hovered() || response.dragged() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    for row in 0..3 {
        for column in 0..2 {
            let centre = rect.center() + vec2(-2.0 + 4.0 * column as f32, -5.0 + 5.0 * row as f32);
            ui.painter().circle_filled(centre, 1.2, color);
        }
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response
}

/// A small square button with a drawn icon, highlighted while `on`.
fn icon_toggle(
    ui: &mut egui::Ui,
    on: bool,
    draw: fn(&egui::Painter, egui::Rect, Color32, bool),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, false);
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
    }
    let color = if on {
        visuals.fg_stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    draw(ui.painter(), rect.shrink(3.0), color, on);
    response
}

/// An eye: open when the layer shows, crossed out when it's hidden.
fn draw_eye(painter: &egui::Painter, rect: egui::Rect, color: Color32, open: bool) {
    let stroke = egui::Stroke::new(1.5, color);
    let c = rect.center();
    let (w, h) = (rect.width() / 2.0, rect.height() / 3.2);
    // The eye's outline: two arcs meeting at the corners.
    let arc = |sign: f32| -> Vec<egui::Pos2> {
        (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                let x = c.x - w + 2.0 * w * t;
                let y = c.y + sign * h * (std::f32::consts::PI * t).sin();
                egui::pos2(x, y)
            })
            .collect()
    };
    painter.add(egui::Shape::line(arc(-1.0), stroke));
    painter.add(egui::Shape::line(arc(1.0), stroke));
    if open {
        painter.circle_filled(c, h * 0.75, color);
    } else {
        painter.line_segment([rect.left_bottom(), rect.right_top()], stroke);
    }
}

/// A padlock: filled when the alpha lock is on.
fn draw_padlock(painter: &egui::Painter, rect: egui::Rect, color: Color32, locked: bool) {
    let stroke = egui::Stroke::new(1.5, color);
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, rect.center().y - 1.0),
        egui::pos2(rect.right() - 2.0, rect.bottom()),
    );
    if locked {
        painter.rect_filled(body, 2.0, color);
    } else {
        painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
    }
    // The shackle: a half ring over the body (open on one side when unlocked).
    let centre = egui::pos2(body.center().x, body.top());
    let radius = body.width() / 2.0 - 2.0;
    let end = if locked {
        std::f32::consts::PI
    } else {
        std::f32::consts::PI * 0.75
    };
    let points: Vec<egui::Pos2> = (0..=10)
        .map(|i| {
            let angle = std::f32::consts::PI + end * i as f32 / 10.0;
            centre + vec2(radius * angle.cos(), radius * 1.2 * angle.sin())
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

/// Grey and white squares, so see-through parts of a thumbnail show.
fn draw_checkerboard(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(200));
    let cell = 6.0;
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left() + if row % 2 == 0 { 0.0 } else { cell };
        while x < rect.right() {
            let square =
                egui::Rect::from_min_size(egui::pos2(x, y), vec2(cell, cell)).intersect(rect);
            painter.rect_filled(square, 0.0, Color32::from_gray(240));
            x += 2.0 * cell;
        }
        y += cell;
        row += 1;
    }
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.0, Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );
}
