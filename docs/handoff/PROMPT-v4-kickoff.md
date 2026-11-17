# 新会话启动条目 —— 按 V4 结构推进（不要偏移）

> 把这份文档作为新会话的第一条指令。读完再动手。
> 它不是任务清单，是**结构守则**：任何改动都要先过第二、四节。

---

## 一、项目当前状态

泰深输入法正处在**结构过渡期**：

| 层面 | 状态 |
|---|---|
| 现行结构 | `docs/ARCHITECT.md` v3.1 —— 单一 TSF DLL 承载引擎 + 渲染 + UI |
| 目标结构 | `docs/ARCHITECT-V4.md` —— 薄 DLL + 单一 server 进程 |
| 代码现状 | **仍是 v3.1**，V4 一步都还没迁 |

**已经完成、不要重做的**：

1. 静态链接 MSVC CRT（修 QQ 内崩溃）—— 提交 `fcfec6f`
2. 引擎延迟销毁（切换不再重载 179MB 词库）—— `0c94fa7`
3. `user_dict_db_path()` 区分「显式禁用 / 从未设置」（容器内切泰深不再假死 43 秒）—— `75cec82`
4. 输入法考试工具（`engine/examples/exam.rs`，26 条用例）—— `5df320a`

**未解决、V4 就是为了它们**：

| 故障 | 直接原因 | V4 哪一步解决 |
|---|---|---|
| 容器内候选窗建不出来 | DLL 内 `CreateWindowExW` + D2D 初始化阻塞 | 第 2 步（渲染外移） |
| 沉浸式宿主里空格上屏卡 22 秒 | `TF_ES_SYNC` 同步编辑会话 | 第 1 步（异步会话） |

背景与三次误判复盘见 `docs/modules/engine-process-isolation/DESIGN.md` §六点五。

---

## 二、结构铁律（四条，任何改动先过）

**1. 先判归属，再动手。**
一个动作属于 DLL 还是进程外，由 API 契约决定，不由方便程度决定：

| 动作 | 归属 | 判据 |
|---|---|---|
| `ITfKeyEventSink` 收键 | DLL | COM 回调只能由宿主进程内实例实现 |
| `ITfContext` 写组合串 / 上屏 | DLL | 编辑会话绑定宿主文档 |
| 光标矩形测量 | DLL | `GetTextExt` 需要宿主上下文 |
| 其余全部（查询、渲染、词库、语音、设置） | **进程外** | 都是值或 IO |

**2. 不往 `tsf_module.cpp` 加职责。**
它已经 3506 行、混着激活 / 按键 / 候选窗更新 / 配置应用四类。新逻辑一律放进
目标位置（`tsf/`、`ui/`、`server/`），不塞回这个文件。

**3. 结构性改动先改 `ARCHITECT-V4.md`，再动代码。**
文档先行是项目约定。改了结构但没改文档，视为未完成。

**4. 一次只推进 V4 四步中的一步。**
四步是：异步会话 → 候选窗外移 → 引擎外移 → 清理。每步独立验收，
不允许「顺手把下一步也做了」。

---

## 三、当前该做的：V4 第 1 步（异步编辑会话）

**目标**：去掉 `TF_ES_SYNC`，让上屏不再阻塞宿主 UI 线程。

**位置**：`platform/windows/src/tsf_module.cpp`
- L2990 附近：`RequestEditSession(m_tid, pSession, TF_ES_SYNC | TF_ES_READ, ...)` 读选区
- L3175 附近：`RequestEditSession(m_tid, pSession, TF_ES_SYNC | TF_ES_READWRITE, ...)` 组合操作

**依据**：沉浸式应用（任务栏搜索、Win11 新记事本）的文本存区隔着进程边界，
同步会话会让调用方一直等。同类产品源码注释里明确写了这个约束。

**要改的**：`RequestEditSession` 不带 `TF_ES_SYNC`；上屏从「同步拿结果」
改为「指令驱动」——发起会话后由回调继续，调用方不等待。

**风险**：这条路径**所有宿主都走**（记事本、浏览器、QQ、微信、游戏）。
必须做完整回归，不能只测容器。

**验收**：
1. 任务栏搜索里打拼音、按空格，上字不卡；
2. 普通宿主逐项回归：记事本 / 浏览器 / QQ / 微信 / 一个游戏，各打十几行字；
3. `cargo test --release --manifest-path engine/Cargo.toml -- --test-threads=1` 全过；
4. `cargo run --release --manifest-path engine/Cargo.toml --example exam` 无新增 FAIL。

---

## 四、禁区（这几条写在血里）

1. **不在容器分支加特判。** 2026-11-17 已经在容器分支上试了三轮
   （词库降级、跳过词表、强制自绘），全部失败——因为根本障碍是渲染与会话模式，
   不是资源访问。容器兼容只能靠第 1、2 步，不靠 if。

2. **不在 DLL 里新增任何渲染或文件 IO。** 包括「临时调试用」的。

3. **不同时推进两步。** 一步不稳不开始下一步。

4. **不凭推测改代码。** 要定位问题就先打时间戳量出来。
   2026-11-17 因为把日志采样间隔当成加载耗时，改了三轮无效代码。

5. **不改 `engine/src/lib.rs`**，如果仍有其他会话在改它；构建前确认它的 mtime
   距今 > 20 秒，否则可能读到半写状态。

6. **同一安装目录只有一个部署方**。2026-11-17 两个会话并发部署互相覆盖，
   导致「改完测试还是不行」的假象。

---

## 五、验证要求

任何改动合并前：

| 项 | 命令 / 方式 |
|---|---|
| 引擎测试 | `cargo test --release --manifest-path engine/Cargo.toml -- --test-threads=1` |
| 候选质量 | `cargo run --release --manifest-path engine/Cargo.toml --example exam` |
| C++ 构建 | `vcvars64` + `cmake --build build`（NMake，必须先加载 MSVC 环境） |
| 依赖检查 | `dumpbin /dependents`，看是否引入了不该有的依赖 |
| 宿主回归 | 见第三节验收第 2 条 |

**判断成败看输出内容，不看 `$LASTEXITCODE`** —— PowerShell 管道会吞退出码。

---

## 六、文档地图

| 文档 | 用途 |
|---|---|
| `docs/ARCHITECT-V4.md` | **目标结构 + 四步迁移 + 代码组织与边界规则**（必读） |
| `docs/ARCHITECT.md` | 现行结构 v3.1（描述今天跑的这套） |
| `docs/modules/engine-process-isolation/DESIGN.md` | 问题记录、三条故障的实测证据、三次误判复盘 |
| `docs/handoff/HANDOFF-2026-11-17-qq-crash-and-searchbox.md` | 上一次会话的完整交接 |
| `engine/examples/exam.rs` | 考试工具；用例在 `engine/tests/exam-cases.tsv` |
| `taishenIME.md` | L2 宪法：工作流、工具链、词库协作、推送前闸门 |

---

## 七、一句话总结

**这次不是修 bug，是换结构。**
凡是让 DLL 变重、让职责回流到适配层、让代码先于文档的改动，
即使当下能让某个场景跑通，也不是正确方向。
