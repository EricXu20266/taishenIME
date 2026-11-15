# SPEC — 智能联想（前文参与候选排序）

> 模块：engine（Rust 核心）+ platform/windows（C++ 配置传递）
> 日期：2026-08-08（初版）／ 2026-10-28（加权改造，默认开启）／
>       2026-11-16（bigram 档位表接入主排序 + 排序键修正 + 加载优化）
> 状态：已实现

## 一、需求

候选排序参考**已上屏前文**：上屏"北京"后，再输入 `da`，"大学"因与前文"北京"构成搭配（北京大学）而前置。

初版是二值前置——命中搭配表就无条件提到最前，命中一次与命中十次效果相同，且搭配表为空时完全失效。当前实现改为**加权前移**：权重来自人工精选表、外部搭配表与用户实际共现次数，逐次累积、可解释、可关闭。

## 二、接口设计

### 引擎层

```rust
// Engine 字段
last_committed: String,                                  // 最近上屏词（独立于输入态）
context_enabled: bool,                                   // 开关，默认 true
context_map: HashMap<String, Vec<(String, u32)>>,         // 前词 → [(后词, 权重)]

// 方法
pub fn set_context_enabled(&mut self, enabled: bool)
pub fn context_enabled(&self) -> bool
pub fn load_contexts(&mut self, entries: Vec<(String, Vec<String>)>)          // 外部表
pub fn load_learned_contexts(&mut self, entries: Vec<(String, String, i64)>)  // 学习表（带次数）
```

**上下文状态更新**：`select_candidate` 选中普通中文词时更新 `last_committed`（英文/短语/符号/计算器/日期/拆字等特殊候选不更新）。

**加权重排**（挂 `apply_sort_mode` 之前）：

1. `last_committed` 非空且 `context_enabled`
2. 合并四类来源，同一个后词取**较大**权重（不相加，避免同一份共现被计两遍）：
   - `context_map[last_committed]`（外部表 / 学习表）
   - `context_prior`（离线先验，1-6 档）
   - 内置精选表（`context.rs::lookup`，tuning 可配权重）
   - `bigram.db` 档位表（1-6 档，2026-11-16 接入）
3. 只有四张表都空且 bigram 不可用才早退——bigram 是独立数据源，
   不经过 `merged`；只装 bigram 的机器上不能误判为「无搭配数据」
4. 排序键：**权重降序，同权重保持原位置**（稳定排序）
5. 英文候选区（`Source::English` 之后）不参与；不新增候选，只在原列表内重排

**权重取值**

| 来源 | 权重 |
|------|------|
| 内置精选表 | tuning `context_builtin_weight`（默认 32，命中即置顶） |
| 外部搭配表 | 12 |
| 用户共现 | 共现次数，上限 `context_weight_cap`（默认 20） |
| 离线先验 / bigram | 1-6 档，直接作为前移权重 |

**排序键历史修正（2026-11-16，审计 P1）**：原实现用 `score = weight - 原位置`
把位置先验与搭配权重放进同一个标量做减法。候选超过 20 条时位置项压过权重项——
第 30 位命中权重 20 的搭配词会排到首位命中权重 5 的词之后，长列表下搭配几乎失效。
现改为字典序 `(权重降序, 原位置升序)`，权重绝对优先。

**性能**：`bigram_scores(prev, &nexts)` 一次取锁批量查。
逐条调 `bigram_score` 会在长候选下反复取 RwLock 并线性扫同一个前词的后继列表。

**约束**：

- `reset()` 不清 `last_committed`（上下文跨输入会话）
- 特殊候选（短语/日期/符号）不受影响
- 关闭开关后候选顺序与原版本完全一致（`apply_context_boost` 直接返回）

### FFI 层

```rust
#[no_mangle] pub extern "C" fn engine_set_context_assoc(enabled: i32) -> i32
#[no_mangle] pub extern "C" fn engine_get_context_assoc() -> i32
```

### 配置层

config.ini 键（默认开）：

```ini
# 智能联想（按前文搭配与用词习惯调整候选，1=开 0=关）
context_assoc=1
```

设置页文案：「智能联想（学习你的用词习惯）」。

## 三、数据模型

**内置精选表**（`context.rs`，前词 → [后词...]）覆盖 13 个高频前词：

```
北京 → 大学、市、时间、人、欢迎你
中国 → 人民、特色、梦、制造、电信
上海 → 市、人、交通、大学
今天 → 天气、晚上、早上、上班、开会
我们 → 的、要、可以、一起、公司
```

**学习表**（`user_dict.db` 的 `context_pairs` 表）：

```sql
CREATE TABLE context_pairs (
  prev TEXT NOT NULL,
  next TEXT NOT NULL,
  count INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (prev, next)
);
```

每次中文上屏记录 `(prev, next)` 共现 +1；启动加载 `count >= 2` 的条目，每前词最多 8 条，按次数降序。

**离线先验表**（`context_prior.db`，与 DLL 同目录，可选）：

```sql
CREATE TABLE context_prior (
  prev TEXT NOT NULL,
  next TEXT NOT NULL,
  count INTEGER NOT NULL,   -- 归一化档位 1-6，直接当前移权重
  PRIMARY KEY (prev, next)
);
```

由 `taishen-dict/tools/build_context_prior.py` 从纯文本语料统计相邻词共现产出：
按系统词库做正向最大匹配分词 → 同句相邻词计数 → 按次数分位映射到 1-6 档 → 取 top N。

引擎侧 `engine_set_context_prior_path` 在激活时加载（文件缺失则跳过）。权重上限 6，低于学习表上限 20，
保证真实用词能盖过语料统计；同一个后词同时存在两侧记录时取较大权重。

**bigram 档位表**（2026-11-16 接入主排序）：

```sql
CREATE TABLE bigram (
  prev  TEXT NOT NULL,
  next  TEXT NOT NULL,
  count INTEGER NOT NULL,   -- 1-6 档
  PRIMARY KEY (prev, next)
);
```

- 数据由 `taishen-dict` 产出（约 40 万对），部署到 DLL 同目录 `bigram.db`
- 语义同样是 1-6 档，与学习表/先验表取 max
- **加载（性能关键）**：
  1. `engine_set_bigram_path` 只登记路径——TSF 每次 `ActivateEx` 都会调用，
     原实现无条件重读 20MB / 40 万对，日志记录 175 次加载累计约 284 秒
  2. 首次 `engine_process_key` 由 `ensure_bigram_loaded` 在后台线程加载，
     切换输入法瞬间零开销
  3. 同进程幂等：路径 + mtime 未变直接返回当前对数
  4. `.bin` 缓存（bincode）：头部 `magic | version | db_len | db_mtime`，
     签名不符自动重建；损坏时回退 SQLite
- 实测：SQLite 冷读 2760ms → `.bin` 103ms（约 27 倍）

## 四、验收

| 判据 | 方式 |
|------|------|
| 新装用户输入「北京」后 `da`，「大学」在首屏 | 内置权重 32 |
| `context_pairs` 某对 count 从 1 涨到 5，该候选位次单调前移 | `test_context_weight_monotonic` |
| 关闭开关后候选顺序不变 | `test_context_disabled_keeps_order` |
| 学习表命中时内置表不失效 | `apply_context_boost` 合并取大权重 |
| 只装 bigram.db（无学习/先验/内置）时搭配仍生效 | 早退条件含 `bigram_available()` 判定 |
| 长候选（>20 条）中高权重搭配词仍能前移 | 排序键为 `(权重降序, 原位置)` |
| 重复 `ActivateEx` 不重复加载 bigram | `set_bigram` 路径+mtime 幂等日志 |

## 五、实施记录

| 步骤 | 内容 | 状态 |
|------|------|------|
| 1 | context.rs：内置搭配表 + lookup | ✅ |
| 2 | lib.rs：last_committed / context_map + 候选前置 | ✅ |
| 3 | 学习表落库与重载（context_pairs） | ✅ |
| 4 | 加权改造：带权重数据结构 + 打分排序 + 默认开启 | ✅ 2026-10-28 |
| 5 | 文案统一为「智能联想」 | ✅ 2026-10-28 |
| 6 | bigram 档位表接入主排序（四源取 max） | ✅ 2026-11-15（`336ecb1`） |
| 7 | 排序键修正：权重优先、同权重保位置 | ✅ 2026-11-16（审计 P1） |
| 8 | bigram 加载幂等 + 首键延迟 + `.bin` 缓存 | ✅ 2026-11-16（`be36004` / `db1c44a` / `0bb8e1e`） |

## 六、风险

- 权重上限 20 防止单条搭配无限累积压过一切
- 只在候选列表内重排，不新增候选 → 不会冒出用户没打过的词
- 内置表与学习表合并取大 → 修复了「一旦有学习记录，内置条目全部失效」的旧问题
