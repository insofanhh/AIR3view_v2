@echo off
setlocal
set "AIR3VIEW_APP_DIR=%~dp0"
if not exist "%AIR3VIEW_APP_DIR%AIR3view.exe" (
  set "AIR3VIEW_APP_DIR=%~dp0dist\AIR3view\"
)
if not exist "%AIR3VIEW_APP_DIR%AIR3view.exe" (
  echo Build the app with scripts\package.ps1 first.
  pause
  exit /b 1
)
start "" "%AIR3VIEW_APP_DIR%AIR3view.exe"
