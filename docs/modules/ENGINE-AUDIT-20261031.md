# 泰深输入法引擎审计与优化方案

> 审计对象：`engine/src`（Rust 引擎）当前工作区代码，HEAD `8ddb451`
> 方法：源码通读 + 调用链梳理 + 现有测试与注释交叉验证
> 性质：纯代码审计，所有结论均可由文中标注的文件行号复核
> 日期：2026-10-31

---

## 0. 审计范围与边界

**覆盖**：候选生成管线（`query_all`）、排序后处理（5 个 `apply_*`）、
词库查询分层（`dictionary::query`）、学习写入（`learn_user_word` + `write_back`）、
词库初始化（`DICT` / `DICT_PATH` / `DICT_GENERATION` 等全局）。

**未覆盖**：平台层 C++（`platform/windows/src`）、词库构建管线（外部仓库）、UI 渲染。

**判断标准**：只把"能在代码里指出位置、能说清影响"的项列为问题；
没有证据的猜测一律标为「待实测」，放进附录。

---

## 1. 现状架构

```
按键 → Engine::process_key
        ├─ 模式分流：符号(v) / 计算器(c) / 拆字(u) / 数字(r) / Unicode(U) / 双拼 / 组词
        └─ query_all()
             ├─ dictionary::query()        分层查询 P1→P4
             ├─ 补充：简拼 / 混合简拼 / 缩写全拼 / 模糊音 / 拼写纠错 / 删字 / 相邻键 / 错音 / 组合联想
             ├─ 特殊候选插入：日期 / 短语 / 英文 / Emoji
             └─ 后处理五段：
                  apply_context_boost → apply_word_len_match → apply_pin_boost
                  → apply_demote → apply_sort_mode
        → 截断入 all_candidates → repage()
```

分层查询已在 `dictionary::query`（`mod.rs:2055`）实现，顺序为：

| 层 | 数据源 | 层内排序 |
|----|--------|---------|
| P1 | 用户词（热 / 温 / 过期三段） | 频度 + 最近使用时间 |
| P2 | 常用词 | 人工维护的 rank 行序 |
| P3 | 系统词 | 词频降序（建索引时预排序） |
| P4 | 领域词 | 热度 > 词长 > 原序 |

这一段设计清晰，是本轮审计中质量最高的部分。问题集中在**引擎层**：
`query_all` 的补充逻辑与五段后处理。

---

## 2. 关键数据结构现状

```rust
// lib.rs:42,44
all_candidates: Vec<String>,
candidates: Vec<String>,

// lib.rs 内的来源标记（位置索引）
english_candidate_pos: Option<usize>,
phrase_candidate_pos: Option<usize>,
datetime_candidate_pos: Option<usize>,
```

`Vec<String>` 只承载文本，**不承载来源、分数、词长、字典类型**。
所有"这段候选是什么"的信息，通过三个 `Option<usize>` 位置标记表达。

这一选择带来连锁代价，见 §3.1。

---

## 3. 问题清单

### P0-1 候选缺元数据，来源靠位置索引维系

**证据**

| 位置 | 事实 |
|------|------|
| `lib.rs:42,44` | 候选容器是 `Vec<String>` |
| `lib.rs:2063,2066,2073` | 英文候选用 `english_candidate_pos` 记位置 |
| `lib.rs:1772` | 短语用 `phrase_candidate_pos` 记位置 |
| `lib.rs:2129-2136` | 插 Emoji 后必须手动 `*p += emo.len()` 修正英文位置 |
| `lib.rs:2209-2214` | 词长分区靠 `phrase_candidate_pos == Some(0)` 判断"短语是否在首位" |
| `lib.rs:1162,1196,1262,2200` | 四个 `apply_*` 都要先算 `eng_start` 才能避免打乱英文区 |

**影响**

任何一次 `insert` 都会让位置标记失真，而修正责任散落在每个插入点。
`lib.rs:2129-2136` 是典型：插 Emoji 的同时必须同步改另一个字段。
漏改一处就是"英文候选区被打乱 / 短语被分区挤走"这类回归。

**根因**：类型系统里没有"候选来源"这个概念，只能借位置表达。

---

### P0-2 五段后处理全部整表克隆

**证据**（均为 `Vec<String>` 克隆后再写回）

| 函数 | 行号 | 克隆点 |
|------|------|--------|
| `apply_context_boost` | 1182 | `scored.iter().map(...).collect::<Vec<String>>()` |
| `apply_pin_boost` | 1214 | `pinned.append(&mut rest)` 后逐条写回 |
| `apply_demote` | 1243-1248 | `rest` / `demoted` 两个新 Vec |
| `apply_sort_mode` | 1271-1284 | `first` / `second` 两个新 Vec |
| `apply_word_len_match` | 2224-2240 | `exact` / `shorter` / `longer` 三个新 Vec |

**影响**：候选 N 条 → 每次后处理产生 N 次 `String` 分配，
五段合计约 5N 次（长串场景 N 可达 100+）。这些分配全部发生在按键路径上。

**根因**：分区实现用"复制到新容器"，而不是"就地下标置换"。

---

### P0-3 去重策略不统一，热路径存在 O(n²)

**证据**

- `dictionary::query` 内部已改用 HashSet：`mod.rs:2060` 建 `seen`，`2064-2082` 两个 `push_entries*` 闭包用 `seen.insert()`。
- 但 `query_all` 里仍有 11 处线性 `contains`：
  `1778, 1786, 1795, 1806, 1811, 1827, 1839, 1849, 1878, 1915, 1928, 1943, 1949`
  （覆盖简拼、混合简拼、缩写全拼、模糊音、拼写纠错、删字容错、错音、组合联想各分支）

**影响**：补充分支每命中一批词都要对已有候选全扫一遍。
长串输入（如 `zhegeweomende`）会同时触发 5-6 个分支，叠加成 O(n²)。

**根因**：`query_all` 是逐步累加的流水线，没有维护统一的 seen 集合。

---

### P1-1 触发条件与"候选数量"耦合

**证据**

| 行号 | 条件 |
|------|------|
| 1823 / 1835 / 1924 | `candidates.len() < self.page_size` |
| 1783 / 1792 / 1860 / 1940 / 1947 | `candidates.len() < self.page_size * self.max_pages` |

**影响**：分支是否触发取决于**词库里碰巧有多少词**，而不是输入本身的质量。
体现为：内置兜底词库（条目少）与完整词库（37 万条）下行为不同；
词库扩容会静默改变纠错/联想策略。

代码注释已记录多次因此返工：
`1820-1822`（`zhonggou` 场景 deletion 变体命中 364 词灌满候选导致「中国」丢失）、
`1847-1849`（条件放宽）、`1961-1963`（触发条件再放宽）。

**根因**：用"结果数量"代替"输入质量"作为触发信号。

---

### P1-2 排序无统一分数，规则靠执行顺序表达优先级

**证据**

| 规则 | 行号 | 排序键 |
|------|------|--------|
| context | 1174 | `lift - i`（权重减原始位置） |
| word_len | 2224-2238 | 三分法：字数==n / <n / >n |
| pin | 1201-1217 | 二分：命中置顶词 / 其余 |
| demote | 1240-1248 | 固定插到第 11 位（`10usize.min(rest.len())`） |
| sort_mode | 1271-1282 | 二分：单字 / 多字 |

**影响**：五条规则各自整表重排，谁在后面谁说了算。
`lib.rs:2092-2097` 的注释记录了真实事故：`8fd8172` 重构把置顶移到分区之前，
导致置顶被词长分区覆盖，V0.5.7 才修回。

**根因**：没有可比分数，规则之间只能用"执行顺序"表达优先级；
而顺序一旦调整就会互相覆盖。

---

### P1-3 上下文权重与位置混算，长候选列表下权重失效

**证据**：`lib.rs:1167-1181`

```rust
let lift = ... weight.min(CONTEXT_WEIGHT_CAP) as i64;  // 上限 20
(lift - i as i64, lift, i)                              // i = 候选原始下标
```

**影响**：候选长度 > 20 时，`i` 的量级压过 `lift`，
第 30 位命中权重 20 的搭配词（得分 -10）会排在第 1 位命中权重 5 的词（得分 4）之后。
即"搭配命中"在长列表下几乎无效。

**根因**：把"位置先验"与"搭配权重"放在同一个标量里做减法。

---

### P2-1 特殊模式分支重复

**证据**：`lib.rs:1620-1714` 六个分支（v / c / u / r / U / 双拼 / 组词）
结构完全相同：构造候选 → 三个 pos 字段置 `None` → 赋值 `all_candidates` → `page=0` → `repage()` → `return`。

**影响**：新增一个输入模式要复制 15 行；三个 pos 字段必须逐个清空，漏一个就污染该模式。

---

### P2-2 初始化状态散落在 5 个全局

**证据**：`mod.rs:3223-3241`

```
DICT / DICT_PATH / USER_DICT_PATH / DOMAIN_DICT_PATH / PENDING_DOMAIN / DICT_GENERATION / DICT_READY
```

配合 `init()` 的幂等判断（`mod.rs:3258-3268`）与异步代际校验。

**影响**：初始化顺序敏感。`PENDING_DOMAIN` 的存在本身是为解决一个竞态
（`mod.rs:3234-3238` 注释）；`learn_user_word` 另有一处 APPDATA 回退（`mod.rs:1840-1857`）。
这些补丁各自正确，但合起来让"词库何时可用"这一状态分布在 7 个地方。

---

### P2-3 汉字判定用 `c as u32 > 0x7F`

**证据**：`lib.rs:1268`、`lib.rs:2219`

```rust
let hanzi_count = |w: &str| w.chars().filter(|c| *c as u32 > 0x7F).count();
```

**影响**：全角标点、拉丁扩展字母（é、ü）、emoji 都会被计入"汉字数"，
影响词长分区与单字/长词判定。当前候选集里这类字符少，属潜在问题而非现症。

---

## 4. 优化方案

原则：**先消除结构性隐患，再动行为**。每一步都可独立验收、独立回滚。

两条附加判断贯穿全文：

- **不为对齐而改造**。只有泰深自己承担了对应成本时才做该项。
  因此：排序参数外置（§4.9）替代"独立排序数据层"，候选来源 side-table（§4.1）替代"全量结构体化"。
- **条件触发的项不排期**。统一打分（§4.5.2）写清启动条件，三条不满足就不启动，
  方案留档但不进计划表。

### 4.1 候选来源 side-table（对应 P0-1）

**目标**：让"来源"可查，但不做全量类型迁移。

```rust
// 与 candidates 严格等长，只由 push_cand 维护
candidates:    Vec<String>,
candidate_src: Vec<Source>,

fn push_cand(&mut self, text: String, src: Source) {
    self.candidates.push(text);
    self.candidate_src.push(src);
    debug_assert_eq!(self.candidates.len(), self.candidate_src.len());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    UserHot, UserWarm, UserStale,   // P1
    Common,                          // P2
    System,                          // P3
    Domain,                          // P4
    Phrase, Datetime,                // 特殊候选
    English, Emoji,
    Correction, Combo, Fuzzy,        // 补充候选
    Symbol, Radical, Calc, Number,   // 模式候选
}
```

**为什么不用 `Vec<Candidate>`**：平台层与全部测试只依赖 `&[String]`，
换成结构体要改 20+ 个 push 点、4 个 `apply_*`、以及 `candidates()` 的返回类型；
而 side-table 拿到的是同一组信息（每条候选的来源），改动面只有一个入口函数与三处查找。
结构体化的其余收益（带分数、带词长）在 §4.5 的打分体系启动前用不上，先不付这个成本。

**迁移三步**（每步可编译）：

1. 加 `candidate_src` 与 `push_cand`，把所有 `self.candidates.push(...)` 换成 `push_cand(..., Source::X)`
   —— 编译器会逐个报错指出全部位置，不会漏。
2. 三个 `*_candidate_pos` 的读取点改为辅助函数：

```rust
fn first_index_of(&self, src: Source) -> Option<usize> {
    self.candidate_src.iter().position(|&s| s == src)
}
```

3. 删除 `english_candidate_pos` / `phrase_candidate_pos` / `datetime_candidate_pos`；
   `apply_*` 里的 `eng_start` 改为 `self.first_index_of(Source::English).unwrap_or(candidates.len())`。

**不变量守则**：`candidates` 与 `candidate_src` 只能成对修改，统一走四个封装
（`push_cand` / `retain_src` / `truncate_src` / `insert_src`），
禁止在任何地方单独 `candidates.push`。debug 构建下断言长度相等。

**收益**：
- 插入 Emoji 不再需要 `*p += emo.len()` 这类位置修正（`lib.rs:2129-2136`）
- 词长分区、置顶、降权可按来源精确作用
- 将来升级为 `Vec<Candidate>` 时，`push_cand` 就是唯一的改动点

**验收**：debug 断言常开；新增单测断言各分支产出的 `source` 正确；192 个既有测试全绿。


---

### 4.2 分区改为原地稳定排序（对应 P0-2）

**目标**：消除五段后处理的全表克隆。

```rust
// 原地稳定分区：命中在前，其余在后；不分配新 Vec
fn partition_stable<T, F>(items: &mut [T], pred: F)
where F: Fn(&T) -> bool
{
    items.sort_by_key(|item| !pred(item));   // 稳定排序，元素移动不克隆
}
```

`sort_by_key` 是稳定排序，对 `String` 只做移动（`mem::swap`），不触发堆分配；
它内部需要临时缓冲，但缓冲里存的是移动后的元素位置，不是克隆体。

**改造点**：

| 函数 | 改法 |
|------|------|
| `apply_context_boost` | 先算每条的 lift，再 `sort_by_key(|i| -(lift[i] as i64) * 1000 + i)`，去掉 `scored` 的 String 克隆 |
| `apply_pin_boost` / `apply_demote` / `apply_sort_mode` | 直接 `partition_stable` + 一次 rotate 到目标位置 |
| `apply_word_len_match` | 三分法 → `sort_by_key(|c| rank_of(char_count(c), n))`，rank ∈ {0,1,2} |

**边界保留**：所有函数仍以 `eng_start`（改造后为 `Source::English` 的位置）为右边界。

**验收**：候选顺序与改造前**逐字节一致**（用现有候选快照测试对比），
外加一次分配计数对比（推荐 `dhat` 或自写计数分配器）。

---

### 4.3 统一 seen 集合（对应 P0-3）

**目标**：把 11 处线性 `contains` 收敛成一次集合查询。

```rust
// query_all 中所有"补充候选"统一走这个入口
struct CandidateSink {
    items: Vec<Candidate>,
    seen: std::collections::HashSet<String>,
}
impl CandidateSink {
    fn push_unique(&mut self, text: String, source: Source) -> bool { ... }
    fn extend_unique<I: IntoIterator<Item = String>>(&mut self, it: I, source: Source) { ... }
}
```

`seen` 存 `String`（与 `items` 各存一份，内存翻倍但可控），
也可以用 `HashSet<u64>`（存 hash）牺牲极小碰撞率换内存——建议先用 `String`，实测后再决定。

**验收**：长串（≥12 字符）输入下，`query_all` 内 `contains` 调用次数降为 0；
候选内容与改造前一致（顺序也一致，因为集合只用于去重判定）。

---

### 4.4 输入质量分类，替代候选数量阈值（对应 P1-1）

**目标**：分支触发由"输入是什么"决定，而不是"词库恰好给了几条"。

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputQuality {
    Exact,        // 输入串本身就是词条拼音（含前缀命中）
    FullPinyin,   // 可完整切分为合法音节，但词库无整词
    Mixed,        // 全拼段 + 声母段混插
    Initials,     // 纯声母
    Partial,      // 不完整拼音（末尾半个音节）
    Typo,         // 完整拼音但含非法组合，需要纠错
}

fn classify_input(&self, s: &str) -> InputQuality { ... }
```

替换规则：

| 分支 | 现在 | 改为 |
|------|------|------|
| 拼写纠错 `spelling_variants` | `len() < page_size` | `quality == Typo` |
| 删字容错 `deletion_variants` | `len() < page_size` | `quality == Typo \|\| quality == Partial` |
| 相邻键纠错 | `len() < page_size * max_pages` | `quality != Exact` |
| 错音提示 | `len() < page_size` | `quality == FullPinyin` |
| 组合联想 `query_combo` | `!is_full_pinyin && len() < ...` | `quality == Mixed \|\| Initials` |
| 模糊音 | `is_full_pinyin && may_have_fuzzy` | 不变（这条本来就对） |

**验收**：新增"跨词库规模一致性"测试——
同一输入在内置兜底词库与完整词库下，`quality` 相同、触发的分支集合相同。
这是当前代码做不到的，也是 P1-1 的价值所在。

---

### 4.5 上下文权重修正（P1-3 立即做）与统一打分（P1-2 暂缓）

这两项原本写在一起，但它们的前置条件不同，需要拆开。

#### 4.5.1 P1-3：上下文权重改为纯加分（立即可做）

**问题**：`lib.rs:1167-1181` 用 `lift - i` 排序，位置项压过权重项。

**改法**：排序键从 `(lift - i)` 改为 `(-weight, i)` —— 权重降序，同权重时保持原位置顺序。

```rust
let mut scored: Vec<(i64, usize)> = (0..limit)
    .map(|i| {
        let lift = merged.iter()
            .find(|(w, _)| w == &candidates[i])
            .map(|(_, weight)| (*weight).min(Self::CONTEXT_WEIGHT_CAP) as i64)
            .unwrap_or(0);
        ((lift > 0) as i64, -lift, i)  // 命中优先 → 权重降序 → 原序
    })
    .collect();
```

**影响面**：单个函数，约 10 行；不改变其余四段的执行逻辑。
**行为变化**：长候选列表（>20 条）下搭配词会重新生效。需要在真实词库上对比首选率。
**验收**：`zhongguo`/`womende` 这类组合下，搭配词进入前 3 的比例不下降。

#### 4.5.2 P1-2：统一打分（**条件触发后再做，本轮不启动**）

**启动条件**（三条全满足才做，缺一条就继续用现有五段）：

1. 词库需要从云端增量更新，排序数据必须独立于词库版本；
2. 排序策略要按场景（办公/聊天/代码）或用户画像动态下发；
3. 出现第 6 条 `apply_*` 规则，或五段执行顺序再出一次互相覆盖的事故。

**当前判断**：三条都不满足。
泰深是本地全量词库、随安装包发布、调参频率约等于"每次发版顺手改几个常数"。
在这种形态下引入统一打分，收益是"规则更好维护"，代价是
2-3 天结构改造 + 一轮行为回归（192 个测试里大量断言依赖当前顺序）。

**以下方案留档，触发条件满足时按此执行**：

```rust
// 分数构成（int，便于排序与调试）
fn score_of(c: &Candidate, ctx: &ScoreCtx) -> i64 {
    let base = match c.source {
        Source::UserHot   => 100_000,
        Source::Phrase    =>  90_000,
        Source::Datetime  =>  85_000,
        Source::UserWarm  =>  80_000,
        Source::UserStale =>  60_000,
        Source::Common    =>  50_000,
        Source::System    =>  40_000,
        Source::Domain    =>  30_000,
        Source::Emoji     =>  10_000,
        Source::English   =>   5_000,
        _                 =>   1_000,
    };
    let freq      = c.freq_norm * 1000;              // 层内频度 0..=1000
    let context   = ctx.context_weight(c) * 100;     // 0..=2000，纯加分
    let pin       = if ctx.pinned(c) { 50_000 } else { 0 };
    let len_match = if ctx.word_len_match(c) { 10_000 } else { 0 };
    let demote    = if ctx.demoted(c) { -40_000 } else { 0 };
    base + freq + context + pin + len_match + demote
}
```

一旦启动，需配套三件事：

1. 候选升级为 `Vec<Candidate>`（此时 §4.1 的 `push_cand` 是唯一改动点）；
2. `demote` 从"固定第 11 位"改为减分——当前实现在候选不足 10 条时退化为排末尾，
   减分方案下自然沉底且不影响其它词相对顺序；
3. 影子模式灰度：同时算新旧两套顺序，输出新顺序、把差异写日志，观察一周再定。


---

### 4.6 模式分发表（对应 P2-1）

```rust
enum Mode { Symbol, SymbolQuery, Calc, Radical, Number, Unicode, Shuangpin, Compose }

fn detect_mode(&self) -> Option<Mode> { ... }
fn apply_mode(&mut self, mode: Mode) -> bool {  // 返回是否已处理
    let (cands, source) = match mode { ... };
    self.publish(cands, source);
    true
}
```

`publish()` 统一负责：清空并重建 `candidate_src`（side-table 的唯一维护点）、设 `all_candidates`、
`page=0`、`repage()`。

**验收**：六个模式的行为与现状一致；新增模式只需加一个 `Mode` 变体 + 一个分支。

---

### 4.7 词库注册表收敛（对应 P2-2）

```rust
struct DictRegistry {
    system: Option<(PathBuf, Dictionary)>,
    user:   Option<PathBuf>,
    domain: Option<PathBuf>,
    ready:  AtomicBool,
    generation: AtomicU64,
}
```

把 7 个散落全局收进一个结构体，`init()` 的幂等判断、代际校验、待合并领域数据
都变成该结构体的方法。

**验收**：初始化顺序相关测试（切换输入法、先领域后系统、加载中查询）全部通过；
新增一个"并发 init 两次"的测试。

---

### 4.8 汉字判定区间化（对应 P2-3）

```rust
fn is_hanzi(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}
```

替换 `lib.rs:1268` 与 `2219` 的两处闭包。

---

### 4.9 排序参数外置（新增项，成本最低、收益最直接）

**问题**：影响候选顺序的全部参数都是编译期常量，改一个数就要重新编译发版。

| 常量 | 位置 | 当前值 |
|------|------|--------|
| `CONTEXT_WEIGHT_CAP` | `lib.rs:1063` | 20 |
| `CONTEXT_BUILTIN_WEIGHT` | `lib.rs:1065` | 32 |
| `CONTEXT_EXTERNAL_WEIGHT` | `lib.rs:1067` | 12 |
| `CONTEXT_PRIOR_MAX_WEIGHT` | `lib.rs:1090` | 6 |
| `LEARNED_CONTEXT_CAP` | `lib.rs:1061` | 8 |
| 降权落点 | `lib.rs:1242` | 第 11 位（`10usize.min(rest.len())`） |

**方案**：新增 `tuning.json`，启动时读一次，缺省回退到内置常量。

```json
{
  "schema_version": 1,
  "context_weight_cap": 20,
  "context_builtin_weight": 32,
  "context_external_weight": 12,
  "context_prior_max_weight": 6,
  "learned_context_cap": 8,
  "demote_position": 10
}
```

**实现约束**（三条，缺一不可）：

1. **范围钳制**：每个值有合法区间（例如 `context_weight_cap` ∈ [0, 100]），
   越界取边界值并写一条 warn 日志，不静默接受。
2. **缺省回退**：文件缺失、解析失败、字段缺失 → 用代码内置值，不 panic。
3. **不做热更新**：启动读一次。热更新会让"这次输入为什么变了"无法复现，收益不抵成本。

**为什么不单独建"排序数据层"**：
排序数据层解决的是"排序策略要独立于词库版本演进"（云端下发、按版本回滚）。
泰深的词库与代码同版本发布，不存在这个诉求。
把常数外置到 JSON，已经覆盖「不改代码就能调参」这一真实需求，
再往上加一层数据库，是为不存在的场景付费。

**验收**：
- 改 `tuning.json` 后不重新编译即可观察到候选顺序变化；
- 非法值被钳制且有日志；
- 文件删除后行为与改造前完全一致；
- 新增单测覆盖：缺省、越界、部分字段缺失三种情况。


---

## 5. 实施顺序与工作量

| 步骤 | 内容 | 工作量 | 行为变化 | 风险 |
|------|------|--------|---------|------|
| 1 | P0-3 统一 seen | 0.5 天 | 无 | 低 |
| 2 | P0-2 原地分区 | 1 天 | 无 | 低 |
| 3 | 4.9 排序参数外置 | 0.5-1 天 | 无 | 低 |
| 4 | P0-1 候选来源 side-table | 1 天 | 无 | 低-中 |
| 5 | P2-1 模式分发表 | 0.5 天 | 无 | 低 |
| 6 | P1-3 上下文权重纯加分 | 0.5 天 | 有（局部） | 中 |
| 7 | P1-1 输入质量分类 | 1 天 | 有 | 中 |
| 8 | P2-2 注册表 + P2-3 汉字判定 | 1 天 | 无 | 低 |
| — | P1-2 统一打分 | 2-3 天 | 有（最大） | 高 |

**总投入约 6-7 天**（含测试，不含最后一行）。建议按步骤独立提交，每步保持可发布状态。

**P1-2 不排进计划**：启动条件见 §4.5.2，三条全满足时再单独立项。
届时的工作量估算（2-3 天）与风险（高）都写在该节，不占用当前排期。

第 6、7 步是行为变更，需要先有基线数据（附录 A）才能验收。


---

## 6. 验收与回归策略

### 6.1 既有资产

- `lib.rs` 146 个 `#[test]`，`dictionary/mod.rs` 46 个，共 192 个
- 已有探针：`engine/examples/probe_shuangpin*.rs`（6 个）
- 命令：`cargo test --manifest-path engine/Cargo.toml -- --test-threads=1`

### 6.2 新增测试

| 测试 | 覆盖 |
|------|------|
| 候选来源正确性 | 每个分支产出预期 `Source` |
| 跨词库规模一致性 | 内置词库 vs 完整词库，同输入 → 同分支集合 |
| 顺序不变性（改造前后） | 步骤 1-4 的顺序逐字节一致 |
| 长串性能 | 12 字符输入下，候选生成耗时不随分支数量线性增长 |
| 降权/置顶语义 | 降权词不在前两屏；置顶词在首位 |

### 6.3 需要采集的基线（附录 A）

在动手前先跑一组数据，否则无法判断优化是否有效：

1. **候选生成耗时**：1 / 2 / 4 / 8 / 12 / 16 字符输入，各 1000 次，记 P50 / P95。
2. **各分支耗时占比**：在 `query_all` 每个分支前后打点（可用 `Instant::now()` + 环境变量开关）。
3. **候选列表长度分布**：真实输入下 `candidates.len()` 的分布，判断 P0-2/P0-3 的实际收益。
4. **分配次数**：用 `dhat` 或自写计数分配器，测一次 `query_all` 的 `String` 分配数量。
5. **词库规模影响**：同一批输入在两种词库下的候选差异（验证 P1-1 的必要性）。

### 6.4 不建议一次做完的事

- 不要同时改 P0-1 与 P1-2：前者是结构迁移（应零行为变化），后者是行为变更，
  混在一起无法归因。
- 不要在步骤 5/6 之前删旧分支：保留旧路径作为对照，用环境变量切换（`TAISHEN_RANK_OLD=1`），
  灰度一周后再删。

---

## 7. 附：不在本轮范围的项

以下问题存在但本轮未深入，列为后续：

| 项 | 说明 |
|----|------|
| 双拼 ü 系列 | `shuangpin.rs` 表中有 `ve` 但无 `van`/`vn`；`pinyin::normalize_v` 只处理 j/q/x/y 后的 v。**需实测确认** `lv`/`lve`/`nve` 等输入是否正常，再决定是否补表。 |
| 词库构建 | 属外部仓库，本审计只看消费侧（`from_bin` / `build_index`）。 |
| 平台层 | `platform/windows/src` 未审。 |
| 内存占用 | 引擎全量反序列化，需要内部探针才能测准，外部无法归因。 |

