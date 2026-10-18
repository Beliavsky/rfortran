@echo off
setlocal
rem Run the existing compiler build without checking whether it is current.
rem Use rfort.bat to rebuild first when the sources changed.
set "RFORT_NO_BUILD=1"
call "%~dp0rfort.bat" %*
