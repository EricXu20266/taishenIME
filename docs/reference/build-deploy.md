# taishen_ime.dll 构建与部署手册

> 2026-08-08 实战验证。另一会话部署失败根因：**VS 2017 无 C++ 工具链**，须用 VS 2022 BuildTools。

## 1. 前置环境（关键）

MSVC 工具链（唯一可用的）：`C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat`
（`\18\` = VS2022 BuildTools；`\2017\` 只有壳无 cl.exe，cmake 会报 "CMAKE_CXX_COMPILER not set"）

## 2. 构建

```powershell
# ① Rust 引擎 release（只改 Rust 时跑）
cd E:\AllinDeepSeek\taishenIME\engine; cargo build --release

# ② CMake 配置 + ③ 构建（vcvars64 环境内，反引号被 PathGuard 拦 → 单引号字符串 + cmd /c）
$cmd = 'call "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && cd /d E:\AllinDeepSeek\taishenIME\platform\windows && cmake -B build -G "NMake Makefiles" -DCMAKE_BUILD_TYPE=Release'
cmd /c $cmd
$cmd = 'call "..." >nul && cd /d ... && cmake --build build --target taishen_ime > build_log.txt 2>&1 && echo BUILD_OK'
cmd /c $cmd
# 产物：platform\windows\out\taishen_ime.dll
```

**坑**：① generator 冲突 → 删 `platform\windows\build` 重配；② 只 build `--target taishen_ime`（全量会编 test_* 超时）；③ cmd 不支持 PowerShell `*>`。

## 3. 部署

```powershell
$dir = "$env:LOCALAPPDATA\TaishenIME"
Rename-Item "$dir\taishen_ime.dll" "taishen_ime.dll.old-$(Get-Date -Format yyyyMMdd-HHmmss)" -Force
Copy-Item "E:\AllinDeepSeek\taishenIME\platform\windows\out\taishen_ime.dll" "$dir\taishen_ime.dll" -Force
& regsvr32 /s "$dir\taishen_ime.dll"   # exit 0 = 成功
```

## 4. 验证

- regsvr32 退出码 0；注册表 `HKCU\Software\Classes\CLSID\{7D77E4AA-276E-4582-B952-94B6EFAADA28}\InprocServer32` 指向新路径

## 5. 生效（必读）

TSF DLL **进程内加载**——已打开的程序仍用旧 DLL，**必须重启应用/重登**才生效。

## 6. 本次（0.2.34）遗留提示

- 热重载改为 OnKeyDown 检查 config mtime（托盘无消息循环，WM_TIMER 死锁已废弃）
- 诊断日志 ForceLog 绕过开关；`%LOCALAPPDATA%\TaishenIME\ime_debug.log`
