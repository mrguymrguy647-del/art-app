# The Qt Quick application: windows, panels and input handling.
# Anything that stores or draws painting data belongs in engine/, not here.

find_package(Qt6 6.8 REQUIRED COMPONENTS Gui Quick QuickControls2)

# Turns on Qt's code generators (moc, etc.) and the recommended defaults
# for this Qt version.
qt_standard_project_setup(REQUIRES 6.8)

# Put compiled QML modules in one folder (build/.../qml). This keeps the
# module's folder from clashing with the program file, and helps Qt's QML
# tools find our types.
set(QT_QML_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml)

qt_add_executable(ArtApp
    src/main.cpp
)

# The QML module "ArtApp": our QML files plus the C++ types QML can use.
# QML files load it with "import ArtApp".
qt_add_qml_module(ArtApp
    URI ArtApp
    VERSION 1.0
    # Our C++ types build on QtQuick types (PenTestArea is a QQuickItem).
    # Saying so lets Qt's QML tools and Qt Creator understand them.
    DEPENDENCIES
        QtQuick
    QML_FILES
        qml/Main.qml
        qml/PenInfoPanel.qml
    SOURCES
        src/pen_test_area.h
        src/pen_test_area.cpp
)

target_include_directories(ArtApp PRIVATE src)

target_link_libraries(ArtApp PRIVATE
    art::engine
    art_warnings
    Qt6::Gui
    Qt6::Quick
    Qt6::QuickControls2
)

# On Windows, build a normal windowed program (no console window behind it).
set_target_properties(ArtApp PROPERTIES WIN32_EXECUTABLE TRUE)
