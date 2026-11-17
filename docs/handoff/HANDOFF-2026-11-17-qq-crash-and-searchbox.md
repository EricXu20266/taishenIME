# HANDOFF 2026-11-17 — QQ 崩溃修复与任务栏搜索排查

> 会话时间：2026-11-17 08:24 → 14:30
> 分支：master，最新提交 `75cec82`

---

## 一、这次会话解决了什么

### 1. QQ 内切换输入法崩溃（已完成）

**现象**：QQ 里切到泰深，QQ 崩溃，错误模块 `MSVCP140.dll`。

**根因**：QQ 9.9.16 在 `versions\9.9.16-29927\resources\app\` 自带
`MSVCP140.dll` **14.29.30154**（VS2019）；本机系统/工具链是 **14.51**（VS18）。
Windows 加载器对同名模块按 basename 复用「进程内已加载的那一份」，不看路径。
泰深 DLL 动态依赖 CRT，一进 QQ 进程就被绑到旧版 C++ 标准库上。

**修复**：
- `platform/windows/CMakeLists.txt`：`CMP0091` + `CMAKE_MSVC_RUNTIME_LIBRARY = MultiThreaded`
- 仓库根 `.cargo/config.toml`（新建）：x64 / i686 加 `-C target-feature=+crt-static`
- C++ 与 Rust **必须同时静态**，否则 `libcmt` / `msvcrt` 混链 LNK2005
- 安全性前提已核对：FFI 全部是「调用方分配缓冲、引擎写入」的两阶段协议，
  `ffi.rs` 里 grep 不到 `*mut c_char` 返回值 / `into_raw` / `mem::forget`，
  不存在跨 DLL 堆所有权转移

**验证**：`dumpbin /dependents` 四份产物（x64/x86 × TSF/IMM32）动态 CRT 依赖清零。

**提交**：`fcfec6f`

### 2. 切换输入法卡顿（已完成）

**现象**：每次切换输入法卡 1 秒以上。

**根因**：`ReleaseEngineLease` 里租约归零即 `engine_destroy()`，下次 `Activate` 重新
加载 179MB 词库索引。`engine.log` 实测 `engine_destroy → engine_init` 间隔 1.3 秒。

**修复**：改为延迟销毁——租约归零只记空闲起点（`g_engineIdleSince`），
5 分钟内复用引擎（`dictionary::init` 本身按路径幂等），超过阈值才重建。

**提交**：`0c94fa7`

### 3. 输入法考试工具（已完成）

`engine/examples/exam.rs` + `engine/tests/exam-cases.tsv`：用引擎 API 复现
「敲拼音看候选」，逐条断言期望词位次，替代人工在宿主里逐项验证。26 条用例，
FAIL 时退出码 1 可接 CI。

**首跑基线 24/26（92.3%）**，暴露两个真实缺陷（未修）：
- `ji → 急` 掉出前 9（前 9：及 / 即 / 既 / 极 / 继续）
- `wu → 武` 掉出前 9（前 9：无 / 五 / 物 / 吴 / 无语）

`cargo test` 全过但未覆盖这两例，说明 `first_char_verify` 的断言口径更松。

**提交**：`5df320a`

---

## 二、任务栏搜索（AppContainer）—— 查出三条故障，修好一条

### 故障 1：切到泰深后整个搜索框假死 43 秒（已修）

**根因**：`dictionary/mod.rs` 的 `user_dict_db_path()` 把「显式禁用」和
「从未设置」混为一谈。容器分支调用 `engine_set_user_dict_path(nullptr)` 本意是
禁用用户词库，但该函数看到 `paths().user` 为空就**回退到 `%APPDATA%`**，随后
`load_pin_demote_state()` / `load_context_pairs()` 打开那个路径的 SQLite——
AppContainer 文件虚拟化下这一步阻塞 43 秒。

**修复**：加 `USER_DICT_DISABLED` 原子标志，`set_user_dict_path(None)` 时置位，
`user_dict_db_path()` 见到直接返回 `None`。「从未设置」的回退行为保持不变，
普通应用零影响。

**实测证据（同一段代码前后对比）**：
```
修复前  ActivateEx step: apply-config +43938ms
修复后  ActivateEx step: apply-config +0ms
        词库加载 1 秒完成（status=2 entries=372828）
```

**提交**：`75cec82`

### 故障 2：候选窗在容器内建不出来（未解）

日志证明候选数据是有的（`pinyin=2 cands=5`），但 `EnumWindows` 枚举 SearchApp
进程得到 **0 个窗口**。卡点在 `m_candidateWindow.Update()` 首次调用：
`CreateWindowExW` + D2D/DWrite 初始化在 AppContainer 里阻塞宿主 UI 线程。

**当前处理**：容器内直接 `m_candidateWindow.Hide(); return;`，不自绘。

### 故障 3：空格上屏卡 22 秒（未解）

日志：`pinyin=5 cands=5`（14:08:17）→ 22 秒后 → `pinyin=0 cands=0`（14:08:39）。
中间 `UpdateCandidateWindow` 一次都没被调用，说明卡在更早的上屏路径。

**根因**：`RequestEditSession(m_tid, pSession, TF_ES_SYNC | TF_ES_READWRITE, &hrSession)`
用的是同步编辑会话。沉浸式应用（任务栏搜索、Win11 新记事本这类）的文本存区
隔着进程边界，同步会话会一直等。

同类产品的源码注释明确写着「必须用异步会话（不带 TF_ES_SYNC）：沉浸式应用的
文本存区隔着进程边界」，它用的就是 `TF_ES_READWRITE`（异步）。

**为什么没改**：上屏路径影响**所有宿主**（QQ / 微信 / 浏览器 / 游戏全走这里），
改异步会改变控制流（`RequestEditSession` 返回时结果可能还没出来），
必须配完整回归，不适合在边试边改的状态下做。

---

## 三、当前系统状态

| 项 | 值 |
|---|---|
| 安装目录 DLL | `taishen_ime.dll` = **14:01:55** 那一版（带 `ActivateEx` 耗时埋点） |
| 容器读权限 | **未授权** —— 任务栏搜索里泰深不可用，系统回退微软拼音 |
| 工作区 | 干净 |
| 最新提交 | `75cec82` |

**各场景可用性**：

| 场景 | 状态 |
|---|---|
| QQ | 正常（崩溃已修） |
| 微信 / 浏览器 / Notepad++ / 游戏 | 正常（切换卡顿已改善） |
| 任务栏搜索 | 泰深不可用，用微软拼音。不卡 |

---

## 四、遗留与风险

### 1. 安装目录堆积 20+ 个 `.old-*` 备份 DLL

约 150MB 以上，多数被进程映射无法立即删除，需要重启后清理。

### 2. 并发部署冲突（重要）

本次会话期间，**另一个会话也在同一仓库、同一 `out` 目录、同一安装目录上
构建部署**。观察到 12:35、13:17 两次覆盖，导致「改完测试还是不行」的假象——
测的根本不是最新改动。

安装目录里存在两种备份命名（`.old-YYYYMMDD-HHmmss` 与 `.old.YYYYMMDDHHmmss`），
是两边各自留下的痕迹。

**下次继续前，先确认部署权归谁**，否则会重复今天的混乱。

### 3. 考试工具发现的两个候选排序缺陷

`ji → 急`、`wu → 武` 都掉出前 9。这是真实问题，但属于候选排序领域，
`lib.rs` 当时被另一个会话占用，未动。

---

## 五、下一步建议（按性价比）

**优先级 1：异步编辑会话改造**

唯一有现成解法的一条。把 `RequestEditSession` 的两处调用（`tsf_module.cpp`
的 L2990 读选区、L3175 组合操作）从 `TF_ES_SYNC` 改为异步，同时重构上屏流程
以处理「结果延迟返回」。

风险：影响全部宿主。必须做完整回归——记事本、浏览器、QQ、微信、游戏各测一遍。

**优先级 2：候选窗外移**

容器内 D2D 建窗阻塞，应用层无解。只有把候选窗搬到独立进程才能同时解决
「容器内不可见」和「高 z-band 宿主遮挡」。这是设计稿阶段 2 的内容。

**优先级 3：修 ji / wu 的候选排序**

考试工具已经能稳定复现，改完跑一次 exam 即可验证。

**不建议**：继续在容器分支上加特判。今天已经证明这条路走不通——
在应用进程内既不能建 D2D 窗口，也不能用同步编辑会话。

---

## 六、踩坑记录（避免重复）

1. **日志只能证明「记录时刻的状态」，不能反推事件耗时**。
   `LogDictLoadStatusOnce` 只在按键 / 焦点 / 定时器被调用，两次采样间隔 43 秒
   **不是**加载耗时。我据此改了三轮代码，全部白费。

2. **用独立 EXE 在容器里跑引擎，不能证明 TSF 在容器里能工作**。
   探针测出 `status=2 / entries=372828`，一度让人误判「容器内引擎是好的」。
   两者的文件访问上下文、注册表视图、UI 线程约束都不同。

3. **`EnumWindows` 返回 0 个窗口 ≠ 窗口被盖住**。
   0 个窗口说明窗口**没创建**，与 z-band 遮挡是两件事。先分清这两个，
   再决定往哪个方向查。

4. **PowerShell 管道会吞退出码**：`cargo build | Select-Object -Last N` 之后
   `$LASTEXITCODE` 不可信。判断成败要看输出里的 `Finished` / `error`，
   或显式写 `"EXIT=$LASTEXITCODE"`。

5. **构建前确认 `engine/src/lib.rs` 稳定**（mtime 距今 > 20 秒），
   否则 cargo 可能读到半写状态，报出与眼前源码不符的错误。

---

## 七、相关文档

- `docs/bugs/BUG_2026-11-17.md` —— QQ 崩溃根因与静态 CRT 修复
- `docs/modules/engine-process-isolation/DESIGN.md` —— 进程隔离设计稿，
  含任务栏搜索三次误判与纠正（§六点五）
- `engine/tests/exam-cases.tsv` —— 考试用例，改断言不用改代码
