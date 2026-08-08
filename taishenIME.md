# 泰深输入法 — 开发环境与工作流

> 📘 本文件由 AI 自动生成，是项目的 **L2 宪法**。
> 所有 AI 辅助开发遵循此文件的约定。
> 配置是活的——需要调整时告诉 AI 即可。

---

## 项目信息

| 字段 | 值 |
|------|-----|
| 项目名称 | 泰深输入法 (taishenIME) |
| 项目类型 | 桌面应用（PC 中文拼音输入法） |
| 技术栈 | Rust (核心引擎) + C++17 (Windows TSF 平台层) + Node.js (Biome 工具链) |
| 词库项目 | [taishen-dict](https://github.com/EricXu20266/taishen-dict) — 独立词库构建管线，与 App 解耦迭代 |
| 团队规模 | 个人项目 |
| 生成日期 | 2026-07-28 |

---

## 目录结构

```
taishenIME/
├── .editorconfig              # 编辑器行为统一
├── .gitignore
├── biome.json                 # Lint + Format 配置
├── taishenIME.md              # 本文件 — L2 宪法
├── engine/                    # Rust 核心引擎
│   ├── Cargo.toml             # cdylib + staticlib
│   └── src/
│       ├── lib.rs             # Engine 状态机
│       ├── ffi.rs             # C ABI 接口
│       ├── pinyin/mod.rs      # 音节表 + 切分算法
│       └── dictionary/mod.rs  # 词库查询
├── platform/
│   ├── windows/               # Windows TSF 实现
│   │   ├── CMakeLists.txt
│   │   ├── include/           # 头文件
│   │   └── src/               # C++ 源文件
│   └── macos/                 # 预留
├── resources/                 # 词库文件（由 taishen-dict 构建产出，发布时复制）
│   ├── system_dict.db         # SQLite 系统词库（gitignore）
│   ├── domains/               # 专业分类词库（*.txt，引擎运行时自动加载）
│   └── common_dict.txt        # 手工维护超高频常用词表
├── install/                   # 安装器脚本
├── tools/                     # 辅助工具（词库构建已迁移至 taishen-dict）
│   ├── ARCHITECT.md            # 架构骨架
│   ├── DEV-TRACKER.md          # 需求看板
│   ├── business-flow.md        # 核心数据流
│   ├── index.md                # 文档索引入口
│   ├── CHANGELOG               # 版本变更记录
│   ├── modules/                # 模块规格
│   ├── changelogs/             # 变更日志归档
│   ├── bugs/                   # Bug 修复记录
│   ├── handoff/                # 会话交接
│   ├── reference/              # 参考资料
│   └── archive/                # 历史归档
└── sessions/（运行时生成，gitignore）
```

---

## 环境分层

| 环境 | 用途 | 运行位置 | 变量管理 | 数据库 |
|------|------|----------|----------|--------|
| DEV  | 日常开发 | 本地机器 | 代码库默认值 | 本地测试词库 |
| PROD | 最终用户 | 用户机器 | 安装目录 config.yaml | SQLite 系统词库 + 用户词库文本 |

环境分层精简：个人项目，DEV 本机开发调试，PROD 通过安装器发布到用户机器。

---

## 版本控制

### 远程仓库
- 平台：GitHub
- 仓库地址：https://github.com/EricXu20266/taishenIME
- 认证方式：HTTPS Token（token 存本机 git credential，不入库）
- 代理：本机 git config 已配置（`git config --get http.proxy`）

### 分支模型
标准流 — 个人正式项目：
```
main ─────●────────●────────●  (稳定版本)
           \       / \       /
feature/* ──●─────●   ●─────●  (开发分支)
```

### Commit 规范
- 格式：`type: description`（如 `feat: 添加候选窗口 Skia 渲染`）
- 类型：feat / fix / refactor / chore / docs / test
- 粒度：每完成一个可独立验证的改动就 commit
- AI 代码标注：加 `[AI:泰深]` 前缀

### 合并策略
- feature 完成后 `git merge --no-ff` 合并到 main，保留分支痕迹

### 远程推送策略
- 指令推送——用户说「推送」AI 才 push。

---

## 工作流

### 完整开发流程

```
需求澄清 → 架构骨架 → 基础环境 → 逐模块开发
                                        ↓
                        ┌───────────────────────────────┐
                        │  SPEC → Plan → 实现 → 验证    │
                        │    ↑                    ↓      │
                        │    └── 不通过，回到实现 ←──┘   │
                        │          通过 ↓                │
                        │    测试 → commit → 继续下一个   │
                        └───────────────────────────────┘
                                        ↓
                              全部模块完成 → 打包发布
```

**逐模块流程**（每个模块走一遍）：

| 阶段 | 操作 | AI 行为 | 验证标准 |
|------|------|---------|----------|
| **SPEC** | 写入 `docs/modules/{模块名}/SPEC.md` | 需求 + 接口 + 数据模型 + 实施计划合一 | 与 ARCHITECT.md 的 Root 关系一致 |
| **Plan** | SPEC 文件内包含实施计划 | 拆分可独立验证的步骤，每步 < 50 行变更 | 步骤间依赖清晰 |
| **实现** | 按 Plan 写代码 | 每步 commit，加 `[AI:泰深]` 前缀 | cargo build 通过 |
| **验证** | typecheck + lint | `cargo test` + `biome check` | 零错误 |
| **测试** | 单元测试 | `cargo test` | 全部通过 |
| **打包** | Rust release build | `cargo build --release` | 构建成功 |

### AI 自检契约

**AI 每轮代码生成后必须主动执行以下验证，不需要用户提醒**：

```
代码生成 → cargo build → cargo test → biome check → commit
              │             │            │
              └─ 失败 ──────┴────────────┴→ AI 自行修复 → 重新验证（最多 2 轮）
                                                │
                                                └─ 2 轮后仍失败 → 报告用户，不 commit
```

| 检查 | 命令 | 不通过的后果 |
|------|------|-------------|
| **编译** | `cd engine && cargo build` | 拒绝 commit，AI 自行修复 |
| **测试** | `cd engine && cargo test` | 拒绝 commit，AI 分析失败原因后修复 |
| **lint** | `biome check --write .` | 先自动修复，修不了的拒绝 commit |

**硬规则**：
- 任何一项不通过 → **禁止 commit**
- 每次 commit 只包含本轮 AI 实际修改的文件，不混入工作区其他改动

### 审查
- 每轮 AI 代码生成后，查看 `git diff`
- 确认逻辑正确、无多余改动后 commit

### 测试

| 类型 | 框架 | 命令 |
|------|------|------|
| 单元测试 | Rust cargo test | `cd engine && cargo test` |
| FFI 集成测试 | 待定 | `cd engine && cargo test --test ffi` |

**AI 测试约定**：
- 写新功能 → 同时生成测试，测试和实现一起 commit
- 修 bug → 先写复现测试，确认失败 → 再修 → 测试通过
- build + test + biome 三连通过后才算完成

---

## 工具链

| 类别 | 工具 | 备注 |
|------|------|------|
| 编辑器 | VS Code | 推荐，已配置 .editorconfig |
| Rust 工具链 | rustc 1.97 + cargo | 核心引擎编译 |
| C++ 工具链 | CMake 3.26 + MSVC | Windows TSF 平台层 |
| 代码格式化 | Biome 2.5.6 | JSON/JS/TS/Markdown |
| Rust 格式化 | rustfmt（cargo fmt） | Rust 代码风格 |
| Lint | Biome + cargo clippy | 代码质量 |
| 测试框架 | cargo test | Rust 内置 |
| Git | 标准流 | feature → main |
| 词库构建 | [taishen-dict](https://github.com/EricXu20266/taishen-dict) | 独立管线（jieba MIT + Wikipedia CC BY-SA），与 App 解耦迭代 |

### MCP 工具链

| MCP | 用途 | 状态 |
|-----|------|------|
| AnySearch | 互联网搜索（匿名可用） | ✅ 已安装 |
| CodeGraph | 代码索引与导航 | ✅ 已安装 |
| Firecrawl | 深度网页抓取 | ✅ 已安装 |
| TrendRadar | 热点趋势监控 | ✅ 已安装 |

---

## 文档管理

### 文档索引

`docs/index.md` 是所有文档的入口。每次新增文档后 AI 主动更新索引。

### 文档与目录

| 文件/目录 | 用途 | 维护方式 |
|-----------|------|----------|
| `docs/ARCHITECT.md` | 架构骨架 | 从 app-project-architect 产出，后续按需修正 |
| `docs/DEV-TRACKER.md` | 需求看板 | AI 主动维护——检测到新需求时确认并记录 |
| `docs/business-flow.md` | 核心数据流 | 架构变更时更新 |
| `docs/index.md` | 文档索引入口 | AI 主动维护——每次新增文档后更新 |
| `docs/modules/` | 模块 SPEC | 逐模块开发时创建 |
| `docs/changelogs/` | 变更日志 | AI 主动维护——每轮开发任务完成后追加 |
| `docs/bugs/` | Bug 修复记录 | AI 主动维护——排错完成后记录 |
| `docs/handoff/` | 会话交接 | 用户手动触发 |
| `docs/reference/` | 参考资料 | 竞品调研 + 用户提供的资料 |
| `docs/archive/` | 历史归档 | 过时文档移入，**不删除** |

### 归档规则

- **什么时候归档**：模块重构后旧 SPEC 过时 → 移入 `archive/`
- **怎么归档**：`mv docs/旧文档 docs/archive/旧文档_日期`，不是 `rm`
- **索引同步**：归档后更新 `docs/index.md`

---

## 变更记录

| 日期 | 变更内容 | 原因 |
|------|----------|------|
| 2026-07-28 | 初始创建 | AI 自动生成（vibe-coding-setup） |
| 2026-08-08 | 词库独立 | 创建 taishen-dict 独立仓库，词库构建与 App 解耦 |
