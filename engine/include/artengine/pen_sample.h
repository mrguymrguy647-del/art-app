#pragma once

#include <cstdint>

namespace art {

/// What produced an input sample.
enum class InputKind {
    Pen,        ///< tip of a stylus
    PenEraser,  ///< the "eraser" end of a stylus (if it has one)
    Mouse,
    Touch,
};

/// One reading from a pen, mouse or finger.
///
/// The app turns Qt's input events into PenSamples, so the engine never has
/// to know about Qt. Later the brush engine will receive a stream of these
/// and turn them into brush dabs on the canvas.
struct PenSample {
    InputKind kind = InputKind::Mouse;

    double x = 0.0;            ///< position in pixels
    double y = 0.0;

    /// 0.0 = not touching, 1.0 = pressing hard.
    /// A mouse has no pressure sensor, so it reports 1.0 while a button is held.
    double pressure = 1.0;

    /// Tilt in degrees, measured from "pen standing straight up".
    /// tiltX > 0: the top of the pen leans to the right.
    /// tiltY > 0: the top of the pen leans towards you (the bottom of the tablet).
    double tiltX = 0.0;
    double tiltY = 0.0;

    double rotation = 0.0;     ///< barrel rotation in degrees (only a few pens report this)

    std::uint64_t timeMs = 0;  ///< when the sample was taken, in milliseconds
};

/// Pen tilt expressed in a way that is easier to use for brushes.
struct PenAngles {
    /// How upright the pen is, in degrees.
    /// 90 = standing straight up, smaller numbers = leaning over more.
    double altitude = 90.0;

    /// Which way the pen leans, in degrees, 0 <= azimuth < 360.
    /// Uses screen directions: 0 = right, 90 = down, 180 = left, 270 = up.
    /// When the pen is perfectly upright this is 0 (there is no direction).
    double azimuth = 0.0;
};

/// Converts the two tilt angles a tablet reports into altitude and azimuth.
PenAngles penAngles(double tiltXDegrees, double tiltYDegrees);

} // namespace art
