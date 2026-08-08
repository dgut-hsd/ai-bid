@echo off
setlocal
cd /d "%~dp0"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0start-env.ps1"
if errorlevel 1 (
    echo.
    echo Project environment startup failed.
    pause
    exit /b 1
)

exit /b 0
