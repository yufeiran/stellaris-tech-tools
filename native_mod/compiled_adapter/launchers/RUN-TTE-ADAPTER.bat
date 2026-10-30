@echo off
setlocal
"%~dp0tte-core.exe" --build-wizard %*
set "TTE_CORE_EXIT=%ERRORLEVEL%"
if not defined TTE_CORE_NO_PAUSE pause
exit /b %TTE_CORE_EXIT%
