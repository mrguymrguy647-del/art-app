#include "artengine/pen_sample.h"

#include <algorithm>
#include <cmath>
#include <numbers>

namespace art {

namespace {

constexpr double kDegToRad = std::numbers::pi / 180.0;
constexpr double kRadToDeg = 180.0 / std::numbers::pi;

// Real pens report at most about 60 degrees of tilt. We clamp anyway because
// tan(90 degrees) is infinite and would break the maths below.
constexpr double kMaxTilt = 89.0;

} // namespace

PenAngles penAngles(double tiltXDegrees, double tiltYDegrees)
{
    const double tiltX = std::clamp(tiltXDegrees, -kMaxTilt, kMaxTilt);
    const double tiltY = std::clamp(tiltYDegrees, -kMaxTilt, kMaxTilt);

    // Picture the pen as an arrow from its tip (on the tablet) to its top,
    // and say the top is 1 unit above the tablet. Leaning by tiltX moves the
    // top tan(tiltX) units to the right; leaning by tiltY moves it tan(tiltY)
    // units towards you.
    const double dx = std::tan(tiltX * kDegToRad);
    const double dy = std::tan(tiltY * kDegToRad);
    const double lean = std::hypot(dx, dy); // sideways distance of the top

    PenAngles result;

    // Angle between the tablet surface and the pen: atan(height / sideways).
    result.altitude = std::atan2(1.0, lean) * kRadToDeg;

    // Direction of the lean. atan2 gives -180..180; fmod maps it to 0..360.
    if (lean > 0.0)
        result.azimuth = std::fmod(std::atan2(dy, dx) * kRadToDeg + 360.0, 360.0);

    return result;
}

} // namespace art
