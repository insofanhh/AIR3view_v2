@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\setup-workers.ps1"
if errorlevel 1 (
  echo Setup failed. Check the error above, internet access and free disk space, then retry.
  pause
  exit /b 1
)
echo Setup finished. Open AIR3view.exe to use the app.
pause
