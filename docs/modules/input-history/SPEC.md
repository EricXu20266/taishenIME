# SPEC: 输入历史记录 + AI 事后总结（Root #1 #3，V0.7）

> 关联 DEV-TRACKER: V0.7 输入历史记录 + AI 事后总结
> 前置：0.2.2 用户词库（学习+持久化）✅、0.6 IMM32 兼容层 ✅
> 需求提出：Eric 2026-08-17（替代原 V0.3「输入中 AI」六项）

---

## 一、需求

用户在使用输入法时，**每次输入的内容（全部上屏文本）都被记录下来**。
用户可**主动或定时**发起 AI 复盘（总结），复盘包含**三项任务**：

1. **输入提炼（核心思想凝练）**：把用户在时间段内断断续续输入的字、词、短句，
   **构建成句、成段**，凝练出输入内容的**核心思想**——用户这段时间在思考什么、
   输入了什么信息，帮助用户回顾自己的信息足迹。
2. **个人词库更新**：将用户输入的词/短句与当前词库（system/common/domain/user 四层）
   对比，找出**未录入的词/短句** → 作为用户组词录入，或作为热词提高候选位。
   注：选词自动学习（0.2.2）已在输入法内实现，本功能是对它的**补充**——
   覆盖「输入了但没选词学习」的内容（如英文直通、翻页跳过、组词未学完整词等）。
3. **个人词库整理（纠错 + 噪音词排除）**：识别 user_dict 中的**误学词**
   （错字、误选、临时输入、口头语），建议删除或降权——**输入法的进化：词库越用越干净**。

**⚠️ 数据源铁律（Eric 2026-08-17）**：复盘的输入分析**必须基于 input_history.db**
（真实输入足迹：完整文本流 + 时间戳 + 应用 + 拼音），**不得用 user_dict.db 代替**——
user_dict 只是选词学习副产品（无时间线、无上下文、无句子），无法支撑「构建成句成段」。
user_dict.db 仅用于任务③（词库整理/噪音排查）。

**核心决策（Eric 2026-08-17）**：
- **不做输入中 AI**。原 V0.3 六项（AI 帮写/候选重排/输入即搜索/翻译/剪贴板 AI/人设）全部作废。
- **AI 总结绑定泰深**：输入法侧不内置 LLM 链路。总结流程 = 启动泰深 → 向泰深发总结任务 →
  泰深读输入法数据 → AI 生成总结 → 结果写回输入法（推荐词）→ 用户在泰深查看总结。
- **记录范围 = 全部上屏文本**（含中文候选、英文直通、标点、符号、短语、日期等），按类型打标，总结时可过滤。

**用户价值**：输入法的数据资产被盘活——「输入即足迹」，AI 帮你回顾输入历史、
持续补充 + 整理个人词库，越用越懂你且**不打断输入流程**（事后复盘，零输入干扰）。

**合约**：
- 记录只发生在**上屏成功**时（CommitText / ImeToAsciiEx 输出点），不记录候选浏览、翻页
- 输入历史独立于 user_dict.db 存储（input_history.db），避免用户词库膨胀
- 泰深侧只读历史与对比结果，写入推荐词走 pending_import 通道（不直接改词库）
- 隐私：记录默认开启但可关；记录文件路径明确，泰深读取需用户授权（泰深侧控制）

**不做**：
- 输入中 AI 能力（本方案已取代）
- 云同步输入历史——本地文件，后续可扩展
- 输入历史加密——一期明文 SQLite（个人本机），后续可加
- **剪贴板监控（Eric 确认 2026-08-17）**：剪贴板内容不是「输入」是「复制」，会污染输入意图分析
  且隐私风险高（密码/密钥/链接常驻）。剪贴板历史是独立功能（V0.2.12 遗留待做），不进 input history。
- 泰深侧 AI 总结逻辑——在 taishen-pisdk 项目实现（见第六节需求清单）

---

## 二、数据模型

### 输入历史表（独立 SQLite：`%APPDATA%/taishen-ime/input_history.db`）

```sql
CREATE TABLE IF NOT EXISTS input_history (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    ts         INTEGER NOT NULL,        -- unix 秒（本地时区偏移单独存）
    tz_offset  INTEGER NOT NULL DEFAULT 480, -- 分钟，东八区=480，历史记录可回算本地时间
    app        TEXT NOT NULL,           -- 进程名（chrome.exe / wechat.exe ...），空=未知
    type       TEXT NOT NULL,           -- 类型：candidate / english / symbol / phrase / datetime / emoji / calc / v_symbol / compose / other
    text       TEXT NOT NULL,           -- 上屏文本（UTF-8）
    pinyin     TEXT,                    -- 拼音串（必录：候选/组词/纠错意图还原的关键，英文/符号可空）
    source     TEXT                     -- 上屏通道：tsf / imm32
);

CREATE INDEX IF NOT EXISTS idx_history_ts   ON input_history(ts);
CREATE INDEX IF NOT EXISTS idx_history_text ON input_history(text);
CREATE INDEX IF NOT EXISTS idx_history_type ON input_history(type);

-- 滚动清理：按 retention_days 删除旧记录（引擎/平台层启动时执行）
-- DELETE FROM input_history WHERE ts < ?;
```

**type 分类规则**（平台层上屏点打标）：

| type | 判定 |
|------|------|
| candidate | 拼音选词上屏（TSF Commit / IMM32 WM_IME_CHAR 链） |
| english | 英文直通/英文候选/Shift+字母大写 |
| symbol | 标点全角化（。，！？等）、配对引号 |
| v_symbol | v 模式符号上屏 |
| phrase | 快捷短语上屏 |
| datetime | rq/sj/xq/nl 日期时间上屏 |
| emoji | emoji 候选上屏 |
| calc | c+ 算式计算结果上屏 |
| compose | 组词逐字上屏（候选中间音节） |
| other | 兜底 |

### pending_import 表（推荐词写入通道，复用 user_dict.db 或独立文件）

```sql
CREATE TABLE IF NOT EXISTS pending_import (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    pinyin    TEXT NOT NULL,            -- 推荐拼音（泰深侧生成，可由词库工具补充）
    word      TEXT NOT NULL,            -- 推荐词/短句
    source    TEXT NOT NULL DEFAULT 'ai', -- 来源标记：ai_summary / user_confirm
    action    TEXT NOT NULL DEFAULT 'learn', -- learn=录入用户组词(自动) / pin=热词置顶(人工确认) / both
    status    TEXT NOT NULL DEFAULT 'pending', -- pending / imported / rejected
    note      TEXT,                     -- 泰深附注（如「本周输入 12 次」）
    created   INTEGER NOT NULL,
    imported  INTEGER                   -- 实际导入时间
);

-- 决策（Eric 确认 2026-08-17）：learn 类自动导入（词频+1 排后位，误收影响极小）；
-- pin 类必须人工确认后才置顶（置顶改候选第一位，污染代价大，用户把关）。

CREATE UNIQUE INDEX IF NOT EXISTS idx_pending_word ON pending_import(word);
```

### 对比输出 JSON（input_analyze.exe 产物）

```json
{
  "window": {"start": 1755400000, "end": 1755486400},
  "stats": {"records": 8420, "candidate": 5310, "english": 2100, "symbol": 800, "other": 210},
  "unmatched": [
    {"text": "具身智能", "freq": 8, "chars": 4, "in_dict": false,
     "recommend": "learn", "hint": "system/common/domain/user 均未收录"}
  ],
  "matched_hot": [
    {"text": "大模型", "freq": 15, "in_dict": true, "layer": "common",
     "recommend": "pin", "hint": "已收录但频次低于词频阈值，可提权"}
  ],
  "filtered": {"english_words": 210, "single_chars": 980, "punct": 800}
}
```

---

## 三、接口设计

### 3.1 输入历史写入（平台层，无 FFI）

上屏点在平台层，记录直接由平台层完成，**不引入新 FFI**（避免引擎层持锁膨胀）：

- `history_log(app, type, text, pinyin)` — C++ 侧工具函数，内部打开 input_history.db 写入 + 批量缓冲（每 50 条或 2s flush 一次，降低磁盘 IO）
- 调用点：
  - TSF：`CTextService::CommitText`（候选/英文/符号/短语/日期等汇聚处）
  - IMM32：`ImeToAsciiEx` WM_IME_CHAR 输出处
- 数据库路径来自 `engine_get_data_dir()`（与 user_dict.db 同目录约定，config 可覆盖）

### 3.2 词库对比分析（引擎侧 CLI 工具）

**形态**：独立可执行 `input_analyze.exe`（链接引擎 staticlib，复用词库加载/查询），泰深任务 spawn 调用。

```
input_analyze.exe --history <input_history.db> --dict <system_dict.db>
                 [--domain <domains.db>] [--common <common.db>] [--user <user_dict.db>]
                 [--start <unix>] [--end <unix>] [--min-freq 3] [--min-chars 2] [--max-chars 6]
                 [--out <result.json>]
```

**对比逻辑**（引擎 dictionary 模块新增 `analyze.rs`）：

1. 读时间窗内记录，按 text 聚合频率（忽略大小写、去重）
2. 过滤：单字（< min_chars）、纯英文词（≥2 字母归 english 统计）、纯标点/数字
3. 剩余候选逐条查词库四层（full_index 精确匹配 / common / domain / user）：
   - 未命中 → unmatched（推荐 learn）
   - 命中但词频 < 阈值（如 user frequency < 3 且 common 层无）→ matched_hot（推荐 pin）
4. 短句处理：2-6 字直接查；更长串 n-gram 滑窗切 2-4 字词查，聚合子词频率
5. 输出 JSON（见二节格式），exit code 0=成功

### 3.3 推荐词导入（泰深 → 输入法）

**形态 A（一期，推荐）**：CLI `ime_import.exe --file <recommendations.json> [--confirm]`
- `recommendations.json` 格式：`[{word, pinyin?, action: learn|pin|both}]`
- **learn 类**：自动导入 user_dict（词频 +1，排后位）——误收不扰排序，风险低
- **pin 类**：写入 pending_import 待人工确认；`--confirm` 时对 pin 类也直接导入 pin_words，
  不带则 pin 类只进 pending_import，用户设置页「待导入」清单勾选确认
- 导入日志：导入词条数、跳过（已存在）、失败（拼音缺失）明细

**形态 B（远期可选）**：引擎 FFI 直导 `engine_import_recommendation(word, pinyin, action)`，
供泰深通过输入法进程内 IPC 调用——一期不做（泰深无法直接进输入法进程，CLI 更简单可靠）。

### 3.4 配置项（config.ini 新增 [history] 段）

```ini
[history]
enabled = true                  # 输入历史记录总开关
retention_days = 30             # 滚动保留天数（默认 30，可调 1~90，设置页约束上限 90）
max_records = 100000            # 单文件上限（二次保险），超出删最旧
min_freq_analyze = 3            # 词库对比最小频次（供工具默认值）
```

**空间管理约束（Eric 确认 2026-08-17）**：history 数据不能无限增长。
双保险：① retention_days 滚动删除（设置页滑块/数值框，默认 30 天、范围 1-90 天）② max_records
条数上限（超限删最旧）。每次启动 + 每次写入时惰性检查执行清理。

### 3.5 设置页入口（#8）

设置窗新增「历史」页（kNavNames 扩容至 7）：记录开关、保留天数、
当前记录数/占用空间展示、「导出近期记录」（复制到剪贴板/打开文件位置）、
「待导入词条」清单（pending_import status=pending，勾选导入/拒绝）。

### 3.6 复盘入口与双模式（Eric 2026-08-17 补充）

**工具栏「AI 复盘」按钮**（第 5 个按钮）+ 设置页开关（功能总开关，不需要的用户可关；
开关关 → 工具栏按钮隐藏）。

**触发模式二选一（设置页配置）**：

| 模式 | 行为 | 依赖 |
|------|------|------|
| ① 泰深一键复盘 | 点击 → 启动泰深（无头命令）→ 发起新会话 → 自动复盘 | 泰深无头 CLI（`taishen --new-session --prompt <复盘任务>`），泰深侧实现 |
| ② 提示词复制 | 点击 → 生成一段自包含提示词（含 DB 路径 + 时间区间 + 分析指令）→ **自动复制到剪贴板** → 用户粘贴到任意 agent | 无（输入法侧独立完成），默认模式 |

**模式②细节**（用户否决 JSON 文件方案——DB 固定，agent 自主读库）：
- 提示词 = 模板 + 分析区间 + DB 路径，**不导出数据文件**
- agent 需具备：将断断续续的输入字/词/短句**连贯成句成段**、揣摩输入意图、排除噪音、
  提炼内容的能力；可感知情绪（某日活跃度异常 → 小彩蛋）
- 提示词模板默认内置（见下），设置页可自定义覆盖
- 隐私提示：首次使用弹确认「输入记录将发送给外部 AI」
- 时间窗：点击弹出小菜单（上次以来 / 今天 / 本周 / 自定义）
- 游标：模式②生成提示词即推进 last_summary_ts（决策：简单优先）

**按钮状态**：开关关→隐藏；模式①+未检测到泰深→置灰提示（复用 0.5.3 检测）；
模式②→始终可用。

**定时复盘归属**：定时（泰深 scheduler）仅模式①；模式②纯手动。

### 3.7 模式②提示词模板（默认内置，可覆盖）

```text
【输入复盘任务】

你是用户的输入复盘助手。请自主读取下面的数据库，分析用户在指定时间段的输入，
完成三项任务：

① 输入提炼（核心）：把用户断断续续输入的字、词、短句，按时间线构建成句、成段，
   凝练出用户输入内容的【核心思想】——用户这段时间在思考什么、输入了什么信息。
   - 用时间戳 + 应用名切分段落（间隔 > N 分钟或切换应用 = 新段）
   - 保守重建：优先用原文词，只补连接词；重建句段标注置信度（高/中/低）
   - 识别并排除噪音（错字、误选、临时输入、删改残留）
   - 可感知情绪：某日活跃度异常或用词突变 → 小彩蛋

② 词库更新：识别输入中词库未收录的词/短句，给出建议
   （learn=录入用户组词 / pin=热词置顶），附理由。
   - 提示：「打错字」本身是词频不足的强信号（用户打错说明正确词没排到首选）
   - 英文只统计次数，不参与收录判定

③ 词库整理：检查用户词库中的误学词（错字、误选、临时输入、口头语），
   建议删除或降权，让词库越用越干净。

数据源（SQLite）：
- 输入足迹：%APPDATA%/taishen-ime/input_history.db
  表 input_history：ts(时间戳) / app(应用) / type(类型) / text(上屏文本) / pinyin(拼音) / source(通道)
- 用户词库（仅任务③）：%APPDATA%/taishen-ime/user_dict.db
  表 user_dict：pinyin / word / frequency / last_used
- 分析区间：{start} 至 {end}（unix 秒，按 ts 过滤）

输出格式：
1. 输入概览（时间段活跃度、统计、按应用分布）
2. 核心思想（重建后的句段 + 凝练结论，标注置信度）
3. 词库更新建议（词 / 动作 learn|pin / 理由）
4. 词库整理建议（误学词 / 动作 delete|demote / 理由）
5. 噪音清单（已排除的内容）
6. 彩蛋（情绪或趣事）
```

---

## 四、实施计划（输入法侧）

| 步骤 | 内容 | 验证 | 预估 |
|------|------|------|------|
| 1 | history 存储层：SQLite 表 + 批量写入 + 滚动清理（C++ sqlite3 封装） | 单测：写/读/清理/类型打标 | 6h |
| 2 | TSF/IMM32 上屏点接线 history_log | 实测：打几个字查 db 有记录、type 正确 | 2h |
| 3 | analyze.rs：四层对比 + n-gram + JSON 输出 | 单测：已知输入 → 期望 unmatched/matched_hot | 6h |
| 4 | input_analyze.exe CLI 包装 | CLI 冒烟：--help/空库/样本库 | 4h |
| 5 | ime_import.exe + pending_import 通道 | 导入 user_dict/pin_words 验证 + 冲突跳过 | 5h |
| 6 | 设置页「历史」页 + config [history] | UI 冒烟 + 配置读写回 | 4h |
| 7 | 端到端：记录→analyze→模拟泰深写回→导入生效 | 全链路真机 | 4h |

**涉及文件**（预估）：
- `platform/windows/src/history_log.cpp/.h`（新）
- `platform/windows/src/tsf_module.cpp`、`imm32/imm32_ime.cpp`（上屏点接线）
- `engine/src/analyze.rs`（新）、`engine/src/lib.rs`（导出分析入口）、`engine/src/dictionary/mod.rs`
- `tools/input_analyze/main.rs` + `tools/ime_import/main.rs`（新 CLI，Cargo workspace 或独立工程）
- `platform/windows/src/settings_window.cpp`（历史页）、config 加载
- `install/install.ps1`（可选：打包 tools）

---

## 五、定时/主动触发约定（泰深侧消费）

- **主动**：用户在泰深对话中说「总结我的输入」→ 泰深读取 `%APPDATA%/taishen-ime/input_history.db`
  （时间窗=上次总结点或用户指定）→ 调 input_analyze.exe → AI 总结 → 展示 + 可选写回推荐词。
- **定时**：泰深 scheduler 建定时任务（如每日 22:00），prompt 约定读库 + 调工具 + 总结 +
  推荐词写回，结果通知用户（泰深 IM/GUI）。输入法侧不做定时——定时能力泰深已有。
- **总结游标**：泰深在用户目录记 last_summary_ts（如 `%APPDATA%/taishen-ime/ai_summary_state.json`，
  泰深读写），避免重复总结。

---

## 六、泰深侧需求清单（taishen-pisdk 项目实现）

> 输入法侧交付后，泰深侧实现以下任务能力：

1. **数据读取**：读 input_history.db（SQLite 表结构见二节）+ input_analyze.exe 产物
2. **无头 CLI 复盘入口（模式①）**：泰深支持无头命令启动新会话并注入复盘任务——
   `taishen --new-session --prompt "<复盘指令>"`（复盘指令含 DB 路径 + 时间区间 + 分析要求，
   即模式②模板的泰深版）。输入法工具栏「AI 复盘」在模式①下 spawn 该命令。
   *泰深侧在 taishen-pisdk 重新评审本需求时确定具体 CLI 参数形态*
3. **总结任务「输入提炼」**：输入时间段 → AI 归纳输入内容（话题/热点/常用表达/写作素材），
   输出 Markdown 报告，可在泰深会话/画布呈现
4. **总结任务「词库更新」**：调 input_analyze.exe → AI 筛选推荐词（去噪：人名误拆/临时输入）→
   生成推荐清单 → 写 recommendations.json → 调 ime_import.exe（或先落 pending_import 待用户确认）
5. **总结游标管理**：读写 ai_summary_state.json，支持「上次总结以来」与「任意时间段」双模式
6. **定时任务**：scheduler 建定时任务（如每日 22:00），仅模式①场景；结果通知用户
7. **授权**：首次读取历史库前向用户确认（隐私透明）

---

## 七、开放问题（已决策）

| # | 问题 | 决策（Eric 2026-08-17） |
|---|------|------|
| 1 | 英文直通是否进词库对比素材？ | ✅ 已决策：**一期过滤**。英文只统计次数（english 类），不参与 unmatched 判定。理由：英文字母直通无需"录入"，词表以中文为主，n-gram 切分会碎。二期若英文候选需求强再补收录 |
| 2 | 推荐词自动导入 or 手动确认？ | ✅ 已决策：**learn 自动 / pin 人工**。learn 词频+1 排后位（误收不扰排序）；pin 置顶改第一位必须设置页人工确认。教训来源：B-22/23/24 词库污染致候选错乱 |
| 3 | 输入历史是否含密码/敏感输入？ | 纯本地文件，泰深读取需授权；可后续加关键词屏蔽名单 |
| 4 | retention_days 默认 30 天够不够？ | 与泰深定时任务频率联动，用户可调 |

