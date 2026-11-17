# 任务：泰深输入法 · AppContainer 兼容（阶段 1）

> 交给新会话直接执行。本文件自包含，不需要外部背景。
> 先读 `docs/modules/engine-process-isolation/DESIGN.md`（设计稿）与
> `docs/bugs/BUG_2026-11-17.md`（前置事故），再动手。

---

## 一、背景（已完成，不要重做）

2026-11-17 QQ 内切换输入法崩溃，根因是宿主自带旧版 `MSVCP140.dll`，已用静态 CRT 修复：

- `platform/windows/CMakeLists.txt` 已设 `CMAKE_MSVC_RUNTIME_LIBRARY = MultiThreaded`（/MT）
- 仓库根 `.cargo/config.toml` 已给 x64/i686 加 `-C target-feature=+crt-static`
- 四个产物（`taishen_ime.dll` / `_x86` / `_imm32.ime` / `_imm32_x86.ime`）已零动态 CRT 依赖
- 已部署到 `%LOCALAPPDATA%\TaishenIME`，提交 `fcfec6f`、`da81878`、`d195016`

**这些是已完成状态，不要改动，不要重跑构建验证之外的修改。**

## 二、本次任务：AppContainer 兼容阶段 1

### 问题

TSF DLL 会被加载进 AppContainer 进程（任务栏搜索、商店应用）。当前资源位置：

| 资源 | 路径 | 代码位置 |
|------|------|---------|
| 系统/领域/bigram 词库 | `dllDir + cfg.dict_path` → `%LOCALAPPDATA%\TaishenIME\*` | `config_reader.cpp:ResolveDictPath` |
| 配置文件 | 同上 `config.ini` | `config_reader.cpp` |
| 用户词库 | `%APPDATA%\taishen-ime\user_dict.db` | `config_reader.cpp:ResolveUserDictPath` |

Container 进程读不到这些路径，且失败**完全静默**：

1. `engine_init` 永远返回 0（`engine/src/ffi.rs`，函数末尾 `0`）
2. `dictionary::init` 加载失败走 `builtin_common()`（`engine/src/dictionary/mod.rs:379`）
3. 用户只感觉「输入法变笨」，没有任何日志

### 目标（三项，缺一不算完成）

**1) 资源 ACL —— 让词库可被容器读取**

`install/install.ps1`（以及 `install_latest.ps1`）对安装目录加读权限：

```powershell
icacls "$InstallDir" /grant '*S-1-15-2-1:(OI)(CI)(RX)' /T /Q
```

- `S-1-15-2-1` = `ALL APPLICATION PACKAGES`
- 只给读，**不给写**（跨容器写入口是安全倒退）
- 不动 `%APPDATA%\taishen-ime`（用户词库保持容器外）

**2) 运行时检测与降级**

在 TSF 激活路径（`platform/windows/src/tsf_module.cpp`，`CTextService::ActivateEx`
或 `AcquireEngineLease` 附近）检测一次：

```cpp
BOOL isContainer = FALSE; DWORD returned = 0;
GetTokenInformation(GetCurrentProcessToken(), TokenIsAppContainer,
                    &isContainer, sizeof(isContainer), &returned);
```

检测结果缓存到 TextService 实例（不要每次按键都查）。容器内：

| 功能 | 行为 |
|------|------|
| 系统词库 | 正常加载（ACL 已放行） |
| 用户词库加载 | 跳过 |
| 学习写回（`flush_user_writes` / `learn_*`） | 跳过，并记一次 INFO |
| `config.ini` 读取 | 尽力读，失败用默认值 |
| `config.ini` 写入 / 设置窗口 | 禁用并记 WARN |

**3) 失败可见化（最关键）**

当前「词库不可达」和「词库还在异步加载」不可区分。需要：

- `engine/src/dictionary/mod.rs`：`init` 区分「文件不存在」「拒绝访问」「正在加载」，
  对外暴露一个查询接口（如 `last_load_status() -> i32`）
- `engine/src/ffi.rs`：新增或扩展一个 FFI 返回加载状态，**不要改 `engine_init` 的返回语义**
  （它恒 0 是现有契约，改会让平台层误判失败）
- 加载失败降级到 `builtin_common()` 时记 **WARN**，带路径与错误码
- 成功加载时记 INFO，带词条数
- 平台层日志加一行 `AppContainer=<0|1>`

## 三、验收标准

1. 在任务栏搜索框切到泰深，能打出系统词库里的常用词（不是只有「的、了、是」）；
2. `ime_debug.log` 或引擎日志能看到 `AppContainer=1` 与词库加载词条数；
3. 普通进程（记事本、浏览器、QQ）里行为**零变化**——这条必须逐项对比验证；
4. `cargo test --release --manifest-path engine/Cargo.toml -- --test-threads=1` 全通过；
5. 四个产物重新构建后仍 `CRT_DYNAMIC=NONE`（`dumpbin /dependents` 检查）。

## 四、构建与验证命令

Rust：

```
cargo build --release --manifest-path engine/Cargo.toml
cargo build --release --manifest-path engine/Cargo.toml --target i686-pc-windows-msvc
cargo test --release --manifest-path engine/Cargo.toml -- --test-threads=1
```

C++（NMake generator，必须先加载 vcvars）：

```
# 64 位
cmd /c "call \"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat\" && cd /d E:\AllinDeepSeek\taishenIME\platform\windows && cmake --build build"

# 32 位
cmd /c "call \"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars32.bat\" && cd /d E:\AllinDeepSeek\taishenIME\platform\windows && cmake -S . -B build_x86 -G \"NMake Makefiles\" && cmake --build build_x86"
```

验证依赖：`dumpbin /dependents <产物>`，CRT 相关 DLL 必须为空。

## 五、硬约束

1. **不要做阶段 2**（引擎进程外）——那是独立评审项，见设计稿。
2. **不要改 `engine/src/lib.rs`**——另一个会话正在改它，你改会冲突。
3. **构建前确认 `engine/src/lib.rs` 稳定**：读文件 mtime，若少于 20 秒前被改过，
   等它稳定再编译（实测撞过半写状态，报的错误与眼前源码不符）。
4. **PowerShell 管道会吞退出码**：`cargo build | Select-Object -Last N` 后
   `$LASTEXITCODE` 不可信，用 `"EXIT=$LASTEXITCODE"` 显式输出或在输出里找 `Finished` / `error`。
5. **竞品名不入库**：文档与注释里不出现其他输入法产品的名称，
   写「同类产品」即可。
6. **提交范围**：只提交本次改动，不要把工作区里其他会话的改动一起提交。
7. **推送前跑公开仓库闸门**（`taishenIME.md` 第三章「推送前安全隐私闸门」），
   用户没说「推送」就不要 push。

## 六、完成后要产出

1. 代码改动 + 提交（`[AI:泰深]` 前缀）
2. `docs/modules/engine-process-isolation/DESIGN.md` 的阶段 1 状态改为「已实现」，
   补实测结果（容器内词条数、普通进程对比结论）
3. 若发现设计稿与实现有偏差，直接在设计稿里更正，不要另起一份文档
