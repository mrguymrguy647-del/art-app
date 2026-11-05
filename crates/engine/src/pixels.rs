//! Pixel art: a small canvas where every pixel is a whole, crisp pixel
//! in one of the palette's colours.
//!
//! The trick is to paint bigger and shrink. A pixel canvas is painted
//! `supersample` times larger with every ordinary command (so shapes,
//! strokes, text and transforms need nothing new), then each block of
//! samples becomes one pixel: opaque if more than half of the samples
//! were painted, in the palette colour that the most of them had.
//! Averaging would give muddy in-between colours at the edge between an
//! outline and a fill; the vote gives one of the two. An optional
//! `outline` colour is then put on every see-through pixel next to a
//! painted one.

use crate::document::Document;
use crate::fill::Rgba;
use crate::image::unpremultiply;
use crate::layer::Layer;
use crate::tile::Rgba8;

/// How a pixel canvas is painted and shrunk.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelMode {
    /// How many times bigger it's painted (1 to 16). 4 unless given.
    pub supersample: u32,
    /// A colour for a one-pixel line around everything painted.
    pub outline: Option<Rgba>,
    /// The colours every pixel snaps to (straight, not premultiplied).
    pub palette: Vec<Rgba>,
}

/// The most times bigger a pixel canvas may be painted.
pub const MAX_SUPERSAMPLE: u32 = 16;

impl PixelMode {
    /// `document`, painted `supersample` times bigger, shrunk to its real
    /// size (`width` x `height`): every layer, with its settings.
    pub fn shrink(&self, document: &Document, width: u32, height: u32) -> Document {
        let s = self.supersample.max(1);
        let mut small = Document::new(width, height);
        small.set_paper(document.paper());
        let first = small.active_id();
        for (index, layer) in document.layers().iter().enumerate() {
            let mut shrunk = small.new_layer();
            shrunk.set_props(layer.props());
            self.shrink_layer(layer, &mut shrunk, width, height, s);
            small.insert_layer(index, shrunk);
        }
        // The layer the new document started with isn't wanted.
        if document.layers().len() > 1 {
            small.remove_layer(first);
        } else if let Some(only) = small.layers().last().map(Layer::id) {
            // One layer: replace the starter with it.
            let id = only;
            small.remove_layer(first);
            small.set_active(id);
        }
        if let Some(active) = document.layer_index(document.active_id())
            && let Some(layer) = small.layers().get(active)
        {
            let id = layer.id();
            small.set_active(id);
        }
        small
    }

    fn shrink_layer(&self, big: &Layer, small: &mut Layer, width: u32, height: u32, s: u32) {
        if big.surface.painted_area().is_none() {
            return;
        }
        let samples = (s * s) as usize;
        let mut votes: Vec<usize> = vec![0; self.palette.len()];
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                votes.iter_mut().for_each(|v| *v = 0);
                let mut painted = 0;
                for sy in 0..s as i32 {
                    for sx in 0..s as i32 {
                        let pixel = big.surface.pixel(x * s as i32 + sx, y * s as i32 + sy);
                        if pixel.a >= 128 {
                            painted += 1;
                            if let Some(nearest) = self.nearest(unpremultiply(pixel)) {
                                votes[nearest] += 1;
                            }
                        }
                    }
                }
                if painted * 2 <= samples {
                    continue; // less than half painted: see-through
                }
                let winner = (0..votes.len()).max_by_key(|&i| (votes[i], usize::MAX - i));
                if let Some(i) = winner {
                    let [r, g, b, _] = self.palette[i];
                    small
                        .surface
                        .set_pixel(x, y, Rgba8::from_straight(r, g, b, 255));
                }
            }
        }
        if let Some([r, g, b, _]) = self.outline {
            let ink = Rgba8::from_straight(r, g, b, 255);
            let mut edge = Vec::new();
            for y in 0..height as i32 {
                for x in 0..width as i32 {
                    if small.surface.pixel(x, y).a > 0 {
                        continue;
                    }
                    let next_to_paint = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|&(dx, dy)| small.surface.pixel(x + dx, y + dy).a > 0);
                    if next_to_paint {
                        edge.push((x, y));
                    }
                }
            }
            for (x, y) in edge {
                small.surface.set_pixel(x, y, ink);
            }
        }
    }

    /// The palette colour nearest `color` (by red, green and blue), if
    /// the palette has any.
    fn nearest(&self, color: [u8; 4]) -> Option<usize> {
        let distance = |p: &Rgba| {
            let d = |a: u8, b: u8| {
                let d = i32::from(a) - i32::from(b);
                d * d
            };
            d(p[0], color[0]) + d(p[1], color[1]) + d(p[2], color[2])
        };
        (0..self.palette.len()).min_by_key(|&i| (distance(&self.palette[i]), i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba = [255, 0, 0, 255];
    const BLUE: Rgba = [0, 0, 255, 255];
    const INK: Rgba = [20, 20, 20, 255];

    fn mode(outline: Option<Rgba>) -> PixelMode {
        PixelMode {
            supersample: 4,
            outline,
            palette: vec![RED, BLUE, INK],
        }
    }

    /// A 16 x 16 (4 x 4 at 4 times) picture with one layer.
    fn big() -> Document {
        Document::new(16, 16)
    }

    fn fill(document: &mut Document, left: i32, top: i32, right: i32, bottom: i32, color: Rgba) {
        let pixel = Rgba8::from_straight(color[0], color[1], color[2], color[3]);
        for y in top..bottom {
            for x in left..right {
                document.active_layer_mut().surface.set_pixel(x, y, pixel);
            }
        }
    }

    #[test]
    fn more_than_half_painted_is_a_pixel_in_the_most_common_colour() {
        let mut document = big();
        // Pixel (0, 0): 10 of 16 samples red (a column of 2 is blue): red.
        fill(&mut document, 0, 0, 4, 4, RED);
        fill(&mut document, 0, 0, 1, 4, BLUE);
        fill(&mut document, 1, 0, 2, 2, BLUE);
        // Pixel (1, 0): exactly half painted: see-through.
        fill(&mut document, 4, 0, 8, 2, RED);
        // Pixel (2, 0): 9 of 16 painted, 5 blue and 4 red: blue, not a
        // purple average snapped to who knows what.
        fill(&mut document, 8, 0, 11, 3, BLUE);
        fill(&mut document, 11, 0, 12, 4, RED);
        // Pixel (3, 3): an anti-aliased shade of red snaps to red.
        fill(&mut document, 12, 12, 16, 16, [255, 60, 60, 200]);
        let small = mode(None).shrink(&document, 4, 4);
        let at = |x, y| unpremultiply(small.active_layer().surface.pixel(x, y));
        assert_eq!(at(0, 0), RED);
        assert_eq!(at(1, 0), [0, 0, 0, 0]);
        assert_eq!(at(2, 0), BLUE);
        assert_eq!(at(3, 3), RED);
        assert_eq!((small.width(), small.height()), (4, 4));
        // Every pixel is see-through or exactly a palette colour.
        for y in 0..4 {
            for x in 0..4 {
                let p = at(x, y);
                assert!(p[3] == 0 || [RED, BLUE, INK].contains(&p), "{p:?}");
            }
        }
    }

    #[test]
    fn the_outline_goes_around_what_is_painted() {
        let mut document = big();
        fill(&mut document, 4, 4, 12, 12, RED); // pixels (1..3, 1..3)
        let small = mode(Some(INK)).shrink(&document, 4, 4);
        let at = |x, y| unpremultiply(small.active_layer().surface.pixel(x, y));
        assert_eq!(at(1, 1), RED);
        assert_eq!(at(0, 1), INK);
        assert_eq!(at(1, 0), INK);
        assert_eq!(at(0, 0), [0, 0, 0, 0], "corners aren't next to paint");
    }

    #[test]
    fn layers_keep_their_settings_and_order() {
        let mut document = big();
        document
            .active_layer_mut()
            .set_props(crate::layer::LayerProps {
                name: "Bottom".into(),
                visible: true,
                opacity: 0.5,
                blend: crate::layer::BlendMode::Multiply,
            });
        let mut top = document.new_layer();
        top.set_props(crate::layer::LayerProps {
            name: "Top".into(),
            visible: false,
            opacity: 1.0,
            blend: crate::layer::BlendMode::Normal,
        });
        document.insert_layer(1, top);
        let small = mode(None).shrink(&document, 4, 4);
        let names: Vec<&str> = small.layers().iter().map(Layer::name).collect();
        assert_eq!(names, ["Bottom", "Top"]);
        assert_eq!(small.layers()[0].opacity(), 0.5);
        assert!(!small.layers()[1].visible());
        assert_eq!(small.active_layer().name(), "Bottom");
    }
}
