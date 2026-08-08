@echo off
REM Find vcvars and build DLL
for /f "delims=" %%i in ('dir /s /b "C:\Program Files\Microsoft Visual Studio\vcvars64.bat" 2^>nul') do set VCVARS=%%i
if defined VCVARS (
  echo Found: %VCVARS%
  call "%VCVARS%" >nul 2>&1
  cd /d E:\AllinDeepSeek\taishenIME\platform\windows\out
  nmake /f Makefile
) else (
  echo vcvars64.bat not found
  dir /b "C:\Program Files\Microsoft Visual Studio"
)
echo EXIT=%ERRORLEVEL%
