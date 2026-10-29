# Design decisions

Short notes on *why* things are the way they are. Newest at the bottom.

## 1. Windows only, for now
We dropped Android to keep the first version simple. The engine is plain
C++ and Qt also runs on Android, so adding it back later would not need a
rewrite.

## 2. MSVC compiler on Windows
MSVC (Visual Studio) is the compiler Qt tests most on Windows. It usually
makes fast code, and its debugger is very good. Our CMake files don't use
anything MSVC-only, so MinGW should also work.

## 3. Qt 6.12 LTS, minimum Qt 6.8
LTS ("long-term support") versions get bug fixes for years. We develop on
6.12, the newest LTS, and the build accepts any Qt from 6.8 (the previous
LTS) up.

## 4. The engine never uses Qt
`engine/` only uses standard C++. That means:
- we can test brush maths, tiles and undo without opening a window;
- the engine can't accidentally depend on how the UI works.

The app converts Qt's input events into the engine's own `art::PenSample`
type, and later will show the engine's tiles on screen.

## 5. How the app reads the pen
Qt Quick items have no `tabletEvent()` function (unlike Qt Widgets), and
the QML input handlers give pressure but not tilt. So `PenTestArea`
installs an *event filter* on the window: it gets a look at every tablet,
mouse and touch event before Qt Quick handles it, keeps the ones over its
own area, and lets them continue as normal. The canvas will read input in
the same way.

## 6. Tests use doctest, stored in the repo
doctest is a small test library (MIT license, so it's fine for us to use).
It is one header file in `tests/third_party/`, so building never needs an
internet connection.

## 7. Fusion style with a dark palette
Fusion looks the same on every Windows version and is easy to colour. A
dark grey interface is usual for painting apps because it doesn't compete
with the colours on the canvas.

## 8. Compiled QML modules go into `build/.../qml`
Qt puts a QML module's files in a folder named after the module
(`ArtApp`). Without this setting, that folder would sit next to the
program file `ArtApp` and clash with it on some systems. One shared `qml`
folder also helps Qt Creator understand our QML types.
