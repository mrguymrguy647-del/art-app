//! Painting one stroke, from pen down to pen up.
//!
//! The pen only reports its position every few milliseconds, so a fast
//! stroke gives points far apart. We join the points with a smooth curve
//! (not straight lines, which would make fast curves look angular) and
//! stamp dabs at even distances along it, with the pressure blending
//! smoothly too. This filling-in is called "interpolation".
//!
//! While painting, the stroke keeps a "coverage" value per pixel (0..1):
//! how strongly the stroke covers it so far. Each pixel's final colour is
//! the brush colour laid over the pixel *as it was before the stroke*, as
//! strong as its coverage. That's why going over the same spot again
//! doesn't make it darker than the brush opacity.

use std::collections::HashMap;

use crate::brush::{BrushSettings, dab_coverage};
use crate::surface::{PixelRect, Surface, TileCoord};
use crate::tile::{Rgba8, TILE_PIXELS, TILE_SIZE, Tile};

/// A point along a stroke: position in canvas pixels, and pen pressure (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

impl StrokePoint {
    pub const fn new(x: f32, y: f32, pressure: f32) -> Self {
        Self { x, y, pressure }
    }

    pub fn distance_to(&self, other: &StrokePoint) -> f32 {
        (other.x - self.x).hypot(other.y - self.y)
    }
}

/// The curve between two points is drawn as short straight pieces of at
/// most this many pixels: too short to see the corners.
const CURVE_PIECE_LENGTH: f32 = 2.0;

/// The tiles a stroke changed, as they were before it. `None` means the
/// tile didn't exist yet. Undo (Step 5) will put these back.
pub type TilesBefore = HashMap<TileCoord, Option<Tile>>;

/// A stroke being painted.
pub struct Stroke {
    brush: BrushSettings,
    /// Never paint outside this rectangle (the canvas).
    limits: PixelRect,
    /// How strongly this stroke covers each pixel so far (0..1), per tile.
    coverage: HashMap<TileCoord, Box<[f32]>>,
    before: TilesBefore,
    /// The last few points we were given (at most 4), to shape the curve.
    recent: Vec<StrokePoint>,
    /// How far the painted path has got.
    path_end: Option<StrokePoint>,
    /// How far past `path_end` (along the path) the next dab goes.
    next_dab_in: f32,
}

impl Stroke {
    /// Starts a stroke that only paints inside `limits` (normally the
    /// canvas, `Document::bounds`).
    pub fn new(brush: BrushSettings, limits: PixelRect) -> Self {
        Self {
            brush,
            limits,
            coverage: HashMap::new(),
            before: HashMap::new(),
            recent: Vec::with_capacity(5),
            path_end: None,
            next_dab_in: 0.0,
        }
    }

    /// Continues the stroke to `point`.
    ///
    /// The curve's shape near a point depends on the point after it, so
    /// the line is drawn up to the *previous* point; `finish` draws the rest.
    pub fn add_point(&mut self, layer: &mut Surface, point: StrokePoint) {
        self.recent.push(point);
        if self.recent.len() > 4 {
            self.recent.remove(0);
        }

        if self.path_end.is_none() {
            // The very first point: one dab right where the pen landed.
            self.stamp(layer, point);
            self.path_end = Some(point);
            self.next_dab_in = self.dab_distance(point.pressure);
            return;
        }

        // With points ... p0 p1 p2 p3 (p3 newest), draw the curve p1 -> p2.
        // At the very start there is no p0 yet, so p1 stands in for it. That
        // makes the first piece a little flatter, but it never loops, however
        // unevenly the pen moves.
        let n = self.recent.len();
        if n >= 3 {
            let before = self.recent[n.saturating_sub(4)];
            let (from, to, after) = (self.recent[n - 3], self.recent[n - 2], self.recent[n - 1]);
            self.draw_curve(layer, before, from, to, after);
        }
    }

    /// Ends the stroke: draws the last piece of the line and returns the
    /// changed tiles as they were before the stroke.
    pub fn finish(mut self, layer: &mut Surface) -> TilesBefore {
        let n = self.recent.len();
        if n >= 2 {
            let before = self.recent[n.saturating_sub(3)];
            let (from, to) = (self.recent[n - 2], self.recent[n - 1]);
            self.draw_curve(layer, before, from, to, to);
        }
        self.before
    }

    /// Paints the smooth curve from `from` to `to`, shaped by the points
    /// just before and after them.
    fn draw_curve(
        &mut self,
        layer: &mut Surface,
        before: StrokePoint,
        from: StrokePoint,
        to: StrokePoint,
        after: StrokePoint,
    ) {
        let pieces = (from.distance_to(&to) / CURVE_PIECE_LENGTH).ceil().max(1.0) as usize;
        for piece in 1..=pieces {
            let t = piece as f32 / pieces as f32;
            let (x, y) = curve_point(before, from, to, after, t);
            let pressure = from.pressure + (to.pressure - from.pressure) * t;
            self.paint_line_to(layer, StrokePoint::new(x, y, pressure));
        }
    }

    /// Stamps dabs at even distances along the straight line from the end
    /// of the path so far to `point`.
    fn paint_line_to(&mut self, layer: &mut Surface, point: StrokePoint) {
        let Some(from) = self.path_end else {
            return;
        };
        let length = from.distance_to(&point);
        let mut along = self.next_dab_in;
        while along <= length {
            // `t` goes from 0 (at `from`) to 1 (at `point`).
            let t = along / length;
            let dab_point = StrokePoint::new(
                from.x + (point.x - from.x) * t,
                from.y + (point.y - from.y) * t,
                from.pressure + (point.pressure - from.pressure) * t,
            );
            self.stamp(layer, dab_point);
            along += self.dab_distance(dab_point.pressure);
        }
        self.next_dab_in = along - length;
        self.path_end = Some(point);
    }

    fn dab_distance(&self, pressure: f32) -> f32 {
        self.brush.dab_distance(self.brush.dab(pressure).radius)
    }

    /// Stamps one dab centred on `at`.
    fn stamp(&mut self, layer: &mut Surface, at: StrokePoint) {
        let dab = self.brush.dab(at.pressure);
        if dab.opacity <= 0.0 || dab.flow <= 0.0 {
            return;
        }
        let reach = dab.radius + 1.0; // the soft edge can go half a pixel further
        let rect = PixelRect::new(
            (at.x - reach).floor() as i32,
            (at.y - reach).floor() as i32,
            (at.x + reach).ceil() as i32,
            (at.y + reach).ceil() as i32,
        )
        .intersect(&self.limits);

        for coord in rect.tiles() {
            let (left, top) = coord.origin();
            let size = TILE_SIZE as i32;
            let area = rect.intersect(&PixelRect::new(left, top, left + size, top + size));
            if !circle_touches(&area, at, reach) {
                continue; // a corner of the dab's box that the round dab misses
            }

            // 1. Raise this stroke's coverage where the dab is stronger.
            let coverage = self
                .coverage
                .entry(coord)
                .or_insert_with(|| vec![0.0; TILE_PIXELS].into_boxed_slice());
            let mut changed = false;
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    // Measure from the pixel's centre, hence + 0.5.
                    let distance = (x as f32 + 0.5 - at.x).hypot(y as f32 + 0.5 - at.y);
                    let target =
                        dab.opacity * dab_coverage(distance, dab.radius, self.brush.hardness);
                    let i = tile_index(x - left, y - top);
                    if target > coverage[i] {
                        // Flow 1: jump straight to the target. Lower flow:
                        // move part of the way, so paint builds up.
                        coverage[i] += (target - coverage[i]) * dab.flow;
                        changed = true;
                    }
                }
            }
            if !changed {
                continue;
            }

            // 2. Remember the tile as it was before the stroke (first time only).
            let before = self
                .before
                .entry(coord)
                .or_insert_with(|| layer.tile(coord).cloned())
                .as_ref();

            // 3. Lay the colour over the original pixels, as strong as the coverage.
            let tile = layer.tile_mut(coord);
            for y in area.top..area.bottom {
                for x in area.left..area.right {
                    let (tile_x, tile_y) = ((x - left) as usize, (y - top) as usize);
                    let original = before.map_or(Rgba8::TRANSPARENT, |t| t.pixel(tile_x, tile_y));
                    let strength = coverage[tile_index(x - left, y - top)];
                    tile.set_pixel(
                        tile_x,
                        tile_y,
                        paint_over(original, self.brush.color, strength),
                    );
                }
            }
        }
    }
}

/// A point on the smooth curve from `b` to `c` (`t` from 0 to 1), shaped by
/// the neighbouring points `a` and `d`.
///
/// This is a "centripetal Catmull-Rom spline": it passes exactly through
/// every point, and unlike simpler curves it doesn't overshoot or make
/// little loops at sharp turns. It's built from repeated in-between steps
/// ("lerps"), each one weighted by the distances between the points.
fn curve_point(
    a: StrokePoint,
    b: StrokePoint,
    c: StrokePoint,
    d: StrokePoint,
    t: f32,
) -> (f32, f32) {
    // How "long" each gap counts as: the square root of its length.
    // (Never exactly 0, so repeated points don't divide by zero.)
    let gap = |p: StrokePoint, q: StrokePoint| p.distance_to(&q).sqrt().max(1e-3);
    let t0 = 0.0;
    let t1 = t0 + gap(a, b);
    let t2 = t1 + gap(b, c);
    let t3 = t2 + gap(c, d);
    let t = t1 + (t2 - t1) * t;

    // Moves from p (at time `start`) to q (at time `end`), as far as time `t`.
    let lerp = |p: (f32, f32), q: (f32, f32), start: f32, end: f32| {
        let w = (t - start) / (end - start);
        (p.0 + (q.0 - p.0) * w, p.1 + (q.1 - p.1) * w)
    };
    let (a, b, c, d) = ((a.x, a.y), (b.x, b.y), (c.x, c.y), (d.x, d.y));
    let ab = lerp(a, b, t0, t1);
    let bc = lerp(b, c, t1, t2);
    let cd = lerp(c, d, t2, t3);
    let abc = lerp(ab, bc, t0, t2);
    let bcd = lerp(bc, cd, t1, t3);
    lerp(abc, bcd, t1, t2)
}

/// Position of a pixel inside a tile's pixel list.
fn tile_index(x: i32, y: i32) -> usize {
    y as usize * TILE_SIZE + x as usize
}

/// True if a circle around `centre` reaches into `area`.
fn circle_touches(area: &PixelRect, centre: StrokePoint, radius: f32) -> bool {
    // The point of the area closest to the centre.
    let nearest_x = centre.x.clamp(area.left as f32, area.right as f32);
    let nearest_y = centre.y.clamp(area.top as f32, area.bottom as f32);
    (nearest_x - centre.x).hypot(nearest_y - centre.y) <= radius
}

/// `color` laid over `under` with the given strength (0..1).
///
/// Both pixels are premultiplied, so this is simply
/// "paint * strength + what was under * (1 - strength)".
fn paint_over(under: Rgba8, color: [u8; 3], strength: f32) -> Rgba8 {
    let keep = 1.0 - strength;
    let mix = |paint: f32, under: u8| (paint * strength + f32::from(under) * keep).round() as u8;
    Rgba8::new(
        mix(f32::from(color[0]), under.r),
        mix(f32::from(color[1]), under.g),
        mix(f32::from(color[2]), under.b),
        mix(255.0, under.a),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: [u8; 3] = [0, 0, 0];

    fn hard_brush(size: f32) -> BrushSettings {
        BrushSettings {
            size,
            opacity: 1.0,
            flow: 1.0,
            hardness: 1.0,
            pressure_size: false,
            pressure_opacity: false,
            pressure_flow: false,
            color: BLACK,
            ..BrushSettings::default()
        }
    }

    /// The canvas used in these tests.
    const CANVAS: PixelRect = PixelRect::new(0, 0, 2000, 1000);

    fn paint(brush: BrushSettings, layer: &mut Surface, points: &[StrokePoint]) -> TilesBefore {
        let mut stroke = Stroke::new(brush, CANVAS);
        for &point in points {
            stroke.add_point(layer, point);
        }
        stroke.finish(layer)
    }

    #[test]
    fn a_single_tap_paints_one_round_dot() {
        let mut layer = Surface::new();
        paint(
            hard_brush(20.0),
            &mut layer,
            &[StrokePoint::new(100.0, 100.0, 1.0)],
        );

        assert_eq!(layer.pixel(100, 100), Rgba8::new(0, 0, 0, 255)); // centre
        assert_eq!(layer.pixel(100, 115), Rgba8::TRANSPARENT); // outside the radius of 10
        assert_eq!(layer.tile_count(), 1);
    }

    #[test]
    fn a_fast_stroke_has_no_gaps() {
        // Only two points, 490 pixels apart, like a very fast flick.
        let mut layer = Surface::new();
        let points = [
            StrokePoint::new(10.0, 50.0, 1.0),
            StrokePoint::new(500.0, 50.0, 1.0),
        ];
        paint(hard_brush(4.0), &mut layer, &points);

        for x in 10..500 {
            let alpha = layer.pixel(x, 49).a;
            assert!(alpha > 200, "gap at x = {x} (alpha {alpha})");
        }
    }

    #[test]
    fn going_over_the_same_spot_never_exceeds_the_opacity() {
        let brush = BrushSettings {
            opacity: 0.5,
            ..hard_brush(30.0)
        };
        let mut layer = Surface::new();
        // Scribble back and forth over the same place many times.
        let points: Vec<_> = (0..40)
            .map(|i| StrokePoint::new(if i % 2 == 0 { 100.0 } else { 140.0 }, 100.0, 1.0))
            .collect();
        paint(brush, &mut layer, &points);

        assert_eq!(layer.pixel(120, 100).a, 128); // 50% of 255
    }

    #[test]
    fn low_flow_builds_up_where_dabs_overlap() {
        let brush = BrushSettings {
            flow: 0.2,
            ..hard_brush(30.0)
        };

        let mut one_dab = Surface::new();
        paint(brush, &mut one_dab, &[StrokePoint::new(100.0, 100.0, 1.0)]);
        let after_one = one_dab.pixel(100, 100).a;
        assert!((48..=54).contains(&after_one), "{after_one}"); // about 20%

        let mut many_dabs = Surface::new();
        let points = [
            StrokePoint::new(90.0, 100.0, 1.0),
            StrokePoint::new(110.0, 100.0, 1.0),
        ];
        paint(brush, &mut many_dabs, &points);
        assert!(
            many_dabs.pixel(100, 100).a > after_one + 50,
            "paint should build up"
        );
    }

    #[test]
    fn half_transparent_paint_mixes_with_what_was_there() {
        let mut layer = Surface::new();
        layer.set_pixel(100, 100, Rgba8::new(255, 0, 0, 255)); // red
        let brush = BrushSettings {
            opacity: 0.5,
            ..hard_brush(20.0)
        };
        let points = [
            StrokePoint::new(95.0, 100.0, 1.0),
            StrokePoint::new(105.0, 100.0, 1.0),
        ];
        paint(brush, &mut layer, &points);

        // Half black over red, however many dabs overlapped there.
        assert_eq!(layer.pixel(100, 100), Rgba8::new(128, 0, 0, 255));
    }

    #[test]
    fn light_pressure_paints_a_thinner_line() {
        let brush = BrushSettings {
            pressure_size: true,
            ..hard_brush(40.0)
        };
        let mut layer = Surface::new();
        paint(brush, &mut layer, &[StrokePoint::new(100.0, 100.0, 0.5)]);

        assert_eq!(layer.pixel(100, 108).a, 255); // inside radius 10
        assert_eq!(layer.pixel(100, 115).a, 0); // outside radius 10 (full pressure would reach 20)
    }

    #[test]
    fn the_curve_passes_through_the_points() {
        let (a, b, c, d) = (
            StrokePoint::new(0.0, 0.0, 1.0),
            StrokePoint::new(10.0, 5.0, 1.0),
            StrokePoint::new(30.0, -5.0, 1.0),
            StrokePoint::new(40.0, 0.0, 1.0),
        );
        let start = curve_point(a, b, c, d, 0.0);
        let end = curve_point(a, b, c, d, 1.0);
        assert!(
            (start.0 - 10.0).abs() < 1e-3 && (start.1 - 5.0).abs() < 1e-3,
            "{start:?}"
        );
        assert!(
            (end.0 - 30.0).abs() < 1e-3 && (end.1 + 5.0).abs() < 1e-3,
            "{end:?}"
        );
    }

    #[test]
    fn fast_curves_come_out_round_not_angular() {
        // Eight points around a circle (radius 200), like a fast circular
        // scribble that only gives a reading every 45 degrees.
        let (cx, cy, radius) = (500.0_f32, 500.0_f32, 200.0_f32);
        let points: Vec<_> = (0..=8)
            .map(|i| {
                let angle = (i as f32 * 45.0).to_radians();
                StrokePoint::new(cx + radius * angle.cos(), cy + radius * angle.sin(), 1.0)
            })
            .collect();
        let mut layer = Surface::new();
        paint(hard_brush(6.0), &mut layer, &points);

        // Halfway between two readings (at 112.5 degrees) the line follows
        // the circle (within 2 px)...
        let angle = 112.5_f32.to_radians();
        let on_circle = (cx + radius * angle.cos(), cy + radius * angle.sin());
        assert_eq!(layer.pixel(on_circle.0 as i32, on_circle.1 as i32).a, 255);
        // ...instead of cutting the corner with a straight line (15 px inside).
        let chord = radius * 22.5_f32.to_radians().cos();
        let on_chord = (cx + chord * angle.cos(), cy + chord * angle.sin());
        assert_eq!(layer.pixel(on_chord.0 as i32, on_chord.1 as i32).a, 0);
    }

    #[test]
    fn nothing_is_painted_outside_the_canvas() {
        let mut layer = Surface::new();
        // A big dab right on the top-left corner of the canvas.
        paint(
            hard_brush(100.0),
            &mut layer,
            &[StrokePoint::new(0.0, 0.0, 1.0)],
        );

        assert_eq!(layer.pixel(0, 0).a, 255);
        assert_eq!(layer.pixel(-1, -1), Rgba8::TRANSPARENT);
        assert_eq!(
            layer.tile_count(),
            1,
            "no tiles above or left of the canvas"
        );
    }

    #[test]
    fn finish_returns_the_tiles_as_they_were_before() {
        let mut layer = Surface::new();
        layer.set_pixel(5, 5, Rgba8::WHITE);
        let points = [
            StrokePoint::new(5.0, 5.0, 1.0),
            StrokePoint::new(300.0, 5.0, 1.0),
        ];
        let before = paint(hard_brush(4.0), &mut layer, &points);

        // Tile (0,0) existed: it is saved with the white pixel, not the black stroke.
        let old = before[&TileCoord::new(0, 0)]
            .as_ref()
            .expect("tile (0,0) existed");
        assert_eq!(old.pixel(5, 5), Rgba8::WHITE);
        // Tile (1,0) didn't exist before the stroke.
        assert_eq!(before[&TileCoord::new(1, 0)], None);
        assert_eq!(before.len(), 2);
    }
}
