@echo off
setlocal EnableDelayedExpansion

:: Build the arachnea-docker image with `docker compose build`, resolving the
:: release version to package.
::
:: Usage:
::   build-image.cmd [--version=<version>] [docker compose build options]
::
:: The version is the ARACHNEA_VERSION build variable of docker-compose.yml.
:: When --version is omitted, the highest `release-*` folder of the release
:: tree is used (`..\releases`, or ARACHNEA_RELEASES_DIR from the environment
:: or `.env`); with no release folder and no --version argument the script
:: fails. Any other argument is forwarded to `docker compose build`, including
:: `--no-cache` to force a full rebuild of the image.

:: The build reads `.env`, the compose file and `..\releases` relative to this
:: folder, so always work from there.
cd /d "%~dp0"

set "VERSION="
set "VERSION_GIVEN="
set "EXTRA_ARGS="

:: --- Arguments -------------------------------------------------------------
:PARSE_ARGS
if "%~1"=="" goto :PARSE_DONE
set "ARG=%~1"
if /i "!ARG!"=="-h" goto :USAGE
if /i "!ARG!"=="--help" goto :USAGE
if /i "!ARG:~0,10!"=="--version=" (
    set "VERSION=!ARG:~10!"
    set "VERSION_GIVEN=1"
    shift
    goto :PARSE_ARGS
)
if /i "!ARG!"=="--version" (
    set "VERSION=%~2"
    set "VERSION_GIVEN=1"
    shift
    shift
    goto :PARSE_ARGS
)
set "EXTRA_ARGS=!EXTRA_ARGS! %1"
shift
goto :PARSE_ARGS

:PARSE_DONE
:: Rejects `--version=`, `--version --no-cache` and other empty/option values.
if defined VERSION_GIVEN (
    if not defined VERSION goto :BAD_VERSION
    if "!VERSION:~0,1!"=="-" goto :BAD_VERSION
)

:: --- Release tree root (environment > .env > ..\releases) ------------------
set "RELEASES_DIR=%ARACHNEA_RELEASES_DIR%"
if not defined RELEASES_DIR if exist ".env" (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        if /i "%%A"=="ARACHNEA_RELEASES_DIR" set "RELEASES_DIR=%%~B"
    )
)
if not defined RELEASES_DIR set "RELEASES_DIR=..\releases"

:: --- Version: --version argument, else the highest `release-*` folder ------
if defined VERSION goto :VERSION_RESOLVED

echo No --version argument: looking for the highest release in "%RELEASES_DIR%\release-*"...
set "PS_TMP=%TEMP%\arachnea-build-image-version.txt"
if exist "%PS_TMP%" del "%PS_TMP%" >nul 2>&1
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$best = Get-ChildItem -LiteralPath '%RELEASES_DIR%' -Directory -Filter 'release-*' -ErrorAction SilentlyContinue |" ^
  "  ForEach-Object { ($_.Name -replace '^release-', '') -replace '[-+].*$', '' } |" ^
  "  Where-Object { $_ -match '^[0-9]' } | Sort-Object -Property { [version]$_ } | Select-Object -Last 1;" ^
  "if ($best) { Set-Content -LiteralPath '%PS_TMP%' -Value $best -Encoding Ascii }"
if not exist "%PS_TMP%" goto :NO_VERSION
set /p VERSION=<"%PS_TMP%"
del "%PS_TMP%" >nul 2>&1
if not defined VERSION goto :NO_VERSION
echo Highest release found: %VERSION%

:VERSION_RESOLVED
:: --- Fail before Docker when the release tree lacks the archives ----------
set "RELEASE_DIR=%RELEASES_DIR%\release-%VERSION%"
if not exist "%RELEASE_DIR%\linux\arachnea-%VERSION%-linux-amd64-portable.tar.gz" goto :MISSING_ARCHIVE
if not exist "%RELEASE_DIR%\linux\arachnea-%VERSION%-linux-arm64-portable.tar.gz" goto :MISSING_ARCHIVE

where docker >nul 2>&1
if errorlevel 1 (
    echo Error: docker is required but was not found in PATH.
    exit /b 1
)

:: Compose otherwise follows the globally selected Buildx builder. A
:: docker-container builder keeps the result only in its BuildKit cache unless
:: it is explicitly exported. The builder named after the active Docker
:: context writes both tags to the local image store used by Compose.
for /f "delims=" %%I in ('docker context show') do set "LOCAL_BUILDER=%%I"
if not defined LOCAL_BUILDER (
    echo Error: could not determine the active Docker context.
    exit /b 1
)
docker buildx inspect "%LOCAL_BUILDER%" >nul 2>&1
if errorlevel 1 (
    echo Error: no local Buildx builder found for Docker context "%LOCAL_BUILDER%".
    exit /b 1
)

:: The environment wins over `.env`, so the image tag, the two archive paths and
:: the stamped `ENV ARACHNEA_VERSION` all follow this version.
set "ARACHNEA_VERSION=%VERSION%"
echo Building arachnea-stream:%VERSION% from "%RELEASE_DIR%" ^(ARACHNEA_VERSION=%VERSION%^)...
:: --no-cache is not forced: pass it as an extra argument (see usage) to
:: rebuild every layer instead of reusing the cache of a previous release.
docker compose build --builder "%LOCAL_BUILDER%" %EXTRA_ARGS%
if errorlevel 1 (
    echo Error: docker compose build failed.
    exit /b 1
)
docker image inspect "arachnea-stream:%VERSION%" >nul 2>&1
if errorlevel 1 (
    echo Error: the build completed but arachnea-stream:%VERSION% was not exported to the local Docker image store.
    exit /b 1
)
docker image inspect "arachnea-stream:latest" >nul 2>&1
if errorlevel 1 (
    echo Error: the build completed but arachnea-stream:latest was not exported to the local Docker image store.
    exit /b 1
)

echo.
echo Image arachnea-stream:%VERSION% is ready. Start it with ^(from %CD%^):
echo docker compose up -d
exit /b 0

:BAD_VERSION
echo Error: --version requires a value ^(--version=^<version^>^).
exit /b 1

:NO_VERSION
echo Error: no --version argument and no release-* folder under "%RELEASES_DIR%".
echo        Build a release first ^(node build-release\release.mjs^) or pass --version=^<version^>.
exit /b 1

:MISSING_ARCHIVE
echo Error: no portable Linux archive pair for version %VERSION% in:
echo        "%RELEASE_DIR%\linux"
echo        Expected: arachnea-%VERSION%-linux-amd64-portable.tar.gz
echo                  arachnea-%VERSION%-linux-arm64-portable.tar.gz
echo        docker-compose.yml expands ARACHNEA_AMD64_ARCHIVE/ARACHNEA_ARM64_ARCHIVE
echo        to exactly these two paths and the Dockerfile copies both of them.
exit /b 1

:USAGE
echo Build the arachnea-docker image (docker compose build).
echo.
echo Usage:
echo   build-image.cmd [--version=^<version^>] [docker compose build options]
echo.
echo Options:
echo   --version=^<version^>  Value of the ARACHNEA_VERSION build variable.
echo                        Default: highest release-* folder of the release tree.
echo   -h, --help            Show this help.
echo.
echo The release tree is ..\releases, or ARACHNEA_RELEASES_DIR (environment or .env).
echo Any other argument is forwarded to "docker compose build": pass --no-cache
echo to force a full rebuild of every layer, --progress=plain or --push to
echo change the output.
exit /b 0
