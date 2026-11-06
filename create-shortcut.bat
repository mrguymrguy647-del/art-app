@echo off
rem ================================================================
rem  Double-click this file to put a "Duck Quill" shortcut on your
rem  desktop. Run run.bat at least once first, so the app is built.
rem  The shortcut always opens the newest version you've built.
rem ================================================================
cd /d "%~dp0"

if not exist "target\release\DuckQuill.exe" (
    echo Duck Quill hasn't been built yet.
    echo Double-click run.bat first, then double-click this file again.
    echo.
    pause
    exit /b 1
)

powershell -NoProfile -Command "$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop') + '\Duck Quill.lnk'); $s.TargetPath = '%~dp0target\release\DuckQuill.exe'; $s.WorkingDirectory = '%~dp0'; $s.IconLocation = '%~dp0target\release\DuckQuill.exe,0'; $s.Save()"
if errorlevel 1 (
    echo.
    echo Could not create the shortcut. Take a screenshot of this window and send it to Claude.
) else (
    echo.
    echo Done! There is now a "Duck Quill" shortcut on your desktop.
    echo Tip: right-click it and choose "Pin to taskbar" if you like.
    echo If you still have the old "Art App" shortcut, you can delete it.
)
echo.
pause
