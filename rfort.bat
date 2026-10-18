@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Rebuild the release compiler when the sources changed, so the wrapper never
rem runs a stale build. Cargo does nothing when the build is current. Set
rem RFORT_NO_BUILD=1 to skip the check and use whichever build exists.
set "CARGO_TARGET_DIR="
if not defined RFORT_NO_BUILD (
    where cargo >nul 2>nul && if exist "%~dp0Cargo.toml" (
        cargo build --quiet --release --bin rfort --manifest-path "%~dp0Cargo.toml"
        if errorlevel 1 (
            echo Error: building the compiler failed. 1>&2
            exit /b 1
        )
    )
)
set "RFORT_COMPILER=%~dp0src\bin\releases\rfort.exe"
if exist "%~dp0target\debug\rfort.exe" set "RFORT_COMPILER=%~dp0target\debug\rfort.exe"
if exist "%~dp0target\release\rfort.exe" set "RFORT_COMPILER=%~dp0target\release\rfort.exe"
if not exist "%RFORT_COMPILER%" (
    echo Error: Build the compiler with cargo build --release --bin rfort. 1>&2
    exit /b 1
)
"%RFORT_COMPILER%" %*
exit /b %errorlevel%
