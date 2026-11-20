# 泰深输入法 — 文档索引

## 核心文档

| 文档 | 说明 | 状态 |
|------|------|------|
| [../taishenIME.md](../taishenIME.md) | 项目 L2 宪法：开发环境、工作流、工具链约定 | ✅ |
| [ARCHITECT-V4.md](ARCHITECT-V4.md) | **架构 V4：进程边界重构**——薄 TSF DLL + 单一 server 进程；含结构判据、四步迁移、代码组织与模块边界规则 | 📐 目标结构（待实施） |
| [ARCHITECT.md](ARCHITECT.md) | 架构骨架 v3.1：双仓库分工 + 引擎 24 模块 + TSF/IMM32 双链路 + 整句/组词/渲染层（v2 见 archive/architecture-v2.md） | ✅ 现行结构 |
| [business-flow.md](business-flow.md) | 核心数据流 v3：按键→候选装配（27 阶段）→排序→渲染→上屏；含整句与组词支线 | ✅ |
| [DEV-TRACKER.md](DEV-TRACKER.md) | 需求看板，新需求先记账 | ✅ |
| [CHANGELOG](CHANGELOG) | 版本变更记录 | ✅ |
| [index.html](index.html) | 可视化文档仪表盘 | ✅ |

## 质量工具

| 工具 | 说明 | 用法 |
|------|------|------|
| 输入法考试 | 分级考试：正向/反向/来源/多字词/总数/窗宽断言，L1-L5 通过率矩阵 | `cargo run --release --manifest-path engine/Cargo.toml --example exam`（用例：[engine/tests/exam-cases.tsv](../engine/tests/exam-cases.tsv)；规范 [SPEC](modules/exam/SPEC.md)；报告 [REPORT](modules/exam/REPORT.md)；修复记录 [FIX-A2-SCAN](modules/exam/FIX-A2-SCAN.md)、[FIX-COMBO-SHORT](modules/exam/FIX-COMBO-SHORT.md)） |
| 文章模式考试 | 真实文章逐词模拟输入：排名分布 + 上下文生效 + 简拼/混拼/手误命中率 | `cargo run --release --manifest-path engine/Cargo.toml --example article_exam -- --article engine/tests/article-real-2026-11-19.json`（说明 [ARTICLE-MODE](modules/exam/ARTICLE-MODE.md)；自测样例 [article-sample.json](../engine/tests/article-sample.json)） |
| 打字基准 | 全拼 + 双拼首选/top5 命中率与单键延迟 P50/P95 | `cargo run --release --manifest-path engine/Cargo.toml --example typing_test` |
| 容器探针 | AppContainer 内词库可达性与加载状态码 | `tmp/probe-appcontainer.cpp` |

## 变更日志

| 文件 | 日期 |
|------|------|
| [changelogs/CHANGELOG_2026-11-16.md](changelogs/CHANGELOG_2026-11-16.md) | 2026-11-16 长句整句解码（词图 Viterbi + 束搜索）+ 同日审计修复 |
| [changelogs/CHANGELOG_2026-11-15.md](changelogs/CHANGELOG_2026-11-15.md) | 2026-11-15 纠错漏打字母 + 组词候选重写 + 组合串收敛 + bigram 加载优化 |
| [changelogs/CHANGELOG_2026-10-31.md](changelogs/CHANGELOG_2026-10-31.md) | 2026-10-31 引擎审计优化 A/B/C/D 四批（去重/分区/来源表/分发表/参数外置/词库路径收敛） |
| [changelogs/CHANGELOG_2026-08-13.md](changelogs/CHANGELOG_2026-08-13.md) | 2026-08-13 V0.5.13 音节可视化 + 部署期 .bin 预生成 + V0.5.14 用户词持久化 |
| [changelogs/CHANGELOG_2026-08-10.md](changelogs/CHANGELOG_2026-08-10.md) | 2026-08-10 V0.5.7 候选全量体检优化 |
| [changelogs/CHANGELOG_2026-08-09.md](changelogs/CHANGELOG_2026-08-09.md) | 2026-08-09 候选排序重构 + 词库补词 |
| [changelogs/CHANGELOG_2026-08-08.md](changelogs/CHANGELOG_2026-08-08.md) | 2026-08-08 输入法构建部署 + 全屏场景调研 |
| [changelogs/CHANGELOG_2026-08-05.md](changelogs/CHANGELOG_2026-08-05.md) | 2026-08-05 V0.3.x 候选逻辑重构 + 平台层 10 项修复 |
| [changelogs/CHANGELOG_2026-07-28.md](changelogs/CHANGELOG_2026-07-28.md) | 2026-07-28 项目初始化 |

## Bug 记录

| 文件 | 日期 |
|------|------|
| [bugs/DICT-GAP-2026-11-19.md](bugs/DICT-GAP-2026-11-19.md) | 2026-11-19 词库缺口补齐 13 词 + 日期简码不再独占候选池（article_exam miss 7→3，exam 248/291） |
| [bugs/DICT-DROP-2026-11-19.md](bugs/DICT-DROP-2026-11-19.md) | 2026-11-19 第二批审计落地：删字 1128 条 + 删候选 5599 词（软降权，保护 11 条真用字 + 4 条合法干支） |
| [bugs/BUG_2026-11-17.md](bugs/BUG_2026-11-17.md) | 2026-11-17 QQ 内切换输入法崩溃（宿主自带旧版 MSVCP140 → 改静态 CRT，四产物零动态 CRT 依赖） |
| [bugs/BUG_2026-11-16.md](bugs/BUG_2026-11-16.md) | 2026-11-16 houxuan 首候选被邻键纠错抢占（已修）+ 组词/vhyang 混合输入两项遗留 |
| [bugs/BUG_2026-11-15.md](bugs/BUG_2026-11-15.md) | 2026-11-15 组词链路多根因（组合串散装 / 有状态 FFI 读两遍 / 组词候选只有两头） |
| [bugs/BUG_2026-11-09.md](bugs/BUG_2026-11-09.md) | 2026-11-09 候选与组合问题 |
| [bugs/BUG_2026-10-31.md](bugs/BUG_2026-10-31.md) | 2026-10-31 IMM32 系统级注册导致 winlogon 崩溃、微软账户无限登录循环（P0） |
| [bugs/BUG_2026-10-30.md](bugs/BUG_2026-10-30.md) | 2026-10-30 当日实测问题 |
| [bugs/BUG_2026-10-29.md](bugs/BUG_2026-10-29.md) | 2026-10-29 当日实测问题 |
| [bugs/BUG_2026-08-09.md](bugs/BUG_2026-08-09.md) | 2026-08-09 测试挂起（.bin 缺失）+ zhonggou 纠错丢失（双根因）+ radical 并行 flaky |
| [bugs/BUG_2026-08-08.md](bugs/BUG_2026-08-08.md) | 2026-08-08 中文模式无组合打不出标点（提交链路） |
| [bugs/BUG_2026-08-02.md](bugs/BUG_2026-08-02.md) | 2026-08-02 TSF IME 四大根因（候选窗/退格/英文直出/光标跟随） |
| [bugs/BUG_2026-07-28.md](bugs/BUG_2026-07-28.md) | 2026-07-28 项目初始化期 |

## 参考资料

| 文件 | 日期 |
|------|------|
| [reference/RESEARCH_2026-07-28.md](reference/RESEARCH_2026-07-28.md) | 2026-07-28 竞品调研报告 |
| [reference/RESEARCH_2026-08-04-app-options.md](reference/RESEARCH_2026-08-04-app-options.md) | 2026-08-04 主流输入法跨程序使用设计调研 |
| [reference/RESEARCH_2026-08-08-ime-benchmark.md](reference/RESEARCH_2026-08-08-ime-benchmark.md) | 2026-08-08 五大输入法功能与选词逻辑对标 |
| [reference/symbol-quickref.md](reference/symbol-quickref.md) | 2026-08-08 特殊符号速查表（v+数字 / v+分类码 双模式 + 短码） |
| [reference/build-deploy.md](reference/build-deploy.md) | 2026-08-08 taishen_ime.dll 构建与部署手册（VS2022 BuildTools + CMake + regsvr32） |
| [reference/candidate-ranking-logic.md](reference/candidate-ranking-logic.md) | 候选排序逻辑全景（2026-11-16 修订：现行装配顺序 + 三分区 + 整句插入，B-22/23/24 根因保留） |
| [reference/](reference/) | 资料目录 |

## 会话交接

| 目录 | 说明 |
|------|------|
| [handoff/](handoff/) | 会话交接文件（用户手动触发） |

## 模块设计手册

| Root | 手册 | 状态 |
|------|------|------|
| #1 数据模型与持久化 | [modules/data-persistence/SPEC.md](modules/data-persistence/SPEC.md) | ✅ SPEC 已编写 |
| #2 业务领域层 | `modules/pinyin-engine` | ⏳ 待编写 |
| #3 接口层 | [modules/interface-layer/SPEC.md](modules/interface-layer/SPEC.md) | ✅ SPEC 已编写 |
| #4 状态管理 | `modules/state-machine` | ⏳ 待编写 |
| #2 业务领域层·中英切换 | [modules/ascii-mode/SPEC.md](modules/ascii-mode/SPEC.md) | ✅ SPEC 已编写 |
| #6 安全边界 | `modules/security` | ⏳ 待编写 |
| #7 配置系统 | [modules/config-system/SPEC.md](modules/config-system/SPEC.md) | ✅ SPEC 已编写 |
| #7 配置系统·设置 UI | [modules/settings-ui/SPEC.md](modules/settings-ui/SPEC.md) | ✅ SPEC 已编写 |
| #8 呈现层·自绘 UI 与候选窗 | [modules/presentation/SPEC.md](modules/presentation/SPEC.md) | ✅ 2026-11-16 修订 |
| #8 呈现层·窗体系统 | [modules/ui-framework/SPEC.md](modules/ui-framework/SPEC.md) | ✅ SPEC 已编写 |
| #8 呈现层·视觉现代化 | [modules/ui-framework/SPEC-modern-minimal.md](modules/ui-framework/SPEC-modern-minimal.md) | ✅ SPEC 已编写 |
| #9 可观测性 | `modules/observability` | ⏳ 待编写 |
| #10 可靠性 | [modules/reliability/SPEC.md](modules/reliability/SPEC.md) | ✅ SPEC 已编写 |
| #11 性能 | `modules/performance` | ⏳ 待编写 |
| #12 生命周期 | [modules/lifecycle/SPEC.md](modules/lifecycle/SPEC.md) | ✅ SPEC 已编写 |
| #3 #8 · 组合区与上屏 | [modules/composition/SPEC.md](modules/composition/SPEC.md) | ✅ 2026-11-16 修订（组词/分段属性/UIElement） |
| #2 #3 #7 #8 · 语音输入 | [modules/voice-input/SPEC.md](modules/voice-input/SPEC.md) | ✅ SPEC 已编写 |
| #3 #8 · IMM32 兼容层（老游戏/老应用） | [modules/imm32-layer/SPEC.md](modules/imm32-layer/SPEC.md) | ✅ SPEC 已编写 |
| #3 #12 · 引擎进程隔离与 AppContainer | [modules/engine-process-isolation/DESIGN.md](modules/engine-process-isolation/DESIGN.md) | 📐 历史复盘（阶段划分已由 [ARCHITECT-V4.md](ARCHITECT-V4.md) 取代；**D1 异步编辑会话 + D2 `taishen_server.exe` 候选窗外移 + AC/LPAC ACL 已于 0.1.77 落地**） |
| #3 #8 · IMM32 候选数据接口 + 全屏降级 | [modules/imm32-candidate-data/SPEC.md](modules/imm32-candidate-data/SPEC.md) | 📋 设计完成，待在场实测 |
| #2 · **候选体系总设计（候选排序唯一总纲）** | [modules/candidate-ranking/DESIGN.md](modules/candidate-ranking/DESIGN.md) | ✅ 2026-11-17（分层/字数分层/五维排序键/学习/搭配） |
| #2 #4 · 智能联想（前文参与候选排序） | [modules/context-assoc/SPEC.md](modules/context-assoc/SPEC.md) | ✅ 2026-11-16（bigram 接入主排序 + 加载优化） |
| #2 · 前缀补全（Completion）实现记录 | [modules/candidate-ranking/COMPLETION-IMPL.md](modules/candidate-ranking/COMPLETION-IMPL.md) | ✅ 2026-11-17（只差一个音节 4.8% → 37.8%） |
| #2 · 整句解码（长输入） | [modules/sentence-decode/SPEC.md](modules/sentence-decode/SPEC.md) | ✅ 新增 2026-11-16（Viterbi + 束搜索） |
| #1 #3 · 输入历史 + AI 事后总结 | [modules/input-history/SPEC.md](modules/input-history/SPEC.md) | ✅ SPEC 已编写（V0.7，泰深侧清单见第六节） |
| #2 · 笔形辅码（智能ABC 音形） | [modules/shape-aux-code/VERIFY.md](modules/shape-aux-code/VERIFY.md) | ⏸ 数据层验证完成（98.8%），暂不实现（2026-10-31 决策） |
| #2 · 排序参数外置 | [modules/ENGINE-TUNING.md](modules/ENGINE-TUNING.md) | ✅ 已实现（tuning.json，2026-10-31） |
| #2 · 引擎审计与优化方案 | [modules/ENGINE-AUDIT-20261031.md](modules/ENGINE-AUDIT-20261031.md) | ✅ 审计完成，A/B 批已落地 |
| #2 · 引擎层重构独立复审 | [modules/AUDIT-REVIEW-20261031.md](modules/AUDIT-REVIEW-20261031.md) | ✅ 已闭环（2 P1 + 8 P2 全部修复） |

## 归档

| 文档 | 说明 |
|------|------|
| [archive/architecture-v1.md](archive/architecture-v1.md) | 第一版架构设计（方法论前），已归档 |
