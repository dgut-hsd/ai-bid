@echo off
setlocal
cd /d "%~dp0"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0stop-env.ps1"
if errorlevel 1 (
    echo.
    echo Project environment stop failed.
    pause
    exit /b 1
)

exit /b 0
