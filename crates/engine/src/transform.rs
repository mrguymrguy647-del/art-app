//! Moving, turning, resizing and mirroring drawings.
//!
//! A [`Transform`] takes a point in a drawing's own coordinates and says
//! where it lands on the canvas. Groups and symbols in painting scripts
//! use them, and so do quick previews (a preview is the whole picture
//! scaled down). Later, animation can change them from frame to frame.
//!
//! In maths terms it's an "affine transform": the new point is
//! `x' = a·x + c·y + e` and `y' = b·x + d·y + f`. Any mix of moving,
//! turning, resizing and mirroring can be written that way, and two
//! transforms combine into one.

/// Where points end up: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    /// Leaves every point where it is.
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Moves points by (x, y).
    pub fn translate(x: f32, y: f32) -> Self {
        Transform {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    /// Resizes around (0, 0): `sx` across, `sy` down.
    pub fn scale(sx: f32, sy: f32) -> Self {
        Transform {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    /// Turns around (0, 0) by `degrees`, clockwise on screen (where y
    /// grows downwards).
    pub fn rotate(degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Transform {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            e: 0.0,
            f: 0.0,
        }
    }

    /// The transform scripts describe: around `origin`, first mirror
    /// (left-right if `mirror_x`, up-down if `mirror_y`), then resize by
    /// `scale`, then turn by `rotate` degrees; then move by `translate`.
    pub fn about(
        origin: (f32, f32),
        translate: (f32, f32),
        scale: (f32, f32),
        rotate: f32,
        mirror_x: bool,
        mirror_y: bool,
    ) -> Self {
        let flip = Transform::scale(
            if mirror_x { -1.0 } else { 1.0 },
            if mirror_y { -1.0 } else { 1.0 },
        );
        Transform::translate(-origin.0, -origin.1)
            .then(flip)
            .then(Transform::scale(scale.0, scale.1))
            .then(Transform::rotate(rotate))
            .then(Transform::translate(
                origin.0 + translate.0,
                origin.1 + translate.1,
            ))
    }

    /// This transform followed by `next`, as one transform.
    pub fn then(self, next: Transform) -> Transform {
        Transform {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            e: next.a * self.e + next.c * self.f + next.e,
            f: next.b * self.e + next.d * self.f + next.f,
        }
    }

    /// `self` done `times` times in a row (`times` = 0 gives no change).
    pub fn repeated(self, times: usize) -> Transform {
        (0..times).fold(Transform::IDENTITY, |done, _| done.then(self))
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// Where the point (x, y) lands.
    pub fn apply(&self, (x, y): (f32, f32)) -> (f32, f32) {
        if self.is_identity() {
            return (x, y); // exactly unchanged, not just nearly
        }
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// How much this transform resizes things, on average: 2 for twice
    /// as big. Brush sizes are multiplied by this. (With different
    /// scales across and down, it's the size in between.)
    pub fn size_factor(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }

    /// How much this transform resizes a line's width: its smallest
    /// stretch in any direction. The same as `size_factor` for an even
    /// scale; for [3, 1] it's 1, so a line drawn 2 wide stays 2 wide
    /// instead of fattening to 3.5. 0 when it squashes things flat.
    pub fn line_factor(&self) -> f32 {
        // The smaller singular value of the 2 x 2 part.
        let sum = self.a * self.a + self.b * self.b + self.c * self.c + self.d * self.d;
        let det = self.a * self.d - self.b * self.c;
        let root = (sum * sum - 4.0 * det * det).max(0.0).sqrt();
        ((sum - root) / 2.0).max(0.0).sqrt()
    }

    /// True if it mirrors (left and right swap places).
    pub fn mirrors(&self) -> bool {
        self.a * self.d - self.b * self.c < 0.0
    }

    /// The transform that undoes this one: it takes each point back to
    /// where it came from. `None` if this one squashes everything flat
    /// (a scale of 0), which can't be undone.
    pub fn inverse(&self) -> Option<Transform> {
        let det = self.a * self.d - self.b * self.c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        Some(Transform {
            a: self.d / det,
            b: -self.b / det,
            c: -self.c / det,
            d: self.a / det,
            e: (self.c * self.f - self.d * self.e) / det,
            f: (self.b * self.e - self.a * self.f) / det,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: (f32, f32), expected: (f32, f32)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-4 && (actual.1 - expected.1).abs() < 1e-4,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn the_identity_changes_nothing_exactly() {
        let p = (0.1 + 0.2, 1e-9);
        assert_eq!(Transform::IDENTITY.apply(p), p);
    }

    #[test]
    fn moving_resizing_and_turning() {
        close(
            Transform::translate(5.0, -2.0).apply((1.0, 1.0)),
            (6.0, -1.0),
        );
        close(Transform::scale(2.0, 3.0).apply((1.0, 1.0)), (2.0, 3.0));
        // 90 degrees clockwise on screen: right becomes down.
        close(Transform::rotate(90.0).apply((1.0, 0.0)), (0.0, 1.0));
    }

    #[test]
    fn then_applies_in_order() {
        let move_then_double = Transform::translate(1.0, 0.0).then(Transform::scale(2.0, 2.0));
        close(move_then_double.apply((0.0, 0.0)), (2.0, 0.0));
        let double_then_move = Transform::scale(2.0, 2.0).then(Transform::translate(1.0, 0.0));
        close(double_then_move.apply((0.0, 0.0)), (1.0, 0.0));
    }

    #[test]
    fn about_works_around_the_origin() {
        // Turning around (10, 10): that point stays put.
        let turn = Transform::about((10.0, 10.0), (0.0, 0.0), (1.0, 1.0), 90.0, false, false);
        close(turn.apply((10.0, 10.0)), (10.0, 10.0));
        close(turn.apply((20.0, 10.0)), (10.0, 20.0));
        // Mirroring left-right around x = 50.
        let mirror = Transform::about((50.0, 0.0), (0.0, 0.0), (1.0, 1.0), 0.0, true, false);
        close(mirror.apply((40.0, 7.0)), (60.0, 7.0));
        assert!(mirror.mirrors());
        // Then moved.
        let moved = Transform::about((0.0, 0.0), (100.0, 5.0), (2.0, 2.0), 0.0, false, false);
        close(moved.apply((1.0, 1.0)), (102.0, 7.0));
    }

    #[test]
    fn the_inverse_takes_points_back() {
        let t = Transform::about((30.0, 40.0), (5.0, -7.0), (2.0, 0.5), 33.0, true, false);
        let back = t.inverse().unwrap();
        for p in [(0.0, 0.0), (12.5, -3.0), (100.0, 80.0)] {
            close(back.apply(t.apply(p)), p);
        }
        // Mirroring is its own inverse, exactly.
        let flip = Transform::about((50.0, 0.0), (0.0, 0.0), (1.0, 1.0), 0.0, true, false);
        assert_eq!(flip.inverse().unwrap(), flip);
        assert!(Transform::scale(0.0, 1.0).inverse().is_none());
    }

    #[test]
    fn size_factor_and_repeating() {
        assert_eq!(Transform::scale(2.0, 8.0).size_factor(), 4.0);
        assert!((Transform::rotate(33.0).size_factor() - 1.0).abs() < 1e-6);
        let step = Transform::translate(10.0, 0.0);
        close(step.repeated(3).apply((0.0, 0.0)), (30.0, 0.0));
        assert!(step.repeated(0).is_identity());
    }

    #[test]
    fn a_line_widens_by_the_smallest_stretch() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(Transform::scale(2.0, 2.0).line_factor(), 2.0));
        assert!(close(Transform::scale(3.0, 1.0).line_factor(), 1.0));
        assert!(close(Transform::scale(0.5, 4.0).line_factor(), 0.5));
        // Turning doesn't change it.
        let turned = Transform::scale(3.0, 1.0).then(Transform::rotate(33.0));
        assert!(close(turned.line_factor(), 1.0));
        assert!(close(Transform::scale(1.0, 0.0).line_factor(), 0.0));
    }
}
