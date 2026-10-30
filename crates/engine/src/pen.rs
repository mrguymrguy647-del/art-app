//! Pen input as the engine sees it.
//!
//! The app turns whatever the operating system reports (Windows Ink, mouse,
//! touch) into [`PenSample`]s, so the engine never has to know about Windows
//! or the UI library. Later the brush engine will turn a stream of these into
//! brush dabs on the canvas.

/// What produced an input sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputKind {
    /// The tip of a stylus.
    Pen,
    /// The "eraser" end of a stylus (if it has one).
    PenEraser,
    #[default]
    Mouse,
    Touch,
}

/// One reading from a pen, mouse or finger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenSample {
    pub kind: InputKind,

    /// Position in pixels.
    pub x: f32,
    pub y: f32,

    /// 0.0 = not touching, 1.0 = pressing hard.
    /// A mouse has no pressure sensor, so it reports 1.0 while a button is held.
    pub pressure: f32,

    /// Tilt in degrees, measured from "pen standing straight up".
    /// `tilt_x > 0`: the top of the pen leans to the right.
    /// `tilt_y > 0`: the top of the pen leans towards you (the bottom of the tablet).
    pub tilt_x: f32,
    pub tilt_y: f32,

    /// Barrel rotation in degrees (only a few pens report this).
    pub rotation: f32,

    /// When the sample was taken, in milliseconds.
    pub time_ms: u64,
}

impl Default for PenSample {
    fn default() -> Self {
        Self {
            kind: InputKind::default(),
            x: 0.0,
            y: 0.0,
            pressure: 1.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            rotation: 0.0,
            time_ms: 0,
        }
    }
}

/// Pen tilt expressed in a way that is easier to use for brushes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenAngles {
    /// How upright the pen is, in degrees.
    /// 90 = standing straight up, smaller numbers = leaning over more.
    pub altitude: f32,

    /// Which way the pen leans, in degrees, `0 <= azimuth < 360`.
    /// Uses screen directions: 0 = right, 90 = down, 180 = left, 270 = up.
    /// When the pen is perfectly upright this is 0 (there is no direction).
    pub azimuth: f32,
}

impl PenSample {
    /// This sample's tilt as altitude and lean direction.
    pub fn angles(&self) -> PenAngles {
        pen_angles(self.tilt_x, self.tilt_y)
    }
}

/// Real pens report at most about 60 degrees of tilt. We clamp anyway
/// because tan(90 degrees) is infinite and would break the maths below.
const MAX_TILT_DEGREES: f32 = 89.0;

/// Converts the two tilt angles a tablet reports into altitude and azimuth.
pub fn pen_angles(tilt_x_degrees: f32, tilt_y_degrees: f32) -> PenAngles {
    let tilt_x = tilt_x_degrees.clamp(-MAX_TILT_DEGREES, MAX_TILT_DEGREES);
    let tilt_y = tilt_y_degrees.clamp(-MAX_TILT_DEGREES, MAX_TILT_DEGREES);

    // Picture the pen as an arrow from its tip (on the tablet) to its top,
    // and say the top is 1 unit above the tablet. Leaning by tilt_x moves the
    // top tan(tilt_x) units to the right; leaning by tilt_y moves it
    // tan(tilt_y) units towards you.
    let dx = tilt_x.to_radians().tan();
    let dy = tilt_y.to_radians().tan();
    let lean = dx.hypot(dy); // sideways distance of the top

    // Angle between the tablet surface and the pen: atan(height / sideways).
    let altitude = 1.0_f32.atan2(lean).to_degrees();

    // Direction of the lean. atan2 gives -180..180; rem_euclid maps it to 0..360.
    let azimuth = if lean > 0.0 {
        dy.atan2(dx).to_degrees().rem_euclid(360.0)
    } else {
        0.0
    };

    PenAngles { altitude, azimuth }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks that two numbers are equal up to rounding errors.
    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn upright_pen_has_altitude_90_and_no_direction() {
        let a = pen_angles(0.0, 0.0);
        assert_close(a.altitude, 90.0);
        assert_close(a.azimuth, 0.0);
    }

    #[test]
    fn leaning_in_each_direction() {
        // (tilt_x, tilt_y, expected azimuth)
        let cases = [
            (45.0, 0.0, 0.0),    // right
            (0.0, 45.0, 90.0),   // towards you (down on screen)
            (-45.0, 0.0, 180.0), // left
            (0.0, -45.0, 270.0), // away from you (up on screen)
        ];
        for (tilt_x, tilt_y, azimuth) in cases {
            let a = pen_angles(tilt_x, tilt_y);
            assert_close(a.altitude, 45.0);
            assert_close(a.azimuth, azimuth);
        }
    }

    #[test]
    fn leaning_diagonally() {
        // tan(30 deg) = 0.577, so the top moves 0.577 right and 0.577 down.
        // Sideways distance = 0.8165, altitude = atan(1 / 0.8165) = 50.77 degrees.
        let a = pen_angles(30.0, 30.0);
        assert_close(a.altitude, 50.77);
        assert_close(a.azimuth, 45.0);
    }

    #[test]
    fn extreme_tilt_is_clamped_instead_of_breaking_the_maths() {
        let a = pen_angles(90.0, 0.0);
        assert!(
            a.altitude > 0.0 && a.altitude < 2.0,
            "altitude {}",
            a.altitude
        );
        assert_close(a.azimuth, 0.0);
    }

    #[test]
    fn angles_always_stay_in_range() {
        for i in -8..=8 {
            for j in -8..=8 {
                let a = pen_angles(i as f32 * 7.5, j as f32 * 7.5);
                assert!((0.0..360.0).contains(&a.azimuth), "azimuth {}", a.azimuth);
                assert!(
                    a.altitude > 0.0 && a.altitude <= 90.0,
                    "altitude {}",
                    a.altitude
                );
            }
        }
    }

    #[test]
    fn sample_angles_use_its_tilt() {
        let sample = PenSample {
            tilt_x: 0.0,
            tilt_y: 45.0,
            ..PenSample::default()
        };
        assert_close(sample.angles().azimuth, 90.0);
    }
}
