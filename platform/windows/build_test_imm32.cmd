@echo off
set "VCVARS="
for /f "delims=" %%i in ('dir /s /b "C:\Program Files\Microsoft Visual Studio\vcvars64.bat" 2^>nul') do set "VCVARS=%%i"
if not defined VCVARS for /f "delims=" %%i in ('dir /s /b "C:\Program Files (x86)\Microsoft Visual Studio\vcvars64.bat" 2^>nul') do set "VCVARS=%%i"
if not defined VCVARS (
  echo VCVARS NOT FOUND
  exit /b 1
)
call "%VCVARS%" >nul 2>&1
cd /d E:\AllinDeepSeek\taishenIME\platform\windows
cmake --build out --target test_imm32_load
