@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1
cd /d E:\AllinDeepSeek\taishenIME\platform\windows
cmake -S . -B build_vs18 -G "NMake Makefiles" -DCMAKE_BUILD_TYPE=Release > C:\voice_cfg.log 2>&1
cmake --build build_vs18 --config Release > C:\voice_build.log 2>&1
echo DONE_EXIT=%ERRORLEVEL% >> C:\voice_build.log
