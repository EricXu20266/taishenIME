@echo off
REM 定位 vcvars64.bat（VS2017/2019/2022/18）
set "VCVARS="
for /f "delims=" %%i in ('dir /s /b "C:\Program Files\Microsoft Visual Studio\vcvars64.bat" 2^>nul') do set "VCVARS=%%i"
if not defined VCVARS for /f "delims=" %%i in ('dir /s /b "C:\Program Files (x86)\Microsoft Visual Studio\vcvars64.bat" 2^>nul') do set "VCVARS=%%i"
if not defined VCVARS (
  echo VCVARS NOT FOUND
  exit /b 1
)
echo VCVARS=%VCVARS%
call "%VCVARS%" >nul 2>&1
cd /d E:\AllinDeepSeek\taishenIME\platform\windows
echo [1/2] cmake configure...
cmake -S . -B out -G "NMake Makefiles" -DCMAKE_BUILD_TYPE=Release
if errorlevel 1 exit /b 1
echo [2/2] build taishen_ime_imm32...
cmake --build out --target taishen_ime_imm32
exit /b %errorlevel%
