//! Moving, turning, resizing and mirroring everything on a layer.
//!
//! Each new pixel looks up where it came from (the transform done
//! backwards) and mixes the four old pixels around that spot ("bilinear"
//! sampling, done on premultiplied colours so see-through edges don't
//! darken). When the spot lands exactly on an old pixel, as it does for
//! mirroring around the canvas's middle and for moves by whole pixels,
//! that pixel is copied as it is: flipping twice gives back exactly what
//! was there.
//!
//! Whatever ends up off the canvas is cut off.

use crate::document::Document;
use crate::layer::LayerId;
use crate::stroke::TilesBefore;
use crate::surface::{PixelRect, Surface};
use crate::tile::{Rgba8, TILE_SIZE, Tile};
use crate::transform::Transform;

/// Moves everything on `layer` by `transform` (in canvas pixels).
/// Returns the changed tiles as they were before, for undo (empty if
/// nothing changed).
pub fn transform_layer(
    document: &mut Document,
    layer: LayerId,
    transform: Transform,
) -> TilesBefore {
    let canvas = document.bounds();
    let Some(target) = document.layer_mut(layer) else {
        return TilesBefore::new();
    };
    let Some(area) = target.surface.painted_area() else {
        return TilesBefore::new(); // nothing to move
    };
    if transform.is_identity() {
        return TilesBefore::new();
    }
    let Some(back) = transform.inverse() else {
        return TilesBefore::new(); // squashed flat: nothing sensible to do
    };
    let source = Block::copy(&target.surface, area);

    // Where the old pixels go: the box around their moved corners.
    let corners = [
        (area.left, area.top),
        (area.right, area.top),
        (area.left, area.bottom),
        (area.right, area.bottom),
    ]
    .map(|(x, y)| transform.apply((x as f32, y as f32)));
    let fold = |pick: fn((f32, f32)) -> f32, start: f32, keep: fn(f32, f32) -> f32| {
        corners.iter().map(|&p| pick(p)).fold(start, keep)
    };
    let left = fold(|p| p.0, f32::MAX, f32::min);
    let top = fold(|p| p.1, f32::MAX, f32::min);
    let right = fold(|p| p.0, f32::MIN, f32::max);
    let bottom = fold(|p| p.1, f32::MIN, f32::max);
    let destination = PixelRect::new(
        left.floor() as i32,
        top.floor() as i32,
        right.ceil() as i32,
        bottom.ceil() as i32,
    )
    .intersect(&canvas);

    // Take everything off the layer (keeping it for undo), then put the
    // moved pixels back, a tile at a time.
    let mut before: TilesBefore = target
        .surface
        .clear()
        .into_iter()
        .map(|(coord, tile)| (coord, Some(tile)))
        .collect();
    let size = TILE_SIZE as i32;
    for coord in destination.tiles() {
        let (tile_left, tile_top) = coord.origin();
        let part = destination.intersect(&PixelRect::new(
            tile_left,
            tile_top,
            tile_left + size,
            tile_top + size,
        ));
        let mut tile = Tile::new();
        let mut painted = false;
        for y in part.top..part.bottom {
            for x in part.left..part.right {
                let (sx, sy) = back.apply((x as f32 + 0.5, y as f32 + 0.5));
                let pixel = source.sample(sx - 0.5, sy - 0.5);
                if pixel.a > 0 {
                    tile.set_pixel((x - tile_left) as usize, (y - tile_top) as usize, pixel);
                    painted = true;
                }
            }
        }
        if painted {
            before.entry(coord).or_insert(None);
            target.surface.replace_tile(coord, Some(tile));
        }
    }
    before
}

/// A copy of part of a layer, as one block of pixels.
struct Block {
    area: PixelRect,
    width: usize,
    pixels: Vec<Rgba8>,
}

impl Block {
    fn copy(surface: &Surface, area: PixelRect) -> Self {
        let width = (area.right - area.left) as usize;
        let height = (area.bottom - area.top) as usize;
        let mut pixels = vec![Rgba8::TRANSPARENT; width * height];
        let size = TILE_SIZE as i32;
        for coord in area.tiles() {
            let Some(tile) = surface.tile(coord) else {
                continue;
            };
            let (left, top) = coord.origin();
            let part = area.intersect(&PixelRect::new(left, top, left + size, top + size));
            for y in part.top..part.bottom {
                for x in part.left..part.right {
                    let i = (y - area.top) as usize * width + (x - area.left) as usize;
                    pixels[i] = tile.pixel((x - left) as usize, (y - top) as usize);
                }
            }
        }
        Self {
            area,
            width,
            pixels,
        }
    }

    /// The pixel at (x, y), transparent outside the block.
    fn get(&self, x: i32, y: i32) -> Rgba8 {
        let a = self.area;
        if x < a.left || y < a.top || x >= a.right || y >= a.bottom {
            return Rgba8::TRANSPARENT;
        }
        self.pixels[(y - a.top) as usize * self.width + (x - a.left) as usize]
    }

    /// The colour at (x, y), where whole numbers are pixels' centres:
    /// mixed from the four pixels around it.
    fn sample(&self, x: f32, y: f32) -> Rgba8 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (ix, iy) = (x0 as i32, y0 as i32);
        if fx == 0.0 && fy == 0.0 {
            return self.get(ix, iy); // exactly on a pixel: a perfect copy
        }
        let corners = [
            (self.get(ix, iy), (1.0 - fx) * (1.0 - fy)),
            (self.get(ix + 1, iy), fx * (1.0 - fy)),
            (self.get(ix, iy + 1), (1.0 - fx) * fy),
            (self.get(ix + 1, iy + 1), fx * fy),
        ];
        let mix = |channel: fn(Rgba8) -> u8| {
            let sum: f32 = corners
                .iter()
                .map(|&(p, weight)| f32::from(channel(p)) * weight)
                .sum();
            sum.round().clamp(0.0, 255.0) as u8
        };
        let a = mix(|p| p.a);
        // Premultiplied: no colour may be stronger than its opacity.
        Rgba8::new(
            mix(|p| p.r).min(a),
            mix(|p| p.g).min(a),
            mix(|p| p.b).min(a),
            a,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Command, TileCommand};

    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const HALF_BLUE: Rgba8 = Rgba8::new(0, 0, 128, 128);

    /// A 300 x 200 picture with a few pixels painted on its layer.
    fn picture() -> Document {
        let mut document = Document::new(300, 200);
        let surface = &mut document.active_layer_mut().surface;
        for (x, y) in [(10, 20), (11, 20), (12, 21), (280, 150)] {
            surface.set_pixel(x, y, RED);
        }
        surface.set_pixel(100, 50, HALF_BLUE);
        // Across a tile edge too.
        for x in 250..262 {
            surface.set_pixel(x, 100, RED);
        }
        document
    }

    fn pixels(document: &Document) -> Vec<(i32, i32, Rgba8)> {
        let surface = &document.active_layer().surface;
        let mut found = Vec::new();
        for y in 0..document.height() as i32 {
            for x in 0..document.width() as i32 {
                let p = surface.pixel(x, y);
                if p.a > 0 {
                    found.push((x, y, p));
                }
            }
        }
        found
    }

    fn flip_x(document: &Document) -> Transform {
        let middle = (
            document.width() as f32 / 2.0,
            document.height() as f32 / 2.0,
        );
        Transform::about(middle, (0.0, 0.0), (1.0, 1.0), 0.0, true, false)
    }

    #[test]
    fn flipping_mirrors_each_pixel_exactly_and_twice_gives_it_back() {
        let mut document = picture();
        let original = pixels(&document);
        let layer = document.active_id();
        let flip = flip_x(&document);
        transform_layer(&mut document, layer, flip);
        let flipped = pixels(&document);
        assert_eq!(flipped.len(), original.len());
        // x becomes 299 - x (300 pixels across), nothing else changes.
        for &(x, y, p) in &original {
            assert!(flipped.contains(&(299 - x, y, p)), "({x}, {y})");
        }
        transform_layer(&mut document, layer, flip);
        assert_eq!(pixels(&document), original);
    }

    #[test]
    fn moving_by_whole_pixels_copies_exactly_and_cuts_off_at_the_edge() {
        let mut document = picture();
        let layer = document.active_id();
        transform_layer(&mut document, layer, Transform::translate(25.0, -30.0));
        let moved = pixels(&document);
        // 25 right and 30 up, exactly...
        assert!(moved.contains(&(125, 20, HALF_BLUE)));
        assert!((275..287).all(|x| moved.contains(&(x, 70, RED))));
        // ...and what went off the canvas is gone: the three pixels near
        // the top, and the one near the right edge (now at x = 305).
        assert_eq!(moved.len(), 13);
        assert!(moved.iter().all(|&(x, y, _)| x < 300 && y >= 0));
    }

    #[test]
    fn undo_puts_everything_back() {
        let mut document = picture();
        let original = pixels(&document);
        let layer = document.active_id();
        let turn = Transform::about((150.0, 100.0), (3.5, 0.0), (1.5, 1.5), 30.0, false, false);
        let before = transform_layer(&mut document, layer, turn);
        assert_ne!(pixels(&document), original);
        let mut step = TileCommand::new("Transform layer", layer, before);
        step.undo(&mut document);
        assert_eq!(pixels(&document), original);
        step.redo(&mut document);
        assert_ne!(pixels(&document), original);
    }

    #[test]
    fn turning_and_resizing_keep_the_paint_roughly_the_same() {
        let mut document = Document::new(200, 200);
        let layer = document.active_id();
        // A 20 x 20 red square in the middle.
        for y in 90..110 {
            for x in 90..110 {
                document.active_layer_mut().surface.set_pixel(x, y, RED);
            }
        }
        let ink = |document: &Document| -> f32 {
            pixels(document)
                .iter()
                .map(|&(_, _, p)| f32::from(p.a) / 255.0)
                .sum()
        };
        let turn = Transform::about((100.0, 100.0), (0.0, 0.0), (1.0, 1.0), 45.0, false, false);
        transform_layer(&mut document, layer, turn);
        assert!((ink(&document) - 400.0).abs() < 20.0, "{}", ink(&document));
        let bigger = Transform::about((100.0, 100.0), (0.0, 0.0), (2.0, 2.0), 0.0, false, false);
        transform_layer(&mut document, layer, bigger);
        assert!((ink(&document) - 1600.0).abs() < 60.0, "{}", ink(&document));
    }

    #[test]
    fn nothing_changes_for_an_empty_layer_or_no_movement() {
        let mut document = Document::new(100, 100);
        let layer = document.active_id();
        assert!(transform_layer(&mut document, layer, Transform::translate(5.0, 5.0)).is_empty());
        let mut document = picture();
        assert!(transform_layer(&mut document, layer, Transform::IDENTITY).is_empty());
    }
}
