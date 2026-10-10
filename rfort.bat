@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Compile Fortran sources in dependency order and link with the installed LLVM linker.
rem These library paths match the Visual Studio and Windows SDK on this machine.
set "MSVC_LIB=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.42.34433\lib\x64"
set "UCRT_LIB=C:\Program Files (x86)\Windows Kits\10\Lib\10.0.22621.0\ucrt\x64"
set "RFORT_COMPILER=%~dp0src\bin\releases\rfort.exe"
if exist "%~dp0target\debug\rfort.exe" set "RFORT_COMPILER=%~dp0target\debug\rfort.exe"
if exist "%~dp0target\release\rfort.exe" set "RFORT_COMPILER=%~dp0target\release\rfort.exe"
set "RFORT_LIB=%~dp0src\libs\x64"

if "%~1"=="" goto usage
for %%F in (%*) do (
    if not exist "%%~fF" (
        set "RFORT_MISSING=%%~fF"
        goto missing_source
    )
    set "RFORT_OUTPUT=%%~dpnF"
)
if not exist "%RFORT_COMPILER%" (
    echo Error: Compiler not found: "%RFORT_COMPILER%" 1>&2
    exit /b 1
)
where ld.lld.exe >nul 2>&1
if errorlevel 1 (
    echo Error: ld.lld.exe was not found on PATH. 1>&2
    exit /b 1
)
if not exist "%MSVC_LIB%\legacy_stdio_wide_specifiers.lib" (
    echo Error: Update MSVC_LIB in this batch file for your Visual Studio installation. 1>&2
    exit /b 1
)
if not exist "%UCRT_LIB%\ucrt.lib" (
    echo Error: Update UCRT_LIB in this batch file for your Windows SDK installation. 1>&2
    exit /b 1
)

set "RFORT_INPUT=%~f1"
if "%~2"=="" goto compile
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0f90c-sources.ps1" -Output "%RFORT_OUTPUT%.f90c-sources.f90" %*
if errorlevel 1 exit /b %errorlevel%
set "RFORT_INPUT=%RFORT_OUTPUT%.f90c-sources.f90"

:compile
"%RFORT_COMPILER%" --quiet emit-obj "%RFORT_INPUT%" "%RFORT_OUTPUT%.obj"
if errorlevel 1 exit /b %errorlevel%

ld.lld.exe -flavor link "/OUT:%RFORT_OUTPUT%.exe" "%RFORT_OUTPUT%.obj" /SUBSYSTEM:CONSOLE "/LIBPATH:%RFORT_LIB%" "/LIBPATH:%MSVC_LIB%" "/LIBPATH:%UCRT_LIB%" msvcrt.lib legacy_stdio_definitions.lib kernel32.lib
if errorlevel 1 exit /b %errorlevel%

exit /b 0

:missing_source
echo Error: Source file not found: "%RFORT_MISSING%" 1>&2
exit /b 1

:usage
echo Usage: %~nx0 [module1.f90 module2.f90 ...] main.f90
echo Put dependencies first and the main program last.
echo Creates main.obj and main.exe alongside the last source file.
echo Multiple sources also create main.f90c-sources.f90 for diagnostics.
exit /b 1
