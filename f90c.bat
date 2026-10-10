@echo off
rem Compatibility entry point; rfort.bat is the primary wrapper.
call "%~dp0rfort.bat" %*
exit /b %errorlevel%
