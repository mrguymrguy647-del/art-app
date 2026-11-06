//! Senses: tools that look at a picture's pixels and report. They never
//! change a painting. Everything here is deterministic: the same picture
//! gives the same numbers and the same view pictures.
//!
//! - [`Grey`]: a picture's lightness (luminance), and from it the squint
//!   (blurred) view.
//! - [`Heat`]: where the eye goes: contrast, edges, saturation and fine
//!   detail on a fixed grid of cells, summed with fixed weights.
//! - the views: black and white, squinted, thumbnail, flipped, the
//!   silhouettes of named parts, and the heat map as a picture.
//!
//! The grid is 32 x 32 cells whatever the picture's size, so every number
//! is comparable between pictures, and a 1920 x 1080 picture has cells of
//! 60 x 34 pixels.

use crate::fill::Mask;
use crate::filters;
use crate::image::{Color, Image};

/// The grid of cells every measurement uses: this many across and down.
pub const GRID: usize = 32;
/// The thumbnail's width in pixels: what reads at a glance.
pub const THUMB_WIDTH: u32 = 96;

/// A picture's lightness, 0 (black) to 1 (white), row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Grey {
    pub width: u32,
    pub height: u32,
    pub values: Vec<f32>,
}

/// The lightness of a colour (straight, not premultiplied), 0..1: the
/// usual weights for red, green and blue.
pub fn luminance([r, g, b, _]: Color) -> f32 {
    (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0
}

/// A colour's hue in degrees (0 = red, 120 = green, 240 = blue), its
/// saturation and its value, each 0..1 but the hue.
pub fn hsv([r, g, b, _]: Color) -> (f32, f32, f32) {
    let hsv = crate::color::Hsv::from_rgb([r, g, b]);
    (hsv.h * 360.0, hsv.s, hsv.v)
}

impl Grey {
    /// The lightness of every pixel, with see-through pixels laid on white.
    pub fn of(image: &Image) -> Grey {
        let mut values = Vec::with_capacity((image.width() * image.height()) as usize);
        for y in 0..image.height() {
            for x in 0..image.width() {
                values.push(luminance(on_white(image.pixel(x, y))));
            }
        }
        Grey {
            width: image.width(),
            height: image.height(),
            values,
        }
    }

    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.values[(y * self.width + x) as usize]
    }

    /// As a picture: grey from black to white.
    pub fn to_image(&self) -> Image {
        let mut out = Image::new(self.width, self.height, [0, 0, 0, 255]);
        for y in 0..self.height {
            for x in 0..self.width {
                let v = (self.get(x, y).clamp(0.0, 1.0) * 255.0).round() as u8;
                out.set_pixel(x as i32, y as i32, [v, v, v, 255]);
            }
        }
        out
    }

    /// Softened by about `radius` pixels (the same blur the filters use).
    pub fn blurred(&self, radius: f32) -> Grey {
        let mut pixels: Vec<[f32; 4]> = self.values.iter().map(|&v| [v, v, v, 1.0]).collect();
        filters::blur(
            &mut pixels,
            self.width as usize,
            self.height as usize,
            radius,
        );
        Grey {
            width: self.width,
            height: self.height,
            values: pixels.iter().map(|p| p[0]).collect(),
        }
    }

    /// The squint: blurred by a fortieth of the picture's width, so only
    /// the big shapes are left.
    pub fn squint(&self) -> Grey {
        self.blurred((self.width as f32 / 40.0).max(1.0))
    }

    /// The lightness range (95th minus 5th percentile) of the pixels in
    /// a box, 0..1; 0 for an empty box.
    pub fn range_in(&self, left: u32, top: u32, right: u32, bottom: u32) -> f32 {
        let mut values: Vec<f32> = Vec::new();
        for y in top..bottom.min(self.height) {
            for x in left..right.min(self.width) {
                values.push(self.get(x, y));
            }
        }
        if values.len() < 2 {
            return 0.0;
        }
        values.sort_by(|a, b| a.total_cmp(b));
        let at = |p: f32| values[((values.len() - 1) as f32 * p).round() as usize];
        (at(0.95) - at(0.05)).max(0.0)
    }
}

/// A colour laid on white: what a see-through pixel looks like on paper.
pub fn on_white([r, g, b, a]: Color) -> Color {
    if a == 255 {
        return [r, g, b, 255];
    }
    let mix = |c: u8| ((u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8;
    [mix(r), mix(g), mix(b), 255]
}

/// The picture mirrored left to right.
pub fn flipped(image: &Image) -> Image {
    let mut out = Image::new(image.width(), image.height(), [0, 0, 0, 0]);
    for y in 0..image.height() {
        for x in 0..image.width() {
            out.set_pixel((image.width() - 1 - x) as i32, y as i32, image.pixel(x, y));
        }
    }
    out
}

/// The picture shrunk to `THUMB_WIDTH` wide (averaging the pixels of
/// each cell), on white.
pub fn thumbnail(image: &Image) -> Image {
    let height = ((u64::from(image.height()) * u64::from(THUMB_WIDTH)) / u64::from(image.width()))
        .max(1) as u32;
    image
        .on_color([255, 255, 255, 255])
        .fit_within(THUMB_WIDTH, height.max(1))
}

/// One cell of the heat grid, each part 0..1 over the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeatCell {
    /// The lightness range in the cell.
    pub contrast: f32,
    /// How strong the edges are (the mean gradient).
    pub edges: f32,
    /// How colourful (saturation times value).
    pub saturation: f32,
    /// Fine detail: what the squint takes away.
    pub detail: f32,
    /// The weighted sum, smoothed over the neighbours.
    pub heat: f32,
}

/// Where the eye goes: a `GRID` x `GRID` map of cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Heat {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<HeatCell>,
    /// The hottest cell, as (column, row).
    pub peak: (usize, usize),
}

/// The fixed weights of the four parts of the heat.
pub const HEAT_WEIGHTS: [f32; 4] = [0.35, 0.3, 0.15, 0.2];

impl Heat {
    pub fn of(image: &Image) -> Heat {
        let grey = Grey::of(image);
        Heat::of_grey(image, &grey, &grey.squint())
    }

    /// With the grey and squinted pictures already made.
    pub fn of_grey(image: &Image, grey: &Grey, squint: &Grey) -> Heat {
        let (w, h) = (image.width(), image.height());
        let mut cells = vec![HeatCell::default(); GRID * GRID];
        for (i, cell) in cells.iter_mut().enumerate() {
            let (left, top, right, bottom) = cell_box(i, w, h);
            cell.contrast = grey.range_in(left, top, right, bottom);
            let (mut edge_sum, mut sat_sum, mut detail_sum, mut count) = (0.0, 0.0, 0.0, 0.0);
            for y in top..bottom {
                for x in left..right {
                    // Sobel-like: the differences to the next pixels.
                    let gx = if x + 1 < w {
                        grey.get(x + 1, y) - grey.get(x, y)
                    } else {
                        0.0
                    };
                    let gy = if y + 1 < h {
                        grey.get(x, y + 1) - grey.get(x, y)
                    } else {
                        0.0
                    };
                    edge_sum += gx.hypot(gy);
                    let (_, s, v) = hsv(on_white(image.pixel(x, y)));
                    sat_sum += s * v;
                    detail_sum += (grey.get(x, y) - squint.get(x, y)).abs();
                    count += 1.0;
                }
            }
            if count > 0.0 {
                cell.edges = edge_sum / count;
                cell.saturation = sat_sum / count;
                cell.detail = detail_sum / count;
            }
        }
        // Each part 0..1 over the picture (its biggest cell is 1).
        let mut normalise = |pick: fn(&HeatCell) -> f32, set: fn(&mut HeatCell, f32)| {
            let most = cells.iter().map(pick).fold(0.0f32, f32::max);
            if most > 0.0 {
                for cell in cells.iter_mut() {
                    set(cell, pick(cell) / most);
                }
            }
        };
        normalise(|c| c.contrast, |c, v| c.contrast = v);
        normalise(|c| c.edges, |c, v| c.edges = v);
        normalise(|c| c.saturation, |c, v| c.saturation = v);
        normalise(|c| c.detail, |c, v| c.detail = v);
        let raw: Vec<f32> = cells
            .iter()
            .map(|c| {
                HEAT_WEIGHTS[0] * c.contrast
                    + HEAT_WEIGHTS[1] * c.edges
                    + HEAT_WEIGHTS[2] * c.saturation
                    + HEAT_WEIGHTS[3] * c.detail
            })
            .collect();
        // Smoothed over the 8 neighbours (the cell itself counts double).
        let mut peak = (0, 0);
        let mut hottest = -1.0;
        for row in 0..GRID {
            for column in 0..GRID {
                let (mut sum, mut n) = (0.0, 0.0);
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let (c, r) = (column as i32 + dx, row as i32 + dy);
                        if c < 0 || r < 0 || c >= GRID as i32 || r >= GRID as i32 {
                            continue;
                        }
                        let weight = if dx == 0 && dy == 0 { 2.0 } else { 1.0 };
                        sum += raw[r as usize * GRID + c as usize] * weight;
                        n += weight;
                    }
                }
                let heat = sum / n;
                cells[row * GRID + column].heat = heat;
                if heat > hottest {
                    hottest = heat;
                    peak = (column, row);
                }
            }
        }
        Heat {
            width: w,
            height: h,
            cells,
            peak,
        }
    }

    pub fn cell(&self, column: usize, row: usize) -> &HeatCell {
        &self.cells[row * GRID + column]
    }

    /// The cell a canvas point is in.
    pub fn cell_at(&self, x: f32, y: f32) -> (usize, usize) {
        let column = ((x / self.width as f32) * GRID as f32)
            .floor()
            .clamp(0.0, (GRID - 1) as f32);
        let row = ((y / self.height as f32) * GRID as f32)
            .floor()
            .clamp(0.0, (GRID - 1) as f32);
        (column as usize, row as usize)
    }

    /// The middle of a cell, in canvas pixels.
    pub fn centre_of(&self, column: usize, row: usize) -> (f32, f32) {
        (
            (column as f32 + 0.5) * self.width as f32 / GRID as f32,
            (row as f32 + 0.5) * self.height as f32 / GRID as f32,
        )
    }

    /// Where the heat's weight sits, as fractions of the width and height.
    pub fn centre_of_weight(&self) -> (f32, f32) {
        let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
        for row in 0..GRID {
            for column in 0..GRID {
                let heat = self.cell(column, row).heat;
                sx += (column as f32 + 0.5) * heat;
                sy += (row as f32 + 0.5) * heat;
                total += heat;
            }
        }
        if total <= 0.0 {
            return (0.5, 0.5);
        }
        (sx / total / GRID as f32, sy / total / GRID as f32)
    }

    /// The heat as a picture the size of the original: dark blue (cold)
    /// through red and yellow to white (hot), one block per cell, with
    /// a ring around `focal` if given.
    pub fn to_image(&self, focal: Option<(f32, f32)>) -> Image {
        let mut out = Image::new(self.width, self.height, [0, 0, 0, 255]);
        let most = self
            .cells
            .iter()
            .map(|c| c.heat)
            .fold(0.0f32, f32::max)
            .max(1e-6);
        for (i, cell) in self.cells.iter().enumerate() {
            let (left, top, right, bottom) = cell_box(i, self.width, self.height);
            let color = heat_color(cell.heat / most);
            out.fill_rect(left as i32, top as i32, right - left, bottom - top, color);
        }
        if let Some((fx, fy)) = focal {
            let radius = (self.width.min(self.height) as f32 / 16.0).max(6.0);
            for ring in [radius, radius + 1.0, radius + 2.0] {
                let steps = (ring * 8.0) as u32;
                for i in 0..steps {
                    let angle = i as f32 / steps as f32 * std::f32::consts::TAU;
                    let x = fx + angle.cos() * ring;
                    let y = fy + angle.sin() * ring;
                    out.set_pixel(x.round() as i32, y.round() as i32, [255, 255, 255, 255]);
                    out.set_pixel(x.round() as i32, y.round() as i32 - 3, [0, 0, 0, 255]);
                }
            }
        }
        out
    }
}

/// The pixels of cell number `i` of the grid, on a picture this size:
/// (left, top, right, bottom), right and bottom excluded.
pub fn cell_box(i: usize, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let (column, row) = ((i % GRID) as u64, (i / GRID) as u64);
    let (w, h, g) = (u64::from(width), u64::from(height), GRID as u64);
    (
        (column * w / g) as u32,
        (row * h / g) as u32,
        ((column + 1) * w / g) as u32,
        ((row + 1) * h / g) as u32,
    )
}

/// Cold to hot: dark blue, blue, red, yellow, white.
fn heat_color(t: f32) -> Color {
    let stops: [(f32, [f32; 3]); 5] = [
        (0.0, [10.0, 10.0, 50.0]),
        (0.3, [30.0, 60.0, 200.0]),
        (0.6, [220.0, 50.0, 40.0]),
        (0.85, [255.0, 220.0, 60.0]),
        (1.0, [255.0, 255.0, 255.0]),
    ];
    let t = t.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        let (a, ca) = pair[0];
        let (b, cb) = pair[1];
        if t <= b {
            let w = ((t - a) / (b - a)).clamp(0.0, 1.0);
            let mix = |i: usize| (ca[i] + (cb[i] - ca[i]) * w).round() as u8;
            return [mix(0), mix(1), mix(2), 255];
        }
    }
    [255, 255, 255, 255]
}

/// A fixed list of flat colours for silhouettes, one per part, repeating.
pub const SILHOUETTE_COLORS: [Color; 8] = [
    [30, 30, 30, 255],
    [200, 50, 50, 255],
    [40, 110, 220, 255],
    [40, 160, 70, 255],
    [230, 150, 30, 255],
    [150, 60, 180, 255],
    [20, 160, 170, 255],
    [120, 90, 50, 255],
];

/// The named parts' silhouettes, each filled solid in its own colour
/// (`SILHOUETTE_COLORS`, in order), on white; later parts over earlier
/// ones. `masks` are in canvas pixels.
pub fn silhouettes(width: u32, height: u32, masks: &[Mask]) -> Image {
    let mut out = Image::new(width, height, [255, 255, 255, 255]);
    for (i, mask) in masks.iter().enumerate() {
        let color = SILHOUETTE_COLORS[i % SILHOUETTE_COLORS.len()];
        let rect = mask.rect();
        for y in rect.top.max(0)..rect.bottom.min(height as i32) {
            for x in rect.left.max(0)..rect.right.min(width as i32) {
                if mask.get(x, y) >= 128 {
                    out.set_pixel(x, y, color);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Image {
        // Grey all over, a black and white checker in one corner.
        let mut image = Image::new(128, 96, [128, 128, 128, 255]);
        for y in 0..24 {
            for x in 0..24 {
                let v = if (x / 4 + y / 4) % 2 == 0 { 0 } else { 255 };
                image.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        image
    }

    #[test]
    fn grey_squint_flip_and_thumbnail() {
        let image = picture();
        let grey = Grey::of(&image);
        assert_eq!(grey.get(100, 50), 128.0 / 255.0);
        assert_eq!(luminance([255, 255, 255, 255]), 1.0);
        assert!((luminance([0, 255, 0, 255]) - 0.7152).abs() < 1e-5);
        assert_eq!(on_white([0, 0, 0, 0]), [255, 255, 255, 255]);
        let squint = grey.squint();
        assert!(
            squint.range_in(0, 0, 24, 24) < grey.range_in(0, 0, 24, 24),
            "softer"
        );
        assert!(
            (squint.get(100, 50) - 128.0 / 255.0).abs() < 0.01,
            "flat stays flat"
        );
        let flip = flipped(&image);
        assert_eq!(flip.pixel(127, 0), image.pixel(0, 0));
        assert_eq!(flip.pixel(0, 50), image.pixel(127, 50));
        let thumb = thumbnail(&image);
        assert_eq!((thumb.width(), thumb.height()), (96, 72));
        assert_eq!(grey.to_image().pixel(100, 50), [128, 128, 128, 255]);
        // Deterministic.
        assert_eq!(Grey::of(&image), grey);
    }

    #[test]
    fn the_heat_peaks_where_contrast_and_edges_are() {
        let image = picture();
        let heat = Heat::of(&image);
        let (column, row) = heat.peak;
        assert!(
            column <= 6 && row <= 8,
            "the checker corner: {column}, {row}"
        );
        let hot = heat.cell(column, row).heat;
        let cold = heat.cell(20, 20).heat;
        assert!(hot > cold * 3.0, "{hot} vs {cold}");
        assert_eq!(heat.cell(20, 20).contrast, 0.0);
        let (cx, cy) = heat.centre_of_weight();
        assert!(
            cx < 0.5 && cy < 0.5,
            "weight towards the corner: {cx}, {cy}"
        );
        assert_eq!(heat.cell_at(0.0, 0.0), (0, 0));
        assert_eq!(heat.cell_at(127.9, 95.9), (31, 31));
        let picture = heat.to_image(Some((64.0, 48.0)));
        assert_eq!((picture.width(), picture.height()), (128, 96));
        assert_eq!(Heat::of(&image), heat, "deterministic");
        assert_eq!(cell_box(0, 1920, 1080), (0, 0, 60, 33));
        assert_eq!(
            cell_box(GRID * GRID - 1, 1920, 1080),
            (1860, 1046, 1920, 1080)
        );
    }

    #[test]
    fn silhouettes_fill_each_part_in_its_own_colour() {
        let mut a = Mask::new(crate::surface::PixelRect::new(0, 0, 10, 10));
        a.set(2, 2, 255);
        let mut b = Mask::new(crate::surface::PixelRect::new(5, 5, 20, 20));
        b.set(8, 8, 200);
        b.set(9, 9, 10);
        let image = silhouettes(20, 20, &[a, b]);
        assert_eq!(image.pixel(2, 2), SILHOUETTE_COLORS[0]);
        assert_eq!(image.pixel(8, 8), SILHOUETTE_COLORS[1]);
        assert_eq!(image.pixel(9, 9), [255, 255, 255, 255], "below half: out");
        assert_eq!(image.pixel(15, 15), [255, 255, 255, 255]);
    }
}
