@echo off
rem ================================================================
rem  Double-click this file to build and start Art App.
rem  The first time takes a few minutes: Rust downloads and builds
rem  the libraries we use. After that it starts in seconds.
rem ================================================================
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust is not installed yet.
    echo Install it from https://rustup.rs and then double-click this file again.
    echo If you just installed it, restart your PC first.
    echo.
    pause
    exit /b 1
)

rem Windows can't replace ArtApp.exe while it is open.
tasklist /fi "imagename eq ArtApp.exe" | find /i "ArtApp.exe" >nul
if not errorlevel 1 (
    echo Art App is already open. Close it first, then double-click run.bat again.
    echo.
    pause
    exit /b 1
)

echo Building Art App... (the first build takes a few minutes)
echo.
cargo build --release
if errorlevel 1 (
    echo.
    echo Something went wrong while building.
    echo Take a screenshot of this window and send it to Claude.
    echo.
    pause
    exit /b 1
)

start "" "target\release\ArtApp.exe"
