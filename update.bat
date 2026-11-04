@echo off
rem Updates the installed RulZ Photocraft to the newest release of speedyrulz/rulz-photocraft, and
rem from nowhere else. The call ends this file on the same line: the update replaces it while it
rem runs, and cmd reads a batch file line by line.
setlocal EnableDelayedExpansion
call "%~dp0install.bat" --update %* & exit /b !errorlevel!
