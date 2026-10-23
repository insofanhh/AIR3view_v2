@echo off
setlocal
set "AIR3VIEW_APP_DIR=%~dp0"
if not exist "%AIR3VIEW_APP_DIR%AIR3view.exe" (
  set "AIR3VIEW_APP_DIR=%~dp0dist\AIR3view\"
  if exist "%~dp0.venv\Scripts\python.exe" set "AIR3VIEW_PYTHON=%~dp0.venv\Scripts\python.exe"
)
if not exist "%AIR3VIEW_APP_DIR%AIR3view.exe" (
  echo Build the app with scripts\package.ps1 first.
  pause
  exit /b 1
)
if not defined AIR3VIEW_PYTHON (
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%AIR3VIEW_APP_DIR%scripts\setup-workers.ps1"
  if errorlevel 1 (
    pause
    exit /b 1
  )
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%AIR3VIEW_APP_DIR%scripts\start-worker.ps1"
if errorlevel 1 (
  pause
  exit /b 1
)
start "" "%AIR3VIEW_APP_DIR%AIR3view.exe"
