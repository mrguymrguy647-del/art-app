@echo off
rem ================================================================
rem  Double-click this file to put an "Art App" shortcut on your
rem  desktop. Run run.bat at least once first, so the app is built.
rem  The shortcut always opens the newest version you've built.
rem ================================================================
cd /d "%~dp0"

if not exist "target\release\ArtApp.exe" (
    echo Art App hasn't been built yet.
    echo Double-click run.bat first, then double-click this file again.
    echo.
    pause
    exit /b 1
)

powershell -NoProfile -Command "$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop') + '\Art App.lnk'); $s.TargetPath = '%~dp0target\release\ArtApp.exe'; $s.WorkingDirectory = '%~dp0'; $s.Save()"
if errorlevel 1 (
    echo.
    echo Could not create the shortcut. Take a screenshot of this window and send it to Claude.
) else (
    echo.
    echo Done! There is now an "Art App" shortcut on your desktop.
    echo Tip: right-click it and choose "Pin to taskbar" if you like.
)
echo.
pause
