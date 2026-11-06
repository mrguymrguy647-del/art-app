//! Studying pictures: a palette drawn out of any picture, its values in
//! five greys, and a comparison of two pictures by value structure and
//! colour. For learning from reference pictures (and for checking a
//! study against the picture it studies). All deterministic: no random
//! starting points, fixed bands, fixed grids.

use crate::image::{Color, Image};
use crate::senses::{hsv, luminance, on_white};

/// Pictures are shrunk to this width before their colours are counted.
pub const PALETTE_WIDTH: u32 = 256;
/// How many colours a palette has unless asked otherwise.
pub const PALETTE_COLORS: usize = 8;
/// The values picture has this many greys.
pub const VALUE_BANDS: usize = 5;
/// The grey each band is drawn in, darkest first.
pub const BAND_GREYS: [u8; VALUE_BANDS] = [0, 64, 128, 192, 255];
/// Pictures are shrunk to this width before they're compared.
pub const COMPARE_WIDTH: u32 = 192;
/// The grid of mean values a comparison uses: this many across and down.
pub const COMPARE_GRID: usize = 4;

// ---- Palette ------------------------------------------------------------------------

/// The picture's main colours by median cut, with each one's share of
/// the pixels, biggest share first. See-through pixels are laid on white.
pub fn palette_of(image: &Image, colors: usize) -> Vec<(Color, f32)> {
    let small = image.fit_within(PALETTE_WIDTH, u32::MAX);
    let mut pixels: Vec<[u8; 3]> = Vec::with_capacity((small.width() * small.height()) as usize);
    for y in 0..small.height() {
        for x in 0..small.width() {
            let [r, g, b, _] = on_white(small.pixel(x, y));
            pixels.push([r, g, b]);
        }
    }
    if pixels.is_empty() {
        return Vec::new();
    }
    let total = pixels.len() as f32;
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![pixels];
    while boxes.len() < colors.max(1) {
        // The box with the widest channel range splits at its median on
        // that channel (ties: red before green before blue, the earlier
        // box first); a box of one colour can't split.
        let mut widest: Option<(usize, usize, u8)> = None; // (box, channel, range)
        for (i, b) in boxes.iter().enumerate() {
            for channel in 0..3 {
                let (lo, hi) = b.iter().fold((255u8, 0u8), |(lo, hi), p| {
                    (lo.min(p[channel]), hi.max(p[channel]))
                });
                let range = hi.saturating_sub(lo);
                if b.len() > 1 && widest.is_none_or(|(_, _, r)| range > r) {
                    widest = Some((i, channel, range));
                }
            }
        }
        let Some((i, channel, range)) = widest else {
            break;
        };
        if range == 0 {
            break;
        }
        let mut b = boxes.remove(i);
        b.sort_by_key(|p| (p[channel], p[0], p[1], p[2]));
        let half = b.len() / 2;
        let rest = b.split_off(half);
        boxes.insert(i, rest);
        boxes.insert(i, b);
    }
    let mean_of = |b: &[[u8; 3]]| -> [u8; 3] {
        let n = b.len().max(1) as u64;
        let sum = b.iter().fold([0u64; 3], |s, p| {
            [
                s[0] + u64::from(p[0]),
                s[1] + u64::from(p[1]),
                s[2] + u64::from(p[2]),
            ]
        });
        sum.map(|s| ((s + n / 2) / n) as u8)
    };
    let means: Vec<[u8; 3]> = boxes
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| mean_of(b))
        .collect();
    // Median cut always splits in half, so the boxes' shares say little.
    // One pass of giving every pixel to its nearest colour makes the
    // shares (and the colours) honest, and stays deterministic.
    let mut members: Vec<Vec<[u8; 3]>> = vec![Vec::new(); means.len()];
    for pixel in boxes.iter().flatten() {
        let nearest = means
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| {
                (0..3)
                    .map(|c| (i32::from(pixel[c]) - i32::from(m[c])).pow(2))
                    .sum::<i32>()
            })
            .map_or(0, |(i, _)| i);
        members[nearest].push(*pixel);
    }
    let mut out: Vec<(Color, f32)> = members
        .iter()
        .filter(|b| !b.is_empty())
        .map(|b| {
            let [r, g, bl] = mean_of(b);
            ([r, g, bl, 255], b.len() as f32 / total)
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    out
}

/// `#RRGGBB` for a colour.
pub fn hex_of(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

// ---- Values ---------------------------------------------------------------------------

/// The band (0 = darkest) of a luminance 0..1.
pub fn band_of(value: f32) -> usize {
    ((value.clamp(0.0, 1.0) * VALUE_BANDS as f32).floor() as usize).min(VALUE_BANDS - 1)
}

/// The picture in five greys, and each band's share of the pixels.
pub fn values_of(image: &Image) -> (Image, [f32; VALUE_BANDS]) {
    let mut out = Image::new(image.width(), image.height(), [0, 0, 0, 255]);
    let mut counts = [0usize; VALUE_BANDS];
    for y in 0..image.height() {
        for x in 0..image.width() {
            let band = band_of(luminance(on_white(image.pixel(x, y))));
            counts[band] += 1;
            let g = BAND_GREYS[band];
            out.set_pixel(x as i32, y as i32, [g, g, g, 255]);
        }
    }
    let total = (image.width() * image.height()).max(1) as f32;
    (out, counts.map(|c| c as f32 / total))
}

// ---- Compare --------------------------------------------------------------------------

/// How two pictures compare, 0 (nothing alike) to 100 (the same).
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// Half the overlap of the five-grey shares, half the agreement of
    /// the two 4 x 4 grids of mean values.
    pub values: u32,
    /// The overlap of the saturation-weighted hue histograms.
    pub colour: u32,
    /// Each picture's warm share of its colour (hues 330 to 60).
    pub warm: (f32, f32),
    /// Each picture's mean value.
    pub mean_value: (f32, f32),
}

impl Comparison {
    /// "values similar: 78; colours different: 41, the first is cooler".
    pub fn words(&self) -> String {
        let judge = |score: u32| {
            if score >= 70 {
                "similar"
            } else if score >= 40 {
                "somewhat alike"
            } else {
                "different"
            }
        };
        let mut text = format!(
            "values {}: {}; colours {}: {}",
            judge(self.values),
            self.values,
            judge(self.colour),
            self.colour
        );
        let (a, b) = self.warm;
        if (a - b).abs() >= 0.15 {
            text.push_str(if a > b {
                ", the first is warmer"
            } else {
                ", the first is cooler"
            });
        }
        let (a, b) = self.mean_value;
        if (a - b).abs() >= 0.1 {
            text.push_str(if a > b {
                ", the first is lighter"
            } else {
                ", the first is darker"
            });
        }
        text
    }
}

/// What a comparison measures of one picture.
struct Measured {
    bands: [f32; VALUE_BANDS],
    grid: [f32; COMPARE_GRID * COMPARE_GRID],
    hues: [f32; 24],
    warm: f32,
    mean: f32,
}

fn measure(image: &Image) -> Measured {
    let small = image.fit_within(COMPARE_WIDTH, u32::MAX);
    let (w, h) = (small.width(), small.height());
    let (_, bands) = values_of(&small);
    let mut grid = [0.0f32; COMPARE_GRID * COMPARE_GRID];
    let mut counts = [0u32; COMPARE_GRID * COMPARE_GRID];
    let mut hues = [0.0f32; 24];
    let (mut warm, mut all, mut sum) = (0.0f32, 0.0f32, 0.0f32);
    for y in 0..h {
        for x in 0..w {
            let color = on_white(small.pixel(x, y));
            let v = luminance(color);
            sum += v;
            let column = (u64::from(x) * COMPARE_GRID as u64 / u64::from(w)) as usize;
            let row = (u64::from(y) * COMPARE_GRID as u64 / u64::from(h)) as usize;
            let cell = row.min(COMPARE_GRID - 1) * COMPARE_GRID + column.min(COMPARE_GRID - 1);
            grid[cell] += v;
            counts[cell] += 1;
            let (hue, s, value) = hsv(color);
            if s >= 0.15 && value >= 0.1 {
                hues[((hue / 15.0).floor() as usize).min(23)] += s;
                all += s;
                let d = (hue - 15.0).rem_euclid(360.0);
                if d.min(360.0 - d) <= 45.0 {
                    warm += s;
                }
            }
        }
    }
    for (cell, count) in grid.iter_mut().zip(counts) {
        if count > 0 {
            *cell /= count as f32;
        }
    }
    if all > 0.0 {
        for bin in hues.iter_mut() {
            *bin /= all;
        }
    }
    Measured {
        bands,
        grid,
        hues,
        warm: if all > 0.0 { warm / all } else { 0.0 },
        mean: sum / (w * h).max(1) as f32,
    }
}

/// How much two histograms (each summing to 1) overlap, 0..1.
fn intersection(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.min(*y))
        .sum::<f32>()
        .clamp(0.0, 1.0)
}

/// Pearson's correlation of two grids, or 0 when one is flat.
fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    if saa <= 1e-9 || sbb <= 1e-9 {
        return 0.0;
    }
    sab / (saa * sbb).sqrt()
}

/// Compares two pictures (both shrunk to `COMPARE_WIDTH` first).
pub fn compare(a: &Image, b: &Image) -> Comparison {
    let (ma, mb) = (measure(a), measure(b));
    let shares = intersection(&ma.bands, &mb.bands);
    let shapes = (correlation(&ma.grid, &mb.grid) + 1.0) / 2.0;
    let values = ((shares * 0.5 + shapes * 0.5) * 100.0).round() as u32;
    let colour = (intersection(&ma.hues, &mb.hues) * 100.0).round() as u32;
    Comparison {
        values: values.min(100),
        colour: colour.min(100),
        warm: (ma.warm, mb.warm),
        mean_value: (ma.mean, mb.mean),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_tone() -> Image {
        let mut image = Image::new(64, 32, [200, 40, 40, 255]);
        image.fill_rect(32, 0, 32, 32, [40, 40, 200, 255]);
        image
    }

    #[test]
    fn the_palette_finds_the_main_colours_biggest_first() {
        let mut image = two_tone();
        image.fill_rect(0, 0, 16, 32, [255, 255, 255, 255]);
        let palette = palette_of(&image, 3);
        assert_eq!(palette.len(), 3);
        assert_eq!(palette[0].0, [40, 40, 200, 255]);
        assert!((palette[0].1 - 0.5).abs() < 0.01);
        assert_eq!(hex_of(palette[0].0), "#2828C8");
        let one = palette_of(&Image::new(8, 8, [10, 20, 30, 255]), 8);
        assert_eq!(one.len(), 1, "a flat picture has one colour");
        assert_eq!(palette_of(&image, 3), palette, "deterministic");
    }

    #[test]
    fn values_fall_into_five_bands() {
        let mut image = Image::new(10, 10, [255, 255, 255, 255]);
        image.fill_rect(0, 0, 5, 10, [0, 0, 0, 255]);
        let (greys, shares) = values_of(&image);
        assert_eq!(greys.pixel(0, 0), [0, 0, 0, 255]);
        assert_eq!(greys.pixel(9, 9), [255, 255, 255, 255]);
        assert_eq!(shares, [0.5, 0.0, 0.0, 0.0, 0.5]);
        assert_eq!(band_of(0.5), 2);
    }

    #[test]
    fn a_picture_matches_itself_and_not_its_opposite() {
        let image = two_tone();
        let same = compare(&image, &image);
        assert_eq!((same.values, same.colour), (100, 100));
        let mut other = Image::new(64, 32, [40, 200, 40, 255]);
        other.fill_rect(0, 0, 32, 32, [230, 230, 230, 255]);
        let different = compare(&image, &other);
        assert!(different.colour < 10, "{different:?}");
        assert!(different.values < same.values, "{different:?}");
        let words = different.words();
        assert!(words.contains("colours different"), "{words}");
        assert!(words.contains("the first is warmer"), "{words}");
        assert!(words.contains("the first is darker"), "{words}");
    }
}
