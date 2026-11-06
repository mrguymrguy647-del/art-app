//! The little picture of each layer in the Layers panel.
//!
//! Each layer's thumbnail is kept between frames and only the part over
//! tiles that changed is worked out again, and only when no stroke is in
//! progress (so painting never waits for it). On a 4000-pixel-wide canvas
//! a 256-pixel tile is about three thumbnail pixels wide, so even a big
//! change costs little.

use std::collections::HashMap;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use quill_engine::document::Document;
use quill_engine::layer::LayerId;
use quill_engine::surface::{PixelRect, Surface, TileCoord};
use quill_engine::tile::TILE_SIZE;

/// The box a thumbnail fits in, in screen points (and thumbnail pixels).
pub const WIDTH: usize = 44;
pub const HEIGHT: usize = 32;
/// Each thumbnail pixel averages this many samples across and down.
const SAMPLES: usize = 4;

struct Thumb {
    /// Premultiplied, `size[0]` by `size[1]`.
    pixels: Vec<Color32>,
    texture: Option<TextureHandle>,
    /// The pixels changed since the texture was made.
    outdated: bool,
}

#[derive(Default)]
pub struct Thumbnails {
    by_layer: HashMap<LayerId, Thumb>,
    /// The canvas size they were made for, and the thumbnail size.
    canvas: (u32, u32),
    size: [usize; 2],
}

impl Thumbnails {
    /// The thumbnail's size in screen points (it keeps the canvas's shape).
    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.size[0] as f32, self.size[1] as f32)
    }

    /// Brings every layer's thumbnail up to date. Does nothing while
    /// `painting`, so a stroke never waits for it.
    pub fn update(&mut self, ctx: &egui::Context, document: &mut Document, painting: bool) {
        if painting {
            return;
        }
        let canvas = (document.width(), document.height());
        if canvas != self.canvas {
            // A new or resized picture: start again.
            self.by_layer.clear();
            self.canvas = canvas;
            let scale = (WIDTH as f32 / canvas.0 as f32).min(HEIGHT as f32 / canvas.1 as f32);
            self.size = [
                ((canvas.0 as f32 * scale).round() as usize).max(1),
                ((canvas.1 as f32 * scale).round() as usize).max(1),
            ];
        }
        let grid = Grid {
            canvas: self.canvas,
            size: self.size,
        };
        let ids: Vec<LayerId> = document.layers().iter().map(|l| l.id()).collect();
        self.by_layer.retain(|id, _| ids.contains(id));
        for id in ids {
            let Some(layer) = document.layer_mut(id) else {
                continue;
            };
            let changed = layer.surface.take_thumbnail_tiles();
            let surface = &layer.surface;
            match self.by_layer.get_mut(&id) {
                Some(thumb) => {
                    if !changed.is_empty() {
                        for coord in changed {
                            grid.redo_tile(&mut thumb.pixels, surface, coord);
                        }
                        thumb.outdated = true;
                    }
                }
                None => {
                    let mut thumb = Thumb {
                        pixels: vec![Color32::TRANSPARENT; self.size[0] * self.size[1]],
                        texture: None,
                        outdated: true,
                    };
                    let all = PixelRect::new(0, 0, canvas.0 as i32, canvas.1 as i32);
                    grid.redo_area(&mut thumb.pixels, surface, all);
                    self.by_layer.insert(id, thumb);
                }
            }
        }
        for (id, thumb) in &mut self.by_layer {
            if thumb.outdated {
                let image = ColorImage::new(self.size, thumb.pixels.clone());
                match &mut thumb.texture {
                    Some(texture) => texture.set(image, TextureOptions::LINEAR),
                    None => {
                        thumb.texture = Some(ctx.load_texture(
                            format!("thumbnail-{id:?}"),
                            image,
                            TextureOptions::LINEAR,
                        ));
                    }
                }
                thumb.outdated = false;
            }
        }
    }

    /// A layer's thumbnail, once it's been made.
    pub fn texture(&self, id: LayerId) -> Option<&TextureHandle> {
        self.by_layer.get(&id).and_then(|t| t.texture.as_ref())
    }
}

/// How the canvas maps onto a thumbnail.
#[derive(Clone, Copy)]
struct Grid {
    canvas: (u32, u32),
    size: [usize; 2],
}

impl Grid {
    fn redo_tile(&self, pixels: &mut [Color32], surface: &Surface, coord: TileCoord) {
        let (left, top) = coord.origin();
        let size = TILE_SIZE as i32;
        self.redo_area(
            pixels,
            surface,
            PixelRect::new(left, top, left + size, top + size),
        );
    }

    /// Works out again every thumbnail pixel over `area` (canvas pixels).
    fn redo_area(&self, pixels: &mut [Color32], surface: &Surface, area: PixelRect) {
        let (w, h) = (self.size[0], self.size[1]);
        let per_x = self.canvas.0 as f32 / w as f32;
        let per_y = self.canvas.1 as f32 / h as f32;
        let first_x = ((area.left as f32 / per_x).floor().max(0.0)) as usize;
        let last_x = ((area.right as f32 / per_x).ceil() as usize).min(w);
        let first_y = ((area.top as f32 / per_y).floor().max(0.0)) as usize;
        let last_y = ((area.bottom as f32 / per_y).ceil() as usize).min(h);
        for ty in first_y..last_y {
            for tx in first_x..last_x {
                // The average of a few samples spread over the pixel's
                // part of the canvas (premultiplied, so edges don't darken).
                let mut sum = [0u32; 4];
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let x = (tx as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * per_x;
                        let y = (ty as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * per_y;
                        let p = surface.pixel(x as i32, y as i32);
                        for (total, c) in sum.iter_mut().zip([p.r, p.g, p.b, p.a]) {
                            *total += u32::from(c);
                        }
                    }
                }
                let n = (SAMPLES * SAMPLES) as u32;
                let [r, g, b, a] = sum.map(|total| ((total + n / 2) / n) as u8);
                pixels[ty * w + tx] = Color32::from_rgba_premultiplied(r, g, b, a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quill_engine::tile::Rgba8;

    #[test]
    fn only_changed_tiles_are_redone_and_the_picture_follows_the_layer() {
        let ctx = egui::Context::default();
        let mut document = Document::new(4000, 3000);
        let id = document.active_id();
        let mut thumbnails = Thumbnails::default();
        thumbnails.update(&ctx, &mut document, false);
        assert_eq!(thumbnails.size(), egui::vec2(43.0, 32.0));
        assert!(thumbnails.texture(id).is_some());
        // Paint a red block in the middle; while painting nothing happens.
        for y in 1400..1600 {
            for x in 1900..2100 {
                document.active_layer_mut().surface.set_pixel(
                    x,
                    y,
                    Rgba8 {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                );
            }
        }
        thumbnails.update(&ctx, &mut document, true);
        let middle = |t: &Thumbnails| t.by_layer[&id].pixels[16 * 43 + 21];
        assert_eq!(middle(&thumbnails).a(), 0, "not while painting");
        thumbnails.update(&ctx, &mut document, false);
        assert_eq!(
            middle(&thumbnails),
            Color32::from_rgba_premultiplied(255, 0, 0, 255)
        );
        assert_eq!(
            thumbnails.by_layer[&id].pixels[0].a(),
            0,
            "the rest stays empty"
        );
        // A deleted layer's thumbnail goes too.
        let layer_count = document.layers().len();
        quill_engine::layer_commands::LayerPresenceCommand::add_layer(&mut document);
        thumbnails.update(&ctx, &mut document, false);
        assert_eq!(thumbnails.by_layer.len(), layer_count + 1);
    }
}
