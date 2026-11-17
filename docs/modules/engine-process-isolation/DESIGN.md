# 引擎进程隔离与 AppContainer 兼容 — 设计稿

> 触发事件：2026-11-17 QQ 内切换输入法崩溃，错误模块 `MSVCP140.dll`。
> 该故障已由静态 CRT 修复（见 `docs/bugs/BUG_2026-11-17.md`），本文讨论它暴露的**结构性问题**，
> 以及并行的 AppContainer 兼容缺口。
>
> 状态：设计阶段，未进入实现。每个阶段都需要单独确认后再动手。

---

## 一、问题：两个独立故障面

### 1.1 宿主崩溃面——in-process 结构的代价

TSF 文本服务是一个 COM DLL，系统把它加载进**每一个**接受文本输入的应用进程。
泰深把整套引擎、候选窗渲染、配置读取、语音管理都放在这个 DLL 里：

| 组件 | 位置 | 崩溃影响 |
|------|------|---------|
| 拼音引擎（Rust 静态库） | `engine_bridge.cpp` → `ffi.rs` | 引擎 panic 已是 `ffi_guard!` 兜底，但不是零风险 |
| 候选窗渲染（D2D/DWrite） | `candidate_window.cpp` + `ui_*.cpp` | 渲染异常直接带崩宿主 |
| 配置读取 | `config_reader.cpp` | 文件 I/O 异常路径 |
| 语音管理 | `voice_manager.cpp` | 网络探测 + 进程检测 |

2026-11-17 的 QQ 崩溃是**依赖层**的问题（CRT 版本），静态链接后消除。
但结构层面的风险还在：DLL 内任何一处未捕获的异常，代价都是宿主进程崩溃。

实测调用密度（`grep "engine_[a-z_]+\("` 统计）：

| 文件 | 调用点 | 性质 |
|------|-------|------|
| `tsf_keyevent.cpp` | 75 | 按键主链路（核心） |
| `tsf_module.cpp` | 68 | 激活/焦点/组合串（核心） |
| `imm32/imm32_ime.cpp` | 38 | IMM32 老应用入口（核心） |
| `banner_window.cpp` | 13 | 状态横幅 |
| `voice_manager.cpp` | 9 | 语音 |
| `app_state.cpp` | 7 | 应用级配置 |
| `langbar_item.cpp` | 7 | 语言栏 |
| `candidate_window.cpp` | **0** | 渲染层已与引擎解耦 |

去重后 **50 个唯一 FFI 函数**，其中 181 个调用点集中在 TSF 主链路与 IMM32 入口
（`tsf_keyevent` 75 + `tsf_module` 68 + `imm32_ime` 38）。

一条对后续设计有利的事实：**候选窗已经不直接调引擎**（0 次调用），
它只消费上游传入的候选数据。这意味着渲染层与引擎之间已有天然切口。

### 1.2 AppContainer 文件权限面

AppContainer 进程（任务栏搜索、商店应用、部分 UWP）对文件系统的访问受包能力限制。
泰深当前的资源位置：

| 资源 | 路径 | 来源 |
|------|------|------|
| 系统词库 | `dllDir + config.dict_path` → `%LOCALAPPDATA%\TaishenIME\system_dict.db` | `config_reader.cpp:ResolveDictPath` |
| 领域库 / bigram / 常用词 | 同上目录 | `engine_set_*_path` |
| 配置文件 | 同上目录 `config.ini` | `config_reader.cpp` |
| 用户词库 | `%APPDATA%\taishen-ime\user_dict.db` | `config_reader.cpp:ResolveUserDictPath` |

这条链路的实际行为（代码实测，非推测）：

1. `engine_init` **永远返回 0**（`ffi.rs`），词库加载失败不会让 DLL 加载失败；
2. `dictionary::init` 加载不到文件时走 `builtin_common()` 内置兜底（`dictionary/mod.rs:379`）；
3. 所以 AppContainer 里不会崩，但只有内置的极小词表——打得出「的、了、是」，打不出大部分词。

也就是说这是**功能退化**，不是崩溃。用户感知是「这些应用里输入法很笨」，不会报错。

---

## 二、方案对比

| 方案 | 做什么 | 成本 | 解决 1.1 崩溃面 | 解决 1.2 AppContainer |
|------|-------|------|----------------|---------------------|
| **A 现状 + 静态 CRT**（已完成） | 只修依赖层 | 已完成 | ✗ | ✗ |
| **B 资源 ACL 开放** | 安装时给词库目录加 `ALL APPLICATION PACKAGES` 读 ACE | 1 天 | ✗ | 部分（只读词库，用户词库仍不可写） |
| **C 引擎进程外** | 瘦 TSF DLL + 独立 server 进程 + 命名管道 IPC | 2–3 周 | ✓ | ✓（server 是普通进程，能读用户目录） |
| **D 候选窗外移** | 渲染搬到 server 进程 + `uiAccess` manifest | C 的一部分 | 减少 DLL 内代码 | 与 1.2 无关 |

### 关于 B 的说明

`ALL APPLICATION PACKAGES`（SID `S-1-15-2-1`）是让传统 Win32 资源被 AppContainer 读取的标准做法，
安装时用 `icacls <目录> /grant *S-1-15-2-1:(OI)(CI)(RX)` 即可。

限制：
- 只读可行，**写入不可行**（给 AppContainer 写权限等于开一个跨容器写入口，安全上不接受）；
- 用户学习功能在 AppContainer 里会自动失效，退化回「不学习」；
- 配置也无法写回。

### 关于 C 的说明

行业观察：同类 Windows 输入法里，多数把引擎放在同进程 DLL 内，
少数采用独立服务进程 + 命名管道。

所以 C **不是行业共识**，是结构性取舍。它的价值不在「跟别人一样」，而在：

1. DLL 崩溃面收敛到「按键转发 + 文档写入」两个动作；
2. 引擎状态在进程间天然共享（现在每个宿主进程各有一份 Engine，学习结果靠用户词库文件间接同步）；
3. AppContainer 问题顺带解决——server 是普通权限进程。

代价：
1. 每次按键一个 IPC 往返（本机命名管道，延迟量级 0.1–0.3 ms，相对候选查询 <5ms 可接受）；
2. 生命周期管理（server 被结束后要能自动拉起，否则输入法失效）；
3. 协议版本必须与 DLL 严格同步（老 DLL + 新 server 会静默退化）；
4. 迁移期间要保留进程内路径作为回退。

---

## 三、推荐路径

分三段，**每段独立验收，可以停在任何一段**：

| 阶段 | 内容 | 收益 | 风险 |
|------|------|------|------|
| 0（已完成） | 静态 CRT | 消除依赖层崩溃 | 无 |
| 1 | AppContainer 降级与可见化 + 资源 ACL | 商店应用里可用（只读词库），并且**失败可观测** | 低 |
| 2 | 引擎进程外 | 根治崩溃面 + 完整 AppContainer 支持 | 高，需单独评审 |

阶段 1 现在就可以做，且不依赖阶段 2 的决策。
阶段 2 建议等两件事成熟再启动：一是宿主崩溃有实际的第二次样本（证明不是单点），
二是候选结构体化（`docs/modules/candidate-ranking/DESIGN.md` 的 P1 项）落地——
否则 IPC 协议会被 `Vec<String>` 的数据形态绑死，后续打分体系改动要重做协议。

---

## 四、阶段 1 详细设计：AppContainer 兼容

### 4.1 检测

TSF 激活时（`CTextService::ActivateEx`）调用一次 `GetTokenInformation(TokenIsAppContainer)`，
结果缓存在 TextService 实例上。不引入额外依赖（`advapi32` 已链接）。

### 4.2 资源访问

安装阶段（`install/install.ps1`）给安装目录加读权限：

```powershell
icacls "$InstallDir" /grant '*S-1-15-2-1:(OI)(CI)(RX)' /T /Q
```

这覆盖 `system_dict.db` / `domains.db` / `common.db` / `bigram.db` / `.bin` 索引。
**不动** `%APPDATA%\taishen-ime\user_dict.db` —— 用户词库不给 AppContainer 读写。

### 4.3 运行期降级

检测到 AppContainer 时：

| 功能 | 正常进程 | AppContainer |
|------|---------|-------------|
| 系统/领域词库 | 读 `%LOCALAPPDATA%` | 读安装目录副本（ACL 放行） |
| 用户词库加载 | 读 `%APPDATA%` | 跳过（只读模式） |
| 学习写回 | 写用户词库 | 跳过，记一条 INFO 日志 |
| 配置读写 | 读写 `config.ini` | 只读，写操作跳过 |

### 4.4 可观测性（关键）

当前失败是**静默**的——`engine_init` 恒返回 0，用户只看到「输入法很笨」。
阶段 1 必须让它在日志里可见：

- `engine_init` 记录 `dict_path` 与实际加载到的词条数（`ffi.rs` 已有 info 日志，补词条数）；
- 词库降级到 `builtin_common()` 时记 **WARN**，带原因（文件不存在 / 拒绝访问）；
- AppContainer 检测结果记一次 INFO。

判定错误码：`dictionary::init` 目前不返回错误，需要让 `is_ready()==0` 与「词库文件不可达」
可区分——这是阶段 1 最小代码改动（约 30 行）。

---

## 五、阶段 2 详细设计：引擎进程外（若启动）

### 5.1 进程拓扑

```
应用进程 A ─┐
应用进程 B ─┼─ taishen_ime.dll（瘦）─┐
应用进程 C ─┘                        │  命名管道
                                     ├────────▶ taishen-server.exe
32 位应用  ─── taishen_ime_x86.dll ──┘          持有唯一 Engine
                                                （读词库/写用户词/候选窗自绘）
```

### 5.2 职责切分

| 留在 DLL | 搬到 server |
|---------|------------|
| `ITfKeyEventSink::OnKeyDown` 收键 | 引擎状态机（`process_key` 等 50 个 FFI） |
| `ITfContext` 组合串读写（`tsf_composition.cpp`） | 候选生成与排序 |
| 光标屏幕矩形测量 | 候选窗渲染（`candidate_window.cpp` + `ui_*.cpp`） |
| 会话开/关、焦点事件 | 配置读取、用户词库读写 |
| — | 语音管理（`voice_manager.cpp`） |
| — | 状态横幅（`banner_window.cpp`） |

DLL 侧代码量预计减少 70%（`ui_*.cpp` 13 个文件 + `candidate_window.cpp` + `banner_window.cpp`
+ `voice_manager.cpp` + `audio_capture.cpp` 全部外移）。

### 5.3 IPC 协议

命名管道 `\\.\pipe\taishen-ime-<sid>`，长度前缀 + JSON 帧（与现有 `Cargo` 生态一致，
不引入 protobuf 依赖）。

**请求（DLL → server）**：

| 消息 | 载荷 | 对应现有 FFI |
|------|------|-------------|
| `SessionOpen` | 宿主 exe 名、进程 ID | `engine_init` + `app_state.cpp` |
| `Key` | 字符码、修饰键、是否组合中 | `engine_process_key` / `engine_backspace` / `engine_move_cursor` |
| `CandidatePage` | 页码、每页数量 | `engine_get_candidate_at` 系列 |
| `Select` | 全局候选索引 | `engine_select_candidate` |
| `ComposeUndo` / `ComposePrefix` | 无 | `engine_compose_undo` / `engine_compose_prefix` |
| `TakeChar` | 是否首字 | `engine_take_char` |
| `SessionClose` | 无 | 释放租约（对应 `g_engineLeaseCount`） |

**响应（server → DLL）**：

| 消息 | 载荷 |
|------|------|
| `KeyResult` | 是否消费、上屏文本、组合串、候选页、光标锚点 |
| `FrameUpdate` | 异步刷新（云联想/整句模型就绪后主动推送） |
| `CandidatesReady` | 候选列表（结构体化后带 score/source） |

### 5.4 状态归属

| 状态 | 归属 | 理由 |
|------|------|------|
| 组词已选前缀 | **server**（保持现状：引擎唯一真相源） | 现在就是引擎单点，迁移后仍单点 |
| 中英模式 / 符号模式 | server（按 `SessionId` 分开） | 每会话独立 |
| 组合串显示文本 | DLL（从 `KeyResult` 拿） | 必须同步写回 `ITfContext` |
| 候选窗可见性 | server 持有渲染状态，DLL 只报锚点 | 窗口在 server 进程 |
| 引擎生命周期 | server 自己的进程生命周期 | 退出即销毁 |

### 5.5 迁移步骤

1. **抽协议 crate**：`crates/taishen-protocol`（纯 Rust，无 Windows 依赖），
   两端共用消息定义与编解码，可单测。
2. **server 骨架**：包一层现有 `ffi.rs` 的函数为消息处理器，内部仍调同一 Engine，
   先做到「进程内 DLL 与 server 跑同一套逻辑，结果逐字节核对」。
3. **DLL 双路径**：`config.ini` 加 `use_server=0/1`，0 走现有 FFI、1 走管道；
   两路径共用同一组 `ReadEngineUtf8` 风格接口，便于 A/B。
4. **切换默认值**：A/B 结果一致后改默认 `use_server=1`，保留进程内回退一个版本。
5. **删除进程内路径**：稳定两个版本后移除静态链接与 `engine_bridge.cpp` 直连。

### 5.6 风险与缓解

| 风险 | 缓解 |
|------|------|
| server 未启动 → 输入法失效 | DLL 首次激活时 `CreateProcess` 拉起，并等待握手（上限 300ms），超时回退进程内路径 |
| server 被结束 | 管道断开即重连；重连期间按键透传（不吞键） |
| 协议版本不匹配 | 握手带版本号，不匹配时拒绝并回退进程内路径（**不能只警告**——警告等于静默错误） |
| 32 位宿主连 64 位 server | 管道与位数无关，天然支持；但 x86 DLL 必须是独立产物（现状已有） |
| 延迟 | 本机管道往返 0.1–0.3 ms；首键冷启动需预加载词库，激活时异步触发 |

---

## 六、验收标准

### 阶段 1

- 任务栏搜索 / 商店应用里切到泰深，能打出系统词库的常用词（非仅内置表）；
- 日志里能看到 `AppContainer=1` 与词库加载条数；
- 普通进程（记事本、浏览器）行为零变化。

### 阶段 2

- 应用进程模块列表里 `taishen_ime.dll` 不再持有引擎静态库（体积显著下降）；
- server 崩溃后 300ms 内自动重建，用户的组合串不丢失（或明确丢弃并记日志）；
- 同一宿主连续输入 100 字，候选与进程内路径逐条一致；
- 断电/强杀 server 后，下一次按键能拉起。

---

## 七、不做什么

| 不做 | 理由 |
|------|------|
| 一次完成全部外移 | 候选窗与引擎可分离迁移，先迁引擎收益最大、风险最小 |
| 给 AppContainer 写权限 | 跨容器写入口是安全倒退；宁可功能降级 |
| 用 RPC/DCOM 替代管道 | 额外 COM 注册负担，管道零注册、易调试 |
| 为 AppContainer 单独做词库副本 | 同一份安装目录 + ACL 即可，避免两套词库分叉 |
| 现在就改架构 | 阶段 2 依赖候选结构体化落地，否则协议返工 |

---

## 八、附：本文依据

| 结论 | 依据 |
|------|------|
| DLL 内引擎调用密度 | `grep "engine_[a-z_]+\(" platform/windows/src/**/*.cpp` 统计 |
| 候选窗不调引擎 | 同上，`candidate_window.cpp` 命中 0 次 |
| 词库路径落在 `%LOCALAPPDATA%` | `config_reader.cpp:ResolveDictPath` |
| 加载失败走内置兜底 | `dictionary/mod.rs:379` `builtin_common()` |
| `engine_init` 恒返回 0 | `ffi.rs:engine_init` 结尾 `0` |
| 同类产品架构对比 | `reference/ime-reverse-20261031/cross-ime-comparison.md` §2 |

---

