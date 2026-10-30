//! Undoable layer actions: add, delete, move, and changing a layer's name,
//! visibility or opacity. See `history.rs` for how commands work.

use crate::document::Document;
use crate::history::Command;
use crate::layer::{Layer, LayerId, LayerProps};
use crate::tile::TILE_PIXELS;

/// Adding or deleting a layer.
///
/// These are mirror images: undoing an add deletes the layer again, and
/// undoing a delete puts it back. So one command does both: each time it's
/// undone or redone it "toggles" the layer in or out of the stack, keeping
/// the layer safe inside itself while it's out.
pub struct LayerPresenceCommand {
    name: &'static str,
    id: LayerId,
    /// Where in the stack the layer goes (0 = bottom).
    index: usize,
    /// The layer while it's out of the document; `None` while it's in.
    stored: Option<Layer>,
}

impl LayerPresenceCommand {
    /// Adds a new, empty layer just above the active one and makes it
    /// active. Returns the command for the history.
    pub fn add_layer(document: &mut Document) -> Self {
        let layer = document.new_layer();
        let id = layer.id();
        let index = document
            .layer_index(document.active_id())
            .map_or(0, |i| i + 1);
        document.insert_layer(index, layer);
        document.set_active(id);
        Self {
            name: "Add layer",
            id,
            index,
            stored: None,
        }
    }

    /// Deletes the active layer. Returns `None` (and does nothing) if it's
    /// the only layer.
    pub fn delete_active_layer(document: &mut Document) -> Option<Self> {
        let id = document.active_id();
        let (index, layer) = document.remove_layer(id)?;
        Some(Self {
            name: "Delete layer",
            id,
            index,
            stored: Some(layer),
        })
    }

    /// Puts the stored layer back, or takes the layer out and stores it.
    fn toggle(&mut self, document: &mut Document) {
        match self.stored.take() {
            Some(layer) => {
                document.insert_layer(self.index, layer);
                document.set_active(self.id);
            }
            None => {
                if let Some((_, layer)) = document.remove_layer(self.id) {
                    self.stored = Some(layer);
                }
            }
        }
    }
}

impl Command for LayerPresenceCommand {
    fn name(&self) -> &str {
        self.name
    }

    fn undo(&mut self, document: &mut Document) {
        self.toggle(document);
    }

    fn redo(&mut self, document: &mut Document) {
        self.toggle(document);
    }

    fn memory_size(&self) -> usize {
        self.stored
            .as_ref()
            .map_or(0, |layer| layer.surface.tile_count() * TILE_PIXELS * 4)
    }
}

/// Moving a layer up or down the stack.
pub struct MoveLayerCommand {
    id: LayerId,
    from: usize,
    to: usize,
}

impl MoveLayerCommand {
    /// Moves the active layer one place up (towards the top) or down.
    /// Returns `None` if it's already at the top or bottom.
    pub fn move_active_layer(document: &mut Document, up: bool) -> Option<Self> {
        let id = document.active_id();
        let from = document.layer_index(id)?;
        let to = if up { from + 1 } else { from.checked_sub(1)? };
        if to >= document.layers().len() {
            return None;
        }
        document.move_layer(id, to);
        Some(Self { id, from, to })
    }
}

impl Command for MoveLayerCommand {
    fn name(&self) -> &str {
        if self.to > self.from {
            "Move layer up"
        } else {
            "Move layer down"
        }
    }

    fn undo(&mut self, document: &mut Document) {
        document.move_layer(self.id, self.from);
    }

    fn redo(&mut self, document: &mut Document) {
        document.move_layer(self.id, self.to);
    }

    fn memory_size(&self) -> usize {
        0
    }
}

/// Changing a layer's name, visibility or opacity.
pub struct LayerPropsCommand {
    name: &'static str,
    id: LayerId,
    before: LayerProps,
    after: LayerProps,
}

impl LayerPropsCommand {
    /// Changes the layer to `after` and returns the command, or `None` if
    /// nothing actually changed.
    pub fn apply(document: &mut Document, id: LayerId, after: LayerProps) -> Option<Self> {
        let layer = document.layer_mut(id)?;
        let before = layer.props();
        layer.set_props(after.clone());
        Self::already_applied(id, before, after)
    }

    /// For a change that has already been made, e.g. while dragging an
    /// opacity slider: the whole drag becomes one undo step. Returns
    /// `None` if nothing actually changed.
    pub fn already_applied(id: LayerId, before: LayerProps, after: LayerProps) -> Option<Self> {
        let name = if before.name != after.name {
            "Rename layer"
        } else if before.visible != after.visible {
            if after.visible {
                "Show layer"
            } else {
                "Hide layer"
            }
        } else if before.opacity != after.opacity {
            "Layer opacity"
        } else {
            return None;
        };
        Some(Self {
            name,
            id,
            before,
            after,
        })
    }
}

impl Command for LayerPropsCommand {
    fn name(&self) -> &str {
        self.name
    }

    fn undo(&mut self, document: &mut Document) {
        if let Some(layer) = document.layer_mut(self.id) {
            layer.set_props(self.before.clone());
        }
    }

    fn redo(&mut self, document: &mut Document) {
        if let Some(layer) = document.layer_mut(self.id) {
            layer.set_props(self.after.clone());
        }
    }

    fn memory_size(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{History, TileCommand};
    use crate::tile::Rgba8;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);

    /// The layer names from bottom to top.
    fn names(document: &Document) -> Vec<String> {
        document
            .layers()
            .iter()
            .map(|l| l.name().to_owned())
            .collect()
    }

    #[test]
    fn adding_a_layer_puts_it_above_the_active_one_and_selects_it() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));
        assert_eq!(names(&doc), ["Layer 1", "Layer 2"]);
        assert_eq!(doc.active_layer().name(), "Layer 2");

        // Select the bottom layer and add again: it goes in between.
        doc.set_active(doc.layers()[0].id());
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));
        assert_eq!(names(&doc), ["Layer 1", "Layer 3", "Layer 2"]);
    }

    #[test]
    fn undo_and_redo_adding_a_layer() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));

        history.undo(&mut doc);
        assert_eq!(names(&doc), ["Layer 1"]);
        assert_eq!(doc.active_layer().name(), "Layer 1");
        history.redo(&mut doc);
        assert_eq!(names(&doc), ["Layer 1", "Layer 2"]);
        assert_eq!(doc.active_layer().name(), "Layer 2");
    }

    #[test]
    fn undoing_a_delete_brings_the_layer_back_with_its_paint() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));
        doc.active_layer_mut().surface.set_pixel(5, 5, RED);

        let delete = LayerPresenceCommand::delete_active_layer(&mut doc).expect("two layers");
        assert!(
            delete.memory_size() > 0,
            "the deleted layer is kept for undo"
        );
        history.push(Box::new(delete));
        assert_eq!(names(&doc), ["Layer 1"]);
        assert_eq!(doc.composite_pixel(5, 5), Rgba8::TRANSPARENT);

        history.undo(&mut doc);
        assert_eq!(names(&doc), ["Layer 1", "Layer 2"]);
        assert_eq!(doc.active_layer().name(), "Layer 2");
        assert_eq!(doc.composite_pixel(5, 5), RED);
    }

    #[test]
    fn the_only_layer_cannot_be_deleted() {
        let mut doc = Document::new(100, 100);
        assert!(LayerPresenceCommand::delete_active_layer(&mut doc).is_none());
        assert_eq!(doc.layers().len(), 1);
    }

    #[test]
    fn moving_layers_up_and_down_and_undoing_it() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));
        // "Layer 2" is on top and active: it can't go higher.
        assert!(MoveLayerCommand::move_active_layer(&mut doc, true).is_none());

        let down = MoveLayerCommand::move_active_layer(&mut doc, false).expect("can go down");
        assert_eq!(down.name(), "Move layer down");
        history.push(Box::new(down));
        assert_eq!(names(&doc), ["Layer 2", "Layer 1"]);
        assert!(
            MoveLayerCommand::move_active_layer(&mut doc, false).is_none(),
            "already at the bottom"
        );

        history.undo(&mut doc);
        assert_eq!(names(&doc), ["Layer 1", "Layer 2"]);
        history.redo(&mut doc);
        assert_eq!(names(&doc), ["Layer 2", "Layer 1"]);
    }

    #[test]
    fn hide_rename_and_opacity_can_be_undone() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        let id = doc.active_id();

        let hidden = LayerProps {
            visible: false,
            ..doc.active_layer().props()
        };
        let hide = LayerPropsCommand::apply(&mut doc, id, hidden).expect("it changed");
        assert_eq!(hide.name(), "Hide layer");
        history.push(Box::new(hide));
        assert!(!doc.active_layer().visible());

        let renamed = LayerProps {
            name: "Sky".into(),
            ..doc.active_layer().props()
        };
        history.push(Box::new(
            LayerPropsCommand::apply(&mut doc, id, renamed).unwrap(),
        ));
        assert_eq!(doc.active_layer().name(), "Sky");

        history.undo(&mut doc);
        assert_eq!(doc.active_layer().name(), "Layer 1");
        history.undo(&mut doc);
        assert!(doc.active_layer().visible());
    }

    #[test]
    fn an_unchanged_layer_makes_no_command() {
        let mut doc = Document::new(100, 100);
        let id = doc.active_id();
        let same = doc.active_layer().props();
        assert!(LayerPropsCommand::apply(&mut doc, id, same).is_none());
    }

    #[test]
    fn a_whole_opacity_drag_is_one_undo_step() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        let id = doc.active_id();
        let before = doc.active_layer().props();
        // Many small changes while dragging...
        for step in 1..=10 {
            doc.active_layer_mut().set_opacity(1.0 - step as f32 * 0.05);
        }
        // ...recorded once at the end.
        let after = doc.active_layer().props();
        history.push(Box::new(
            LayerPropsCommand::already_applied(id, before, after).unwrap(),
        ));
        assert_eq!(history.undo_steps(), 1);

        history.undo(&mut doc);
        assert_eq!(doc.active_layer().opacity(), 1.0);
    }

    #[test]
    fn strokes_are_undone_on_their_own_layer() {
        let mut doc = Document::new(100, 100);
        let mut history = History::default();
        let bottom = doc.active_id();
        doc.active_layer_mut().surface.set_pixel(1, 1, RED);
        history.push(Box::new(LayerPresenceCommand::add_layer(&mut doc)));

        // "Paint" on the top layer and record it.
        let top = doc.active_id();
        let before =
            std::collections::HashMap::from([(crate::surface::TileCoord::new(0, 0), None)]);
        doc.active_layer_mut().surface.set_pixel(2, 2, RED);
        history.push(Box::new(TileCommand::new("Brush stroke", top, before)));

        // Switch to the bottom layer, then undo: the top layer's paint goes.
        doc.set_active(bottom);
        history.undo(&mut doc);
        assert_eq!(doc.layer(top).unwrap().surface.tile_count(), 0);
        assert_eq!(doc.layer(bottom).unwrap().surface.pixel(1, 1), RED);
    }
}
