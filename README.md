# Art App

A digital painting app for Windows, written in C++20 with Qt 6 (Qt Quick).

**Status:** Phase 1, Step 1: project setup and a pen test screen.

## Project layout

```
art-app/
├── CMakeLists.txt          top level: turns on C++20, adds the parts below
├── CMakePresets.json       ready-made build setups
├── engine/                 painting engine: plain C++20, no Qt
│   ├── include/artengine/  headers the app is allowed to use
│   └── src/                the code behind them
├── app/                    Qt Quick app: everything you see and touch
│   ├── src/                C++ glue: pen input -> engine, engine -> screen
│   └── qml/                the interface (QML files)
├── tests/                  engine unit tests (doctest)
└── docs/                   notes explaining design decisions
```

## What you need (Windows)

- **Visual Studio 2022** (Community is free) with the *Desktop development with C++* workload
- **Qt 6.12 LTS** (any Qt 6.8 or newer works) with the *MSVC 2022 64-bit* component, plus **Qt Creator**
- **Git for Windows**

## Build and run with Qt Creator

1. Download the code into a short folder path (long paths can break builds):

   ```
   git clone -b claude/stoic-cannon-vyu6qr https://github.com/mrguymrguy647-del/art-app.git C:\dev\art-app
   ```

2. In Qt Creator: **File → Open File or Project…** and pick `C:\dev\art-app\CMakeLists.txt`.
3. On the *Configure Project* page, tick the kit **Desktop Qt 6.12.0 MSVC2022 64bit** and click
   **Configure Project**. (You can ignore the "Windows MSVC" preset entries; they are for
   command-line builds.)
4. Press **Ctrl+R** to build and run. If Qt Creator asks which program to run, choose **ArtApp**.
5. To run the engine tests, switch the run target (the computer icon at the bottom left) to
   **engine_tests** and press **Ctrl+R**. The output should end with `Status: SUCCESS!`.

### Command line (optional)

Open the **x64 Native Tools Command Prompt for VS 2022**, go to the project folder, then:

```
cmake --preset windows-msvc-debug
cmake --build --preset windows-msvc-debug
ctest --preset windows-msvc-debug
```

If Qt is not in `C:/Qt/6.12.0/msvc2022_64`, change `CMAKE_PREFIX_PATH` in `CMakePresets.json`.
Starting `ArtApp.exe` by double-clicking won't work yet because it can't find Qt's DLLs.
Run it from Qt Creator for now; we'll set up packaging in a later step.

### Linux (engine and tests only, no Qt needed)

```
cmake --preset linux-tests && cmake --build --preset linux-tests && ctest --preset linux-tests
```

## Pen tablet tip

Qt reads pen pressure and tilt through **Windows Ink**. If your tablet driver (Wacom, XP-Pen,
Huion, …) has a "Use Windows Ink" option, keep it switched on.
