@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "SCRIPT_DIR=%~dp0"

pushd "%SCRIPT_DIR%front"
call npm update
if errorlevel 1 (
  set "EXIT_CODE=!errorlevel!"
  popd
  exit /b !EXIT_CODE!
)
popd

pushd "%SCRIPT_DIR%front\admin-app"
call npm update
if errorlevel 1 (
  set "EXIT_CODE=!errorlevel!"
  popd
  exit /b !EXIT_CODE!
)
popd

pushd "%SCRIPT_DIR%front\public-app"
call npm update
if errorlevel 1 (
  set "EXIT_CODE=!errorlevel!"
  popd
  exit /b !EXIT_CODE!
)
popd
endlocal
