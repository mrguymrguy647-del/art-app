//! Colours: switching between RGB and HSV, hex codes, and what a pixel
//! looks like on the paper.
//!
//! Colours here are normal ("straight") RGB, 0..255 per channel, the way
//! people think about colours. Pixels on the canvas are premultiplied
//! (see `tile.rs`); `over_paper` turns one back into a normal colour.

use crate::tile::Rgba8;

/// A colour as hue, saturation and value (brightness), each 0..1.
///
/// - hue: where on the colour wheel (0 = red, 1/3 = green, 2/3 = blue,
///   and back to red at 1);
/// - saturation: 0 = grey, 1 = full colour;
/// - value: 0 = black, 1 = as bright as possible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub const fn new(h: f32, s: f32, v: f32) -> Self {
        Self { h, s, v }
    }

    /// The same colour as red, green, blue (0..255 each).
    pub fn to_rgb(self) -> [u8; 3] {
        let h = self.h.rem_euclid(1.0) * 6.0; // which sixth of the wheel
        let s = self.s.clamp(0.0, 1.0);
        let v = self.v.clamp(0.0, 1.0);
        let chroma = v * s; // how far from grey
        let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = v - chroma; // the grey part, added to all three
        let to_byte = |c: f32| ((c + m) * 255.0).round() as u8;
        [to_byte(r), to_byte(g), to_byte(b)]
    }

    /// The same colour as hue, saturation and value. Greys (and black)
    /// have no real hue; they get hue 0.
    pub fn from_rgb(rgb: [u8; 3]) -> Self {
        let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;

        let h = if chroma == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / chroma).rem_euclid(6.0) / 6.0
        } else if max == g {
            ((b - r) / chroma + 2.0) / 6.0
        } else {
            ((r - g) / chroma + 4.0) / 6.0
        };
        let s = if max == 0.0 { 0.0 } else { chroma / max };
        Self::new(h, s, max)
    }
}

/// The colour as a hex code, like "#FF8800".
pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// Reads a hex code like "#FF8800", "ff8800" or the short "#F80".
/// Returns `None` if it isn't a valid colour code.
pub fn from_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => {
            let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
            Some([channel(0)?, channel(2)?, channel(4)?])
        }
        // Short form: each digit is doubled, so "F80" means "FF8800".
        3 => {
            let channel = |i: usize| u8::from_str_radix(&digits[i..=i], 16).ok().map(|d| d * 17);
            Some([channel(0)?, channel(1)?, channel(2)?])
        }
        _ => None,
    }
}

/// The colour you see where a (premultiplied) `pixel` lies on the paper.
pub fn over_paper(pixel: Rgba8, paper: Rgba8) -> [u8; 3] {
    // Premultiplied "over": the pixel, plus as much paper as shows through.
    let show_through = 1.0 - f32::from(pixel.a) / 255.0;
    let mix = |top: u8, under: u8| {
        (f32::from(top) + f32::from(under) * show_through)
            .round()
            .min(255.0) as u8
    };
    [
        mix(pixel.r, paper.r),
        mix(pixel.g, paper.g),
        mix(pixel.b, paper.b),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_colours_have_the_expected_hue() {
        assert_eq!(Hsv::from_rgb([255, 0, 0]), Hsv::new(0.0, 1.0, 1.0));
        let green = Hsv::from_rgb([0, 255, 0]);
        assert!((green.h - 1.0 / 3.0).abs() < 1e-6);
        let blue = Hsv::from_rgb([0, 0, 255]);
        assert!((blue.h - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(Hsv::new(0.0, 1.0, 1.0).to_rgb(), [255, 0, 0]);
    }

    #[test]
    fn greys_have_no_saturation() {
        for grey in [0, 1, 128, 254, 255] {
            let hsv = Hsv::from_rgb([grey, grey, grey]);
            assert_eq!(hsv.s, 0.0);
            assert_eq!(hsv.to_rgb(), [grey, grey, grey]);
        }
    }

    #[test]
    fn rgb_to_hsv_and_back_gives_the_same_colour() {
        // Thousands of colours across the whole range.
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let rgb = [r as u8, g as u8, b as u8];
                    assert_eq!(Hsv::from_rgb(rgb).to_rgb(), rgb, "{rgb:?}");
                }
            }
        }
    }

    #[test]
    fn hex_codes_both_ways() {
        assert_eq!(to_hex([255, 136, 0]), "#FF8800");
        assert_eq!(from_hex("#FF8800"), Some([255, 136, 0]));
        assert_eq!(from_hex("ff8800"), Some([255, 136, 0]));
        assert_eq!(from_hex("  #f80 "), Some([255, 136, 0]));
        assert_eq!(from_hex(&to_hex([12, 34, 56])), Some([12, 34, 56]));
    }

    #[test]
    fn bad_hex_codes_are_rejected() {
        for bad in ["", "#", "#12345", "#GG0000", "red", "#1234567", "#ÿÿÿ"] {
            assert_eq!(from_hex(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn colour_over_paper() {
        let white = Rgba8::WHITE;
        assert_eq!(over_paper(Rgba8::TRANSPARENT, white), [255, 255, 255]);
        assert_eq!(over_paper(Rgba8::new(10, 20, 30, 255), white), [10, 20, 30]);
        // Half-transparent black (premultiplied: 0, 0, 0, 128) on white is grey.
        assert_eq!(over_paper(Rgba8::new(0, 0, 0, 128), white), [127, 127, 127]);
    }
}
