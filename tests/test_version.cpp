#include <artengine/version.h>

#include <doctest/doctest.h>

#include <algorithm>

TEST_CASE("engine reports a version like 1.2.3")
{
    const std::string_view version = art::engineVersion();

    CHECK_FALSE(version.empty());
    // Two dots: major.minor.patch
    CHECK(std::count(version.begin(), version.end(), '.') == 2);
}
