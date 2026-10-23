@echo off
setlocal
if defined VSCMD_ARG_TGT_ARCH goto build
set "AIR3VIEW_VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%AIR3VIEW_VSWHERE%" (
  echo Install Visual Studio Build Tools with Desktop development with C++ and Windows SDK.
  exit /b 1
)
for /f "usebackq delims=" %%i in (`"%AIR3VIEW_VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "AIR3VIEW_VS=%%i"
if not defined AIR3VIEW_VS exit /b 1
call "%AIR3VIEW_VS%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 exit /b %errorlevel%
:build
pushd "%~dp0.."
if "%~1"=="" (
  "%USERPROFILE%\.cargo\bin\cargo.exe" build --release --bins --locked
) else (
  "%USERPROFILE%\.cargo\bin\cargo.exe" %*
)
set "AIR3VIEW_BUILD_RESULT=%errorlevel%"
popd
exit /b %AIR3VIEW_BUILD_RESULT%
