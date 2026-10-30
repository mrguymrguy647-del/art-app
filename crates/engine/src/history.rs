//! Undo and redo.
//!
//! Every change to the picture is recorded as a **command**: a small object
//! that knows how to undo itself and redo itself (the "command pattern").
//! The history keeps two lists: commands you can undo, and commands you
//! have undone and can redo.
//!
//! Painting, erasing and clearing all change tiles, so they share one
//! command type, [`TileCommand`], which only keeps the tiles that changed.
//! Later steps add more command types (e.g. adding or deleting a layer).

use std::collections::VecDeque;

use crate::document::Document;
use crate::layer::LayerId;
use crate::stroke::TilesBefore;
use crate::surface::TileCoord;
use crate::tile::{TILE_PIXELS, Tile};

/// How much memory the history may use by default: 1 GB.
pub const DEFAULT_MEMORY_LIMIT: usize = 1024 * 1024 * 1024;

/// Memory used by one tile's pixels (4 bytes per pixel).
const TILE_BYTES: usize = TILE_PIXELS * 4;

/// A change that can be undone and redone.
pub trait Command {
    /// A short name for the change, e.g. "Brush stroke".
    fn name(&self) -> &str;

    /// Puts the document back to how it was before this change.
    fn undo(&mut self, document: &mut Document);

    /// Makes this change again, after it was undone.
    fn redo(&mut self, document: &mut Document);

    /// Roughly how much memory this command is keeping, in bytes.
    fn memory_size(&self) -> usize;
}

/// A change to some of a layer's tiles: a stroke, erasing, clearing.
///
/// It keeps the *other* version of each changed tile: right after the
/// change that is the old version (for undo). Undoing swaps it with the
/// tile on the canvas, so it then holds the new version (for redo), and so
/// on. Each changed tile is only stored once.
pub struct TileCommand {
    name: String,
    /// The layer whose tiles changed.
    layer: LayerId,
    tiles: Vec<(TileCoord, Option<Tile>)>,
}

impl TileCommand {
    /// Makes a command for a change that has just been made to `layer`,
    /// from its tiles as they were before (e.g. from `Stroke::finish`).
    pub fn new(name: impl Into<String>, layer: LayerId, tiles_before: TilesBefore) -> Self {
        Self {
            name: name.into(),
            layer,
            tiles: tiles_before.into_iter().collect(),
        }
    }

    /// Swaps the stored tiles with the ones on the canvas.
    fn swap(&mut self, document: &mut Document) {
        // The layer always exists here: any later change that removed it
        // has been undone first.
        let Some(layer) = document.layer_mut(self.layer) else {
            return;
        };
        for (coord, stored) in &mut self.tiles {
            *stored = layer.surface.replace_tile(*coord, stored.take());
        }
    }
}

impl Command for TileCommand {
    fn name(&self) -> &str {
        &self.name
    }

    fn undo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn redo(&mut self, document: &mut Document) {
        self.swap(document);
    }

    fn memory_size(&self) -> usize {
        let stored = self.tiles.iter().filter(|(_, tile)| tile.is_some()).count();
        stored * TILE_BYTES
    }
}

/// Several changes that are undone and redone together, as one step
/// (e.g. playing a whole painting script).
pub struct CommandGroup {
    name: String,
    /// In the order they were made.
    commands: Vec<Box<dyn Command>>,
}

impl CommandGroup {
    /// Groups changes that have already been made, oldest first.
    pub fn new(name: impl Into<String>, commands: Vec<Box<dyn Command>>) -> Self {
        Self {
            name: name.into(),
            commands,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

impl Command for CommandGroup {
    fn name(&self) -> &str {
        &self.name
    }

    fn undo(&mut self, document: &mut Document) {
        // Newest first, so each change finds the document as it left it.
        for command in self.commands.iter_mut().rev() {
            command.undo(document);
        }
    }

    fn redo(&mut self, document: &mut Document) {
        for command in &mut self.commands {
            command.redo(document);
        }
    }

    fn memory_size(&self) -> usize {
        self.commands.iter().map(|c| c.memory_size()).sum()
    }
}

/// The undo and redo lists.
pub struct History {
    /// Oldest first, newest last.
    undo: VecDeque<Box<dyn Command>>,
    /// The most recently undone command is last.
    redo: Vec<Box<dyn Command>>,
    memory_limit: usize,
    /// How many undo steps there were when the picture was last saved.
    /// `None` if that point can't be reached any more (e.g. you undid past
    /// it and then made a new change).
    saved_at: Option<usize>,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_MEMORY_LIMIT)
    }
}

impl History {
    /// An empty history that keeps at most `memory_limit` bytes.
    pub fn new(memory_limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            memory_limit,
            saved_at: Some(0),
        }
    }

    /// Remembers that the picture was just saved (or freshly opened).
    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.undo.len());
    }

    /// True if the picture has changed since it was last saved. Undoing
    /// back to exactly the saved state counts as unchanged again.
    pub fn is_modified(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    /// Records a change that has just been made to the document.
    pub fn push(&mut self, command: Box<dyn Command>) {
        // After a new change, the undone changes can't be redone any more.
        // If the saved state was among them, it can never come back.
        if self.saved_at.is_some_and(|saved| saved > self.undo.len()) {
            self.saved_at = None;
        }
        self.redo.clear();
        self.undo.push_back(command);

        // Too much memory? Forget the oldest changes, but always keep the
        // newest one, however big it is.
        while self.memory_used() > self.memory_limit && self.undo.len() > 1 {
            self.undo.pop_front();
            // Everything shifted down by one step.
            self.saved_at = self.saved_at.and_then(|saved| saved.checked_sub(1));
        }
    }

    /// Undoes the newest change. Returns false if there was nothing to undo.
    pub fn undo(&mut self, document: &mut Document) -> bool {
        let Some(mut command) = self.undo.pop_back() else {
            return false;
        };
        command.undo(document);
        self.redo.push(command);
        true
    }

    /// Redoes the most recently undone change. Returns false if there was
    /// nothing to redo.
    pub fn redo(&mut self, document: &mut Document) -> bool {
        let Some(mut command) = self.redo.pop() else {
            return false;
        };
        command.redo(document);
        self.undo.push_back(command);
        true
    }

    /// The name of what Undo would undo, if anything.
    pub fn undo_name(&self) -> Option<&str> {
        self.undo.back().map(|command| command.name())
    }

    /// The name of what Redo would redo, if anything.
    pub fn redo_name(&self) -> Option<&str> {
        self.redo.last().map(|command| command.name())
    }

    /// How many changes can be undone.
    pub fn undo_steps(&self) -> usize {
        self.undo.len()
    }

    /// Memory used by all remembered changes, in bytes.
    pub fn memory_used(&self) -> usize {
        self.undo
            .iter()
            .map(|command| command.memory_size())
            .chain(self.redo.iter().map(|command| command.memory_size()))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::brush::{BrushMode, BrushSettings};
    use crate::stroke::{Stroke, StrokePoint};
    use crate::tile::Rgba8;

    /// Everything on the layer, for comparing "before" and "after".
    fn picture(document: &Document) -> BTreeMap<TileCoord, Tile> {
        document
            .active_layer()
            .surface
            .tiles()
            .map(|(coord, tile)| (coord, tile.clone()))
            .collect()
    }

    /// Paints (or erases) a line from (x1, y) to (x2, y) with a brush of the
    /// given size, and records it in the history.
    fn paint_line(
        document: &mut Document,
        history: &mut History,
        mode: BrushMode,
        size: f32,
        x1: f32,
        x2: f32,
        y: f32,
    ) {
        let brush = BrushSettings {
            mode,
            size,
            pressure_size: false,
            pressure_opacity: false,
            ..BrushSettings::default()
        };
        let mut stroke = Stroke::new(brush, document.bounds());
        let layer = document.active_id();
        let surface = &mut document.active_layer_mut().surface;
        stroke.add_point(surface, StrokePoint::new(x1, y, 1.0));
        stroke.add_point(surface, StrokePoint::new(x2, y, 1.0));
        let before = stroke.finish(surface);
        history.push(Box::new(TileCommand::new("Brush stroke", layer, before)));
    }

    #[test]
    fn undo_brings_back_the_exact_picture_and_redo_repeats_the_change() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            900.0,
            100.0,
        );
        let first = picture(&document);
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            900.0,
            120.0,
        );
        let second = picture(&document);

        assert!(history.undo(&mut document));
        assert_eq!(picture(&document), first);
        assert!(history.undo(&mut document));
        assert_eq!(document.tile_count(), 0, "back to a blank canvas");

        assert!(history.redo(&mut document));
        assert_eq!(picture(&document), first);
        assert!(history.redo(&mut document));
        assert_eq!(picture(&document), second);
    }

    #[test]
    fn unsaved_changes_are_tracked_through_undo_and_redo() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        assert!(!history.is_modified(), "a new picture has nothing to save");

        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            100.0,
        );
        assert!(history.is_modified());
        history.mark_saved();
        assert!(!history.is_modified());

        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            200.0,
        );
        assert!(history.is_modified());
        history.undo(&mut document);
        assert!(!history.is_modified(), "back to exactly what was saved");
        history.undo(&mut document);
        assert!(history.is_modified(), "now it's older than what was saved");
        history.redo(&mut document);
        assert!(!history.is_modified());
    }

    #[test]
    fn a_new_change_after_undoing_past_the_save_stays_modified() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            100.0,
        );
        history.mark_saved();
        history.undo(&mut document);
        // A different change in place of the saved one: the saved state is gone.
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            500.0,
        );
        assert!(history.is_modified());
        history.undo(&mut document);
        assert!(history.is_modified());
    }

    #[test]
    fn nothing_to_undo_or_redo_does_nothing() {
        let mut document = Document::new(100, 100);
        let mut history = History::default();
        assert!(!history.undo(&mut document));
        assert!(!history.redo(&mut document));
        assert_eq!(history.undo_name(), None);
    }

    #[test]
    fn a_new_change_after_undo_clears_redo() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            100.0,
        );
        history.undo(&mut document);
        assert_eq!(history.redo_name(), Some("Brush stroke"));

        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            300.0,
            500.0,
        );
        assert_eq!(history.redo_name(), None);
        assert!(!history.redo(&mut document));
    }

    #[test]
    fn undoing_the_eraser_brings_back_erased_tiles() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            300.0,
            400.0,
            50.0,
        );
        let painted = picture(&document);

        // Erase the whole line: its tile is removed.
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Erase,
            80.0,
            250.0,
            450.0,
            50.0,
        );
        assert_eq!(document.tile_count(), 0);

        history.undo(&mut document);
        assert_eq!(picture(&document), painted);
    }

    #[test]
    fn clearing_the_canvas_can_be_undone() {
        let mut document = Document::new(2000, 1000);
        let mut history = History::default();
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            100.0,
            1500.0,
            300.0,
        );
        let painted = picture(&document);

        let layer = document.active_id();
        let before = document.clear_active_layer();
        history.push(Box::new(TileCommand::new("Clear layer", layer, before)));
        assert_eq!(document.tile_count(), 0);

        history.undo(&mut document);
        assert_eq!(picture(&document), painted);
        assert_eq!(history.redo_name(), Some("Clear layer"));
    }

    #[test]
    fn each_changed_tile_is_stored_once() {
        let mut document = Document::new(2000, 1000);
        document
            .active_layer_mut()
            .surface
            .set_pixel(10, 10, Rgba8::BLACK); // tile (0, 0) exists
        let mut history = History::default();
        // A line across tiles (0, 0) and (1, 0); only (0, 0) existed before.
        paint_line(
            &mut document,
            &mut history,
            BrushMode::Paint,
            20.0,
            50.0,
            400.0,
            50.0,
        );
        assert_eq!(history.memory_used(), TILE_BYTES);

        // After undo, the command holds the new versions of both tiles.
        history.undo(&mut document);
        assert_eq!(history.memory_used(), 2 * TILE_BYTES);
    }

    #[test]
    fn the_oldest_changes_are_forgotten_when_memory_runs_out() {
        let mut document = Document::new(4000, 1000);
        // Room for about three tiles' worth of history.
        let mut history = History::new(3 * TILE_BYTES);
        // Each line paints over existing tiles, so each stores 1-2 tiles.
        document
            .active_layer_mut()
            .surface
            .set_pixel(0, 0, Rgba8::BLACK);
        for i in 0..6 {
            let y = 20.0 + i as f32 * 30.0;
            paint_line(
                &mut document,
                &mut history,
                BrushMode::Paint,
                20.0,
                10.0,
                100.0,
                y,
            );
        }
        assert!(history.memory_used() <= 3 * TILE_BYTES);
        assert!(history.undo_steps() < 6, "some old steps were dropped");
        assert!(history.undo_steps() >= 1, "the newest step is always kept");
    }
}
