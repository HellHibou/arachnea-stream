@echo off
setlocal
set "SCRIPT_DIR=%~dp0"

pushd "%SCRIPT_DIR%"
node build-release\release.mjs %*
set "EXIT_CODE=%errorlevel%"
popd
endlocal & exit /b %EXIT_CODE%
