# Top-level build file for Art App.
#
# The project has three parts:
#   engine/  - the painting engine (plain C++20, no Qt)
#   app/     - the Qt Quick application (everything you see and touch)
#   tests/   - unit tests for the engine
#
# The app and the tests both use the engine, but the engine never uses them.

cmake_minimum_required(VERSION 3.21)

project(ArtApp
    VERSION 0.1.0
    DESCRIPTION "A digital painting app"
    LANGUAGES CXX)

# Use C++20 everywhere, without compiler-specific extensions.
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Switches you can turn on/off when configuring (e.g. -DART_BUILD_APP=OFF).
option(ART_BUILD_APP   "Build the Qt Quick application (needs Qt)" ON)
option(ART_BUILD_TESTS "Build the engine unit tests"               ON)

# Shared compiler warning settings. Our own targets link to this so the
# compiler points out likely mistakes. It is not applied to third-party code.
add_library(art_warnings INTERFACE)
if(MSVC)
    # /W4 = high warning level, /utf-8 = source files are UTF-8.
    target_compile_options(art_warnings INTERFACE /W4 /utf-8)
else()
    target_compile_options(art_warnings INTERFACE -Wall -Wextra -Wpedantic)
endif()

add_subdirectory(engine)

if(ART_BUILD_TESTS)
    enable_testing()
    add_subdirectory(tests)
endif()

if(ART_BUILD_APP)
    add_subdirectory(app)
endif()
