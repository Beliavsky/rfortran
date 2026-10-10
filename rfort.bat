@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "RFORT_COMPILER=%~dp0src\bin\releases\rfort.exe"
if exist "%~dp0target\debug\rfort.exe" set "RFORT_COMPILER=%~dp0target\debug\rfort.exe"
if exist "%~dp0target\release\rfort.exe" set "RFORT_COMPILER=%~dp0target\release\rfort.exe"
if not exist "%RFORT_COMPILER%" (
    echo Error: Build the compiler with cargo build --release --bin rfort. 1>&2
    exit /b 1
)
"%RFORT_COMPILER%" %*
exit /b %errorlevel%
