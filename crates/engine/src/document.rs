//! A document: one picture you are working on.

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

    /// Paints a test picture, so zooming, panning and rotating can be seen
    /// before we have a brush (Step 3). Only some areas are painted, which
    /// also shows that empty areas use no tiles.
    pub fn draw_test_pattern(&mut self) {
        let bounds = self.bounds();
        let (width, height) = (self.width as f32, self.height as f32);

        // 1. Coloured squares in the top-left: the colour changes from left
        //    to right and gets darker downwards, so you can see which way
        //    is up when the canvas is rotated.
        let square = 200;
        let gap = 24;
        for row in 0..5 {
            for column in 0..7 {
                let left = gap + column * (square + gap);
                let top = gap + row * (square + gap);
                let hue = column as f32 / 7.0;
                let value = 1.0 - row as f32 * 0.15;
                let colour = hsv_to_rgba(hue, 0.75, value, 1.0);
                let rect =
                    PixelRect::new(left, top, left + square, top + square).intersect(&bounds);
                self.layer.paint_pixels(rect, |_, _| Some(colour));
            }
        }

        // 2. A big ring with a rainbow around it and soft (anti-aliased)
        //    edges, to check that curves stay smooth when zooming.
        let (cx, cy) = (width * 0.62, height * 0.58);
        let radius = width.min(height) * 0.3;
        let half_thickness = 30.0;
        let reach = (radius + half_thickness + 2.0) as i32;
        let ring = PixelRect::new(
            cx as i32 - reach,
            cy as i32 - reach,
            cx as i32 + reach,
            cy as i32 + reach,
        )
        .intersect(&bounds);
        self.layer.paint_pixels(ring, |x, y| {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let distance_from_ring = ((dx * dx + dy * dy).sqrt() - radius).abs();
            // 1.0 inside the ring, fading to 0.0 over one pixel at its edges.
            let coverage = (half_thickness + 0.5 - distance_from_ring).clamp(0.0, 1.0);
            if coverage == 0.0 {
                return None;
            }
            let hue = dy.atan2(dx) / std::f32::consts::TAU + 0.5;
            Some(hsv_to_rgba(hue, 0.8, 0.95, coverage))
        });

        // 3. A one-pixel black-and-white checkerboard near the bottom-right.
        //    Zoomed in it should show crisp squares; zoomed out, smooth grey.
        let size = 160;
        let left = self.width as i32 - size - 120;
        let top = self.height as i32 - size - 120;
        let checker = PixelRect::new(left, top, left + size, top + size).intersect(&bounds);
        self.layer.paint_pixels(checker, |x, y| {
            Some(if (x + y) % 2 == 0 {
                Rgba8::BLACK
            } else {
                Rgba8::WHITE
            })
        });
    }
}

/// Converts hue (0..1 around the colour wheel), saturation, value and
/// opacity (all 0..1) to a premultiplied pixel.
fn hsv_to_rgba(hue: f32, saturation: f32, value: f32, opacity: f32) -> Rgba8 {
    let h = hue.rem_euclid(1.0) * 6.0;
    let chroma = value * saturation;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = value - chroma;
    let to_byte = |c: f32| ((c + m) * opacity * 255.0).round() as u8;
    Rgba8::new(
        to_byte(r),
        to_byte(g),
        to_byte(b),
        (opacity * 255.0).round() as u8,
    )
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
    fn test_pattern_stays_on_the_canvas_and_leaves_empty_areas_empty() {
        let mut doc = Document::new(4000, 3000);
        doc.draw_test_pattern();

        let (columns, rows) = doc.tile_grid_size();
        let all_tiles = (columns * rows) as usize;
        let used = doc.layer.tile_count();
        assert!(used > 0, "the pattern should paint something");
        assert!(
            used < all_tiles,
            "empty areas should not use tiles ({used} of {all_tiles})"
        );

        for coord in doc.layer.take_dirty_tiles() {
            assert!(doc.contains_tile(coord), "{coord:?} is outside the canvas");
        }
    }

    #[test]
    fn hsv_primary_colours() {
        assert_eq!(hsv_to_rgba(0.0, 1.0, 1.0, 1.0), Rgba8::new(255, 0, 0, 255));
        assert_eq!(
            hsv_to_rgba(1.0 / 3.0, 1.0, 1.0, 1.0),
            Rgba8::new(0, 255, 0, 255)
        );
        assert_eq!(
            hsv_to_rgba(2.0 / 3.0, 1.0, 1.0, 1.0),
            Rgba8::new(0, 0, 255, 255)
        );
        // Half opacity is premultiplied.
        assert_eq!(hsv_to_rgba(0.0, 1.0, 1.0, 0.5), Rgba8::new(128, 0, 0, 128));
    }
}
