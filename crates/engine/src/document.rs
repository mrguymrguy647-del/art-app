//! A document: one picture you are working on.
//!
//! A document is a stack of layers on paper. One layer is "active": that's
//! where painting and erasing happen. The screen shows all visible layers
//! combined; `composite_tile` works that out one tile at a time, and
//! `take_dirty_tiles` says which tiles need working out again.

use std::collections::HashSet;

use crate::color::over_paper;
use crate::layer::{BlendMode, Layer, LayerId, blend_over};
use crate::stroke::TilesBefore;
use crate::surface::{PixelRect, TileCoord};
use crate::tile::{Rgba8, TILE_SIZE, Tile};

/// The widest and tallest a canvas may be, in pixels. Pictures are sparse,
/// so size alone costs little memory, but a limit keeps a damaged file
/// from claiming an absurd size.
pub const MAX_CANVAS_SIDE: u32 = 50_000;

/// One picture: its size, paper colour and layers.
#[derive(Debug, Clone)]
pub struct Document {
    width: u32,
    height: u32,
    /// The colour of the "paper" under all layers.
    paper: Rgba8,
    /// Bottom layer first. Never empty.
    layers: Vec<Layer>,
    active: LayerId,
    /// The number the next new layer gets (for its id and its name).
    next_layer_number: u64,
    /// Tiles of layers that were removed: the screen must update them,
    /// but the removed layer can't report them any more.
    removed_tiles: HashSet<TileCoord>,
}

impl Document {
    /// A blank document with white paper and one empty layer.
    pub fn new(width: u32, height: u32) -> Self {
        let first = Layer::new(LayerId(1), "Layer 1");
        Self {
            width,
            height,
            paper: Rgba8::WHITE,
            active: first.id(),
            layers: vec![first],
            next_layer_number: 2,
            removed_tiles: HashSet::new(),
        }
    }

    /// Rebuilds a document from saved parts (used when opening a file).
    /// `layers` is bottom first and must not be empty.
    pub(crate) fn from_parts(
        width: u32,
        height: u32,
        paper: Rgba8,
        layers: Vec<Layer>,
        active_index: usize,
        next_layer_number: u64,
    ) -> Self {
        let active = layers[active_index.min(layers.len() - 1)].id();
        Self {
            width,
            height,
            paper,
            layers,
            active,
            next_layer_number,
            removed_tiles: HashSet::new(),
        }
    }

    /// The number the next new layer will get (saved in files, so layer
    /// names keep counting up after you open a picture again).
    pub(crate) fn next_layer_number(&self) -> u64 {
        self.next_layer_number
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The colour of the "paper" under all layers.
    pub fn paper(&self) -> Rgba8 {
        self.paper
    }

    /// Changes the paper colour. Every painted tile is redrawn, because
    /// the paper shows through (and takes part in blend modes).
    pub fn set_paper(&mut self, paper: Rgba8) {
        self.paper = paper;
        for layer in &mut self.layers {
            layer.surface.mark_all_dirty();
        }
    }

    /// The whole canvas as a rectangle.
    pub fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// True if the canvas pixel (x, y) is on the canvas.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        (0.0..self.width as f32).contains(&x) && (0.0..self.height as f32).contains(&y)
    }

    /// How many tiles across and down the canvas is (counting partly
    /// covered tiles at the right and bottom edges).
    pub fn tile_grid_size(&self) -> (u32, u32) {
        let size = TILE_SIZE as u32;
        (self.width.div_ceil(size), self.height.div_ceil(size))
    }

    /// True if `coord` is one of the tiles that make up the canvas.
    pub fn contains_tile(&self, coord: TileCoord) -> bool {
        let (columns, rows) = self.tile_grid_size();
        (0..columns as i32).contains(&coord.x) && (0..rows as i32).contains(&coord.y)
    }

    // ---- Layers -------------------------------------------------------------

    /// All layers, bottom first.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id() == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|layer| layer.id() == id)
    }

    /// Where the layer is in the stack (0 = bottom).
    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|layer| layer.id() == id)
    }

    /// The layer you paint on.
    pub fn active_id(&self) -> LayerId {
        self.active
    }

    pub fn active_layer(&self) -> &Layer {
        self.layer(self.active)
            .expect("the active layer always exists")
    }

    pub fn active_layer_mut(&mut self) -> &mut Layer {
        let active = self.active;
        self.layer_mut(active)
            .expect("the active layer always exists")
    }

    /// Makes another layer the one you paint on.
    pub fn set_active(&mut self, id: LayerId) {
        if self.layer(id).is_some() {
            self.active = id;
        }
    }

    /// Makes a new, empty layer (not yet in the stack), named "Layer N".
    pub fn new_layer(&mut self) -> Layer {
        let number = self.next_layer_number;
        self.next_layer_number += 1;
        Layer::new(LayerId(number), format!("Layer {number}"))
    }

    /// Puts `layer` into the stack at `index` (0 = bottom).
    pub fn insert_layer(&mut self, index: usize, mut layer: Layer) {
        layer.surface.mark_all_dirty(); // its tiles appear on screen
        self.layers.insert(index.min(self.layers.len()), layer);
    }

    /// Takes a layer out of the stack and returns it with its position.
    /// The last remaining layer can't be removed (returns `None`).
    pub fn remove_layer(&mut self, id: LayerId) -> Option<(usize, Layer)> {
        if self.layers.len() <= 1 {
            return None;
        }
        let index = self.layer_index(id)?;
        let mut layer = self.layers.remove(index);
        // Its tiles disappear from the screen, so they need redrawing.
        self.removed_tiles
            .extend(layer.surface.tiles().map(|(coord, _)| coord));
        layer.surface.take_dirty_tiles();
        if self.active == id {
            // Paint on the layer that was below it (or the new bottom one).
            self.active = self.layers[index.saturating_sub(1)].id();
        }
        Some((index, layer))
    }

    /// Moves a layer to position `to` in the stack (0 = bottom).
    pub fn move_layer(&mut self, id: LayerId, to: usize) {
        let Some(from) = self.layer_index(id) else {
            return;
        };
        let to = to.min(self.layers.len() - 1);
        if from == to {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        // The layers it passed now overlap differently: redraw them all.
        for layer in &mut self.layers[from.min(to)..=from.max(to)] {
            layer.surface.mark_all_dirty();
        }
    }

    /// Wipes the active layer. Returns its tiles as they were, so clearing
    /// can be undone.
    pub fn clear_active_layer(&mut self) -> TilesBefore {
        self.clear_layer(self.active)
    }

    /// Makes a layer completely empty. Returns its tiles as they were
    /// before (empty if there is no such layer), for undo.
    pub fn clear_layer(&mut self, id: LayerId) -> TilesBefore {
        let Some(layer) = self.layer_mut(id) else {
            return TilesBefore::new();
        };
        layer
            .surface
            .clear()
            .into_iter()
            .map(|(coord, tile)| (coord, Some(tile)))
            .collect()
    }

    /// How many tiles all the layers use together.
    pub fn tile_count(&self) -> usize {
        self.layers
            .iter()
            .map(|layer| layer.surface.tile_count())
            .sum()
    }

    // ---- The combined picture -------------------------------------------------

    /// Canvas tiles whose combined picture changed since the last call.
    pub fn take_dirty_tiles(&mut self) -> Vec<TileCoord> {
        let mut dirty: HashSet<TileCoord> = self.removed_tiles.drain().collect();
        for layer in &mut self.layers {
            dirty.extend(layer.surface.take_dirty_tiles());
        }
        dirty.into_iter().collect()
    }

    /// True if any visible layer has something painted in this tile.
    pub fn has_visible_tile(&self, coord: TileCoord) -> bool {
        self.layers
            .iter()
            .any(|layer| layer.visible() && layer.surface.tile(coord).is_some())
    }

    /// All visible layers in this tile laid over each other, bottom to top,
    /// on `background`. `None` if nothing visible is painted there (so
    /// it's just the background).
    ///
    /// On the paper is what you see. The paper takes part in blend modes
    /// (a Screen layer over white paper disappears, as in other painting
    /// apps), and is only there inside the canvas: the parts of edge
    /// tiles outside the canvas stay transparent.
    pub fn composite_tile(&self, coord: TileCoord, background: Background) -> Option<Tile> {
        let mut showing = self
            .layers
            .iter()
            .filter(|layer| layer.visible() && layer.opacity() > 0.0)
            .filter_map(|layer| layer.surface.tile(coord).map(|tile| (layer, tile)))
            .peekable();
        let (first_layer, first_tile) = *showing.peek()?;

        let mut result = match background {
            // Quickest case: the lowest tile simply covers nothing.
            Background::Transparent
                if first_layer.opacity() >= 1.0 && first_layer.blend() == BlendMode::Normal =>
            {
                showing.next();
                first_tile.clone()
            }
            Background::Transparent => Tile::new(),
            Background::Paper => self.paper_tile(coord),
        };
        for (layer, tile) in showing {
            combine(&mut result, tile, layer);
        }
        Some(result)
    }

    /// A tile of plain paper, transparent outside the canvas.
    fn paper_tile(&self, coord: TileCoord) -> Tile {
        let mut tile = Tile::new();
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        let area = self
            .bounds()
            .intersect(&PixelRect::new(left, top, left + size, top + size));
        for y in area.top..area.bottom {
            let start = (y - top) as usize * TILE_SIZE + (area.left - left) as usize;
            let end = start + (area.right - area.left) as usize;
            tile.pixels_mut()[start..end].fill(self.paper);
        }
        tile
    }

    /// The combined colour of all visible layers at one pixel, on `background`.
    pub fn composite_pixel(&self, x: i32, y: i32, background: Background) -> Rgba8 {
        let inside = x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height;
        let start = match background {
            Background::Paper if inside => self.paper,
            _ => Rgba8::TRANSPARENT,
        };
        self.layers
            .iter()
            .filter(|layer| layer.visible())
            .fold(start, |below, layer| {
                blend_over(
                    below,
                    layer.surface.pixel(x, y),
                    layer.opacity(),
                    layer.blend(),
                )
            })
    }

    /// The colour you see at canvas position (x, y): all visible layers on
    /// the paper. `None` outside the canvas. Used by the eyedropper.
    pub fn sample_color(&self, x: f32, y: f32) -> Option<[u8; 3]> {
        if !self.contains(x, y) {
            return None;
        }
        let pixel = self.composite_pixel(x.floor() as i32, y.floor() as i32, Background::Paper);
        Some(over_paper(pixel, self.paper))
    }
}

/// What's under all the layers when they are combined.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Background {
    /// The paper colour (the picture has no transparent parts).
    Paper,
    /// Nothing: unpainted areas are transparent.
    Transparent,
}

/// Lays `top` (a tile of `layer`) over `result`, pixel by pixel.
fn combine(result: &mut Tile, top: &Tile, layer: &Layer) {
    let (opacity, blend) = (layer.opacity(), layer.blend());
    for (below, &pixel) in result.pixels_mut().iter_mut().zip(top.pixels()) {
        *below = blend_over(*below, pixel, opacity, blend);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::LayerProps;

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const BLUE: Rgba8 = Rgba8::new(0, 0, 255, 255);

    /// A document with a red bottom layer and a blue top layer (active),
    /// both painted at pixel (10, 10).
    fn red_under_blue() -> (Document, LayerId, LayerId) {
        let mut doc = Document::new(1000, 1000);
        let bottom = doc.active_id();
        doc.active_layer_mut().surface.set_pixel(10, 10, RED);
        let top_layer = doc.new_layer();
        let top = top_layer.id();
        doc.insert_layer(1, top_layer);
        doc.set_active(top);
        doc.active_layer_mut().surface.set_pixel(10, 10, BLUE);
        (doc, bottom, top)
    }

    #[test]
    fn new_document_has_one_empty_active_layer() {
        let doc = Document::new(4000, 3000);
        assert_eq!((doc.width(), doc.height()), (4000, 3000));
        assert_eq!(doc.paper(), Rgba8::WHITE);
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.active_layer().name(), "Layer 1");
        assert_eq!(doc.tile_count(), 0);
    }

    #[test]
    fn new_layers_get_new_ids_and_names() {
        let mut doc = Document::new(100, 100);
        let a = doc.new_layer();
        let b = doc.new_layer();
        assert_ne!(a.id(), b.id());
        assert_eq!((a.name(), b.name()), ("Layer 2", "Layer 3"));
    }

    #[test]
    fn the_top_layer_covers_the_bottom_one() {
        let (doc, _, _) = red_under_blue();
        let tile = doc
            .composite_tile(TileCoord::new(0, 0), Background::Transparent)
            .expect("something is painted");
        assert_eq!(tile.pixel(10, 10), BLUE);
        assert_eq!(doc.sample_color(10.5, 10.5), Some([0, 0, 255]));
    }

    #[test]
    fn hidden_layers_do_not_show() {
        let (mut doc, _, top) = red_under_blue();
        doc.layer_mut(top).unwrap().set_visible(false);
        assert_eq!(doc.composite_pixel(10, 10, Background::Transparent), RED);
        assert_eq!(
            doc.composite_tile(TileCoord::new(0, 0), Background::Transparent)
                .unwrap()
                .pixel(10, 10),
            RED
        );
    }

    #[test]
    fn layer_opacity_lets_the_layer_below_show_through() {
        let (mut doc, _, top) = red_under_blue();
        doc.layer_mut(top).unwrap().set_opacity(0.5);
        let mixed = Rgba8::new(128, 0, 128, 255);
        assert_eq!(doc.composite_pixel(10, 10, Background::Transparent), mixed);
        assert_eq!(
            doc.composite_tile(TileCoord::new(0, 0), Background::Transparent)
                .unwrap()
                .pixel(10, 10),
            mixed
        );
    }

    #[test]
    fn a_faint_bottom_layer_is_faint_on_its_own_too() {
        let mut doc = Document::new(100, 100);
        doc.active_layer_mut().surface.set_pixel(1, 1, BLUE);
        doc.active_layer_mut().set_opacity(0.5);
        let tile = doc
            .composite_tile(TileCoord::new(0, 0), Background::Transparent)
            .unwrap();
        assert_eq!(tile.pixel(1, 1), Rgba8::new(0, 0, 128, 128));
        // Light blue on white: half of 255 is 127.5, which rounds to 128.
        assert_eq!(doc.sample_color(1.0, 1.0), Some([128, 128, 255]));
    }

    #[test]
    fn the_paper_takes_part_in_blend_modes() {
        let mut doc = Document::new(100, 100);
        let grey = Rgba8::new(128, 128, 128, 255);
        doc.active_layer_mut().surface.set_pixel(1, 1, grey);
        let id = doc.active_id();
        let screen = LayerProps {
            blend: BlendMode::Screen,
            ..doc.active_layer().props()
        };
        doc.layer_mut(id).unwrap().set_props(screen);
        // Screen over white paper is white; with no paper it's just the grey.
        let on_paper = doc
            .composite_tile(TileCoord::new(0, 0), Background::Paper)
            .unwrap();
        assert_eq!(on_paper.pixel(1, 1), Rgba8::WHITE);
        let alone = doc
            .composite_tile(TileCoord::new(0, 0), Background::Transparent)
            .unwrap();
        assert_eq!(alone.pixel(1, 1), grey);
        // Unpainted pixels show the paper, but only inside the canvas.
        assert_eq!(on_paper.pixel(50, 50), Rgba8::WHITE);
        assert_eq!(on_paper.pixel(150, 50), Rgba8::TRANSPARENT);
    }

    #[test]
    fn nothing_painted_means_no_tile() {
        let doc = Document::new(1000, 1000);
        assert!(
            doc.composite_tile(TileCoord::new(1, 1), Background::Transparent)
                .is_none()
        );
        assert!(!doc.has_visible_tile(TileCoord::new(1, 1)));
        assert_eq!(doc.sample_color(500.0, 500.0), Some([255, 255, 255]));
    }

    #[test]
    fn hiding_a_layer_marks_its_tiles_for_redrawing() {
        let (mut doc, _, top) = red_under_blue();
        doc.take_dirty_tiles();
        doc.layer_mut(top).unwrap().set_visible(false);
        assert_eq!(doc.take_dirty_tiles(), vec![TileCoord::new(0, 0)]);
    }

    #[test]
    fn removing_a_layer_marks_its_tiles_and_picks_a_new_active_layer() {
        let (mut doc, bottom, top) = red_under_blue();
        doc.take_dirty_tiles();
        let (index, removed) = doc.remove_layer(top).expect("two layers, so one can go");
        assert_eq!((index, removed.id()), (1, top));
        assert_eq!(doc.active_id(), bottom);
        assert_eq!(doc.take_dirty_tiles(), vec![TileCoord::new(0, 0)]);
        assert_eq!(doc.composite_pixel(10, 10, Background::Transparent), RED);
    }

    #[test]
    fn the_last_layer_cannot_be_removed() {
        let mut doc = Document::new(100, 100);
        let only = doc.active_id();
        assert!(doc.remove_layer(only).is_none());
        assert_eq!(doc.layers().len(), 1);
    }

    #[test]
    fn moving_a_layer_changes_what_is_on_top() {
        let (mut doc, bottom, top) = red_under_blue();
        doc.take_dirty_tiles();
        doc.move_layer(top, 0);
        assert_eq!(doc.layer_index(top), Some(0));
        assert_eq!(doc.layer_index(bottom), Some(1));
        assert_eq!(doc.composite_pixel(10, 10, Background::Transparent), RED); // red is on top now
        assert_eq!(doc.take_dirty_tiles(), vec![TileCoord::new(0, 0)]);
    }

    #[test]
    fn clear_wipes_only_the_active_layer() {
        let (mut doc, bottom, _) = red_under_blue();
        let before = doc.clear_active_layer();
        assert_eq!(before.len(), 1);
        assert_eq!(doc.active_layer().surface.tile_count(), 0);
        assert_eq!(doc.layer(bottom).unwrap().surface.tile_count(), 1);
        assert_eq!(doc.tile_count(), 1);
    }

    #[test]
    fn contains_checks_the_canvas_edges() {
        let doc = Document::new(100, 50);
        assert!(doc.contains(0.0, 0.0));
        assert!(doc.contains(99.9, 49.9));
        assert!(!doc.contains(100.0, 10.0));
        assert!(!doc.contains(-0.1, 10.0));
        assert_eq!(doc.sample_color(-1.0, 10.0), None);
    }

    #[test]
    fn tile_grid_counts_partly_covered_edge_tiles() {
        // 4000 / 256 = 15.6 -> 16 columns; 3000 / 256 = 11.7 -> 12 rows.
        let doc = Document::new(4000, 3000);
        assert_eq!(doc.tile_grid_size(), (16, 12));
        assert!(doc.contains_tile(TileCoord::new(15, 11)));
        assert!(!doc.contains_tile(TileCoord::new(16, 0)));
        assert!(!doc.contains_tile(TileCoord::new(0, -1)));
    }
}
