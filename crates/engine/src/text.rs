//! Text: letters drawn from a font that comes with Duck Quill.
//!
//! The font is Inter (Regular) by The Inter Project Authors, under the SIL
//! Open Font License (see `assets/fonts/OFL.txt`). Because it's built in,
//! text looks the same on every computer and needs no installed fonts.
//!
//! A font stores each letter as outlines made of curves. Here they're cut
//! into short straight pieces (polygons) and laid out in lines, so the
//! painting script can fill them like any other shape: smooth edges, and
//! they move, turn and mirror inside groups like everything else.

use std::sync::OnceLock;

use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

/// The font file, built into the program.
static FONT_DATA: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.otf");

/// Curves are cut into straight pieces about this long (in canvas pixels).
const PIECE: f32 = 1.0;
/// A tab is as wide as this many spaces.
const TAB_SPACES: usize = 4;

/// The font, read once.
fn font() -> &'static Face<'static> {
    static FONT: OnceLock<Face<'static>> = OnceLock::new();
    FONT.get_or_init(|| Face::parse(FONT_DATA, 0).expect("the built-in font is a valid font"))
}

/// Where each line goes, compared with the text's anchor point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Lines start at the anchor.
    Left,
    /// Lines are centred on the anchor.
    Center,
    /// Lines end at the anchor.
    Right,
}

/// Text laid out and ready to fill.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextShape {
    /// Every letter's outlines, as closed polygons in pixels. (0, 0) is
    /// the anchor: the top of the first line, at the left, middle or right
    /// of each line depending on the alignment. y grows downwards.
    pub contours: Vec<Vec<(f32, f32)>>,
    /// The widest line, in pixels.
    pub width: f32,
    /// From the top of the first line to the bottom of the last, in pixels.
    pub height: f32,
}

/// Lays out `text` (lines separated by "\n") at `size` pixels: the font
/// size, roughly the height of a line. `detail` is how many canvas pixels
/// one pixel here will become (so curves are cut finely enough).
pub fn layout(text: &str, size: f32, align: Align, detail: f32) -> TextShape {
    let face = font();
    let scale = size / f32::from(face.units_per_em());
    let ascender = f32::from(face.ascender()) * scale;
    let line_height = (f32::from(face.ascender()) - f32::from(face.descender())
        + f32::from(face.line_gap()))
        * scale;
    let mut shape = TextShape::default();
    let text = text
        .replace("\r\n", "\n")
        .replace('\t', &" ".repeat(TAB_SPACES));
    let lines: Vec<&str> = text.split('\n').collect();

    for (line_number, line) in lines.iter().enumerate() {
        // First the letters and how far apart they are.
        let glyphs: Vec<GlyphId> = line
            .chars()
            .filter(|c| !c.is_control())
            .map(|c| face.glyph_index(c).unwrap_or(GlyphId(0)))
            .collect();
        let mut positions = Vec::with_capacity(glyphs.len());
        let mut pen = 0.0;
        for (i, &glyph) in glyphs.iter().enumerate() {
            if i > 0 {
                pen += f32::from(kerning(face, glyphs[i - 1], glyph)) * scale;
            }
            positions.push(pen);
            pen += f32::from(face.glyph_hor_advance(glyph).unwrap_or(0)) * scale;
        }
        let width = pen;
        shape.width = shape.width.max(width);
        let start = match align {
            Align::Left => 0.0,
            Align::Center => -width / 2.0,
            Align::Right => -width,
        };
        let baseline = ascender + line_number as f32 * line_height;

        // Then each letter's outlines, in place.
        for (&glyph, &x) in glyphs.iter().zip(&positions) {
            let mut outlines = Outlines {
                contours: Vec::new(),
                current: Vec::new(),
                origin: (start + x, baseline),
                scale,
                detail,
            };
            face.outline_glyph(glyph, &mut outlines);
            outlines.close();
            shape.contours.extend(outlines.contours);
        }
    }
    shape.height = lines.len() as f32 * line_height;
    shape
}

/// How much closer together (negative) or further apart two letters go,
/// from the font's "kern" feature, in font units.
fn kerning(face: &Face, first: GlyphId, second: GlyphId) -> i16 {
    let Some(gpos) = face.tables().gpos else {
        return 0;
    };
    let Some(feature) = gpos.features.find(Tag::from_bytes(b"kern")) else {
        return 0;
    };
    let mut total = 0;
    for lookup_index in feature.lookup_indices {
        let Some(lookup) = gpos.lookups.get(lookup_index) else {
            continue;
        };
        // In each lookup, the first part that knows this pair decides.
        for subtable in lookup.subtables.into_iter::<PositioningSubtable>() {
            let PositioningSubtable::Pair(pair) = subtable else {
                continue;
            };
            let found = match pair {
                PairAdjustment::Format1 { coverage, sets } => coverage
                    .get(first)
                    .and_then(|index| sets.get(index))
                    .and_then(|set| set.get(second)),
                PairAdjustment::Format2 {
                    coverage,
                    classes,
                    matrix,
                } => coverage
                    .get(first)
                    .and_then(|_| matrix.get((classes.0.get(first), classes.1.get(second)))),
            };
            if let Some((value, _)) = found {
                total += value.x_advance;
                break;
            }
        }
    }
    total
}

/// Collects a letter's outlines as polygons, in pixels.
struct Outlines {
    contours: Vec<Vec<(f32, f32)>>,
    current: Vec<(f32, f32)>,
    /// Where the letter's origin goes (on the baseline).
    origin: (f32, f32),
    scale: f32,
    detail: f32,
}

impl Outlines {
    /// A point in font units, in pixels (fonts measure upwards).
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.origin.0 + x * self.scale,
            self.origin.1 - y * self.scale,
        )
    }

    /// How many straight pieces a curve this long (in font units) needs.
    fn pieces(&self, length: f32) -> usize {
        ((length * self.scale * self.detail / PIECE).ceil() as usize).clamp(1, 64)
    }

    fn last(&self) -> (f32, f32) {
        self.current.last().copied().unwrap_or(self.origin)
    }
}

impl OutlineBuilder for Outlines {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let point = self.at(x, y);
        self.current.push(point);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = self.at(x, y);
        self.current.push(point);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.last(), self.at(x1, y1), self.at(x, y));
        let length = (distance(p0, p1) + distance(p1, p2)) / self.scale;
        let pieces = self.pieces(length);
        for i in 1..=pieces {
            let t = i as f32 / pieces as f32;
            let u = 1.0 - t;
            self.current.push((
                u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
            ));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.last(), self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        let length = (distance(p0, p1) + distance(p1, p2) + distance(p2, p3)) / self.scale;
        let pieces = self.pieces(length);
        for i in 1..=pieces {
            let t = i as f32 / pieces as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.current.push((
                a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
                a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
            ));
        }
    }

    fn close(&mut self) {
        if self.current.len() >= 3 {
            self.contours.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(shape: &TextShape) -> (f32, f32, f32, f32) {
        shape.contours.iter().flatten().fold(
            (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
            |(l, t, r, b), &(x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
        )
    }

    #[test]
    fn every_printable_ascii_character_has_a_shape_except_space() {
        for code in 0x20u8..=0x7E {
            let c = char::from(code);
            let shape = layout(&c.to_string(), 32.0, Align::Left, 1.0);
            if c == ' ' {
                assert!(shape.contours.is_empty(), "a space draws nothing");
                assert!(shape.width > 0.0, "but takes room");
            } else {
                assert!(!shape.contours.is_empty(), "{c:?} has no outline");
            }
        }
    }

    #[test]
    fn size_and_lines() {
        let one = layout("Hg", 40.0, Align::Left, 1.0);
        let (_, top, _, bottom) = bounds(&one);
        // The capital H starts a little below the top of the line, and
        // the g hangs below the baseline, but all within the line.
        assert!(top > 0.0 && top < 12.0, "{top}");
        assert!(
            bottom < one.height && bottom > 35.0,
            "{bottom} of {}",
            one.height
        );
        let two = layout("Hg\nHg", 40.0, Align::Left, 1.0);
        assert!((two.height - 2.0 * one.height).abs() < 1e-3);
        assert!(
            layout("MM", 40.0, Align::Left, 1.0).width > layout("ii", 40.0, Align::Left, 1.0).width
        );
    }

    #[test]
    fn alignment_moves_lines_around_the_anchor() {
        let left = bounds(&layout("Duck", 40.0, Align::Left, 1.0));
        let centre = bounds(&layout("Duck", 40.0, Align::Center, 1.0));
        let right = bounds(&layout("Duck", 40.0, Align::Right, 1.0));
        assert!(left.0 >= 0.0 && left.0 < 5.0);
        assert!(
            (centre.0 + centre.2).abs() < 3.0,
            "centred on 0: {centre:?}"
        );
        assert!(right.2 <= 0.5 && right.2 > -5.0, "{right:?}");
    }

    #[test]
    fn letter_pairs_like_av_are_kerned_closer() {
        let face = font();
        let (a, v) = (
            face.glyph_index('A').unwrap(),
            face.glyph_index('V').unwrap(),
        );
        assert!(kerning(face, a, v) < 0, "A and V tuck together");
        let together = layout("AV", 100.0, Align::Left, 1.0).width;
        let apart =
            layout("A", 100.0, Align::Left, 1.0).width + layout("V", 100.0, Align::Left, 1.0).width;
        assert!(together < apart);
    }
}
