@echo off
rem ================================================================
rem  Double-click this file to run the painting engine's tests.
rem ================================================================
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust is not installed yet.
    echo Install it from https://rustup.rs and then double-click this file again.
    echo.
    pause
    exit /b 1
)

cargo test --workspace
if errorlevel 1 (
    echo.
    echo Some tests FAILED. Take a screenshot of this window and send it to Claude.
) else (
    echo.
    echo All tests passed.
)
echo.
pause
