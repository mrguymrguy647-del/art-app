//! A document: one picture you are working on.

use crate::color::over_paper;
use crate::stroke::TilesBefore;
use crate::surface::{PixelRect, Surface, TileCoord};
use crate::tile::{Rgba8, TILE_SIZE};

/// One picture: its size, paper colour and pixels.
#[derive(Debug, Clone)]
pub struct Document {
    width: u32,
    height: u32,
    /// The colour of the "paper" under all layers.
    pub paper: Rgba8,
    /// The pixels. For now there is one layer; Step 7 adds more.
    pub layer: Surface,
}

impl Document {
    /// A blank document with white paper.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            paper: Rgba8::WHITE,
            layer: Surface::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
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

    /// The colour you see at canvas position (x, y): the paint laid over
    /// the paper. `None` outside the canvas. Used by the eyedropper.
    pub fn sample_color(&self, x: f32, y: f32) -> Option<[u8; 3]> {
        if !self.contains(x, y) {
            return None;
        }
        let pixel = self.layer.pixel(x.floor() as i32, y.floor() as i32);
        Some(over_paper(pixel, self.paper))
    }

    /// Wipes the picture (back to blank paper). Returns the tiles as they
    /// were, so clearing can be undone.
    pub fn clear(&mut self) -> TilesBefore {
        self.layer
            .clear()
            .into_iter()
            .map(|(coord, tile)| (coord, Some(tile)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_document_is_blank_white_paper() {
        let doc = Document::new(4000, 3000);
        assert_eq!((doc.width(), doc.height()), (4000, 3000));
        assert_eq!(doc.paper, Rgba8::WHITE);
        assert_eq!(doc.layer.tile_count(), 0);
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

    #[test]
    fn contains_checks_the_canvas_edges() {
        let doc = Document::new(100, 50);
        assert!(doc.contains(0.0, 0.0));
        assert!(doc.contains(99.9, 49.9));
        assert!(!doc.contains(100.0, 10.0));
        assert!(!doc.contains(-0.1, 10.0));
    }

    #[test]
    fn sampling_shows_paint_on_paper() {
        let mut doc = Document::new(100, 100);
        doc.layer.set_pixel(10, 20, Rgba8::new(200, 0, 0, 255));
        assert_eq!(doc.sample_color(10.5, 20.9), Some([200, 0, 0]));
        // Unpainted canvas shows the paper.
        assert_eq!(doc.sample_color(50.0, 50.0), Some([255, 255, 255]));
        // Off the canvas there is nothing to pick.
        assert_eq!(doc.sample_color(-1.0, 50.0), None);
        assert_eq!(doc.sample_color(50.0, 100.0), None);
    }

    #[test]
    fn clear_wipes_the_picture() {
        let mut doc = Document::new(100, 100);
        doc.layer.set_pixel(10, 10, Rgba8::BLACK);
        let before = doc.clear();
        assert_eq!(doc.layer.tile_count(), 0);
        assert_eq!(before.len(), 1);
    }
}
