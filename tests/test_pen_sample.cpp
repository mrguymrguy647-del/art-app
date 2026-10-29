#include <artengine/pen_sample.h>

#include <doctest/doctest.h>

using doctest::Approx;

TEST_CASE("an upright pen has altitude 90 and no direction")
{
    const art::PenAngles a = art::penAngles(0.0, 0.0);
    CHECK(a.altitude == Approx(90.0));
    CHECK(a.azimuth == Approx(0.0));
}

TEST_CASE("leaning in one direction")
{
    SUBCASE("right")
    {
        const art::PenAngles a = art::penAngles(45.0, 0.0);
        CHECK(a.altitude == Approx(45.0));
        CHECK(a.azimuth == Approx(0.0));
    }
    SUBCASE("towards you (down on screen)")
    {
        const art::PenAngles a = art::penAngles(0.0, 45.0);
        CHECK(a.altitude == Approx(45.0));
        CHECK(a.azimuth == Approx(90.0));
    }
    SUBCASE("left")
    {
        const art::PenAngles a = art::penAngles(-45.0, 0.0);
        CHECK(a.altitude == Approx(45.0));
        CHECK(a.azimuth == Approx(180.0));
    }
    SUBCASE("away from you (up on screen)")
    {
        const art::PenAngles a = art::penAngles(0.0, -45.0);
        CHECK(a.altitude == Approx(45.0));
        CHECK(a.azimuth == Approx(270.0));
    }
}

TEST_CASE("leaning diagonally")
{
    // tan(30 deg) = 0.577..., so the top moves 0.577 right and 0.577 down.
    // Sideways distance = 0.8165, altitude = atan(1 / 0.8165) = 50.77 degrees.
    const art::PenAngles a = art::penAngles(30.0, 30.0);
    CHECK(a.altitude == Approx(50.768).epsilon(0.001));
    CHECK(a.azimuth == Approx(45.0));
}

TEST_CASE("extreme tilt values are clamped instead of breaking the maths")
{
    const art::PenAngles a = art::penAngles(90.0, 0.0);
    CHECK(a.altitude > 0.0);
    CHECK(a.altitude < 2.0);
    CHECK(a.azimuth == Approx(0.0));
}

TEST_CASE("azimuth always stays in the range 0 to 360")
{
    for (double tx = -60.0; tx <= 60.0; tx += 7.5) {
        for (double ty = -60.0; ty <= 60.0; ty += 7.5) {
            const art::PenAngles a = art::penAngles(tx, ty);
            CHECK(a.azimuth >= 0.0);
            CHECK(a.azimuth < 360.0);
            CHECK(a.altitude > 0.0);
            CHECK(a.altitude <= 90.0);
        }
    }
}
