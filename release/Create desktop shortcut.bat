@echo off
rem ================================================================
rem  Double-click this file to put a "Duck Quill" shortcut, with the
rem  duck icon, on your desktop. It points at DuckQuill.exe in this
rem  folder, so leave the folder where it is afterwards.
rem ================================================================
cd /d "%~dp0"

if not exist "DuckQuill.exe" (
    echo DuckQuill.exe isn't in this folder.
    echo Unzip the whole download first, then double-click this file inside the unzipped folder.
    echo.
    pause
    exit /b 1
)

powershell -NoProfile -Command "$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop') + '\Duck Quill.lnk'); $s.TargetPath = '%~dp0DuckQuill.exe'; $s.WorkingDirectory = '%~dp0'; $s.IconLocation = '%~dp0DuckQuill.exe,0'; $s.Save()"
if errorlevel 1 (
    echo.
    echo Could not create the shortcut.
) else (
    echo.
    echo Done! There is now a "Duck Quill" shortcut on your desktop.
    echo Tip: right-click it and choose "Pin to taskbar" if you like.
)
echo.
pause
