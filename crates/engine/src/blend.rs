//! Blend modes: how a layer's colours mix with the picture below it.
//!
//! "Normal" simply covers what's below. The other modes mix the two
//! colours with a small formula, for example Multiply (darkens, good for
//! shadows), Screen (lightens, good for light and glow) or Colour (gives
//! the picture below this layer's colour but keeps its light and dark).
//!
//! The formulas are the ones in the public W3C "Compositing and Blending
//! Level 1" standard (the same ones web browsers use), plus "Add".

use crate::tile::Rgba8;

/// How a layer is mixed with the layers below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// The layer simply covers what's below, as much as its opacity says.
    #[default]
    Normal,
    /// Multiplies the colours: always darker, like layers of tinted glass.
    /// White changes nothing. Good for shadows.
    Multiply,
    /// The opposite of Multiply: always lighter. Black changes nothing.
    /// Good for light and glow.
    Screen,
    /// Multiply in the dark parts below, Screen in the light parts: more contrast.
    Overlay,
    /// Keeps whichever is darker.
    Darken,
    /// Keeps whichever is lighter.
    Lighten,
    /// Brightens what's below, strongly: for shine and fire.
    ColorDodge,
    /// Darkens what's below, strongly, with more colour.
    ColorBurn,
    /// Like Overlay, but decided by this layer's colour instead of the one below.
    HardLight,
    /// A gentle Overlay: soft light and shade.
    SoftLight,
    /// The difference between the colours: same colours turn black.
    Difference,
    /// Like Difference, with less contrast.
    Exclusion,
    /// Adds the colours together (brighter, up to white): for glows and sparks.
    Add,
    /// This layer's hue, with the saturation and lightness of what's below.
    Hue,
    /// This layer's saturation (vividness), with the rest from below.
    Saturation,
    /// This layer's hue and saturation, with the lightness of what's below:
    /// for colouring a grey drawing.
    Color,
    /// This layer's lightness, with the colour of what's below.
    Luminosity,
}

impl BlendMode {
    /// Every blend mode, in the order shown in menus.
    pub const ALL: [BlendMode; 17] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Add,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// The name used in files and painting scripts, e.g. "color-dodge".
    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Multiply => "multiply",
            BlendMode::Screen => "screen",
            BlendMode::Overlay => "overlay",
            BlendMode::Darken => "darken",
            BlendMode::Lighten => "lighten",
            BlendMode::ColorDodge => "color-dodge",
            BlendMode::ColorBurn => "color-burn",
            BlendMode::HardLight => "hard-light",
            BlendMode::SoftLight => "soft-light",
            BlendMode::Difference => "difference",
            BlendMode::Exclusion => "exclusion",
            BlendMode::Add => "add",
            BlendMode::Hue => "hue",
            BlendMode::Saturation => "saturation",
            BlendMode::Color => "color",
            BlendMode::Luminosity => "luminosity",
        }
    }

    /// The name shown in the app, e.g. "Colour dodge".
    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "Colour dodge",
            BlendMode::ColorBurn => "Colour burn",
            BlendMode::HardLight => "Hard light",
            BlendMode::SoftLight => "Soft light",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Add => "Add",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Colour",
            BlendMode::Luminosity => "Luminosity",
        }
    }

    /// Finds a mode by name. Forgiving about spelling: capitals, spaces,
    /// "_" or "-" between words, and "colour" for "color" all work, so
    /// "Colour Dodge" and "color_dodge" both find `ColorDodge`.
    pub fn from_name(name: &str) -> Option<BlendMode> {
        let simple: String = name
            .trim()
            .to_ascii_lowercase()
            .replace("colour", "color")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        BlendMode::ALL
            .into_iter()
            .find(|mode| mode.name().replace('-', "") == simple)
    }

    /// Mixes the straight (not premultiplied) colours `below` and `top`,
    /// each channel 0..1, as this mode says.
    fn mix(self, below: [f32; 3], top: [f32; 3]) -> [f32; 3] {
        let each = |f: fn(f32, f32) -> f32| std::array::from_fn(|i| f(below[i], top[i]));
        match self {
            BlendMode::Normal => top,
            BlendMode::Multiply => each(|b, t| b * t),
            BlendMode::Screen => each(screen),
            BlendMode::Overlay => each(|b, t| hard_light(t, b)),
            BlendMode::Darken => each(f32::min),
            BlendMode::Lighten => each(f32::max),
            BlendMode::ColorDodge => each(|b, t| {
                if b <= 0.0 {
                    0.0
                } else if t >= 1.0 {
                    1.0
                } else {
                    (b / (1.0 - t)).min(1.0)
                }
            }),
            BlendMode::ColorBurn => each(|b, t| {
                if b >= 1.0 {
                    1.0
                } else if t <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - b) / t).min(1.0)
                }
            }),
            BlendMode::HardLight => each(hard_light),
            BlendMode::SoftLight => each(|b, t| {
                if t <= 0.5 {
                    b - (1.0 - 2.0 * t) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * t - 1.0) * (d - b)
                }
            }),
            BlendMode::Difference => each(|b, t| (b - t).abs()),
            BlendMode::Exclusion => each(|b, t| b + t - 2.0 * b * t),
            BlendMode::Add => each(|b, t| (b + t).min(1.0)),
            BlendMode::Hue => set_lum(set_sat(top, sat(below)), lum(below)),
            BlendMode::Saturation => set_lum(set_sat(below, sat(top)), lum(below)),
            BlendMode::Color => set_lum(top, lum(below)),
            BlendMode::Luminosity => set_lum(below, lum(top)),
        }
    }
}

fn screen(b: f32, t: f32) -> f32 {
    b + t - b * t
}

/// Hard light: multiply where `t` is dark, screen where it's light.
fn hard_light(b: f32, t: f32) -> f32 {
    if t <= 0.5 {
        b * 2.0 * t
    } else {
        screen(b, 2.0 * t - 1.0)
    }
}

// The four "colour" modes work with lightness ("luminosity") and
// vividness ("saturation") instead of red, green and blue separately.

/// How light a colour looks (green looks lighter than blue to our eyes).
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Changes a colour's lightness to `l`, keeping its hue.
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

/// Brings channels back into 0..1 without changing lightness or hue.
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n).max(1e-6));
    }
    if x > 1.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l).max(1e-6));
    }
    c
}

/// How vivid a colour is: the gap between its strongest and weakest channel.
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Changes a colour's vividness to `s`, keeping its hue.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max - min <= 0.0 {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

/// Lays `top` (from a layer with the given opacity and blend mode) over
/// `below`. Both pixels are premultiplied.
///
/// For every mode except Normal this follows the W3C recipe: mix the two
/// colours with the mode's formula, but only as much as `below` is
/// actually there (over a transparent area the layer looks normal), then
/// lay the result over `below` as strongly as `top` is opaque.
pub fn blend(below: Rgba8, top: Rgba8, opacity: f32, mode: BlendMode) -> Rgba8 {
    if top.a == 0 || opacity <= 0.0 {
        return below;
    }
    if mode == BlendMode::Normal {
        // "Over": the top pixel (made fainter by the layer's opacity),
        // plus as much of the pixel below as still shows through.
        let top_alpha = f32::from(top.a) / 255.0 * opacity;
        let show_through = 1.0 - top_alpha;
        let mix = |t: u8, b: u8| {
            (f32::from(t) * opacity + f32::from(b) * show_through)
                .round()
                .min(255.0) as u8
        };
        return Rgba8::new(
            mix(top.r, below.r),
            mix(top.g, below.g),
            mix(top.b, below.b),
            mix(top.a, below.a),
        );
    }

    let below_alpha = f32::from(below.a) / 255.0;
    let top_alpha = f32::from(top.a) / 255.0;
    let straight = |p: Rgba8, alpha: f32| {
        if alpha <= 0.0 {
            [0.0; 3]
        } else {
            [p.r, p.g, p.b].map(|v| (f32::from(v) / 255.0 / alpha).min(1.0))
        }
    };
    let (b, t) = (straight(below, below_alpha), straight(top, top_alpha));
    let mixed = mode.mix(b, t);
    let strength = top_alpha * opacity;
    let out_alpha = strength + below_alpha * (1.0 - strength);
    let channel = |i: usize| {
        let top_color = (1.0 - below_alpha) * t[i] + below_alpha * mixed[i].clamp(0.0, 1.0);
        strength * top_color + (1.0 - strength) * below_alpha * b[i]
    };
    let to_byte = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    let a = to_byte(out_alpha);
    Rgba8::new(
        to_byte(channel(0)).min(a),
        to_byte(channel(1)).min(a),
        to_byte(channel(2)).min(a),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba8 = Rgba8::WHITE;
    const BLACK: Rgba8 = Rgba8::BLACK;
    const GREY: Rgba8 = Rgba8::new(128, 128, 128, 255);
    const RED: Rgba8 = Rgba8::new(255, 0, 0, 255);
    const BLUE: Rgba8 = Rgba8::new(0, 0, 255, 255);

    #[test]
    fn every_mode_has_a_name_that_finds_it_again() {
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::from_name(mode.name()), Some(mode), "{mode:?}");
            assert_eq!(BlendMode::from_name(mode.label()), Some(mode), "{mode:?}");
        }
        assert_eq!(
            BlendMode::from_name("Colour_Dodge"),
            Some(BlendMode::ColorDodge)
        );
        assert_eq!(
            BlendMode::from_name("SOFT LIGHT"),
            Some(BlendMode::SoftLight)
        );
        assert_eq!(BlendMode::from_name("sparkly"), None);
    }

    #[test]
    fn multiply_darkens_and_white_changes_nothing() {
        assert_eq!(blend(GREY, WHITE, 1.0, BlendMode::Multiply), GREY);
        assert_eq!(blend(RED, BLUE, 1.0, BlendMode::Multiply), BLACK);
        assert_eq!(blend(WHITE, GREY, 1.0, BlendMode::Multiply), GREY);
    }

    #[test]
    fn screen_lightens_and_black_changes_nothing() {
        assert_eq!(blend(GREY, BLACK, 1.0, BlendMode::Screen), GREY);
        assert_eq!(
            blend(RED, BLUE, 1.0, BlendMode::Screen),
            Rgba8::new(255, 0, 255, 255)
        );
        // 0.5 screened with 0.5 = 0.75.
        assert_eq!(blend(GREY, GREY, 1.0, BlendMode::Screen).r, 192);
    }

    #[test]
    fn worked_examples_for_the_other_simple_modes() {
        let px = |v: u8| Rgba8::new(v, v, v, 255);
        // below 0.2 (51), top 0.6 (153), worked out by hand from the formulas:
        let (b, t) = (px(51), px(153));
        let check = |mode: BlendMode, expected: u8| {
            assert_eq!(blend(b, t, 1.0, mode).r, expected, "{mode:?}");
        };
        check(BlendMode::Darken, 51);
        check(BlendMode::Lighten, 153);
        check(BlendMode::Difference, 102); // 0.6 - 0.2 = 0.4
        check(BlendMode::Exclusion, 143); // 0.2 + 0.6 - 2 * 0.12 = 0.56
        check(BlendMode::Add, 204); // 0.8
        check(BlendMode::Overlay, 61); // below is dark: 2 * 0.2 * 0.6 = 0.24
        check(BlendMode::HardLight, 92); // top is light: screen(0.2, 0.2) = 0.36
        check(BlendMode::ColorDodge, 128); // 0.2 / 0.4 = 0.5
        check(BlendMode::ColorBurn, 0); // 1 - min(1, 0.8 / 0.6) = 0
        check(BlendMode::SoftLight, 64); // 0.2 + 0.2 * (d(0.2) - 0.2), d(0.2) = 0.448
    }

    #[test]
    fn colour_mode_keeps_the_lightness_below() {
        // A grey below, coloured red: the result is a red with the grey's lightness.
        let out = blend(GREY, RED, 1.0, BlendMode::Color);
        assert!(out.r > out.g && out.g == out.b, "{out:?}");
        let lightness =
            |p: Rgba8| 0.3 * f32::from(p.r) + 0.59 * f32::from(p.g) + 0.11 * f32::from(p.b);
        assert!((lightness(out) - lightness(GREY)).abs() < 1.5, "{out:?}");
        // Luminosity is the other way round: the grey's lightness takes over.
        let out = blend(RED, GREY, 1.0, BlendMode::Luminosity);
        assert!((lightness(out) - lightness(GREY)).abs() < 1.5, "{out:?}");
    }

    #[test]
    fn hue_and_saturation_on_grey_stay_grey() {
        // Grey has no saturation, so taking only red's hue changes nothing.
        assert_eq!(blend(GREY, RED, 1.0, BlendMode::Hue), GREY);
        // Taking grey's saturation (none) turns red grey.
        let out = blend(RED, GREY, 1.0, BlendMode::Saturation);
        assert!(out.r == out.g && out.g == out.b, "{out:?}");
    }

    #[test]
    fn over_a_transparent_area_every_mode_looks_normal() {
        for mode in BlendMode::ALL {
            assert_eq!(blend(Rgba8::TRANSPARENT, RED, 1.0, mode), RED, "{mode:?}");
        }
    }

    #[test]
    fn opacity_and_transparent_tops_are_respected() {
        for mode in BlendMode::ALL {
            assert_eq!(blend(GREY, Rgba8::TRANSPARENT, 1.0, mode), GREY, "{mode:?}");
            assert_eq!(blend(GREY, RED, 0.0, mode), GREY, "{mode:?}");
        }
        // Half-strength multiply of black over white: half way to black.
        assert_eq!(
            blend(WHITE, BLACK, 0.5, BlendMode::Multiply),
            Rgba8::new(128, 128, 128, 255)
        );
    }

    #[test]
    fn results_stay_valid_premultiplied_pixels() {
        let samples = [
            Rgba8::TRANSPARENT,
            Rgba8::new(10, 20, 30, 40),
            Rgba8::new(100, 0, 50, 128),
            GREY,
            RED,
            WHITE,
        ];
        for mode in BlendMode::ALL {
            for below in samples {
                for top in samples {
                    let out = blend(below, top, 0.7, mode);
                    assert!(
                        out.r <= out.a && out.g <= out.a && out.b <= out.a,
                        "{mode:?} {out:?}"
                    );
                }
            }
        }
    }
}
