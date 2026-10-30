@echo off
setlocal
"%~dp0technology-adapter\Windows-x86_64\tte-core.exe" --web-wizard %*
set "TTE_CORE_EXIT=%ERRORLEVEL%"
if not defined TTE_CORE_NO_PAUSE pause
exit /b %TTE_CORE_EXIT%
