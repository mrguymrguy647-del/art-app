//! Steadier lines ("stabilizer").
//!
//! The drawn line follows the pen as if pulled along on a string: while the
//! pen moves within the string's length, the line doesn't move; once the
//! string is tight, the line is pulled along behind the pen. Small shakes
//! never reach the line, and curves come out smooth. A longer string means
//! steadier lines, but the line trails a little further behind the pen.
//!
//! This only depends on distance, not time, so it feels the same with a
//! mouse or a fast pen.

use crate::stroke::StrokePoint;

pub struct Smoother {
    string_length: f32,
    /// Where the end of the line is now.
    position: Option<StrokePoint>,
}

impl Smoother {
    /// `string_length` is in canvas pixels. 0 = no smoothing.
    pub fn new(string_length: f32) -> Self {
        Self {
            string_length: string_length.max(0.0),
            position: None,
        }
    }

    /// The pen moved to `pen`. Returns where the line should be drawn to,
    /// or `None` if the string isn't tight yet, so nothing moves.
    pub fn add(&mut self, pen: StrokePoint) -> Option<StrokePoint> {
        let Some(current) = self.position else {
            // The first point: the line starts exactly where the pen landed.
            self.position = Some(pen);
            return Some(pen);
        };

        let distance = current.distance_to(&pen);
        if distance <= self.string_length {
            return None;
        }
        // Move towards the pen until we are exactly one string length away.
        let pull = (distance - self.string_length) / distance;
        let moved = StrokePoint::new(
            current.x + (pen.x - current.x) * pull,
            current.y + (pen.y - current.y) * pull,
            pen.pressure,
        );
        self.position = Some(moved);
        Some(moved)
    }

    /// The pen was lifted at `pen`. Returns the last point to draw to, so
    /// the line ends where the pen did.
    pub fn finish(&mut self, pen: StrokePoint) -> Option<StrokePoint> {
        let current = self.position?;
        if current.distance_to(&pen) < 0.01 {
            return None;
        }
        self.position = Some(pen);
        Some(pen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> StrokePoint {
        StrokePoint::new(x, y, 1.0)
    }

    #[test]
    fn without_smoothing_points_pass_straight_through() {
        let mut smoother = Smoother::new(0.0);
        assert_eq!(smoother.add(point(0.0, 0.0)), Some(point(0.0, 0.0)));
        assert_eq!(smoother.add(point(3.0, 4.0)), Some(point(3.0, 4.0)));
    }

    #[test]
    fn small_shakes_do_not_move_the_line() {
        let mut smoother = Smoother::new(10.0);
        smoother.add(point(0.0, 0.0));
        assert_eq!(smoother.add(point(3.0, 0.0)), None);
        assert_eq!(smoother.add(point(-2.0, 4.0)), None);
    }

    #[test]
    fn the_line_trails_one_string_length_behind() {
        let mut smoother = Smoother::new(10.0);
        smoother.add(point(0.0, 0.0));
        let moved = smoother.add(point(30.0, 0.0)).expect("string is tight");
        assert!((moved.x - 20.0).abs() < 1e-4 && moved.y == 0.0, "{moved:?}");
    }

    #[test]
    fn finishing_catches_up_with_the_pen() {
        let mut smoother = Smoother::new(10.0);
        smoother.add(point(0.0, 0.0));
        smoother.add(point(30.0, 0.0));
        assert_eq!(smoother.finish(point(30.0, 0.0)), Some(point(30.0, 0.0)));
        // Already there: nothing more to draw.
        assert_eq!(smoother.finish(point(30.0, 0.0)), None);
    }
}
