# The painting engine: plain C++20, no Qt.
#
# Keeping Qt out of the engine means we can test brush maths, tiles and undo
# without opening a window, and the engine does not care which UI sits on top.

add_library(artengine STATIC
    include/artengine/version.h
    include/artengine/pen_sample.h
    src/version.cpp
    src/pen_sample.cpp
)

# A nicer name for other targets to link against: art::engine
add_library(art::engine ALIAS artengine)

# Anything that links to the engine can #include <artengine/...>.
target_include_directories(artengine PUBLIC include)
target_compile_features(artengine PUBLIC cxx_std_20)
target_link_libraries(artengine PRIVATE art_warnings)

# Pass the project version (from the top-level CMakeLists.txt) into the code.
target_compile_definitions(artengine PRIVATE
    ARTENGINE_VERSION="${PROJECT_VERSION}")
