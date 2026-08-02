@echo off
REM ============================================================
REM taishenIME - Windows TSF platform build script (VS18 MSVC 14.51)
REM Usage: build_vs18.bat [clean]
REM   clean  -> delete build_vs18 and reconfigure (after toolchain change)
REM Output: platform/windows/out/taishen_ime.dll + smoke test exes
REM ============================================================
setlocal
set VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat
set BLD=E:\AllinDeepSeek\taishenIME\platform\windows\build_vs18

call "%VCVARS%" >nul 2>&1
if errorlevel 1 goto :err_vcvars

if "%1"=="clean" (
    echo [1/3] Clean configure VS18 toolchain...
    if exist "%BLD%" rmdir /s /q "%BLD%"
) else (
    echo [1/3] Configure VS18 toolchain...
)

cd /d E:\AllinDeepSeek\taishenIME\platform\windows
cmake -S . -B build_vs18 -G "NMake Makefiles" 2>&1
if errorlevel 1 goto :err_build

echo [2/3] Build DLL + tests...
cmake --build build_vs18 2>&1
if errorlevel 1 goto :err_build

echo [3/3] Run smoke tests...
out\test_tsf_load.exe
if errorlevel 1 goto :err_test
out\test_ascii_mode.exe
if errorlevel 1 goto :err_test
out\test_config_reader.exe
if errorlevel 1 goto :err_test
echo ALL BUILD + TESTS PASSED
goto :eof

:err_vcvars
echo [ERROR] vcvars64.bat not found
exit /b 1

:err_build
echo [ERROR] CMake build failed
exit /b 1

:err_test
echo [ERROR] Smoke test failed
exit /b 1
