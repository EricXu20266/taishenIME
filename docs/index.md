# 泰深输入法 — 文档索引

## 核心文档

| 文档 | 说明 | 状态 |
|------|------|------|
| [../taishenIME.md](../taishenIME.md) | 项目 L2 宪法：开发环境、工作流、工具链约定 | ✅ |
| [ARCHITECT.md](ARCHITECT.md) | 架构骨架 v3：双仓库分工 + 引擎 21 模块 + 平台层 + 词库分层（v2 见 archive/architecture-v2.md） | ✅ |
| [business-flow.md](business-flow.md) | 核心数据流：按键→拼音→候选→上屏 | ✅ |
| [DEV-TRACKER.md](DEV-TRACKER.md) | 需求看板，新需求先记账 | ✅ |
| [CHANGELOG](CHANGELOG) | 版本变更记录 | ✅ |
| [index.html](index.html) | 可视化文档仪表盘 | ✅ |

## 变更日志

| 文件 | 日期 |
|------|------|
| [changelogs/CHANGELOG_2026-08-05.md](changelogs/CHANGELOG_2026-08-05.md) | 2026-08-05 V0.3.x 候选逻辑重构 + 平台层 10 项修复 |
| [changelogs/CHANGELOG_2026-07-28.md](changelogs/CHANGELOG_2026-07-28.md) | 2026-07-28 项目初始化 |

## Bug 记录

| 文件 | 日期 |
|------|------|
| [bugs/BUG_2026-07-28.md](bugs/BUG_2026-07-28.md) | 2026-07-28 项目初始化期 |
| [bugs/BUG_2026-08-02.md](bugs/BUG_2026-08-02.md) | 2026-08-02 TSF IME 四大根因（候选窗/退格/英文直出/光标跟随） |
| [bugs/BUG_2026-08-08.md](bugs/BUG_2026-08-08.md) | 2026-08-08 中文模式无组合打不出标点（提交链路） |
| [bugs/BUG_2026-08-09.md](bugs/BUG_2026-08-09.md) | 2026-08-09 测试挂起（.bin 缺失）+ zhonggou 纠错丢失（双根因）+ radical 并行 flaky |

## 参考资料

| 文件 | 日期 |
|------|------|
| [reference/RESEARCH_2026-07-28.md](reference/RESEARCH_2026-07-28.md) | 2026-07-28 竞品调研报告 |
| [reference/RESEARCH_2026-08-04-app-options.md](reference/RESEARCH_2026-08-04-app-options.md) | 2026-08-04 主流输入法跨程序使用设计调研 |
| [reference/RESEARCH_2026-08-08-ime-benchmark.md](reference/RESEARCH_2026-08-08-ime-benchmark.md) | 2026-08-08 五大输入法功能与选词逻辑对标 |
| [reference/symbol-quickref.md](reference/symbol-quickref.md) | 2026-08-08 特殊符号速查表（v+数字 / v+分类码 双模式 + 短码） |
| [reference/build-deploy.md](reference/build-deploy.md) | 2026-08-08 taishen_ime.dll 构建与部署手册（VS2022 BuildTools + CMake + regsvr32） |
| [reference/candidate-ranking-logic.md](reference/candidate-ranking-logic.md) | 2026-08-09 候选排序逻辑全景（B-22/23/24 根因 + 目标规则 + 修复方案） |
| [reference/](reference/) | 资料目录 |

## 会话交接

| 目录 | 说明 |
|------|------|
| [handoff/](handoff/) | 会话交接文件（用户手动触发） |

## 模块设计手册

| Root | 手册 | 状态 |
|------|------|------|
| #1 数据模型与持久化 | [modules/data-persistence/SPEC.md](modules/data-persistence/SPEC.md) | ✅ SPEC 已编写 |
| #2 业务领域层 | [modules/pinyin-engine.md](modules/pinyin-engine.md) | ⏳ 待实现时补充 |
| #3 接口层 | [modules/interface-layer/SPEC.md](modules/interface-layer/SPEC.md) | ✅ SPEC 已编写 |
| #4 状态管理 | [modules/state-machine.md](modules/state-machine.md) | ⏳ 待实现时补充 |
| #2 业务领域层·中英切换 | [modules/ascii-mode/SPEC.md](modules/ascii-mode/SPEC.md) | ✅ SPEC 已编写 |
| #6 安全边界 | [modules/security.md](modules/security.md) | ⏳ 待实现时补充 |
| #7 配置系统 | [modules/config-system/SPEC.md](modules/config-system/SPEC.md) | ✅ SPEC 已编写 |
| #7 配置系统·设置 UI | [modules/settings-ui/SPEC.md](modules/settings-ui/SPEC.md) | ✅ SPEC 已编写 |
| #8 呈现层 | [modules/presentation/SPEC.md](modules/presentation/SPEC.md) | ✅ SPEC 已编写 |
| #8 呈现层·窗体系统 | [modules/ui-framework/SPEC.md](modules/ui-framework/SPEC.md) | ✅ SPEC 已编写 |
| #8 呈现层·视觉现代化 | [modules/ui-framework/SPEC-modern-minimal.md](modules/ui-framework/SPEC-modern-minimal.md) | ✅ SPEC 已编写 |
| #9 可观测性 | [modules/observability.md](modules/observability.md) | ⏳ 待实现时补充 |
| #10 可靠性 | [modules/reliability/SPEC.md](modules/reliability/SPEC.md) | ✅ SPEC 已编写 |
| #11 性能 | [modules/performance.md](modules/performance.md) | ⏳ 待实现时补充 |
| #12 生命周期 | [modules/lifecycle/SPEC.md](modules/lifecycle/SPEC.md) | ✅ SPEC 已编写 |
| #3 接口层·上屏 | [modules/composition/SPEC.md](modules/composition/SPEC.md) | ✅ SPEC 已编写 |
| #2 #3 #7 #8 · 语音输入 | [modules/voice-input/SPEC.md](modules/voice-input/SPEC.md) | ✅ SPEC 已编写 |
| #3 #8 · IMM32 兼容层（老游戏/老应用） | [modules/imm32-layer/SPEC.md](modules/imm32-layer/SPEC.md) | ✅ SPEC 已编写 |

## 归档

| 文档 | 说明 |
|------|------|
| [archive/architecture-v1.md](archive/architecture-v1.md) | 第一版架构设计（方法论前），已归档 |
