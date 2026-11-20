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
├── resources/                 # 词库文件（全部由 taishen-dict 管线产出，sync_to_ime.py 同步；本仓库只读消费，改动须回溯词库源）
│   ├── system_dict.db         # SQLite 系统词库（37.3 万词，gitignore）
│   ├── system_dict.db.bin     # 预编译索引（秒加载，gitignore，部署期由引擎 ffi 生成）
│   ├── domains/               # 领域词库源 txt（wiki 重建 + curate 源合并）
│   │   └── domains.db         # 领域词库 SQLite（35 领域 16.9 万词，引擎优先加载，gitignore）
│   ├── common_dict.txt        # 常用词表源 txt（源在 taishen-dict/curate，同步而来）
│   ├── common.db              # 常用词表 SQLite（2703 条，gitignore）
│   ├── bigram.db              # 搭配表 SQLite（prev/next/count 1-6 档，gitignore）— 拆组内相邻词打分
│   └── VERSION.json           # 词库版本指针（如 V2026.08.10.1，同步写入）
├── install/                   # 安装器脚本（NSIS + PowerShell）
├── package.ps1                # 一键打包脚本（校验词库 → CMake → NSIS）
├── tools/                     # 辅助工具
│   └── archive/               # 历史遗留词库构建脚本（已移交 taishen-dict，勿运行）
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

### 推送前安全隐私闸门（硬约束）

本仓库是 **public**。任何 push / tag / release / 上传资产之前，必须先过这道闸门；任一项不通过即禁止推送。

**一、禁止上传的内容**

| 类别 | 具体 |
|------|------|
| 未脱敏语料 | 会话库导出、思维块/正文原文、论坛或新闻原文、wiki dump 文本、任何语料的中间切片 |
| 数据文件 | `*.db` / `*.db.bak*` / `*.pkl` / `*.tsv` / `*.log` 等构建产物与中间件（只允许入库 `curate/` 源与代码） |
| 个人信息 | 真实姓名与私人称呼（阿 X / X 总）、手机号、邮箱、证件号、住址、账号、**具体持仓与关注的标的** |
| 凭据 | 密钥、token、cookie、`.env`、私钥、带鉴权的 URL |
| 对话原文 | 任何文档、注释、changelog、commit message 中引用用户原话或 AI 原文 |

**二、描述语料来源时的写法**

- 只写抽象表述：「内部语料」「自有语料」。
- 不写数据库路径（`~/.taishen/taishen.db`）、表名（`messages.content` / `blocks`）、角色字段与数据规模。
- 词库源文件头部的注释同样适用——注释也是公开内容。

**三、推送前必跑的检查（三项全过才允许 push）**

```powershell
# 1. 有没有数据文件/语料中间件进了版本库
git ls-files | Select-String -Pattern '\.(db|db\.bak|pkl|tsv|sqlite)$|corpus|dump|ngram'

# 2. 有没有凭据与隐私模式
git grep -n -I -E 'sk-[A-Za-z0-9]{16,}|ghp_|github_pat_|AKIA[0-9A-Z]{16}|password|Bearer [A-Za-z0-9]|taishen\.db|messages\.content'

# 3. 本次待推内容逐文件过目
git diff --name-only origin/master..HEAD
```

第 3 步不能只看文件名——新文件与改动文件都必须读一遍实际内容，再决定是否推。

**四、本机路径**

文档与脚本中避免写死 `E:\AllinDeepSeek\...` 绝对路径；确需示例时用 `<项目根>` 占位。

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
| **候选质量** | `cargo run --release --example exam` | 引擎 / 词库改动必跑。各分级通过率不得低于基线（2026-11-20 实跑基线：总计 247/291，L1 86 / L2 53 / L3 8 / L4 60 / L5 52；已知缺陷组 12/15）。某一级下降即视为回归。基线以实跑为准，历史文档里出现的 244 / 248 均为中间态 |

**硬规则**：
- 任何一项不通过 → **禁止 commit**
- 每次 commit 只包含本轮 AI 实际修改的文件，不混入工作区其他改动
- 修候选质量时，**不允许改 `exam-cases.tsv` 的断言来让用例通过**——用例是标尺，不是待办
- **交付闭环定义（DoD）**：用户反馈只有在完成「`commit` → 四产物（x64/x86 × TSF/IMM32）原子部署并写入 `DEPLOY-VERSION.json` → 用户实机复测确认」三步后才算闭环；未部署的修复不得宣称已交付，反馈状态区分「已修 / 已部署 / 已确认 / 已复发」四态

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

### 候选质量与成本红线

**修候选质量的三条约束**：

1. **排序问题不动生成**：该出的词排太后 → 先看排序链（`apply_*` 系列），不要从生成阶段增删候选。
2. **生成问题才动生成**：不该出现的词出现 → 才改生成，并评估对排序的连带影响（候选池大小会改变插入位次）。
3. **门槛必须给数据**：任何过滤阈值，报告里必须附「正例 vs 反例」的实测对比。没有数据的阈值等于拍脑袋，且极易误伤正常输入。

**成本红线**：

批量任务（几十批以上的重复判断）**禁止使用 Fork 分身**。Fork 继承主会话完整上下文，每个分身都要全量重发一遍——实测 46 个分身累计消耗 1.37 亿 token，单项最高 776 万，成本是正常做法的 20-30 倍。

正确做法（按成本从低到高）：
- 主会话顺序跑（上下文增量小，缓存命中率高）
- 拆多个独立会话（每 20-30 批一个，跑完即关）
- AgentType 子代理（独立上下文，只带任务 prompt）

---

## 词库协作（硬约束）

词库与 App 是两个仓库：源与产物都在 [taishen-dict](https://github.com/EricXu20266/taishen-dict)，本仓库只消费。

**改词库的唯一起点**：先挂载 `E:\AllinDeepSeek\taishen-dict`，在那边改 `curate/` 源，重建 `output/`，`tools/verify_build.py` 通过后再 `tools/sync_to_ime.py` 单向同步到本仓库 `resources/`。

**curate/ 全部入口**（改哪一类就动哪个文件）：

| 文件 | 格式 | 作用 |
|---|---|---|
| `common_dict.txt` | `拼音<TAB>词`，行序即优先级 | 候选排序的第一层 |
| `boost.yaml` / `demote.yaml` | `词: 倍数/除数` | 频次调校 |
| `fragments.txt` | 每行一个词 | 碎片词频次压到 1 |
| `longwords.txt` | `词<TAB>拼音<TAB>频次` | 补长词 |
| `extra_words.txt` | `词<TAB>拼音<TAB>频次` | 补单字/补词条（不存在才插入） |
| `extra_readings.txt` | `字<TAB>拼音<TAB>频次` | 单字补读音，**输出阶段追加独立行**（一个词可有多个拼音） |
| `pinyin_fix.txt` | `词<TAB>正确拼音` | 覆盖注错的拼音 |
| `domains/` | `词 拼音` | 领域词源 |

**两个必须知道的机制**：

1. `pipeline.py --reuse-wiki` 复用 `output/domains/*.txt` 跳过 wiki 网络采集，重建从几小时降到 30 秒。只取 `domains.yaml` 定义的 wiki 领域，THUOCL 等其他域不并入 system_dict（否则专名污染常用候选）。
2. **同步后必须重建 bin 索引**，否则引擎仍加载旧索引、修正不生效：
   ```
   engine\target\release\build_index.exe resources\system_dict.db resources\system_dict.db.bin
   ```
3. 词库条目拼音是**音节分隔格式**（`zhong'guo`），单音节词不带分隔符。手写 curate 源的拼音串时按此格式，引擎查询侧会做归一化但不保证索引一致。

**禁止**：直接编辑 `taishenIME/resources/common_dict.txt` 补词或改拼音。2026-11-16 已踩过：IME 侧补了 63 条口语整词、改了 `quernaer→qunaer`，没回灌词库源，两边分叉；下次从 taishen-dict 同步会把 IME 的新增覆盖回退。

**校验链路**（任一步失败都不得发布）：

1. 词库侧 `tools/verify_build.py`：双表存在、简繁不混、common rank 连续
2. `tools/sync_to_ime.py`：同步前对产物哈希 == VERSION 清单，同步后对 IME 侧哈希
3. IME 侧 `tools/verify_resources.py` + `tools/verify_common_dict.py`：`package.ps1` 已内置为打包前置，失败即中止

**判据口径**：db 的 sha256 会随 SQLite 文件布局变化（同一内容重建两次也可能不同），内容一致性用全表 `(rank, pinyin, word)` 逐行比对，不要只看哈希。

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
| 词库构建 | [taishen-dict](https://github.com/EricXu20266/taishen-dict) | 独立管线（jieba MIT + Wikipedia CC BY-SA + THUOCL MIT），产出 system_dict.db + domains.db + common.db + VERSION.json；本仓库不构建词库，只经 sync_to_ime.py 消费 |
| 打包 | `.\package.ps1` | 一键：校验词库版本 → CMake Release → NSIS setup.exe |

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
| 2026-08-09 | 词库全量 DB 化 | domains + common 从 txt 迁移到 SQLite，启动性能优化 |
| 2026-08-09 | 打包一键化 | package.ps1 统一 DB 构建 + CMake + NSIS 流程 |
| 2026-11-19 | 候选质量与成本红线 | 五轮候选修复经验（排序/生成分离、门槛要数据）+ Fork 分身成本事故教训 |
| 2026-11-19 | 词库 curate 入口扩充 | 新增 extra_words / extra_readings / pinyin_fix 三个入口，补读音/修拼音/补词可走数据文件 |
