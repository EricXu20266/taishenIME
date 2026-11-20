# 交接 — 审计闭环、候选统一来源裁决与架构 V4（D1+D2）落地（2026-11-20）

> 版本演进：**v0.1.74 → v0.1.75 → v0.1.76 → v0.1.77**（双仓库共 8 个本地提交，尚未 push）。
> 考试通过率：**247/291（84.9%）→ 266/291（91.4%）**（`--top 9`）；`--top 5` 达 **263/291（90.4%）**；已知缺陷回归 **12/15**（top 5 下 **13/15**）。
> 安装包已生成：`install/TaishenIME-Setup-0.1.77.exe`（93.8 MB，含 `taishen_server.exe` 进程外候选窗渲染 + AppContainer/LPAC 通道）。

---

## 一、正在推进的目标

本轮围绕「反馈闭环审计 → 核心方案落地 → 架构 V4 搜索框/AppContainer 破局」四条主线连续推进：

| 目标主线 | 核心内容 | 完成状态 |
|---|---|---|
| **0. 现状审计与解决方案设计** | 梳理历史用户反馈闭环统计、现状汇总及 4 份核心设计稿，对照源码核查 7 处实现陷阱（R1–R7），形成可执行总方案 | ✅ 已完成 |
| **Wave 1：交付与口径闭环** | 统一候选基线口径（247/291）、`engine_build_id()` 运行时版本核验、多产物原子部署 + 部署锁 + `DEPLOY-VERSION.json`、全量 `*.old*` 清理、容器 ACL 开关化 | ✅ 已完成并提交 |
| **Wave 2：候选排序统一来源裁决** | 引入 `tier`（T0–T5）+ `reasons` 位标记 + `score` 可解释元数据；精确词频可信度阈值（`CREDIBLE_EXACT_FREQ = 1600`）；消除纠错抢精确命中；补全锚点与窗宽（5/9）彻底解耦；生僻字（Ext-A/B+）识别修复与降权；清理内置短语占位/冲突项；英文精确词抑制拼音纠错 | ✅ 已完成并提交 |
| **Wave 3：候选长尾修复 + 词库治理 + 设计稿收口** | 修复 `zhyang` 管道层（遗留-02）、`zhegeweomende` 拆组 B 段常用词优先（遗留-01）、中英驼峰混输（`woshiEric`）、明确英文词屏蔽 Combo 噪声（`hello`）；在 `taishen-dict` 完成领域词迁出与常用简拼/单字 boost 并同步至 `2026.11.20.1` + 重建 `.bin`；收口 4 份设计文档与结构检查脚本 | ✅ 已完成并提交 |
| **架构 V4 D1 + D2：容器破局与进程外渲染** | **D1**：彻底清除全部 `TF_ES_SYNC`，改为纯异步编辑会话（COM 生命周期引用计数 + 异步光标回调）；**D2**：新增轻量级 `taishen_server.exe` 承载 `CCandidateWindow` 进程外渲染，通过 Named Pipe（`\\.\pipe\taishen-ime-render`，含 AC + LPAC + Low Integrity SDDL）通信；安装器默认开启最小化只读/执行 ACL 与 Medium Integrity 自启，并配套可选 `tools/enable-uiaccess.ps1` | ✅ 代码、单测、IPC 冒烟与 v0.1.77 打包已完成；⏳ 待实机安装验收 |
| **架构 V4 D3 + D4（后续）** | D3 引擎迁入 `taishen_server.exe`（抽 `crates/taishen-protocol`）；D4 删除进程内渲染/引擎回退路径 | ⏳ 未启动（待 D1+D2 实机验收后推进） |

---

## 二、当前代码或任务的进展状态

### 1. 已完成（代码均已本地 commit，工作树干净）

**自动化测试与构建验证结果（v0.1.77 / HEAD `4553377`）**：

| 检查项 | 结果 | 说明 |
|---|---|---|
| `cargo test --release --lib -- --test-threads=1` | **389 passed, 0 failed, 4 ignored** | 新增来源冲突裁决套件与窗宽不变性属性测试；原 `#[ignore]` 的 `test_group_weom_to_wom` 已取消忽略并通过 |
| `cargo test --release --test ffi_integration` | **1 passed, 0 failed, 1 ignored** | FFI 集成通过 |
| `exam`（`--top 9`） | **266/291（91.4%）** | L1 86/95 · L2 59/63（+6）· L3 8/13 · L4 66/75（+6）· L5 59/60（+7）；唯一 L5 未过项 `suanfa >算法>断法` 系因 `断法` 移出常用词后已完全不再出现在 `suanfa` 候选噪声中 |
| `exam`（`--top 5`） | **263/291（90.4%）** | 已知缺陷回归 **13/15（86.7%）**；5 格与 9 格前 5 候选完全一致 |
| `check_first` | **409 音节，0 FAIL，3 WARN** | WARN 从 9 降至 3 |
| `probe_context` | **13 改善 / 1 不变 / 0 退化** | 上下文联想无退化 |
| `test_render_ipc.exe` | **ALL PASS（fails=0）** | 覆盖协议 JSON 序列化、AC+LPAC+LW SDDL 校验、与真实 `taishen_server.exe` 的管道 Ping/Show/Move/Hide/ReloadConfig |
| `test_tsf_load.exe` & `test_config_reader.exe` | **ALL PASS** | DLL 加载与配置解析通过 |
| PE 静态 CRT 检查（5 产物） | **全部 `dynamic_crt=[]`** | `taishen_ime.dll` / `taishen_ime_x86.dll` / `taishen_ime_imm32.ime` / `taishen_ime_imm32_x86.ime` / `taishen_server.exe` 均零动态 CRT 依赖 |
| `tools/check-structure.ps1` & `biome check .` | **PASS** | 模块边界与格式化检查通过 |
| 安装包构建（`.\package.ps1`） | **已产出** | `install/TaishenIME-Setup-0.1.77.exe`（93.8 MB）及 `install/out/` 五产物与词库 |

**仓库状态**：
- 主仓库（`taishenIME`）：`master` 位于 `4553377`（本轮 7 个 commit，未 push，工作树干净）。
- 词库仓库（`taishen-dict`）：`master` 位于 `fd9a7c9`（本轮 1 个 commit，未 push；保留原有的未暂存改动 `M taishen-dict.md`，未触碰）。

### 2. 进行中 / 待验证 / 已知遗留

- **待实机安装验收（最高优先级）**：`TaishenIME-Setup-0.1.77.exe` 尚未安装到本机验证，重点包括：
  1. AppContainer / LPAC 宿主（Win11 搜索框 `SearchApp.exe`、新版记事本、Microsoft Store 应用）：验证候选窗能通过 `taishen_server.exe` 正常弹出、不卡死、不崩溃；若搜索框存在高 z-band 遮挡，进一步验证 `tools/enable-uiaccess.ps1`。
  2. 常规宿主（QQ、微信、浏览器、Notepad++、游戏）与老版本 7 项待人机验证清单。
  3. 确认 `%LOCALAPPDATA%\TaishenIME` 日志中输出正确的 `engine_build_id`（`0.1.77+...`），以及执行 `tools/cleanup_old.ps1` 清理历史 `*.old*` 残留（约 234 MB）。
- **待排查单测项**：`platform/windows/out/test_ascii_mode.exe` 在本轮 C++ 编译后执行退出码 1，失败点为 `STEP3 FAIL: ctrl+space should switch to english (eat=1 ascii=0)`（本轮未修改 `tsf_keyevent.cpp` 与 `test_ascii_mode.cpp`，需在下轮确认是历史回归还是测试环境状态）。
- **未启动项**：
  - 架构 V4 **D3**（引擎迁入 `taishen_server.exe`，拆出 `crates/taishen-protocol`）与 **D4**（移除进程内渲染与引擎回退）。
  - 遗留单测 `test_group_two_words_both_error`（`zheggeweomende` 连续双多字错）仍保持 `#[ignore]`。
  - 词库低优遗留：高频可疑词 6,790 条、参照库命中降权词 1,934 条抽样复核；`exam` 剩余 25 条未过项（主要集中在 L1 单音节/儿化音如 `e`/`huar`/`wanr`、L3 长句 5 条、L4 英文与专词）。

---

## 三、涉及的文件

### 1. 主仓库 `taishenIME`（7 个 commit）

| # | Commit | 主题 | 新增文件 | 修改文件 |
|---|---|---|---|---|
| 1 | `93b8249` | 统一候选基线口径、补录 DoD 与 0.1.75 版本号 | —（删除误留临时文件 `engine/tests/result.jsonl`） | `engine/Cargo.toml`、`engine/Cargo.lock`、`taishenIME.md`、`docs/index.md` |
| 2 | `e6abbb1` | 四产物原子部署 + 部署锁 + `engine_build_id` + 容器 ACL 开关化 | `engine/build.rs` | `engine/src/ffi.rs`、`platform/windows/include/engine_bridge.h`、`platform/windows/src/tsf_module.cpp`、`install/install.nsi`、`install/install.ps1`、`install/install_latest.ps1`、`install/tools/apply-pending.ps1`、`tools/cleanup_old.ps1` |
| 3 | `590dac0` | 统一来源裁决（tier/reasons/score）、精确词频可信度、窗宽无关补全与生僻字保护 | — | `engine/src/dictionary/mod.rs`、`engine/src/lib.rs`、`engine/examples/probe_rank.rs` |
| 4 | `504e94a` | 修复 `zhyang` 管道层（遗留-02）、中英驼峰混输（`woshiEric`）与拆组 B 段常用词优先 | — | `engine/src/dictionary/mod.rs`、`engine/src/lib.rs` |
| 5 | `0f37b8c` | 同步 `2026.11.20.1` 词库、屏蔽明确英文词 Combo 噪声、收口 4 份设计稿并升 0.1.76 | `tools/check-structure.ps1` | `engine/src/lib.rs`、`engine/Cargo.toml`、`resources/VERSION.json`、`resources/common_dict.txt`（及构建产出的 `resources/system_dict.db.bin`）、`docs/ARCHITECT-V4.md`、`docs/modules/candidate-ranking/DESIGN.md`、`docs/modules/correction/SPEC.md`、`docs/modules/engine-process-isolation/DESIGN.md`、`taishenIME.md` |
| 6 | `5ce55a3` | 同步 `Cargo.lock` 至 0.1.76 | — | `engine/Cargo.lock` |
| 7 | `4553377` | 落地 V4 D1 异步编辑会话 + D2 `taishen_server.exe` 进程外候选窗渲染与 AppContainer/LPAC 权限（v0.1.77） | `platform/windows/include/render_ipc.h`、`platform/windows/include/ipc_render_client.h`、`platform/windows/src/ipc_render_client.cpp`、`platform/windows/src/taishen_server.cpp`、`platform/windows/test/test_render_ipc.cpp`、`tools/enable-uiaccess.ps1` | `docs/ARCHITECT-V4.md`、`docs/index.md`、`engine/Cargo.toml`、`engine/Cargo.lock`、`install/install.nsi`、`install/install.ps1`、`install/install_latest.ps1`、`install/tools/apply-pending.ps1`、`package.ps1`、`platform/windows/CMakeLists.txt`、`platform/windows/include/config_reader.h`、`platform/windows/src/config_reader.cpp`、`platform/windows/src/tsf_module.cpp` |

### 2. 词库仓库 `taishen-dict`（1 个 commit）

| # | Commit | 主题 | 修改文件 |
|---|---|---|---|
| 1 | `fd9a7c9` | 迁出 137 条领域词至 `extra_words`，补齐常用简拼/网络缩略语/称谓与单字 boost | `curate/common_dict.txt`、`curate/extra_words.txt`、`curate/boost.yaml`（经 `pipeline.py --reuse-wiki` → `verify_build.py` → `tools/sync_to_ime.py` 同步至主仓库） |

---

## 四、尝试过但失败了的事情

1. **将被降权的 `Correction` 单独放到 `group = 4`（排在所有词典前缀扩展之后）**
   - **现象**：`suanfa` 中 `断法` 被大量前缀词（`算法语言/算法导论/...`）挤出前 9，导致 `exam` 的 `suanfa >算法>断法 9` 报错 `未进候选: 断法`，L5 通过率从 55 掉到 54。
   - **根因**：`exam.rs` 的 `Assertion::Order`（`>A>B`）要求 A 与 B 都必须出现在 top-N 内，B 被彻底挤出候选窗也会判失败。
   - **最终处理**：降权 `Correction` 留在 `group = 3`（已严格位于精确命中 `group = 0` 之后）；后续从词库 `common_dict.txt` 移出低频领域词 `断法` 后，`suanfa` 纠错噪声自然消失（L5 升至 59/60，仅剩该条历史断言因噪声彻底消失而提示 `未进候选`）。

2. **对短串（`pinyin_str.len() <= 4`）放开单字纠错提权，并在 `!exact_has_credible` 时无条件放宽 `deletion_variants`**
   - **现象**：`test_mixed_abbrev_full_and_hotword` 在 `bru` 上失败——单字删字母纠错（`入/不/布/部`）挤占了 `不如（Combo）`。
   - **根因**：`bru` 属于非全拼简拼混输（`!is_full_pinyin`），对非全拼放宽删字母纠错并提权单字会制造大量高优先级单字噪声。
   - **最终处理**：将放宽 `deletion_variants` 严格限定在 `is_full_pinyin && !exact_has_credible`（覆盖 `zhongguoo`/`nihaoa`/`xianashi`），并将 `corr_promoted` 限定为 `hc >= 2`（`logn` 因无合法音节切分 `counts == None`，本身不进入字数分层逻辑，不受影响）。

3. **在 `apply_word_len_match` 中给 `corr_promoted` 赋予 `common_credit = true`**
   - **现象**：L1 从 86 回退到 85，`zhonggou` 下 `鸿沟/中欧/中古/钟鼓` 排到了 `中国` 前面。
   - **根因**：`deletion_variants` 生成顺序早于 `correction_variants`，若把高频纠错一律当成常用词同级（`common_rank = 0`），它们会在同级排序下凭生成顺序压过真正属于 `common_dict` 的 `中国`。
   - **最终处理**：`common_credit` 仅授予真正的 `common_dict` 词条；高频纠错词依靠 `group = 0` + `combo_like = 0` 依然能压过 `Combo`/`Sentence`，同时不再反超常用词。

4. **中英驼峰混输（`woshiEric`）首版仅用 `sentence::decode` + `query_with_split().take(2)` 拼接**
   - **现象**：`woshiEric` 未能产出 `我是Eric`。
   - **根因**：`woshi` 仅 2 个音节，低于 `sentence::decode` 的 ≥3 音节门槛；且词典中精确匹配优先返回 `卧室/我市`，没有常用词 `我是`。
   - **最终处理**：为 2–3 音节前缀增加「精确词典无常用词时按单音节首选字直拼」兜底（`wo→我` + `shi→是` → `我是Eric` 排 #1）。

5. **统一英文提权后未对 `Combo` 生成加门控**
   - **现象**：`hello` 虽将英文排到 #1，但 #2/#3 仍出现 `和了咯（Combo）`、`和了露（Combo）`，导致 `L4-E hello @no_src=Combo 3` 失败。
   - **根因**：原仅抑制了拼音纠错，未同步抑制 `query_combo`/`combo_guess`/`phrase_group_guess`。
   - **最终处理**：在明确英文词且无高可信精确汉字命中时（`suppress_pinyin_correction`），同步跳过 `query_combo`、`combo_guess` 与 `phrase_group_guess`。

6. **尝试一并取消 `test_group_two_words_both_error`（`zheggeweomende`）的 `#[ignore]`**
   - **现象**：虽然 `group_rec` B 段补齐 `common_full_index` 优先后，`test_group_weom_to_wom`（`zhegeweomende`，单处多字错）顺利通过，但 `zheggeweomende`（相邻两段同时存在多字错误）依然未通过。
   - **根因**：拆组状态空间在双段同时含拼写错误时剪枝阈值过严，超出本轮最小改动边界。
   - **最终处理**：保留 `#[ignore]`，列为后续专项优化项。

7. **`test_ascii_mode.exe` STEP3 未通过**
   - **现象**：编译后运行 `platform/windows/out/test_ascii_mode.exe` 返回退出码 1：`STEP3 FAIL: ctrl+space should switch to english (eat=1 ascii=0)`。
   - **现状**：本轮未改动 `tsf_keyevent.cpp` 或 `test_ascii_mode.cpp`，尚未定位具体触发原因（可能是此前按键状态机改动或用例依赖本机默认配置），需在下一会话优先排查，避免带入 D3。

---

## 五、下一步建议（按优先级排序）

### P0：安装 v0.1.77 并完成实机联合验收
1. 运行 `install/TaishenIME-Setup-0.1.77.exe` 完成安装（或按需重启/注销以刷新 `explorer.exe` 及已有宿主进程中的 DLL），再执行 `tools/cleanup_old.ps1` 清理历史 `*.old*` 残留。
2. 核对 `%LOCALAPPDATA%\TaishenIME` 运行日志：
   - 确认 `engine_build_id` 为 `0.1.77+...`；
   - 确认 `taishen_server.exe` 以 **Medium Integrity** 运行且监听 `\\.\pipe\taishen-ime-render`。
3. **重点验收矩阵**：
   - **AppContainer / LPAC 场景**：Win11 任务栏搜索框（`SearchApp.exe`）、新版记事本、Microsoft Store 应用——验证候选窗正常弹出、跟随光标、选词上屏无卡顿/无崩溃。
     - 若搜索框内能打字且日志显示 IPC 成功，但候选窗被搜索框高 z-band 遮挡，执行 `tools/enable-uiaccess.ps1`（需管理员权限，为 `taishen_server.exe` 签名并放入受信任路径以启用 `uiAccess="true"`）后复测。
   - **常规宿主场景**：QQ、微信、Chrome/Edge、VS Code、Notepad++、全屏/老游戏，验证异步编辑会话（D1）与进程外候选窗（D2）的响应延迟、光标跟随与选词稳定性。
   - **候选质量实测**：重点试 `suanfa`（算法 #1）、`houxuan`（候选 #1）、`zhyang`（这样 #1）、`woshiEric`（我是Eric #1）、`hello`（无 Combo 噪声）、`sj`（时间不被内置短语抢）、5 格/9 格切换前 5 候选一致性、以及历史 7 项实机验证点。

### P1：排查 `test_ascii_mode.exe` STEP3 并收敛实机反馈
1. 复现并定位 `platform/windows/out/test_ascii_mode.exe` 的 `STEP3 FAIL: ctrl+space should switch to english (eat=1 ascii=0)`，确认是配置默认值变化（如 `ctrl+space` 快捷键绑定）还是状态机回归，并修复。
2. 针对 P0 实机验收暴露的任何 IPC 时序、焦点切换、高 DPI / 多屏光标定位或候选排序反馈做一轮快速收敛，并在用户明确指令时统一 `git push`（主仓 + 词库仓）。

### P2：推进架构 V4 — D3（引擎迁入 `taishen_server.exe`）与 D4
1. 在 D1+D2 实机稳定后启动 **D3**：
   - 抽离共享协议 crate `crates/taishen-protocol`，定义按键、候选、选词、配置热重载等 IPC 消息；
   - 将 Rust 引擎从 `taishen_ime.dll` 迁入 `taishen_server.exe`，彻底消除 AppContainer 宿主内的词库只读 ACL 依赖与多进程重复加载 155 MB 词库内存占用；
   - 同步验证 `IMM32`（`taishen_ime_imm32.ime`）接入同一 server 进程的可行性。
2. D3 稳定一个发布周期后执行 **D4**：清理 DLL 内残留的进程内渲染/引擎回退代码与旧 ACL 脚本分支。

### P3：候选与词库长尾收尾 + 产品迭代
1. **引擎长尾**：修复 `zheggeweomende`（`test_group_two_words_both_error`）双错拆组剪枝；按需排查 `exam` 剩余 25 条用例（L1 单音节/儿化音、L3 剩余 5 条长句、`suanfa >算法>断法` 断言评估是否调整或保留观察）。
2. **词库治理复核**：抽样复核历史遗留的 6,790 条高频可疑词与 1,934 条参照库命中降权词（聚焦 2 字独立词误伤子集）。
3. **版本演进**：按 `DEV-TRACKER.md` 排期推进 V0.2.7 自动更新及后续功能。
