# 泰深输入法 — 文档索引

## 核心文档

| 文档 | 说明 | 状态 |
|------|------|------|
| [../taishenIME.md](../taishenIME.md) | 项目 L2 宪法：开发环境、工作流、工具链约定 | ✅ |
| [ARCHITECT.md](ARCHITECT.md) | 架构骨架：拓扑 + Root 裁剪 + 结构映射 + 决策记录 | ✅ |
| [business-flow.md](business-flow.md) | 核心数据流：按键→拼音→候选→上屏 | ✅ |
| [DEV-TRACKER.md](DEV-TRACKER.md) | 需求看板，新需求先记账 | ✅ |
| [CHANGELOG](CHANGELOG) | 版本变更记录 | ✅ |
| [index.html](index.html) | 可视化文档仪表盘 | ✅ |

## 变更日志

| 文件 | 日期 |
|------|------|
| [changelogs/CHANGELOG_2026-07-28.md](changelogs/CHANGELOG_2026-07-28.md) | 2026-07-28 项目初始化 |

## Bug 记录

| 文件 | 日期 |
|------|------|
| [bugs/BUG_2026-07-28.md](bugs/BUG_2026-07-28.md) | 当前无 Bug |

## 参考资料

| 文件 | 日期 |
|------|------|
| [reference/RESEARCH_2026-07-28.md](reference/RESEARCH_2026-07-28.md) | 2026-07-28 竞品调研报告 |
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
| #9 可观测性 | [modules/observability.md](modules/observability.md) | ⏳ 待实现时补充 |
| #10 可靠性 | [modules/reliability/SPEC.md](modules/reliability/SPEC.md) | ✅ SPEC 已编写 |
| #11 性能 | [modules/performance.md](modules/performance.md) | ⏳ 待实现时补充 |
| #12 生命周期 | [modules/lifecycle/SPEC.md](modules/lifecycle/SPEC.md) | ✅ SPEC 已编写 |
| #3 接口层·上屏 | [modules/composition/SPEC.md](modules/composition/SPEC.md) | ✅ SPEC 已编写 |

## 归档

| 文档 | 说明 |
|------|------|
| [archive/architecture-v1.md](archive/architecture-v1.md) | 第一版架构设计（方法论前），已归档 |
