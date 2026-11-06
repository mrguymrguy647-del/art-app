//! Brush settings and the shape of one brush "dab".
//!
//! A brush stroke is made of many dabs stamped close together along the
//! path of the pen (see `stroke.rs`). This file says how big, how opaque
//! and how soft each dab is, what shape it has (its *tip*) and how much
//! it varies from dab to dab (its *dynamics*).

use std::sync::{Arc, LazyLock};

use crate::random::Random;

/// Smallest and largest brush size (diameter in canvas pixels).
pub const MIN_SIZE: f32 = 1.0;
pub const MAX_SIZE: f32 = 1000.0;

/// Whether a brush adds paint or takes it away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushMode {
    #[default]
    Paint,
    /// Removes paint, making the layer transparent again (the eraser).
    Erase,
}

/// The shape of a dab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tip {
    /// A round dab (the plain brush).
    #[default]
    Round,
    /// A square: hard, for pixels and blocks.
    Square,
    /// A row of bristles across the stroke: streaky, like a flat brush.
    Bristle,
    /// A grainy, broken dab, like chalk on rough paper.
    Chalk,
    /// A leaf, pointing along the stroke.
    Leaf,
    /// A blade of grass, standing up.
    Grass,
    /// A four-pointed star with a glow.
    Sparkle,
    /// A shape from a greyscale picture, kept by whoever plays the
    /// script (see `TipShape::from_lightness`); the number tells them apart.
    File(u32),
}

impl Tip {
    /// The built-in tips, in the order they're shown.
    pub const BUILT_IN: [Tip; 7] = [
        Tip::Round,
        Tip::Square,
        Tip::Bristle,
        Tip::Chalk,
        Tip::Leaf,
        Tip::Grass,
        Tip::Sparkle,
    ];

    /// The name a script uses (`None` for a tip from a file).
    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            Tip::Round => "round",
            Tip::Square => "square",
            Tip::Bristle => "bristle",
            Tip::Chalk => "chalk",
            Tip::Leaf => "leaf",
            Tip::Grass => "grass",
            Tip::Sparkle => "sparkle",
            Tip::File(_) => return None,
        })
    }

    pub fn from_name(name: &str) -> Option<Tip> {
        Tip::BUILT_IN
            .into_iter()
            .find(|tip| tip.name() == Some(name))
    }

    /// The built-in tip's shape (`None` for round, which needs none, and
    /// for a tip from a file).
    pub fn shape(self) -> Option<Arc<TipShape>> {
        let index = match self {
            Tip::Round | Tip::File(_) => return None,
            Tip::Square => 0,
            Tip::Bristle => 1,
            Tip::Chalk => 2,
            Tip::Leaf => 3,
            Tip::Grass => 4,
            Tip::Sparkle => 5,
        };
        Some(Arc::clone(&BUILT_IN_SHAPES[index]))
    }
}

/// How much each dab differs from the last, all from the stroke's seed
/// (so a script paints the same picture every time). Everything 0 (and
/// `follow` off) means every dab is the same: the plain brush.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dynamics {
    /// Turn the tip to follow the direction of the stroke.
    pub follow: bool,
    /// Size varies by up to this fraction (0.3: from 70% to 130%).
    pub size: f32,
    /// The tip turns by up to this many degrees either way.
    pub angle: f32,
    /// Opacity varies by up to this fraction.
    pub opacity: f32,
    /// Dabs land up to this many pixels off the line.
    pub scatter: f32,
    /// The colour's hue turns by up to this many degrees either way.
    pub hue: f32,
    /// Lightness varies by up to this fraction (0.1: a tenth lighter or darker).
    pub lightness: f32,
    /// Saturation varies by up to this fraction.
    pub saturation: f32,
}

impl Dynamics {
    /// True if no setting is on: the brush paints like the plain brush.
    pub fn is_off(&self) -> bool {
        *self == Dynamics::default()
    }

    /// True if the colour varies from dab to dab.
    pub fn varies_color(&self) -> bool {
        self.hue != 0.0 || self.lightness != 0.0 || self.saturation != 0.0
    }
}

/// Everything about how the brush paints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub mode: BrushMode,
    /// Diameter in canvas pixels, at full pressure.
    pub size: f32,
    /// The most opaque a stroke can get, 0..1. Going over the same spot
    /// again within one stroke doesn't make it darker than this.
    pub opacity: f32,
    /// How much paint each dab adds, 0..1. Below 1, paint builds up
    /// gradually where dabs overlap, like an airbrush.
    pub flow: f32,
    /// Edge sharpness, 0..1. 1 = crisp edge, 0 = very soft edge.
    pub hardness: f32,
    /// Distance between dabs, as a fraction of the diameter. 0.1 = a new
    /// dab every 10% of the brush width.
    pub spacing: f32,
    /// Which settings pen pressure controls. Pressing lightly makes the
    /// dab smaller / more transparent / add less paint.
    pub pressure_size: bool,
    pub pressure_opacity: bool,
    pub pressure_flow: bool,
    /// Paint colour: red, green, blue (normal, not premultiplied).
    /// The eraser ignores it.
    pub color: [u8; 3],
    /// The shape of each dab.
    pub tip: Tip,
    /// How far the tip is turned, in degrees clockwise (0: as drawn, a
    /// leaf pointing right). Added to `follow` and the angle dynamics.
    pub angle: f32,
    /// How dabs vary along the stroke.
    pub dynamics: Dynamics,
    /// Paint with a pattern (noise or a texture) instead of `color`: a
    /// number into the list kept by whoever plays the script, which
    /// hands the pattern to the stroke (see `Stroke::with_source`).
    pub source: Option<u32>,
    /// What the lightest touch still gives, as a fraction of `size` and of
    /// `opacity` (when pressure changes them). 0: a light touch gives
    /// almost nothing, as before. A pencil keeps its width (size 0.85)
    /// but not its darkness (opacity 0.15).
    pub pressure_size_min: f32,
    pub pressure_opacity_min: f32,
    /// How pressure is felt: 1 as it comes; above 1 you must press harder
    /// to get the full effect (pressure to the power of this).
    pub pressure_curve: f32,
    /// Paper grain, 0..1: a light touch only marks the hilltops of a
    /// paper texture that stays put on the canvas; pressing harder fills
    /// the valleys too. 0: no grain.
    pub grain: f32,
    /// How big the grain is: 1 is fine paper, 2 twice as coarse.
    pub grain_scale: f32,
    /// The most a layer darkens however often you go over it, 0..1, as
    /// graphite goes shiny grey, never black. 0: no limit.
    pub build_up: f32,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            mode: BrushMode::Paint,
            size: 24.0,
            opacity: 1.0,
            flow: 1.0,
            hardness: 0.8,
            spacing: 0.1,
            pressure_size: true,
            pressure_opacity: true,
            pressure_flow: false,
            color: [20, 20, 20],
            tip: Tip::Round,
            angle: 0.0,
            dynamics: Dynamics::default(),
            source: None,
            pressure_size_min: 0.0,
            pressure_opacity_min: 0.0,
            pressure_curve: 1.0,
            grain: 0.0,
            grain_scale: 1.0,
            build_up: 0.0,
        }
    }
}

impl BrushSettings {
    /// The eraser's starting settings: bigger and a bit softer than the
    /// brush, and always at full strength (pressure only changes its size).
    pub fn eraser() -> Self {
        Self {
            mode: BrushMode::Erase,
            size: 40.0,
            hardness: 0.6,
            pressure_opacity: false,
            ..Self::default()
        }
    }
}

/// One dab, after pen pressure has been applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub radius: f32,
    pub opacity: f32,
    pub flow: f32,
}

impl BrushSettings {
    /// The dab this brush makes at the given pen pressure (0..1).
    pub fn dab(&self, pressure: f32) -> Dab {
        let felt = self.felt(pressure);
        // From `min` (the lightest touch) up to the full value.
        let scale = |value: f32, use_pressure: bool, min: f32| {
            if use_pressure {
                value * (min + (1.0 - min) * felt)
            } else {
                value
            }
        };
        Dab {
            // Never smaller than one pixel across, or very light strokes vanish.
            radius: (scale(self.size, self.pressure_size, self.pressure_size_min) / 2.0).max(0.5),
            opacity: scale(
                self.opacity,
                self.pressure_opacity,
                self.pressure_opacity_min,
            ),
            flow: scale(self.flow, self.pressure_flow, 0.0),
        }
    }

    /// The pressure as felt: bent by the curve (left exactly as it is at
    /// 1, so every brush made before this setting paints the same).
    fn felt(&self, pressure: f32) -> f32 {
        let pressure = pressure.clamp(0.0, 1.0);
        if self.pressure_curve == 1.0 {
            pressure
        } else {
            pressure.powf(self.pressure_curve.max(0.05))
        }
    }

    /// The paper tooth for a dab at this pressure (`None` with no grain):
    /// where on the paper the graphite catches.
    pub fn tooth(&self, pressure: f32) -> Option<Tooth> {
        if self.grain <= 0.0 {
            return None;
        }
        let grain = self.grain.min(1.0);
        let felt = self.felt(pressure);
        Some(Tooth {
            along: {
                let per_texel = 1.0 / self.grain_scale.max(0.25);
                let (sin, cos) = FIBRE_ANGLE.to_radians().sin_cos();
                (cos * per_texel, sin * per_texel)
            },
            // The share of the paper left white: about half at a light
            // touch, a few specks pressing hard (with grain 0.75).
            threshold: grain * (0.78 - 0.74 * felt),
            // Towards the dab's edge only the highest bumps catch, so the
            // outline breaks up instead of being a clean capsule.
            edge: grain * 1.0,
            // Graphite already there fills the valleys a little.
            fill: grain * 0.6,
        })
    }

    /// Distance between dab centres for a dab of this radius, in pixels.
    pub fn dab_distance(&self, radius: f32) -> f32 {
        // At least half a pixel apart, so tiny brushes don't stamp
        // thousands of dabs on the same spot.
        (2.0 * radius * self.spacing).max(0.5)
    }
}

/// How much a dab covers a pixel at `distance` from its centre: 1.0 in
/// the solid middle, falling to 0.0 at the edge.
///
/// The soft edge is `(1 - hardness) * radius` wide, and always at least one
/// pixel, so even hard brushes get smooth (anti-aliased) edges.
pub fn dab_coverage(distance: f32, radius: f32, hardness: f32) -> f32 {
    let edge_width = ((1.0 - hardness.clamp(0.0, 1.0)) * radius).max(1.0);
    // 0 at the outer edge, 1 where the solid middle starts.
    let t = ((radius - distance) / edge_width + 0.5 / edge_width).clamp(0.0, 1.0);
    // "Smoothstep": an S-shaped curve, so the fade looks natural.
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---- Paper grain -----------------------------------------------------------------

/// Where the paper tooth catches graphite for one dab, worked out once
/// per dab (see `BrushSettings::tooth`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tooth {
    /// Grain texels per canvas pixel, along the fibres' two directions:
    /// (cos, sin) of the fibre angle, times the texels per pixel.
    along: (f32, f32),
    /// Bumps lower than this stay white (heights are spread evenly 0..1,
    /// so this is the share of paper left white).
    threshold: f32,
    /// How much higher the threshold is at the dab's very edge.
    edge: f32,
    /// How much lower it is where the layer is already fully dark.
    fill: f32,
}

/// How soft each grain's edge is (in height).
const TOOTH_SOFTNESS: f32 = 0.07;

impl Tooth {
    /// How much graphite catches at canvas pixel (x, y), 0..1. `inside` is
    /// how far inside the dab the pixel is (1 in the solid middle, 0 at
    /// the edge); `already` how dark the layer was there before this
    /// stroke (0..1).
    pub fn at(&self, x: i32, y: i32, inside: f32, already: f32) -> f32 {
        let height = GRAIN.at_canvas(x, y, self.along);
        let threshold = self.threshold + self.edge * (1.0 - inside) - self.fill * already;
        smoothstep(
            threshold - TOOTH_SOFTNESS,
            threshold + TOOTH_SOFTNESS,
            height,
        )
    }
}

/// How many texels across the grain picture is (a power of two). It
/// repeats across the canvas without a seam.
const GRAIN_SIZE: usize = 256;

/// A paper texture: hills and valleys, 0 (deepest) to 1 (highest), the
/// same on every machine (made from fixed random numbers, not a file).
pub struct Grain {
    heights: Vec<f32>,
}

/// Made once, the first time a grainy brush paints.
static GRAIN: LazyLock<Grain> = LazyLock::new(Grain::new);

/// The paper's fibres run this way (degrees): tooth is streaky, but not
/// along the canvas edges, where it would line up with straight strokes.
const FIBRE_ANGLE: f32 = 30.0;

impl Grain {
    fn new() -> Grain {
        // Bumps stretched about three times longer than wide (fibres), from
        // coarse to single texels, on grids that wrap around so the
        // picture tiles.
        let mut heights = vec![0.0f32; GRAIN_SIZE * GRAIN_SIZE];
        let mut random = Random::new(0x00D0_C0FF_EE15_9A1E);
        for (cell_x, cell_y, weight) in [
            (6usize, 2usize, 0.25f32),
            (3, 1, 0.3),
            (2, 1, 0.25),
            (1, 1, 0.2),
        ] {
            let (cells_x, cells_y) = (GRAIN_SIZE / cell_x, GRAIN_SIZE / cell_y);
            let lattice: Vec<f32> = (0..cells_x * cells_y).map(|_| random.unit()).collect();
            let at = |x: usize, y: usize| lattice[(y % cells_y) * cells_x + (x % cells_x)];
            for y in 0..GRAIN_SIZE {
                for x in 0..GRAIN_SIZE {
                    let (cx, cy) = (x / cell_x, y / cell_y);
                    let fx = smoothstep(0.0, 1.0, (x % cell_x) as f32 / cell_x as f32);
                    let fy = smoothstep(0.0, 1.0, (y % cell_y) as f32 / cell_y as f32);
                    let top = at(cx, cy) + (at(cx + 1, cy) - at(cx, cy)) * fx;
                    let bottom = at(cx, cy + 1) + (at(cx + 1, cy + 1) - at(cx, cy + 1)) * fx;
                    heights[y * GRAIN_SIZE + x] += weight * (top + (bottom - top) * fy);
                }
            }
        }
        // Spread the heights evenly from 0 to 1 (by rank), so a threshold
        // of 0.3 leaves exactly 30% of the paper white.
        let mut order: Vec<usize> = (0..heights.len()).collect();
        order.sort_by(|&a, &b| heights[a].total_cmp(&heights[b]));
        let last = (order.len() - 1) as f32;
        let mut spread = vec![0.0f32; heights.len()];
        for (rank, &index) in order.iter().enumerate() {
            spread[index] = rank as f32 / last;
        }
        Grain { heights: spread }
    }

    /// The height at texel (x, y), repeating in both directions.
    fn at(&self, x: i32, y: i32) -> f32 {
        // (The size is a power of two, so wrapping round is a bit mask,
        // which is right for negative numbers too.)
        let mask = GRAIN_SIZE as i32 - 1;
        let (x, y) = ((x & mask) as usize, (y & mask) as usize);
        self.heights[y * GRAIN_SIZE + x]
    }

    /// The height under canvas pixel (x, y), with the fibres turned.
    fn at_canvas(&self, x: i32, y: i32, (cos, sin): (f32, f32)) -> f32 {
        let (x, y) = (x as f32, y as f32);
        let u = x * cos + y * sin;
        let v = y * cos - x * sin;
        self.at(u.floor() as i32, v.floor() as i32)
    }
}

// ---- Tip shapes ----------------------------------------------------------------

/// How many texels across the biggest level of a tip shape is.
const TIP_SIZE: usize = 128;

/// The shape of a dab that isn't round: a square picture of how much of
/// the dab is at each point, 0 (nothing) to 1 (solid), kept at several
/// sizes (each half the one before) so a small dab samples a small
/// picture and stays smooth.
#[derive(Debug)]
pub struct TipShape {
    /// Biggest first: (texels across, the values, row by row).
    levels: Vec<(usize, Vec<f32>)>,
}

impl TipShape {
    /// A shape from a greyscale picture: `lightness` is `width` x
    /// `height` values row by row, 0 (black: nothing) to 255 (white:
    /// solid). The picture is fitted into the dab's square, keeping its
    /// proportions.
    pub fn from_lightness(width: usize, height: usize, lightness: &[u8]) -> TipShape {
        let side = width.max(height).max(1);
        let (left, top) = ((side - width) / 2, (side - height) / 2);
        let mut values = vec![0.0; side * side];
        for y in 0..height {
            for x in 0..width {
                if let Some(&v) = lightness.get(y * width + x) {
                    values[(top + y) * side + left + x] = f32::from(v) / 255.0;
                }
            }
        }
        TipShape::from_values(side, values)
    }

    /// A shape from a formula: `inside(x, y)` for x and y from -1 to 1
    /// (the dab's edges), sampled 3 x 3 times per texel so edges are smooth.
    fn from_formula(inside: impl Fn(f32, f32) -> f32) -> TipShape {
        let mut values = vec![0.0; TIP_SIZE * TIP_SIZE];
        let step = 2.0 / TIP_SIZE as f32;
        for (i, value) in values.iter_mut().enumerate() {
            let (tx, ty) = (i % TIP_SIZE, i / TIP_SIZE);
            let mut sum = 0.0;
            for sy in 0..3 {
                for sx in 0..3 {
                    let x = -1.0 + (tx as f32 + (sx as f32 + 0.5) / 3.0) * step;
                    let y = -1.0 + (ty as f32 + (sy as f32 + 0.5) / 3.0) * step;
                    sum += inside(x, y).clamp(0.0, 1.0);
                }
            }
            *value = sum / 9.0;
        }
        TipShape::from_values(TIP_SIZE, values)
    }

    /// The levels, from the biggest picture down to 2 x 2 (each texel of
    /// a smaller level is the average of 2 x 2 texels of the one above).
    fn from_values(side: usize, values: Vec<f32>) -> TipShape {
        let mut levels = vec![(side, values)];
        while let Some((side, values)) = levels.last()
            && *side > 2
        {
            let (side, values) = (*side, values);
            let half = side / 2;
            let mut smaller = vec![0.0; half * half];
            for y in 0..half {
                for x in 0..half {
                    let at = |dx: usize, dy: usize| values[(2 * y + dy) * side + 2 * x + dx];
                    smaller[y * half + x] = (at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) / 4.0;
                }
            }
            levels.push((half, smaller));
        }
        TipShape { levels }
    }

    /// How much of the dab is at (u, v), each -1 to 1 across the dab, for
    /// a dab `pixels` across (picks the level nearest that size).
    pub fn sample(&self, u: f32, v: f32, pixels: f32) -> f32 {
        if !(-1.0..=1.0).contains(&u) || !(-1.0..=1.0).contains(&v) {
            return 0.0;
        }
        let level = self
            .levels
            .iter()
            .rev()
            .find(|(side, _)| *side as f32 >= pixels)
            .unwrap_or(&self.levels[0]);
        let (side, values) = (level.0, &level.1);
        // Bilinear: the four nearest texels, weighted by closeness.
        let fx = ((u + 1.0) / 2.0 * side as f32 - 0.5).clamp(0.0, (side - 1) as f32);
        let fy = ((v + 1.0) / 2.0 * side as f32 - 0.5).clamp(0.0, (side - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(side - 1), (y0 + 1).min(side - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| values[y * side + x];
        let top = at(x0, y0) * (1.0 - tx) + at(x1, y0) * tx;
        let bottom = at(x0, y1) * (1.0 - tx) + at(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }
}

/// The built-in shapes, made once from formulas with fixed seeds (the
/// same on every computer): square, bristle, chalk, leaf, grass, sparkle.
static BUILT_IN_SHAPES: LazyLock<[Arc<TipShape>; 6]> = LazyLock::new(|| {
    let edge = |inside: f32| inside.clamp(0.0, 1.0); // a soft step for formulas
    let square = TipShape::from_formula(|x, y| {
        if x.abs() <= 0.98 && y.abs() <= 0.98 {
            1.0
        } else {
            0.0
        }
    });
    // Bristles: short round marks in a row across the stroke (along y),
    // each a little off the row, some inkier than others.
    let mut random = Random::new(11);
    let bristles: Vec<(f32, f32, f32, f32)> = (0..22)
        .map(|i| {
            let y = -0.9 + 1.8 * (i as f32 + 0.5) / 22.0;
            let x = random.range(-0.35, 0.35);
            let radius = random.range(0.05, 0.12);
            let ink = random.range(0.45, 1.0);
            (x, y, radius, ink)
        })
        .collect();
    let bristle = TipShape::from_formula(move |x, y| {
        bristles
            .iter()
            .map(|&(bx, by, r, ink)| {
                let d = (x - bx).hypot(y - by);
                edge((r - d) / 0.03) * ink
            })
            .fold(0.0, f32::max)
    });
    // Chalk: a round dab with grain, from a grid of random values.
    let mut random = Random::new(23);
    let grain: Vec<f32> = (0..64 * 64).map(|_| random.unit()).collect();
    let chalk = TipShape::from_formula(move |x, y| {
        let disc = edge((0.97 - x.hypot(y)) / 0.06);
        let gx = (((x + 1.0) / 2.0 * 64.0) as usize).min(63);
        let gy = (((y + 1.0) / 2.0 * 64.0) as usize).min(63);
        let g = grain[gy * 64 + gx];
        disc * edge((g - 0.3) / 0.45)
    });
    // A leaf: pointed at both ends, pointing right, with a lighter midrib.
    let leaf = TipShape::from_formula(|x, y| {
        let half_width = 0.5 * (1.0 - x * x);
        let body = edge((half_width - y.abs()) / 0.03);
        let rib = if y.abs() < 0.04 && x.abs() < 0.85 {
            0.55
        } else {
            1.0
        };
        body * rib
    });
    // A blade of grass: wide at the bottom, pointed at the top, bending right.
    let grass = TipShape::from_formula(|x, y| {
        let t = (1.0 - y) / 2.0; // 0 at the base (bottom), 1 at the tip (top)
        let centre = 0.3 * t * t;
        let half_width = 0.16 * (1.0 - t) + 0.015;
        edge((half_width - (x - centre).abs()) / 0.03) * edge((0.98 - y.abs()) / 0.03)
    });
    // A sparkle: two thin arms and a soft glow in the middle.
    let sparkle = TipShape::from_formula(|x, y| {
        let arm = |along: f32, across: f32| {
            let width = 0.07 * (1.0 - along.abs()).max(0.0);
            edge((width - across.abs()) / 0.02) * edge((0.98 - along.abs()) / 0.03)
        };
        let glow = (-(x * x + y * y) * 9.0).exp() * 0.7;
        arm(x, y).max(arm(y, x)).max(glow)
    });
    [square, bristle, chalk, leaf, grass, sparkle].map(Arc::new)
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_tips_have_names_and_shapes() {
        for tip in Tip::BUILT_IN {
            let name = tip.name().unwrap();
            assert_eq!(Tip::from_name(name), Some(tip));
            assert_eq!(tip.shape().is_some(), tip != Tip::Round);
        }
        assert_eq!(Tip::from_name("fluffy"), None);
        assert_eq!(Tip::File(3).name(), None);
        assert!(Tip::File(3).shape().is_none());
    }

    #[test]
    fn shapes_are_solid_in_the_middle_and_empty_outside() {
        let square = Tip::Square.shape().unwrap();
        assert_eq!(square.sample(0.0, 0.0, 128.0), 1.0);
        assert_eq!(square.sample(0.9, 0.9, 128.0), 1.0, "corners are in");
        assert_eq!(square.sample(1.5, 0.0, 128.0), 0.0, "outside the dab");
        let leaf = Tip::Leaf.shape().unwrap();
        assert!(leaf.sample(0.0, 0.3, 128.0) > 0.9, "in the leaf");
        assert!(leaf.sample(0.0, 0.0, 128.0) < 0.7, "the midrib is lighter");
        assert_eq!(leaf.sample(0.0, 0.8, 128.0), 0.0, "above the leaf");
        assert_eq!(leaf.sample(0.99, 0.3, 128.0), 0.0, "past the point");
        let grass = Tip::Grass.shape().unwrap();
        assert!(grass.sample(0.0, 0.9, 16.0) > 0.5, "the base, small dab");
        assert_eq!(grass.sample(-0.5, 0.9, 16.0), 0.0);
        // Smaller levels keep the shape.
        let chalk = Tip::Chalk.shape().unwrap();
        let big: f32 = (0..100)
            .map(|i| chalk.sample(i as f32 / 50.0 - 1.0, 0.0, 128.0))
            .sum();
        let small: f32 = (0..100)
            .map(|i| chalk.sample(i as f32 / 50.0 - 1.0, 0.0, 8.0))
            .sum();
        assert!((big - small).abs() < big * 0.3, "{big} vs {small}");
    }

    #[test]
    fn a_shape_from_a_picture_is_fitted_into_the_square() {
        // A 4 x 2 picture: white on the left half.
        let shape = TipShape::from_lightness(4, 2, &[255, 255, 0, 0, 255, 255, 0, 0]);
        assert_eq!(shape.levels[0].0, 4);
        assert_eq!(shape.sample(-0.75, 0.0, 4.0), 1.0, "left, middle row");
        assert_eq!(shape.sample(0.75, 0.0, 4.0), 0.0, "right");
        assert_eq!(shape.sample(-0.75, -0.9, 4.0), 0.0, "above the picture");
        assert_eq!(shape.levels.len(), 2, "4 then 2");
    }

    #[test]
    fn dynamics_know_when_they_are_off() {
        assert!(Dynamics::default().is_off());
        let some = Dynamics {
            size: 0.2,
            ..Dynamics::default()
        };
        assert!(!some.is_off() && !some.varies_color());
        let hue = Dynamics {
            hue: 5.0,
            ..Dynamics::default()
        };
        assert!(hue.varies_color());
    }

    #[test]
    fn full_pressure_gives_the_full_settings() {
        let brush = BrushSettings {
            size: 40.0,
            opacity: 0.8,
            flow: 0.5,
            ..BrushSettings::default()
        };
        let dab = brush.dab(1.0);
        assert_eq!(
            dab,
            Dab {
                radius: 20.0,
                opacity: 0.8,
                flow: 0.5
            }
        );
    }

    #[test]
    fn pressure_only_changes_what_it_is_switched_on_for() {
        let brush = BrushSettings {
            size: 40.0,
            opacity: 1.0,
            flow: 1.0,
            pressure_size: true,
            pressure_opacity: false,
            pressure_flow: true,
            ..BrushSettings::default()
        };
        let dab = brush.dab(0.5);
        assert_eq!(dab.radius, 10.0); // half the size
        assert_eq!(dab.opacity, 1.0); // unchanged
        assert_eq!(dab.flow, 0.5); // half the flow
    }

    #[test]
    fn the_eraser_erases_at_full_strength_whatever_the_pressure() {
        let eraser = BrushSettings::eraser();
        assert_eq!(eraser.mode, BrushMode::Erase);
        assert_eq!(eraser.dab(0.2).opacity, 1.0);
        assert!(eraser.dab(0.2).radius < eraser.dab(1.0).radius);
    }

    #[test]
    fn very_light_pressure_still_makes_a_visible_dab() {
        let dab = BrushSettings::default().dab(0.0);
        assert_eq!(dab.radius, 0.5);
    }

    #[test]
    fn dab_distance_follows_spacing_but_never_below_half_a_pixel() {
        let brush = BrushSettings {
            spacing: 0.1,
            ..BrushSettings::default()
        };
        assert_eq!(brush.dab_distance(50.0), 10.0);
        assert_eq!(brush.dab_distance(1.0), 0.5);
    }

    #[test]
    fn hard_dab_is_solid_inside_and_empty_outside() {
        assert_eq!(dab_coverage(0.0, 10.0, 1.0), 1.0);
        assert_eq!(dab_coverage(9.0, 10.0, 1.0), 1.0);
        assert_eq!(dab_coverage(11.0, 10.0, 1.0), 0.0);
        // Right at the edge it is half covered: a smooth, anti-aliased edge.
        assert!((dab_coverage(10.0, 10.0, 1.0) - 0.5).abs() < 0.01);
    }

    #[test]
    fn soft_dab_fades_out_gradually() {
        let mut previous = dab_coverage(0.0, 20.0, 0.0);
        assert!(previous > 0.99);
        for step in 1..=21 {
            let coverage = dab_coverage(step as f32, 20.0, 0.0);
            assert!(coverage <= previous, "coverage should never grow outwards");
            previous = coverage;
        }
        assert_eq!(previous, 0.0);
        // Halfway out, a fully soft brush is clearly see-through.
        let middle = dab_coverage(10.0, 20.0, 0.0);
        assert!(middle > 0.2 && middle < 0.8, "{middle}");
    }

    #[test]
    fn paper_grain_is_spread_evenly_and_runs_along_the_fibres() {
        let grain = &*GRAIN;
        // Evenly spread: a threshold of 0.3 leaves 30% of the paper white.
        let below = grain.heights.iter().filter(|&&h| h < 0.3).count();
        let share = below as f32 / grain.heights.len() as f32;
        assert!((share - 0.3).abs() < 0.005, "{share}");
        // Streaky: neighbours along a fibre are more alike than across.
        let (mut along, mut across) = (0.0, 0.0);
        for y in 0..GRAIN_SIZE as i32 {
            for x in 0..GRAIN_SIZE as i32 {
                along += (grain.at(x + 1, y) - grain.at(x, y)).abs();
                across += (grain.at(x, y + 1) - grain.at(x, y)).abs();
            }
        }
        assert!(across > along * 1.3, "along {along}, across {across}");
    }

    #[test]
    fn pressing_harder_leaves_less_paper_white() {
        let pencil = BrushSettings {
            grain: 0.75,
            pressure_curve: 1.4,
            ..BrushSettings::default()
        };
        let white = |pressure: f32| {
            let tooth = pencil.tooth(pressure).unwrap();
            let mut white = 0;
            for y in 0..200 {
                for x in 0..200 {
                    white += usize::from(tooth.at(x, y, 1.0, 0.0) < 0.5);
                }
            }
            white as f32 / 40_000.0
        };
        assert!(white(0.2) > 0.45, "light: {}", white(0.2));
        assert!(white(1.0) < 0.06, "hard: {}", white(1.0));
        assert!(
            BrushSettings::default().tooth(1.0).is_none(),
            "no grain, no tooth"
        );
    }
}
