//! Shapes for painting scripts: outlines of rectangles and ellipses,
//! smooth curves, filling a shape with smooth edges, hatching lines, and
//! gentle random wobble.
//!
//! Every shape is turned into a list of points around its edge (a
//! polygon). Curves get enough points that the corners between them are
//! too small to see.

use std::f32::consts::TAU;

use crate::fill::Mask;
use crate::random::Random;
use crate::stroke::{StrokePoint, curve_point};
use crate::surface::PixelRect;

/// A point on the canvas, in pixels.
pub type Pt = (f32, f32);

/// Curves are cut into straight pieces about this long (in pixels).
const PIECE: f32 = 2.0;

/// The smallest rectangle around `points`: (left, top, right, bottom).
pub fn bounds(points: &[Pt]) -> (f32, f32, f32, f32) {
    points.iter().fold(
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
        |(l, t, r, b), &(x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
    )
}

/// Turns `point` around `centre` by `degrees` (clockwise on screen).
fn rotate(point: Pt, centre: Pt, degrees: f32) -> Pt {
    if degrees == 0.0 {
        return point;
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (point.0 - centre.0, point.1 - centre.1);
    (
        centre.0 + dx * cos - dy * sin,
        centre.1 + dx * sin + dy * cos,
    )
}

/// The outline of an ellipse: radii `rx` and `ry`, turned by `rotation` degrees.
pub fn ellipse_outline(centre: Pt, rx: f32, ry: f32, rotation: f32) -> Vec<Pt> {
    // Roughly the distance around it, to choose how many points to use.
    let around = TAU * ((rx * rx + ry * ry) / 2.0).sqrt();
    let count = ((around / PIECE).ceil() as usize).clamp(24, 4096);
    (0..count)
        .map(|i| {
            let angle = i as f32 / count as f32 * TAU;
            let point = (centre.0 + rx * angle.cos(), centre.1 + ry * angle.sin());
            rotate(point, centre, rotation)
        })
        .collect()
}

/// The outline of a rectangle with corners rounded by `radius`, turned by
/// `rotation` degrees around its middle.
pub fn rect_outline(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    radius: f32,
    rotation: f32,
) -> Vec<Pt> {
    let centre = (x + width / 2.0, y + height / 2.0);
    let radius = radius.clamp(0.0, width.abs().min(height.abs()) / 2.0);
    let points = if radius <= 0.0 {
        vec![
            (x, y),
            (x + width, y),
            (x + width, y + height),
            (x, y + height),
        ]
    } else {
        // A quarter circle at each corner, going round clockwise.
        let steps = ((TAU * radius / 4.0 / PIECE).ceil() as usize).clamp(4, 512);
        let corners = [
            ((x + width - radius, y + radius), -90.0_f32),
            ((x + width - radius, y + height - radius), 0.0),
            ((x + radius, y + height - radius), 90.0),
            ((x + radius, y + radius), 180.0),
        ];
        corners
            .iter()
            .flat_map(|&(c, start)| {
                (0..=steps).map(move |i| {
                    let angle = (start + 90.0 * i as f32 / steps as f32).to_radians();
                    (c.0 + radius * angle.cos(), c.1 + radius * angle.sin())
                })
            })
            .collect()
    };
    points
        .into_iter()
        .map(|p| rotate(p, centre, rotation))
        .collect()
}

/// A smooth curve through `points` (with their pressures), as many short
/// pieces. With `closed`, the curve also comes back round to the start.
/// Uses the same curve as brush strokes, so it never overshoots or loops.
pub fn smooth_path(points: &[StrokePoint], closed: bool) -> Vec<StrokePoint> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let at = |i: isize| -> StrokePoint {
        if closed {
            points[i.rem_euclid(n as isize) as usize]
        } else {
            points[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let segments = if closed { n } else { n - 1 };
    let mut out = vec![points[0]];
    for i in 0..segments as isize {
        let (a, b, c, d) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let pieces = (b.distance_to(&c) / PIECE).ceil().max(1.0) as usize;
        for piece in 1..=pieces {
            let t = piece as f32 / pieces as f32;
            let (x, y) = curve_point(a, b, c, d, t);
            out.push(StrokePoint::new(
                x,
                y,
                b.pressure + (c.pressure - b.pressure) * t,
            ));
        }
    }
    out
}

/// Adds points along straight lines between `points` (closing the loop if
/// `closed`), at most `PIECE` apart, so wobble and tapering are smooth.
pub fn densify(points: &[Pt], closed: bool) -> Vec<Pt> {
    let mut out = Vec::new();
    let n = points.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segments {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let pieces = ((b.0 - a.0).hypot(b.1 - a.1) / PIECE).ceil().max(1.0) as usize;
        for piece in 0..pieces {
            let t = piece as f32 / pieces as f32;
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    if !closed && let Some(&last) = points.last() {
        out.push(last);
    }
    out
}

/// How far along a list of points each one is: 0 for the first, the
/// total length for the last.
pub fn distances(points: &[StrokePoint]) -> Vec<f32> {
    let mut total = 0.0;
    let mut out = Vec::with_capacity(points.len());
    for (i, point) in points.iter().enumerate() {
        if i > 0 {
            total += points[i - 1].distance_to(point);
        }
        out.push(total);
    }
    out
}

/// Thins a line towards its ends: multiplies each point's pressure by a
/// factor that rises smoothly from 0 to 1 over the first `start` part of
/// the length (0..1), and falls back to 0 over the last `end` part.
pub fn apply_taper(points: &mut [StrokePoint], start: f32, end: f32) {
    let along = distances(points);
    let total = along.last().copied().unwrap_or(0.0);
    if total <= 0.0 {
        return;
    }
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    for (point, &d) in points.iter_mut().zip(&along) {
        let t = d / total;
        let mut factor = 1.0_f32;
        if start > 0.0 {
            factor = factor.min(smooth(t / start));
        }
        if end > 0.0 {
            factor = factor.min(smooth((1.0 - t) / end));
        }
        // Never exactly 0, so the very tip still shows a fine point.
        point.pressure *= factor.max(0.05);
    }
}

/// Moves points by a gentle, smooth random wobble of up to `amount`
/// pixels, changing direction about every `wavelength` pixels along the
/// line. A closed outline wobbles back to where it started.
pub fn wobble(
    points: &mut [StrokePoint],
    amount: f32,
    wavelength: f32,
    closed: bool,
    random: &mut Random,
) {
    if amount <= 0.0 || points.len() < 2 {
        return;
    }
    let along = distances(points);
    let total = along.last().copied().unwrap_or(0.0).max(1e-3);
    let knots = ((total / wavelength.max(1.0)).ceil() as usize).max(1);
    let mut offsets: Vec<Pt> = (0..=knots)
        .map(|_| (random.range(-amount, amount), random.range(-amount, amount)))
        .collect();
    if closed {
        offsets[knots] = offsets[0];
    }
    for (point, &d) in points.iter_mut().zip(&along) {
        let position = d / total * knots as f32;
        let i = (position.floor() as usize).min(knots - 1);
        let t = position - i as f32;
        // Cosine easing: no sharp corners where the direction changes.
        let w = (1.0 - (t * std::f32::consts::PI).cos()) / 2.0;
        let (a, b) = (offsets[i], offsets[i + 1]);
        point.x += a.0 + (b.0 - a.0) * w;
        point.y += a.1 + (b.1 - a.1) * w;
    }
}

/// How many slices each pixel row is cut into to work out smooth edges.
const SUBROWS: usize = 16;

/// Which pixels a polygon covers, with smooth edges: 255 inside, 0
/// outside, and in between on the edge. Overlapping or crossing parts
/// count as inside (the "non-zero" rule), so a star drawn in one go is
/// filled completely.
pub fn polygon_mask(points: &[Pt], limits: PixelRect) -> Mask {
    if points.len() < 3 {
        return Mask::new(PixelRect::new(0, 0, 0, 0));
    }
    let (l, t, r, b) = bounds(points);
    let rect = PixelRect::new(
        l.floor() as i32,
        t.floor() as i32,
        r.ceil() as i32,
        b.ceil() as i32,
    )
    .intersect(&limits);
    let mut mask = Mask::new(rect);
    if rect.is_empty() {
        return mask;
    }

    // Each edge: from (x0, y0) to (x1, y1), and whether it goes down (+1)
    // or up (-1). Flat edges never cross a row, so they're left out.
    let n = points.len();
    let edges: Vec<(f32, f32, f32, f32, i32)> = (0..n)
        .filter_map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            (a.1 != b.1).then_some((a.0, a.1, b.0, b.1, if b.1 > a.1 { 1 } else { -1 }))
        })
        .collect();

    let width = (rect.right - rect.left) as usize;
    let mut coverage = vec![0.0_f32; width];
    let mut crossings: Vec<(f32, i32)> = Vec::new();
    for y in rect.top..rect.bottom {
        let (row_top, row_bottom) = (y as f32, y as f32 + 1.0);
        let row_edges: Vec<_> = edges
            .iter()
            .filter(|e| e.1.min(e.3) < row_bottom && e.1.max(e.3) > row_top)
            .collect();
        if row_edges.is_empty() {
            continue;
        }
        coverage.fill(0.0);
        for sub in 0..SUBROWS {
            let sy = row_top + (sub as f32 + 0.5) / SUBROWS as f32;
            crossings.clear();
            for &&(x0, y0, x1, y1, direction) in &row_edges {
                let (top, bottom) = (y0.min(y1), y0.max(y1));
                if sy >= top && sy < bottom {
                    let x = x0 + (sy - y0) * (x1 - x0) / (y1 - y0);
                    crossings.push((x, direction));
                }
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Walk left to right, counting how many times we're "inside".
            let mut winding = 0;
            let mut span_start = 0.0;
            for &(x, direction) in &crossings {
                let was_inside = winding != 0;
                winding += direction;
                if !was_inside && winding != 0 {
                    span_start = x;
                } else if was_inside && winding == 0 {
                    add_span(
                        &mut coverage,
                        span_start - rect.left as f32,
                        x - rect.left as f32,
                    );
                }
            }
        }
        for (x, &c) in coverage.iter().enumerate() {
            if c > 0.0 {
                let value = (c / SUBROWS as f32 * 255.0).round().min(255.0) as u8;
                mask.set(rect.left + x as i32, y, value);
            }
        }
    }
    mask
}

/// Adds the part of each pixel that lies between `from` and `to` (in
/// pixels from the left of the row) to `coverage`.
fn add_span(coverage: &mut [f32], from: f32, to: f32) {
    let from = from.max(0.0);
    let to = to.min(coverage.len() as f32);
    if to <= from {
        return;
    }
    let first = from.floor() as usize;
    let last = (to.ceil() as usize).min(coverage.len());
    for (i, value) in coverage.iter_mut().enumerate().take(last).skip(first) {
        let covered = to.min(i as f32 + 1.0) - from.max(i as f32);
        if covered > 0.0 {
            *value += covered;
        }
    }
}

/// True if `point` is inside the polygon (non-zero rule).
pub fn contains(polygon: &[Pt], point: Pt) -> bool {
    let n = polygon.len();
    let mut winding = 0;
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        let side = (b.0 - a.0) * (point.1 - a.1) - (point.0 - a.0) * (b.1 - a.1);
        if a.1 <= point.1 {
            if b.1 > point.1 && side > 0.0 {
                winding += 1;
            }
        } else if b.1 <= point.1 && side < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}

/// Parallel lines `spacing` apart at `angle` degrees (0 = left to right,
/// 90 = top to bottom), cut to the inside of the polygon. Each line is
/// returned as its two ends.
pub fn hatch_lines(polygon: &[Pt], angle: f32, spacing: f32) -> Vec<(Pt, Pt)> {
    let n = polygon.len();
    if n < 3 || spacing <= 0.0 {
        return Vec::new();
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let along = (cos, sin); // the lines' direction
    let across = (-sin, cos); // the direction from one line to the next
    let dot = |p: Pt, d: Pt| p.0 * d.0 + p.1 * d.1;
    let (low, high) = polygon.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &p| {
        (lo.min(dot(p, across)), hi.max(dot(p, across)))
    });

    let mut lines = Vec::new();
    let mut offset = low + spacing / 2.0;
    let mut hits: Vec<(f32, i32)> = Vec::new();
    while offset < high {
        hits.clear();
        for i in 0..n {
            let (a, b) = (polygon[i], polygon[(i + 1) % n]);
            let (sa, sb) = (dot(a, across) - offset, dot(b, across) - offset);
            if (sa <= 0.0) != (sb <= 0.0) {
                let t = sa / (sa - sb);
                let hit = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                hits.push((dot(hit, along), if sb > sa { 1 } else { -1 }));
            }
        }
        hits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut winding = 0;
        let mut start = 0.0;
        for &(position, direction) in &hits {
            let was_inside = winding != 0;
            winding += direction;
            if !was_inside && winding != 0 {
                start = position;
            } else if was_inside && winding == 0 {
                // Back from (along, across) coordinates to canvas pixels.
                let point = |u: f32| {
                    (
                        u * along.0 + offset * across.0,
                        u * along.1 + offset * across.1,
                    )
                };
                lines.push((point(start), point(position)));
            }
        }
        offset += spacing;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, size: f32) -> Vec<Pt> {
        vec![(x, y), (x + size, y), (x + size, y + size), (x, y + size)]
    }

    #[test]
    fn a_pixel_aligned_square_is_exactly_covered() {
        let mask = polygon_mask(&square(10.0, 10.0, 20.0), PixelRect::new(0, 0, 100, 100));
        assert_eq!(mask.covered_pixels(), 400);
        assert_eq!(mask.get(10, 10), 255);
        assert_eq!(mask.get(29, 29), 255);
        assert_eq!(mask.get(30, 30), 0);
    }

    #[test]
    fn edges_between_pixels_are_partly_covered() {
        // From x = 10.5: the pixel column at x = 10 is half inside.
        let mask = polygon_mask(&square(10.5, 10.0, 20.0), PixelRect::new(0, 0, 100, 100));
        assert_eq!(mask.get(10, 15), 128);
        assert_eq!(mask.get(11, 15), 255);
        assert_eq!(mask.get(30, 15), 128);
    }

    #[test]
    fn shapes_are_cut_to_the_limits() {
        let mask = polygon_mask(&square(-10.0, -10.0, 30.0), PixelRect::new(0, 0, 100, 100));
        assert_eq!(mask.rect(), PixelRect::new(0, 0, 20, 20));
        assert_eq!(mask.covered_pixels(), 400);
    }

    #[test]
    fn a_star_drawn_in_one_go_is_filled_in_the_middle() {
        // A five-pointed star whose lines cross: the middle counts as inside.
        let star: Vec<Pt> = (0..5)
            .map(|i| {
                let a = (i as f32 * 144.0 - 90.0).to_radians();
                (50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin())
            })
            .collect();
        let mask = polygon_mask(&star, PixelRect::new(0, 0, 100, 100));
        assert_eq!(mask.get(50, 50), 255);
        assert!(contains(&star, (50.0, 50.0)));
    }

    #[test]
    fn an_ellipse_covers_about_pi_r_squared() {
        let outline = ellipse_outline((50.0, 50.0), 30.0, 30.0, 0.0);
        let mask = polygon_mask(&outline, PixelRect::new(0, 0, 100, 100));
        let area: f32 = (0..100)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .map(|(x, y)| f32::from(mask.get(x, y)) / 255.0)
            .sum();
        let expected = std::f32::consts::PI * 30.0 * 30.0;
        assert!(
            (area - expected).abs() / expected < 0.005,
            "{area} vs {expected}"
        );
    }

    #[test]
    fn rotating_a_rectangle_by_90_degrees_swaps_its_sides() {
        let outline = rect_outline(0.0, 0.0, 40.0, 20.0, 0.0, 90.0);
        let (l, t, r, b) = bounds(&outline);
        assert!(((r - l) - 20.0).abs() < 1e-3 && ((b - t) - 40.0).abs() < 1e-3);
    }

    #[test]
    fn rounded_corners_cut_the_corners_off() {
        let outline = rect_outline(0.0, 0.0, 100.0, 100.0, 20.0, 0.0);
        assert!(!contains(&outline, (1.0, 1.0)), "the corner is cut away");
        assert!(
            contains(&outline, (50.0, 1.0)),
            "the middle of an edge is not"
        );
    }

    #[test]
    fn a_smooth_path_passes_through_its_points() {
        let points = [
            StrokePoint::new(0.0, 0.0, 1.0),
            StrokePoint::new(50.0, 40.0, 1.0),
            StrokePoint::new(100.0, 0.0, 1.0),
        ];
        let path = smooth_path(&points, false);
        for p in points {
            assert!(path.iter().any(|q| q.distance_to(&p) < 0.01), "{p:?}");
        }
        // Pieces are short (cut from the straight distance, so on a bend
        // they can be a little longer than `PIECE`).
        for pair in path.windows(2) {
            assert!(pair[0].distance_to(&pair[1]) <= PIECE * 1.6);
        }
    }

    #[test]
    fn tapering_thins_both_ends() {
        let mut points: Vec<StrokePoint> = (0..=100)
            .map(|i| StrokePoint::new(i as f32, 0.0, 1.0))
            .collect();
        apply_taper(&mut points, 0.2, 0.2);
        assert!(points[0].pressure < 0.1);
        assert_eq!(points[50].pressure, 1.0);
        assert!(points[100].pressure < 0.1);
        assert!(points[10].pressure > 0.2 && points[10].pressure < 0.8);
    }

    #[test]
    fn wobble_is_the_same_with_the_same_seed_and_stays_within_the_amount() {
        let line: Vec<StrokePoint> = (0..=200)
            .map(|i| StrokePoint::new(i as f32, 0.0, 1.0))
            .collect();
        let mut a = line.clone();
        let mut b = line.clone();
        wobble(&mut a, 5.0, 30.0, false, &mut Random::new(3));
        wobble(&mut b, 5.0, 30.0, false, &mut Random::new(3));
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p.y.abs() <= 5.0));
        assert!(a.iter().any(|p| p.y.abs() > 0.5), "it did move");
    }

    #[test]
    fn hatching_a_square_gives_evenly_spaced_lines_inside_it() {
        let lines = hatch_lines(&square(0.0, 0.0, 100.0), 0.0, 10.0);
        assert_eq!(lines.len(), 10);
        for ((x0, y0), (x1, y1)) in lines {
            assert!((x0 - 0.0).abs() < 1e-3 && (x1 - 100.0).abs() < 1e-3);
            assert!((y0 - y1).abs() < 1e-3 && (5.0..100.0).contains(&y0));
        }
        // Diagonal lines are all inside too.
        let lines = hatch_lines(&square(0.0, 0.0, 100.0), 45.0, 10.0);
        assert!(lines.len() > 10);
        for (a, b) in lines {
            for p in [a, b] {
                assert!(
                    (-0.01..=100.01).contains(&p.0) && (-0.01..=100.01).contains(&p.1),
                    "{p:?}"
                );
            }
        }
    }
}
