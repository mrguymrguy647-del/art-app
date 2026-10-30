@echo off
rem ================================================================
rem  Turns a painting script (.json) into a picture, without opening
rem  the app. Drag a script file onto this file. (Double-clicking it
rem  renders the example, examples\sunset.json.)
rem
rem  The picture is saved in a folder called "render" next to the
rem  script, with a contact sheet of every stage, and opened when
rem  it's done.
rem ================================================================
setlocal
set "SCRIPT=%~1"
cd /d "%~dp0"
if "%SCRIPT%"=="" set "SCRIPT=%~dp0examples\sunset.json"

where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust is not installed yet.
    echo Install it from https://rustup.rs and then try again.
    echo.
    pause
    exit /b 1
)

echo Building art-cli... (the first time takes a minute)
cargo build --release -p art_cli
if errorlevel 1 (
    echo.
    echo Something went wrong while building.
    echo Take a screenshot of this window and send it to Claude.
    echo.
    pause
    exit /b 1
)

for %%F in ("%SCRIPT%") do (
    set "FOLDER=%%~dpFrender"
    set "NAME=%%~nF"
)
echo.
"target\release\art-cli.exe" render "%SCRIPT%" --out "%FOLDER%" --sheet
if errorlevel 1 (
    echo.
    echo The script has a problem: see the message above.
    echo.
    pause
    exit /b 1
)
start "" "%FOLDER%\%NAME%.png"
echo.
pause
