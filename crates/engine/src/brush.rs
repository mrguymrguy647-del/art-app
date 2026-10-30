//! Brush settings and the shape of one brush "dab".
//!
//! A brush stroke is made of many round dabs stamped close together along
//! the path of the pen (see `stroke.rs`). This file says how big, how
//! opaque and how soft each dab is.

/// Smallest and largest brush size (diameter in canvas pixels).
pub const MIN_SIZE: f32 = 1.0;
pub const MAX_SIZE: f32 = 1000.0;

/// Everything about how the brush paints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
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
    pub color: [u8; 3],
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: 24.0,
            opacity: 1.0,
            flow: 1.0,
            hardness: 0.8,
            spacing: 0.1,
            pressure_size: true,
            pressure_opacity: true,
            pressure_flow: false,
            color: [20, 20, 20],
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
        let pressure = pressure.clamp(0.0, 1.0);
        let scale = |value: f32, use_pressure: bool| {
            if use_pressure {
                value * pressure
            } else {
                value
            }
        };
        Dab {
            // Never smaller than one pixel across, or very light strokes vanish.
            radius: (scale(self.size, self.pressure_size) / 2.0).max(0.5),
            opacity: scale(self.opacity, self.pressure_opacity),
            flow: scale(self.flow, self.pressure_flow),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
