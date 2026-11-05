//! Noise and textures: paint that isn't one colour or a gradient but a
//! pattern, worked out for each pixel from where it is.
//!
//! - [`Noise`] is smooth, natural-looking "value noise": random values
//!   on a grid, blended smoothly between, with finer layers added for
//!   `detail`. It's mapped through colour stops, like a gradient.
//! - [`Texture`] is a small picture tiled over the canvas (from a file,
//!   or the built-in paper grain), optionally tinted by its lightness.
//!
//! Both are sampled in *source* coordinates: a transform takes each
//! canvas pixel back into the coordinates of the group the paint was
//! written in, so a textured shape keeps its texture when it's moved,
//! turned or resized by a group.

use std::sync::{Arc, LazyLock};

use crate::fill::GradientStop;
use crate::tile::Rgba8;
use crate::transform::Transform;

/// The most layers of finer noise.
pub const MAX_DETAIL: u32 = 6;

/// A random value, 0 up to 1, for one grid point of one noise layer:
/// always the same for the same point, layer and seed.
fn grid_value(seed: u64, x: i64, y: i64) -> f32 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    // The same mixing as `Random`, so values are evenly spread.
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// An S-shaped blend (0..1 to 0..1) with no visible seams at grid lines.
fn smooth(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// One layer of noise at (x, y), in grid units: the four grid values
/// around the point, blended. `period` (if any) makes it repeat every
/// that many grid units, for seamless tiles.
fn layer(x: f32, y: f32, seed: u64, period: Option<i64>) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (smooth(x - x0), smooth(y - y0));
    let (ix, iy) = (x0 as i64, y0 as i64);
    let at = |dx: i64, dy: i64| {
        let (mut gx, mut gy) = (ix + dx, iy + dy);
        if let Some(period) = period {
            gx = gx.rem_euclid(period);
            gy = gy.rem_euclid(period);
        }
        grid_value(seed, gx, gy)
    };
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

/// Noise with `detail` layers, each twice as fine and half as strong as
/// the one before, at (x, y) in units of the coarsest layer's grid.
/// Returns about 0..1 (stretched, since blended random values crowd
/// around the middle).
pub fn noise_at(x: f32, y: f32, seed: u64, detail: u32, period: Option<i64>) -> f32 {
    let detail = detail.clamp(1, MAX_DETAIL);
    let (mut sum, mut strength, mut total, mut scale) = (0.0, 1.0, 0.0, 1.0);
    for i in 0..detail {
        let period = period.map(|p| p * (1 << i));
        sum += layer(
            x * scale,
            y * scale,
            seed.wrapping_add(u64::from(i) * 7919),
            period,
        ) * strength;
        total += strength;
        strength *= 0.5;
        scale *= 2.0;
    }
    ((sum / total - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0)
}

/// Noise as paint: its value at each point picks a colour from the stops.
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    /// Pixels (source units) per wave of the coarsest layer.
    pub scale: f32,
    /// Layers of finer noise, 1 to `MAX_DETAIL`.
    pub detail: u32,
    /// 1 = as is; more pushes values towards the ends, less towards the middle.
    pub contrast: f32,
    /// Moves the pattern, in source units.
    pub offset: (f32, f32),
    pub seed: u64,
    /// The colours, by noise value: at least one, sorted by position.
    pub stops: Vec<GradientStop>,
    /// From canvas pixels to source coordinates (the group's own).
    pub to_source: Transform,
}

impl Noise {
    /// The noise value (0..1) at a canvas point, after contrast.
    pub fn value_at(&self, x: f32, y: f32) -> f32 {
        let (sx, sy) = self.to_source.apply((x, y));
        let scale = self.scale.max(0.01);
        let v = noise_at(
            (sx - self.offset.0) / scale,
            (sy - self.offset.1) / scale,
            self.seed,
            self.detail,
            None,
        );
        ((v - 0.5) * self.contrast + 0.5).clamp(0.0, 1.0)
    }
}

/// A picture to tile: premultiplied pixels, row by row.
#[derive(Debug, PartialEq)]
pub struct TextureImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgba8>,
}

impl TextureImage {
    /// The premultiplied colour at (u, v) in texture pixels, wrapping
    /// around at the edges and blending the four nearest pixels.
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width as i64, self.height as i64);
        let (fu, fv) = (u - 0.5, v - 0.5);
        let (u0, v0) = (fu.floor(), fv.floor());
        let (tu, tv) = (fu - u0, fv - v0);
        let at = |du: i64, dv: i64| {
            let x = (u0 as i64 + du).rem_euclid(w) as usize;
            let y = (v0 as i64 + dv).rem_euclid(h) as usize;
            let p = self.pixels[y * self.width + x];
            [
                f32::from(p.r),
                f32::from(p.g),
                f32::from(p.b),
                f32::from(p.a),
            ]
        };
        let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
        let mut out = [0.0; 4];
        for i in 0..4 {
            let top = a[i] + (b[i] - a[i]) * tu;
            let bottom = c[i] + (d[i] - c[i]) * tu;
            out[i] = top + (bottom - top) * tv;
        }
        out
    }
}

/// A tiled picture as paint.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub image: Arc<TextureImage>,
    /// Source units per texture pixel (2 = twice as big).
    pub scale: f32,
    /// Moves the picture, in source units.
    pub offset: (f32, f32),
    /// If given, the picture's lightness picks a colour from these stops
    /// instead of its own colours (its opacity still counts).
    pub tint: Option<Vec<GradientStop>>,
    /// From canvas pixels to source coordinates.
    pub to_source: Transform,
}

impl Texture {
    /// Where a canvas point lands in texture pixels.
    pub fn texture_point(&self, x: f32, y: f32) -> (f32, f32) {
        let (sx, sy) = self.to_source.apply((x, y));
        let scale = self.scale.max(0.001);
        ((sx - self.offset.0) / scale, (sy - self.offset.1) / scale)
    }
}

/// The built-in "paper" texture: a 256 x 256 seamless tile of fine,
/// light grain, made from noise once (the same on every computer).
pub static PAPER: LazyLock<Arc<TextureImage>> = LazyLock::new(|| {
    const SIDE: usize = 256;
    let mut pixels = Vec::with_capacity(SIDE * SIDE);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (fx, fy) = (x as f32, y as f32);
            // Soft blotches, then fibres, then speckle.
            let blotch = noise_at(fx / 32.0, fy / 32.0, 101, 3, Some(8));
            let fibre = noise_at(fx / 3.0, fy / 11.0, 202, 2, Some(32));
            let speck = grid_value(303, x as i64, y as i64);
            let light = 0.78 + 0.12 * blotch + 0.06 * fibre + 0.04 * speck;
            let v = (light.clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.push(Rgba8::new(v, v, v, 255));
        }
    }
    Arc::new(TextureImage {
        width: SIDE,
        height: SIDE,
        pixels,
    })
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_smooth_repeatable_and_depends_on_the_seed() {
        let a = noise_at(3.3, 4.7, 5, 3, None);
        assert_eq!(a, noise_at(3.3, 4.7, 5, 3, None));
        assert_ne!(a, noise_at(3.3, 4.7, 6, 3, None));
        assert!((0.0..=1.0).contains(&a));
        // Nearby points have nearby values.
        let b = noise_at(3.31, 4.7, 5, 3, None);
        assert!((a - b).abs() < 0.05, "{a} vs {b}");
        // Over many points it uses the whole range.
        let values: Vec<f32> = (0..400)
            .map(|i| noise_at(i as f32 * 0.37, (i * 7) as f32 * 0.11, 9, 4, None))
            .collect();
        let (lo, hi) = values
            .iter()
            .fold((1.0f32, 0.0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        assert!(lo < 0.15 && hi > 0.85, "{lo}..{hi}");
    }

    #[test]
    fn periodic_noise_repeats() {
        let a = noise_at(1.5, 2.5, 1, 2, Some(4));
        assert_eq!(a, noise_at(5.5, 2.5, 1, 2, Some(4)));
        assert_eq!(a, noise_at(1.5, 6.5, 1, 2, Some(4)));
    }

    #[test]
    fn textures_wrap_and_blend() {
        let image = TextureImage {
            width: 2,
            height: 1,
            pixels: vec![Rgba8::new(0, 0, 0, 255), Rgba8::new(255, 255, 255, 255)],
        };
        assert_eq!(image.sample(0.5, 0.5), [0.0, 0.0, 0.0, 255.0]);
        assert_eq!(image.sample(1.5, 0.5), [255.0, 255.0, 255.0, 255.0]);
        assert_eq!(image.sample(2.5, 0.5), [0.0, 0.0, 0.0, 255.0], "wraps");
        assert_eq!(image.sample(1.0, 0.5)[0], 127.5, "half way between");
        let paper = &*PAPER;
        assert_eq!((paper.width, paper.height), (256, 256));
        assert!(paper.pixels.iter().all(|p| p.a == 255 && p.r > 180));
        assert!(paper.pixels.iter().any(|p| p.r != paper.pixels[0].r));
    }

    #[test]
    fn noise_paint_moves_with_its_source_transform() {
        let stops = vec![GradientStop {
            position: 0.0,
            color: [0, 0, 0, 255],
        }];
        let plain = Noise {
            scale: 10.0,
            detail: 2,
            contrast: 1.0,
            offset: (0.0, 0.0),
            seed: 1,
            stops: stops.clone(),
            to_source: Transform::IDENTITY,
        };
        let moved = Noise {
            to_source: Transform::translate(-30.0, 0.0),
            ..plain.clone()
        };
        let by_offset = Noise {
            offset: (30.0, 0.0),
            ..plain.clone()
        };
        assert_eq!(moved.value_at(50.0, 5.0), plain.value_at(20.0, 5.0));
        assert_eq!(by_offset.value_at(50.0, 5.0), plain.value_at(20.0, 5.0));
        let sharp = Noise {
            contrast: 3.0,
            ..plain.clone()
        };
        let v = plain.value_at(7.0, 3.0);
        let s = sharp.value_at(7.0, 3.0);
        assert!((s - 0.5).abs() >= (v - 0.5).abs() - 1e-6);
    }
}
