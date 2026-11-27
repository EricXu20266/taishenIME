# taishenIME — Project Agent Memory & Rules

> 本文件由 AI 代理在进入本项目工作区时自动加载。若工作区根目录存在本地项目宪法 `taishenIME.md`（已 gitignore），涉及打包发布、词库同步、候选质量回归或 git 推送前安全检查时，应先查阅 `taishenIME.md`。

## 本机部署与 Windows 隔离桌面约束（必须遵守）

在 Windows 上，AI 终端执行环境（例如 `run_command`）可能运行在隔离的隐藏桌面（`WinSta0\exebox-*`），而不是用户的交互桌面 `WinSta0\Default`。

1. **禁止保留隔离桌面拉起的 `taishen_server.exe`**
   - `install/install_latest.ps1` 末尾会执行 `Start-Process explorer.exe "<ProgramFiles>\TaishenIME\taishen_server.exe"`。
   - 若该脚本在非 `Default` 桌面执行，新启动的 `taishen_server.exe` 会运行在隐藏桌面里，却仍持有同一 Session 共享的命名管道 `\\.\pipe\taishen-ime-render`。
   - 结果：用户真实桌面（`WinSta0\Default`）上的宿主应用（如 Notepad++）会连上这个隐藏桌面里的服务端，`%LOCALAPPDATA%\TaishenIME\ime_debug.log` 里也会打印 `taishen_server: Show ... vis=1`，但候选窗实际绘制在不可见桌面里，用户屏幕上完全看不到候选窗。
2. **AI 执行本机部署后的标准收尾**
   - 运行 `install/install_latest.ps1` 后，先检查当前终端的桌面名（`GetThreadDesktop`）是否为 `Default`；若不是 `Default`，**必须立即结束脚本拉起的 `taishen_server.exe`**（`Stop-Process -Name taishen_server -Force`）并确认 `\\.\pipe\taishen-ime-render` 已释放。
   - 不要尝试从隔离桌面跨桌面启动服务。清掉误启进程后，告知用户在任意桌面应用中直接打字触发自愈（`platform/windows/src/ipc_render_client.cpp` 的 `EnsureServerRunning` 会从宿主进程在 `WinSta0\Default` 自动拉起新版 `taishen_server.exe`），或由用户在桌面手动启动 `C:\Program Files\TaishenIME\taishen_server.exe`。
3. **部署校验口径**
   - 校验 `C:\Program Files\TaishenIME` 下 `taishen_server.exe`、`taishen_ime.dll`、`taishen_ime_x86.dll`、`taishen_ime_imm32.ime`、`taishen_ime_imm32_x86.ime` 的版本号、Authenticode 签名与 `platform/windows/out` 的 SHA256 一致性，以及 x64 / x86（`WOW6432Node`）TSF CLSID 注册表路径。
   - 排查「输入有日志但看不到候选窗」时，优先检查 `taishen_server.exe` 的父进程与所属桌面是否落在非 `Default` 的隔离桌面。

## 项目关键约定速览

- **版本号单一真源**：`engine/Cargo.toml` 的 `version` 字段；`package.ps1` 与 `platform/windows/CMakeLists.txt` 均自动读取。
- **词库边界**：本仓库只读消费 `resources/` 下的词库产物，词库修改统一在独立的 `taishen-dict` 仓库完成并单向同步，禁止直接手改 `resources/common_dict.txt`。
- **推送策略**：仅在用户明确指令时 `git push`；推送前遵守公共仓库脱敏与隐私检查（不入库 `*.db`/`*.tsv`/`*.log`/本机绝对路径/私密信息）。
