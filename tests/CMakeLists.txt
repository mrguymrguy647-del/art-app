# Unit tests for the engine.
#
# We use doctest, a small test library (MIT license). It is a single header
# file kept in third_party/ so the build never needs to download anything.

add_library(doctest INTERFACE)
# SYSTEM = this is third-party code, so don't show its compiler warnings.
target_include_directories(doctest SYSTEM INTERFACE third_party)

add_executable(engine_tests
    test_main.cpp
    test_version.cpp
    test_pen_sample.cpp
)
target_link_libraries(engine_tests PRIVATE art::engine art_warnings doctest)

# Lets you run the tests with "ctest" (or from Qt Creator's Tests panel).
add_test(NAME engine_tests COMMAND engine_tests)
