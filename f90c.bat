@echo off
setlocal
rem Compile one Fortran source with f90c and link with the installed LLVM linker.
rem These library paths match the Visual Studio and Windows SDK on this machine.
set "MSVC_LIB=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.42.34433\lib\x64"
set "UCRT_LIB=C:\Program Files (x86)\Windows Kits\10\Lib\10.0.22621.0\ucrt\x64"
set "F90C_COMPILER=%~dp0src\bin\releases\f90c.exe"
if exist "%~dp0target\release\f90c.exe" set "F90C_COMPILER=%~dp0target\release\f90c.exe"
set "F90C_LIB=%~dp0src\libs\x64"

if "%~1"=="" goto usage
if not "%~2"=="" goto usage
if not exist "%~f1" (
    echo Error: Source file not found: "%~f1" 1>&2
    exit /b 1
)
if not exist "%F90C_COMPILER%" (
    echo Error: Compiler not found: "%F90C_COMPILER%" 1>&2
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

"%F90C_COMPILER%" emit-obj "%~f1" "%~dpn1.obj"
if errorlevel 1 exit /b %errorlevel%

ld.lld.exe -flavor link "/OUT:%~dpn1.exe" "%~dpn1.obj" /SUBSYSTEM:CONSOLE "/LIBPATH:%F90C_LIB%" "/LIBPATH:%MSVC_LIB%" "/LIBPATH:%UCRT_LIB%" msvcrt.lib legacy_stdio_definitions.lib kernel32.lib
if errorlevel 1 exit /b %errorlevel%

echo Created "%~dpn1.exe"
exit /b 0

:usage
echo Usage: %~nx0 source.f90
echo Creates source.obj and source.exe alongside the source file.
exit /b 1
