@echo off
setlocal EnableDelayedExpansion

:: Change to the script directory
cd /d "%~dp0"
set "SCRIPT_DIR=%cd%"

:: Check if Node is already installed
where node >nul 2>&1
if %errorlevel% equ 0 (
    echo Node.js is already installed:
    node -v
    goto :LAUNCH
)

echo Node.js is not installed. Installing the latest version...

:: Get the latest version + download and install via PowerShell (native)
:: The MSI file is downloaded into the script directory
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ErrorActionPreference = 'Stop';" ^
  "$scriptDir = '%SCRIPT_DIR%';" ^
  "Write-Host 'Retrieving the latest version...';" ^
  "$index = Invoke-RestMethod -Uri 'https://nodejs.org/dist/index.json';" ^
  "$version = $index[0].version.TrimStart('v');" ^
  "Write-Host \"Latest version detected: v$version\";" ^
  "$arch = if ([Environment]::Is64BitOperatingSystem) { 'x64' } else { 'x86' };" ^
  "$msiUrl = \"https://nodejs.org/dist/v$version/node-v$version-$arch.msi\";" ^
  "$msiPath = Join-Path $scriptDir \"node-v$version-$arch.msi\";" ^
  "Write-Host \"Downloading $msiUrl to $scriptDir...\";" ^
  "Invoke-WebRequest -Uri $msiUrl -OutFile $msiPath;" ^
  "Write-Host 'Silent installation...';" ^
  "Start-Process msiexec.exe -ArgumentList \"/i `\"$msiPath`\" /qn /norestart\" -Wait -NoNewWindow;" ^
  "Remove-Item $msiPath -Force;" ^
  "Write-Host 'Installation complete.'"

:: Refresh PATH for the current session
set "PATH=%PATH%;%ProgramFiles%\nodejs"

echo.
echo Installed version:
node -v
echo npm:
npm -v

:LAUNCH
echo.
echo Running node...
node release.mjs %*

endlocal
