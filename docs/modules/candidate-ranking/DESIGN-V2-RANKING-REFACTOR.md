> 模块：engine 核心（Rust）· 候选排序
> 状态：**重构设计稿，尚未实施**（2026-11-21）
> 关系：`DESIGN.md` 是现行实现总纲；本文件是同目标的**结构性重设计**。实施完成后，本文件替代 `DESIGN.md` 中「统一裁决排序算法」相关章节。
> 迁移：四阶段递进（影子模式 → 双轨开关 → 转正清理 → 扩展），每阶段可独立回滚。
> 校准：2026-11-21 二次修订——`context_rank` 前移至 `freq_rank` 之前；新增 `segment_rank` 并补齐整句候选的 RankKey 取值。
> 校准：2026-11-21 三次校准（开工前代码核查）——修正 L1/L2 判据（词库无 `'` 隔音符，改按字数与音节数对齐；L2 严禁截胡 Fuzzy/Correction/Combo）；补齐 `rare_rank`、`user_tier`、`common_order` 与 B2 非可信精确词保护，移除 `freq_rank` 后失效的 `credible_rank`；统一 §7.1/§7.4/§C.3/§C.5 与现行 `Dictionary`/`sentence.rs` 接口。

---

# 候选排序架构重设计 v2

日期：2026-11-21
状态：设计稿（不含实现）
范围：`engine/src/lib.rs` 候选生成与排序链
纲领：结构自洽优先于局部修补。允许推翻现行结构。

---

## 第一部分 · 问题陈述

### 1.1 现状的三个结构性缺陷

**缺陷一：生成与排序混杂在同一函数里**

`query_all` 单函数 856 行（2542–3398），35 步。其中 21 步是候选生成
（push / insert），14 步是排序与过滤（apply_* / retain / truncate）。
生成与排序交替执行，生成顺序本身成为排序的隐含输入。

后果：新增一路来源时，无法独立评估它对排序的影响——因为它插在哪一步
决定了它最终排在哪。

**缺陷二：排序是多次过程式重排，顺序即语义**

现有 8 个重排函数：

```
apply_context_boost      → 1732
apply_sentence_candidates→ 1810
apply_word_len_match     → 3435   （主排序）
apply_ambiguous_split_boost → 3730
apply_completion_boost   → 3641
apply_pin_boost          → 1868
apply_sort_mode          → 1943
apply_fuzzy_demote       → 3412
apply_demote             → 1900
```

每一个都有独立作用域、独立排序键、独立前提。代码注释里密集出现
「必须放在 X 之前」「必须放在 X 之后」，共 12 处。这些约束不是冗余注释，
它们是**语义本身**：调换两个调用的顺序，结果就不同。

后果：任何一条新规则，必须回答「我插在哪、会不会被后面的步骤推翻」。
这正是 tier 硬边界一加就崩的机制——它和 8 个既有约定同时冲突。

**缺陷三：裁决依据重复计算，口径可能漂移**

同一个事实在多处独立计算：

| 事实 | 计算点 |
|------|--------|
| `exact_has_credible` | 2597（生成阶段）、3483（排序阶段） |
| `input_splits_clean` | 2555、3446 |
| 汉字数 | 2582、3475、3561、3745 |
| 常用词判定 | 2607、2694、3493、3682、3704 |

后果：两次计算之间如果候选集变了（追加了 Completion / 英文），
第二次的口径与第一次不同，出现「生成阶段认为有可信词、排序阶段认为没有」
这类隐性矛盾。

### 1.2 一句话概括

**候选的顺序没有一个单一的裁决点。** 它是 35 步副作用叠加的涌现结果。

---

## 第二部分 · 设计公理

新结构必须满足四条可检验的公理。

### 公理 1：全序性

任意两条候选的先后关系，由它们自身的属性与输入唯一决定，
不依赖它们在候选数组里的初始位置，也不依赖排序调用的顺序。

> 检验：把候选数组随机打乱后重跑排序，结果必须一致。
> 现状不满足——`sort_by_key` 稳定排序在键相等时保留原序。

### 公理 2：相对性

候选的排序资格由它与当前输入的匹配关系决定，不由它的来源决定。

> 检验：同一个词在不同输入下必须可以落在不同层。
> 「你好」在 `nihao` 是精确匹配，在 `niha` 是预测匹配。
> 现状不满足——tier 只认来源，同一个词永远同一 tier。

### 公理 3：单调性

增强任意一个正向信号，不会让该候选的位次变差。

> 检验：给某候选加用户词标记，它不能从第 3 掉到第 5。
> 现状部分不满足——多个 apply_* 的作用域不同，可能出现「加了标记
> 反而被另一个重排挤走」。

### 公理 4：可扩展性

新增一路候选来源，不需要修改任何既有排序代码。

> 检验：新增一路来源后，`git diff` 只应出现在生成侧与 Match 映射表。
> 现状不满足——每加一路，都要重新检查 8 个 apply_* 的交互。

---

## 第三部分 · 核心模型

### 3.1 中心概念：Match（匹配关系）

现行模型用 `source`（来源）当身份，用 `tier` 当层级。
但**来源不是身份，匹配关系才是**。

一条候选的排序资格，由「它和当前输入的关系」决定。
这个关系是二元函数：

```
Match(I, C) -> Level
I = 输入特征（音节切分、目标字数、是否可整串读成拼音）
C = 候选特征（规范拼音、字数、形态）
```

### 3.2 Match 的八层定义

有序，L0 最强，L7 最弱。全序。

| Level | 名称 | 判据 | 例 |
|-------|------|------|-----|
| L0 | Explicit | 用户显式指定，不参与自然竞争 | 置顶词、快捷短语、日期简码 |
| L1 | Exact | 去分隔拼音相等 **且** 音节切分相等 | `nihao` → 你好 |
| L2 | Shape | `C.chars ∈ target_lens`，但切分不同 | `xian` → 西安（`xi'an`，切分 2 段 vs 输入 1 段） |
| L3 | Extension | 输入切分是候选切分的**严格前缀** | `hou` → 后面；`niha` → 你好 |
| L4 | Truncation | 候选切分是输入切分的**严格前缀** | `zhongguoo` → 中国 |
| L5 | Inferred | 需变体规则才成立（模糊音 / 邻键 / 错音） | `zongguo` → 中国 |
| L6 | Composed | 多段拼凑（逐字硬拼 / 拆组） | `rgshni` → 如果是你 |
| L7 | Foreign | 拼音规则不适用，走外文兜底 | `api` → API |

> L0 不进 `match_level`。它由 `explicit_rank` 独立表达，
> 因为「用户指定」与「匹配关系」是两种不同性质的信号。
> 因此 `match_level` 的实际取值域是 L1..L7，编码为 `0..=6`。
>
> L6 的 Composed 不包含整句解码。整句按覆盖率归层，见 3.3 末段。

### 3.3 为什么这个顺序是自洽的

**L1 先于 L2**：完全匹配一定优于「字数对但拼法有出入」。
`xian` 下「先」(xian, 1 音节) 是 L1，「西安」(xi+an, 2 音节) 是 L2。
单字在前，与现行 `apply_ambiguous_split_boost` 的意图一致，
但不再需要一个专门的函数去搬位置。

> 这里有一个必须钉死的细节：现行 `Dictionary::full_index` 在加载时已用 `dict_pinyin`
> 剥离 `'` 隔音符（「西安」与「先」的索引键都是 `"xian"`），所以不能靠字符串里的 `'`
> 区分切分。但中文词满足「一字一音节」：输入 `xian` 的主切分 `best_split = ["xian"]`
> 为 1 段，「先」1 字 = 1 段 → **L1**；「西安」2 字 ∈ `target_lens = [1, 2]` 且字数 ≠ 1 → **L2**。
> 因此 **L1 的判据是「去分隔拼音相等 且 候选字数 == `input.best_split.len()`」**（单字/多字词无需改 `.bin` 格式即可精确区分）。

**L2 先于 L3**：字对 > 多说一个音节。
`nihao` 下「你号」L2（2 字，拼音 nihao），「你好世界」L3（4 字，多两音节）。

**L3 先于 L4**：多说是主动的（用户想接着打），少说是手误。
手误场景下（`zhongguoo`）输入不是合法拼音，L3 候选本就不会产生
（没有以 `zhongguoo` 为前缀的词），所以固定 L3 先于 L4 不会误伤。

**L3 层内按「超出多少」排序**：`hou` 下「后面」（+1 音节）优先于
「后面怎么办」（+3 音节）。这个维度叫 `extent_rank`，只在 L3/L4 内有效。
它替代了现行 `PREFIX_EXT_KEEP` + `ext_overflow` 那套「保留前 3 条」的位置技巧。

**L5 先于 L6**：变体规则（模糊音、邻键）是可解释的音近关系；
拼凑是组合出来的，可信度更低。

**L6 先于 L7**：中文拼凑结果优于外文兜底。

**整句解码不是固定的 L6**：整句候选覆盖输入的全部音节，
按「匹配关系」定义它属于 L1（完全匹配），只是匹配单位是句而非词。
因此整句的 `match_level` 由**覆盖率**决定：覆盖全部音节 → L1，否则 → L6。
短输入不出整句的门槛留在生成侧（它是生成条件，不是排序条件）。

**整句候选的 RankKey 取值**（补齐，否则整句在同层无维度可竞争）：

| 维度 | 整句取值 | 说明 |
|---|---|---|
| `explicit_rank` / `pref_rank` / `demoted_rank` | 1 / 1 / 0 | 不参与显式意图 |
| `match_level` | 全覆盖 → L1，否则 L6 | 见上 |
| `extent_rank` | 0 | 整句只出现在 L1/L6，此维度不生效 |
| `user_rank` / `common_rank` | 1 / 1 | 拼接产物不是用户词或常用词 |
| `segment_rank` | **内部拼接块数** | 关键维度，见下 |
| `context_rank` | 末块与上屏词的 bigram | 与词级同口径 |
| `freq_rank` | `u32::MAX` | 无单一词频 |
| `credible_rank` / `form_rank` | 0 / 0 | |

`segment_rank` 解决的是「整句与词级在同层竞争」的问题。以 `zhongguorenminyinhang` 为例：

```
「中国人民银行」       词级 → segment_rank = 1
「中国 + 人民 + 银行」  整句 → segment_rank = 3
```

段数 1 < 3，词级整词胜出——不需要再靠「整句无条件插首位」这种粗暴手段。
反方向也安全：长整句段数多，不会因为「覆盖音节多」而压过短词，
因为 `match_level` 已把它们分到不同层（未全覆盖的整句落 L6）。

### 3.4 三层结构

```
┌─────────────────────────────────────────────────────┐
│ LAYER 1 · EVIDENCE（证据生成）                       │
│                                                      │
│  输入 ──┬─→ 词库精确查询      ─→ 候选 A             │
│         ├─→ 词库前缀扩展      ─→ 候选 B             │
│         ├─→ 补全预测          ─→ 候选 C             │
│         ├─→ 变体规则（模糊/邻键/错音） → 候选 D      │
│         ├─→ 拼凑（逐字/拆组）  ─→ 候选 E             │
│         ├─→ 整句解码          ─→ 候选 F             │
│         └─→ 外文 / 符号       ─→ 候选 G             │
│                                                      │
│  每个来源只负责「产出候选 + 标注自己是怎么来的」      │
│  不排序、不插位、不知道其他来源存在                   │
└──────────────────────┬──────────────────────────────┘
                       │  Vec<Candidate>（无序，仅去重）
                       ▼
┌─────────────────────────────────────────────────────┐
│ LAYER 2 · REDUCTION（证据归约）                      │
│                                                      │
│  对每条候选，一次性计算并写入 Attestation：           │
│    · 读取生成侧带出的规范拼音（不走全表回查）         │
│    · Match(I, C) → Level                             │
│    · 词频 / 常用行序 / 用户热温过期 / 生僻字         │
│    · 上下文加成（bigram + 学习表，取 max）            │
│    · 显式意图 / 降权标记                              │
│                                                      │
│  同一事实只算一次。后续所有阶段只读 Attestation。     │
└──────────────────────┬──────────────────────────────┘
                       │  Vec<Candidate>（带证据，仍无序）
                       ▼
┌─────────────────────────────────────────────────────┐
│ LAYER 3 · DECISION（裁决）                           │
│                                                      │
│  单一函数：rank(candidates, ctx) -> Vec<Candidate>   │
│    · 计算 RankKey（全序元组）                         │
│    · 一次稳定排序                                     │
│    · 簇内形态排序                                     │
│                                                      │
│  没有第二个函数会再改顺序。                            │
└──────────────────────┬──────────────────────────────┘
                       │
                       ▼
                  候选序列
```

这个结构的核心承诺：**从 Layer 3 出口之后，没有任何代码再动顺序。**

---

## 第四部分 · 排序键

### 4.1 RankKey 全序元组

```rust
fn rank_key(c: &Candidate, ctx: &RankContext) -> RankKey {
    (
        explicit_rank,   // 0 = 显式意图（日期/短语/置顶/强英文置顶），1 = 自然候选   u8
        pref_rank,       // 0 = 用户偏好命中（单字/长词优先）                         u8
        demoted_rank,    // 0 = 正常，1 = 用户降权                                    u8
        rare_rank,       // 0 = 正常，1 = 非用户非常用含 Ext-A/B+ 生僻字（B6）         u8
        match_tier,      // 0..7：L1 可信精确 → L2 形态 → L3 扩展 → L4 截断/可信纠错 → L5 非可信精确/普通变体 → L6 拼凑 → L7 外文  u8
        extent_rank,     // 仅 L3/L4 有效：偏离音节数，越小越优                        u8
        user_tier,       // 0 = 热/温用户词，1 = 常用词（含单字豁免），2 = 过期用户词，3 = 普通词  u8
        segment_rank,    // 切分段数，越少越优（词级=1，整句=内部块数）                u8
        context_rank,    // 上下文加成分降序（**必须在 common_order/freq 之前**）    u8
        common_order,    // 常用词人工行序升序（非常用词取 u32::MAX，保口语「想>向」） u32
        freq_rank,       // 按词频降序（u32::MAX - freq；无词频取 u32::MAX）          u32
        form_rank,       // 簇内形态：文本 0 / emoji 1 / 符号 2                        u8
    )
}
```

稳定排序一次，键相等时按文本字典序兜底（满足公理 1 全序性：打乱输入顺序结果严格一致）。

### 4.2 每个维度的取值来源

| 维度 | 来源 | 说明 |
|------|------|------|
| `explicit_rank` | 日期简码 / 快捷短语 / 用户置顶 / 强英文精确词（`ENG_EXACT_TOP`） | 按子级 `0..=4` 区分日期 > 短语 > 置顶 > 强英文 > 自然候选（5） |
| `pref_rank` | `sort_mode`（单字/长词优先） | 用户显式偏好，可跨层 |
| `demoted_rank` | 用户降权集合 | 显式意图的负向表达 |
| `rare_rank` | `has_rare_hanzi && !is_user && !is_common && !explicit` | 保留现行 B6 生僻字保护（防止 Ext-A/B+ 罕见字进首屏） |
| `match_tier` | `Match(I, C)` + B2 可信度修正 | 正常 `L1..L7`；当精确段无可信中文词（`!exact_has_credible`）时，非可信 L1 错拼撞词（如 `zhonggou→重构`）退至可信纠错（如 `中国`）之后，保留现行 B2 语义 |
| `extent_rank` | 候选音节数 − 输入音节数（L3 取正，L4 取反） | 替代 `ext_overflow` |
| `user_tier` | 用户词热/温/过期状态 + `is_common` | 保留现行 P1 热/温用户词 > P2 常用词 > 过期用户词 > P3 系统词（单字 `chars==1` 且非常用时仍落常用之后，避免生僻单字压过常用字） |
| `segment_rank` | 候选的切分段数：词级恒 1，组合/整句取内部块数 | 越少越优。由 `sentence::decode_meta` 直接带出 |
| `context_rank` | bigram + 学习表取 max | 档位 1 不计（与整句解码口径一致）。**位置必须在 `common_order`/`freq_rank` 之前**，理由见 4.3 |
| `common_order` | `dictionary::common_rank(word)` | 常用词人工行序（升序）；非常用词取 `u32::MAX`，避免书面语料词频把口语字倒挂（`想` 22 vs `向` 490） |
| `freq_rank` | `u32::MAX - freq` | 未知词频取 `u32::MAX`。整句候选无单一词频，按解码分折算或取 `u32::MAX` |
| `form_rank` | `Form` 枚举 | 见第五节 |

> 注：原设计稿中排在 `freq_rank` 之后的 `credible_rank` 在数学上是死维度（`user_tier` 与 `freq_rank` 相等时 `credible` 必然相等），已移除；可信度语义前移至 `match_tier`（处理 B2 精确非可信 vs 可信纠错）。

### 4.3 为什么不用加权求和

加权求和（`score = w1·match + w2·freq + …`）的问题是权重不可解释，
且一次调参会影响全部场景。

字典序（本设计）保证了：**高层永远压过低层**。
`nihao` 下「你好」(L1) 永远在「你好世界」(L3) 之前，不管后者词频多高。

词频只在同层内竞争，这是符合直觉的——用户打完整拼音，要的是那个词；
用户只打了前缀，才轮到「哪个后续词更常见」的竞争。

**为什么 `context_rank` 必须排在 `freq_rank` 之前**

字典序的语义是「前面的维度不同，后面的维度不参与比较」。同层候选（都 L1）
词频几乎不可能相同，所以 `context_rank` 一旦放在 `freq_rank` 之后，
就**永远不会被读取到**——上下文加成等于被彻底屏蔽。

但上下文是「此刻输入的前文」，比静态词频更贴近当下意图。现行
`apply_context_boost` 的语义正是**提权**：把词频低但与上屏词更搭的候选提到前面。
若把它放在词频之后，这个能力在 v2 里直接消失，属于行为回退。

因此顺序定为 `… → segment_rank → context_rank → freq_rank → …`：
先比整体性（段数），再比此刻的上下文契合，最后才比静态词频。

### 4.4 层内为什么不再次分层

层内用 7 个维度（user → common → segment → context → freq → credible → form）。
它们都是「同层内更强的证据」。**注意：字典序下并非「任意一个都能翻转相邻位次」**，
只有前缀维度全部相等时，后一个维度才生效。所以层内顺序本身就是语义，
不能随意排列（`context` 与 `freq` 的先后即为实例，见 4.3）。

如果在层内也做硬边界（比如「用户词永远在常用词前，不管词频」），
会出现「用户词只有一个字，却压过整词」这类反直觉结果。
现行代码里 `user_rank` 就是第 5 位，前面还有 group / exact_len，
这个位置经过多轮调试，保留。

---

## 第五部分 · 簇模型（形态变体）

### 5.1 问题

同音多形态并列时，候选常见如下形态：

```
1. 你好
2. 👋 你好
3. (^▽^)ノ 你好
```

三条候选文本不同，但**意图相同**。它们占三个坑位。

如果按纯文本去重，emoji 版本永远不会出现；如果不去重，它们会挤占
其他词的位置。

### 5.2 设计

引入 `Form` 与 `Cluster`：

```rust
enum Form {
    Text,      // 纯文本
    Emoji,     // 带 emoji：👋 你好
    Symbol,    // 颜文字：\(@^0^@)/
}

struct Candidate {
    // ...
    cluster: ClusterKey,   // 簇键：用于分组
    form: Form,
}

// 簇键取「本体文本」：剥掉 emoji / 符号后的词
```

### 5.3 排序规则

1. 本体的 `match_level` 决定整个簇的位置。
2. 簇内按 `form_rank` 排：文本 → emoji → 符号。
3. 簇占据的坑位数可配置（默认 2：文本 + 一个变体）。

### 5.4 为什么放在 Decision 的最后一步

簇排序不改变任何跨簇的相对顺序，只重排簇内成员。
因此它可以作为「决定赛后处理」，不违反「只排序一次」的原则——
它是同一排序键的一部分（`form_rank` 位于末位），不是第二次重排。

> 关键：`cluster` 本身不进 RankKey。它是**分组键**，不是排序键。
> 实现上按 `(rank_key_without_form, cluster, form_rank)` 排序，
> 等价于先按簇排、再按形态排。这仍然是单次 `sort_by_key`。

### 5.5 当前不具备的能力

- 候选不带拼音 → 无法判断「泥嚎」是不是「你好」的形态变体
  （实际上它们是不同词，共享拼音）
- 词性 / 释义标注（`int. hello`）——本设计**不包含**，属于数据层扩展，
  需要词库侧提供词性字段。设计上预留 `CandidateMeta.extra` 位置。

---

## 第六部分 · 补丁函数的退场

这是本设计最关键的收益：**五个补丁函数可以被 Match 层吸收。**

| 现行函数 | 现行作用 | 新结构中的归属 |
|----------|----------|----------------|
| `apply_fuzzy_demote` | 精确命中时把 Fuzzy 整体后置 | Fuzzy 的 `match_level = L5`，精确命中的词是 L1/L2，天然在后。**直接删除** |
| `apply_completion_boost` | 把 Completion 插到精确段后部 | Completion 的 `match_level = L3`，在 L1/L2 之后。插位逻辑由 `length_bias` 承担。**降级为生成侧的排序提示** |
| `apply_ambiguous_split_boost` | 把隔音歧义词搬到第 5 位 | 隔音歧义词是 L2（Shape），单字是 L1，单字天然在前；L2 内部按词频排。**删除**（若实测需要保底，改为 `L2` 内的 `credible_rank` 提升） |
| `apply_context_boost` | 按 bigram 权重重排 | `context_rank` 维度。**降级为一个维度** |
| `apply_word_len_match` | 主排序（8 元组） | 被 `match_level` + `length_bias` 替代。**重写** |

保留的：

| 函数 | 归属 |
|------|------|
| `apply_pin_boost` | `explicit_rank = 0` |
| `apply_demote` | `demoted_rank = 1` |
| `apply_sort_mode` | 用户在 UI 上的偏好，作为 `form_rank` 之前的用户偏好位 |
| 单字母单字优先 | 属于 `Shape` 的特殊情形（1 音节输入 → 1 字是 L1） |

### 6.1 `apply_sort_mode` 的处理

这是**用户偏好**，不是自然排序。它必须能覆盖自然排序，否则设置无效。

设计：在 RankKey 最前加一个 `pref_rank` 位（仅当 `sort_mode != 0` 时非零）。
它与 `explicit_rank` 同级，但优先级低于显式意图。

```rust
// 显式意图 > 用户偏好 > 降权 > 自然排序
(explicit_rank, pref_rank, demoted_rank, match_level, ...)
```

### 6.2 降权的语义澄清

现行实现把降权词**旋转到第 11 位之后**，而不是排到最末。
这是「压出前两屏」的语义，不是「排到末尾」。

新结构用 `demoted_rank = 1` 会让它排到所有非降权词之后，即最末。
**这是行为变更**，需要在迁移时决策：

- 选项 A：保留「第 11 位后」语义 → `demoted_rank` 不能在 RankKey 里，
  必须作为赛后位移步骤（破坏「只排序一次」）。
- 选项 B：改为「排到末尾」→ 语义更一致，但用户体验变了
  （降权词从第 11 位变成最后一页）。

建议 B。理由：降权是用户的负面意图，排到底比排到第 11 位更符合直觉。
且第 11 位之后仍然可翻页取到，两种方案都满足「可恢复」。
这个决策留给专项会话，设计上标注为**唯一需要用户确认的语义变更点**。

---

## 第七部分 · 数据结构

### 7.1 Candidate

```rust
struct Candidate {
    // ── 身份 ──────────────────────────────
    text: String,
    /// 规范拼音，**由生成侧填充**，不在 Reduction 阶段回查。
    ///
    /// 理由：`canonical_pinyin_for_word` 是遍历式实现（dictionary/mod.rs:2178），
    /// 注释自述「只在选词时调用，不进入按键热路径」。每键对 40 条候选各调一次
    /// 会退化成 O(40 × 索引规模)，不可接受。
    ///
    /// 生成侧本来就知道拼音：
    /// - 词库查询时索引键就是完整拼音，随条目一并带出即可
    /// - 变体 / 纠错侧持有变体串
    /// - 拼凑侧持有各段拼音拼接结果
    /// - 短语 / 日期 / 外文不参与拼音匹配，填 None（直接落 L0 / L7）
    ///
    /// 因此需要在 `dictionary` 侧新增一个返回带拼音条目的查询入口
    /// （见 7.5），旧 API 保持不动。
    pinyin: Option<String>,

    // ── 诊断（不参与排序）──────────────────
    source: Source,
    reason_mask: u32,

    // ── 形态 ──────────────────────────────
    form: Form,
    cluster: String,

    // ── 证据（Reduction 填充）──────────────
    attest: Attestation,
}

struct Attestation {
    match_level: Match,
    extent_rank: u8,
    segment_count: u8,    // 词级=1，整句=内部块数
    char_count: usize,
    freq: Option<u32>,
    user_tier: u8,        // 0=热/温用户词，1=常用词，2=过期用户词，3=普通词
    common_order: Option<u32>,
    is_user: bool,
    is_common: bool,
    has_rare: bool,
    context_lift: u8,     // 0..=20，档位 1 归零
    explicit_rank: u8,    // 0=日期，1=短语，2=置顶，3=强英文，4=自然候选
    demoted: bool,
}
```

**关键变更**：`tier` 字段删除。它记录的是来源分类，与排序无关，
其信息由 `source` + `match_level` 联合表达。诊断侧需要层级时，
读 `attest.match_level`。

### 7.2 Match 计算

```rust
fn match_of(input: &InputShape, cand: &Candidate) -> Match {
    // 整句解码：按覆盖率归层，不固定为最弱层
    if cand.source == Source::Sentence {
        return if cand.clean_coverage {
            Match::Exact          // 覆盖全部音节且无编辑回退 = 完全匹配
        } else {
            Match::Composed
        };
    }

    // 外文 / 符号 / 非中文（无拼音）→ 兜底层
    if cand.source == Source::English || cand.source == Source::Emoji {
        return Match::Foreign;
    }

    // L5 变体（模糊音 / 邻键 / 拼写 / 漏打 / 错音）：必须先于 L2 判定，
    // 防止同字数的模糊音/纠错词（如 nihao→利好、suanfa→断法）被 L2 截胡。
    if matches!(cand.source, Source::Fuzzy | Source::Correction) {
        // 多打字母截断：候选规范拼音是输入的严格前缀（如 zhongguoo → zhongguo，
        // 含末尾多余元音 o/a 导致 is_complete_pinyin=true 的情形）→ L4 Truncation
        if let Some(cp) = cand.pinyin.as_deref() {
            if !cp.is_empty()
                && cp.len() < input.norm.len()
                && input.norm.starts_with(cp)
            {
                return Match::Truncation;
            }
        }
        return Match::Inferred;
    }

    // L6 拼凑：逐字硬拼 / 拆组（STITCHED）必须先于 L2 判定
    if cand.source == Source::Combo && (cand.reason_mask & reason::STITCHED != 0) {
        return Match::Composed;
    }

    // 显式无拼音来源（短语 / 日期 / 模式 / 语音）
    let Some(cp) = cand.pinyin.as_deref() else {
        // 非完整拼音下的简拼/混拼词库直取（如 zg→中国、shurf→输入法，未带全拼时按目标字数归 Shape）
        if !input.splits_clean && cand.source == Source::Combo {
            return if input.accepts_len(cand.char_count) {
                Match::Shape
            } else {
                Match::Composed
            };
        }
        return Match::Foreign;
    };

    // L1 精确：去分隔拼音相等 且 候选字数 == 主切分音节数
    //   （按字数对齐区分「先」(1) 与「西安」(2)，无需词库保留 `'`）
    if cp == input.norm {
        if input.splits_clean && cand.char_count == input.best_split.len() {
            return Match::Exact;
        }
        // 同串异切（xian → 西安、yuan → 预案）→ L2 Shape
        if input.accepts_len(cand.char_count) {
            return Match::Shape;
        }
    }

    // 非完整拼音的简拼/混拼精确模式匹配（zg → 中国、zhyang → 张扬、shurf → 输入法）
    if !input.splits_clean && cand.source == Source::Combo && input.accepts_len(cand.char_count) {
        return Match::Shape;
    }

    // L3 多说：输入切分是候选切分的严格前缀（或前缀补全/前缀扩展）
    if input.splits_clean && cp.len() > input.norm.len() && cp.starts_with(&input.norm) {
        let rest = &cp[input.norm.len()..];
        if crate::pinyin::is_complete_pinyin(rest) {
            return Match::Extension;
        }
    }

    // L4 少说：候选规范拼音是输入的严格前缀（覆盖 tail 非空及末尾多打元音）
    if !cp.is_empty() && cp.len() < input.norm.len() && input.norm.starts_with(cp) {
        return Match::Truncation;
    }

    // 兜底：前缀字符扩展（非音节边界，如 lian → liang）归 Extension（带较大 extent_rank）或 Composed
    if cp.starts_with(&input.norm) {
        return Match::Extension;
    }

    if cand.source == Source::Combo {
        return Match::Composed;
    }

    Match::Foreign
}
```

**判定顺序不可调换**，理由：

- 变体（Fuzzy / Correction）与拼凑（STITCHED）必须先拦截，绝不能被 L2 宽松字数判据截胡（否则 `nihao` 的「利好」和 `suanfa` 的「断法」都会错落 L2）。
- L1 仅在 `cp == input.norm && char_count == best_split.len()` 成立；同串不同切分（`xian → 西安`）落 L2。
- L4 判据改为 `input.norm.starts_with(cp)`，解决 `zhongguoo` / `nihaoa` 因末尾 `o`/`a` 是合法音节导致 `tail` 为空的漏判。

### 7.3 InputShape

```rust
struct InputShape {
    raw: String,
    norm: String,              // 去分隔符 / v→u
    splits_clean: bool,        // 能否整串读成合法拼音
    best_split: Vec<String>,   // 合法时 = 最短切分；否则 = 最长合法前缀切分
    tail: String,              // 不可整串读时的多余字符（合法时为空）
    all_splits: Vec<Vec<String>>,
    target_lens: Vec<usize>,   // 各可行切分的音节数
    syllable_count: usize,
}
```

一次性构造，全部阶段只读。消除现在 4 处重复计算
（2555 / 3446 / 2575 / 3441）。

### 7.4 排序

```rust
fn rank(cands: &mut Vec<Candidate>, ctx: &RankContext) {
    cands.sort_by(|lhs, rhs| {
        let lk = lhs.attest.rank_key(pref_rank(ctx.sort_mode, lhs), lhs.form, ctx.exact_has_credible);
        let rk = rhs.attest.rank_key(pref_rank(ctx.sort_mode, rhs), rhs.form, ctx.exact_has_credible);
        lk.cmp(&rk).then_with(|| lhs.text.cmp(&rhs.text))
    });
}
```

其中 `rank_key` 返回严格与 4.1 对齐的 12 元组：
`(explicit_rank, pref_rank, demoted, rare_rank, match_tier, extent_rank, user_tier, segment_count, u8::MAX - context_lift, common_order.unwrap_or(u32::MAX), u32::MAX - freq.unwrap_or(0), form.rank())`。
键完全相等时以 `text` 字典序兜底，保证公理 1（打乱输入顺序结果完全不变）。

### 7.5 拼音来源（性能约束与现行词库布局的解法）

现行 `Dictionary` 的前缀索引（`index` / `common_index` / `user_index` / `domain_index`）
只存 `(word, freq, pinyin_len)`，不存完整拼音字符串；完整拼音存在 `*_full_index`
（`BTreeMap<String, Vec<(String, u32)>>`）中，且加载时已去除 `'` 隔音符。

在不修改 `.bin` 词库结构的前提下，`dictionary` 与 `sentence` 侧新增轻量带元数据入口：

```rust
pub struct WordEntry {
    pub word: String,
    /// 精确段（pinyin_len == key.len()）直接填 Some(key)（零成本）；
    /// 前缀扩展段填 None（仅记录 pinyin_len）或由 completion_candidates_meta 从 *_full_index 直接带出。
    pub pinyin: Option<Arc<str>>,
    pub pinyin_len: usize,
    pub freq: u32,
    /// 0 = 热/温用户词，1 = 常用词，2 = 过期用户词，3 = 系统词，4 = 领域词
    pub layer: u8,
}

pub fn query_with_split_meta(&self, pinyin_prefix: &str) -> (Vec<WordEntry>, usize);
pub fn completion_candidates_meta(&self, input: &str, limit: usize) -> Vec<WordEntry>;
```

`sentence.rs` 侧新增 `SentenceEntry` + `decode_meta`（不改解码算法，只把 `backtrack` 已算出的 `words` 与是否干净覆盖返回出来）：

```rust
pub struct SentenceEntry {
    pub text: String,
    pub score: f64,
    pub segment_count: u8,
    pub clean_coverage: bool,
}

pub fn decode_meta(input: &str, prev: Option<&str>, limit: usize) -> Vec<SentenceEntry>;
```

旧 `query_with_split` / `completion_candidates` / `sentence::decode` 保持签名不变（内部薄包装），影响面为零。

覆盖的生成路径：

| 生成路 | 拼音来源 |
|--------|----------|
| 精确命中 | `query_with_split_meta` 直接返回 `Some(key)`（零成本） |
| 前缀补全 | `completion_candidates_meta` 从 `*_full_index` 直接带出完整拼音 |
| 前缀扩展 | `query_with_split_meta` 带出 `pinyin_len`（按需归 L3 Extension） |
| 模糊音 / 纠错 | 变体串（生成时就持有，无需查） |
| 逐字拼凑 / 拆组 | `STITCHED` 标记直接归 L6 Composed |
| 整句解码 | `sentence::decode_meta` 带出 `segment_count` 与 `clean_coverage` |
| 短语 / 日期 / 外文 | `None`（落 L0 / L7，不参与匹配） |

---

## 第八部分 · 行为变更清单

重构必然改变部分结果。以下逐项列出，供专项会话决策。

### 8.1 预期改善（正向）

| 场景 | 现行 | 新结构 |
|------|------|--------|
| `nihao` 的 Fuzzy 同音词 | 第 4 位起被「利好/理好/立号」占满 | L5，在所有 L1/L2 之后 |
| `nihaoshj` 的 Combo | 第 5 位（L6 混在 group 0） | L6，在所有精确/补全之后 |
| `zgrm` 的 Combo 抢首位 | 首位 | L6，精确命中的词优先 |
| `nihao` 的长词穿插 | 「你好好休息」第 2 位 | L3，在「你号/拟好」等 L2 之后 |

### 8.2 预期可能回归（需重点验证）

| 场景 | 风险 | 原因 |
|------|------|------|
| 整句解码长输入 | 可能下沉 | 整句是 L6。现行靠 `apply_word_len_match` 的 `counts.contains` 给它 group 0 |
| `hou → 后面` | 可能下沉 | L3 vs 单字「后」L1。现行 Completion 有专门插位 |
| `niha → 你好` | 可能下沉 | 同上 |
| 单字母输入 `w` | 可能变化 | 1 字节输入的「单字优先」在现行是独立分区 |

**整句的特别说明**：整句解码覆盖输入**全部音节**，按定义不应是 L6。
修正：整句候选的 `match_level` 由**覆盖率**决定——
覆盖全部输入音节 → `Match::Exact`（L1）。

这解决了最大的风险点，且保持自洽：整句确实是「完全匹配输入」，
只是匹配单位是句而不是词。

同时要处理：`nihao` 这种短输入不该出整句（现行有 `MIN_SYLLABLES` 门槛）。
门槛保留在生成侧，因为它是「生成条件」而不是「排序条件」。

### 8.3 确定变更（需用户决策）

| 项 | 现行 | 新结构 |
|----|------|--------|
| 降权词位置 | 第 11 位之后 | 全部非降权词之后（最末） |
| 置顶词数量 | 无上限 | 建议上限 3，超出按词频 |

---

## 第九部分 · 迁移路径

分四阶段，每阶段可独立回滚。

### Phase A · 影子模式（不改变行为）

1. 新增 `Attestation` 与 `match_of`，作为**纯函数**实现，不接入主链。
2. 在 `publish_candidates` 之前调用一次，把每条的 `match_level`
   写入 `CandidateMeta`（扩展一个字段）。
3. 探针（`probe_rank`）输出新旧两套排序的对比。
4. 用 291 条用例 + 40 条口语跑一遍，统计差异分布。

**出口判据**：新排序的差异全部可解释（每条差异能说出是哪个维度导致）。
若出现无法解释的差异，说明 Match 定义有缺口，回 Phase A 修正。

**风险评估**：零行为风险。影子模式只增字段。

### Phase B · 双轨开关

1. 加 `ranking_v2` 开关（默认关）。
2. 打开时走新 Decision，关闭时走现行链。
3. 三套指标对比。

**出口判据**：exam 不低于 271，口语首位不低于 13/40。
若下降，逐个差异点分析，不允许「调阈值凑数」。

### Phase C · 转正与清理

1. `ranking_v2` 默认开。
2. 删除 `apply_fuzzy_demote` / `apply_ambiguous_split_boost` /
   `apply_completion_boost` 的插位逻辑。
3. 删除 `tier` 字段。
4. 更新 55 处内部测试。

**出口判据**：三套指标不降；392 个测试全绿。

### Phase D · 扩展

1. 接入黑盒刻度（采集管线）。
2. 补词性 / 释义数据（需词库侧配合）。
3. 形态变体（emoji）。

### 9.1 为什么必须影子模式

现行结构是「过程式涌现」，没有形式规范。任何重写都存在
「我以为等价、实际不等价」的风险。

影子模式让差异**可见**，而不是靠指标掉了再回头猜。

---

## 第十部分 · 验证方案

### 10.1 三层指标

| 层 | 指标 | 当前基线 | 判据 |
|----|------|----------|------|
| L1 合成用例 | exam 291 条 | 271 | 不低于 271 |
| L2 口语短句 | 口语 40 条首位 | 13/40 | 不低于 13/40 |
| L3 黑盒刻度 | 同输入首位对齐率 | 未采集 | 逐条对齐率 |

L1/L2 是内部回归，L3 是黑盒刻度。**没有 L3，无法判断「对齐」**。

### 10.2 公理检验

| 公理 | 检验方法 |
|------|----------|
| 全序 | 打乱候选数组重跑，结果必须一致 |
| 相对性 | 同一词在 `nihao` / `niha` 下必须落在不同层 |
| 单调性 | 给候选加用户词标记，位次不得变差 |
| 可扩展性 | 新增一路来源，`git diff` 只出现在生成侧 |

前两条可以用单元测试固化。后两条靠 code review 与结构约束。

### 10.3 性能

现行：35 步 + 8 次重排，其中 4 处重复计算同一事实。

新结构：生成 + 一次归约 + 一次排序。归约阶段不做任何索引查询
（拼音由生成侧带出，见 7.5），所以不存在「每候选一次查询」的风险。

净变化预估：
- 减少：8 次重排 → 1 次排序，减少 7 次数组置换
- 减少：4 处重复计算合并为 1 次 `InputShape` 构造
- 增加：`WordEntry` 多带一个 `Arc<str>`（指针复制，无分配）
- 增加：`match_of` 的音节切分（候选拼音切分可缓存在 `Candidate` 上，
  或直接由生成侧连同拼音一起给出）

需实测的点：音节切分的分配开销。若明显，改为生成侧预切分
（`pinyin` 存为 `Vec<Arc<str>>` 而非 `String`）。

---

## 第十一部分 · 边界与不变量

### 11.1 必须保留的不变量

1. **常规英文区恒在中文之后**：`Foreign` 是最低层，天然满足（仅 B5 强英文精确词 `ENG_EXACT_TOP` 如 `api`/`email`/`hello` 走显式子级前置）。
2. **短语 / 日期恒在首位**：`explicit_rank` 子级 `0/1`。
3. **用户置顶恒在首位**：`explicit_rank` 子级 `2`。
4. **分页与总候选集一致**：`truncate` 仍在最后，不影响。
5. **单字在单音节输入下优先**：1 音节输入的单字是 L1，多字词是 L2/L3。
6. **特殊注入来源语义保留**：`PREFIX_WORD`（≥3 音节前 2 音节常用词引导）与 `split_cn_en_camel`（`woshiEric` 驼峰混输）在 Phase B 接入前明确归属 `explicit_rank` 或独立保留位。

### 11.2 明确不做

- 不改 `sentence.rs` 的整句解码算法（只新增薄封装 `decode_meta` 透出已算出的 `segment_count` 与 `clean_coverage`）。
- 不改词库 `.bin` 数据结构（拼音由生成侧按 §7.5 零额外索引查询带出）。
- 不引入机器学习排序（无可训练数据，且不可解释）。
- 不改对外 C ABI 与 `CandidateMeta` 的既有字段（只增不改）。

### 11.3 已知的取舍

**取舍一：字典序 vs 加权**

字典序牺牲了「跨层跨越」的能力。例如某个 L3 候选词频极高，
在加权模型下可能压过 L2 低频词；字典序不允许。

这是有意的：**匹配关系是比词频更强的意图信号**。
用户打了 `nihao`，即使「你好世界」词频极高，也不应排在「你号」之前。

**取舍二：一次排序 vs 多阶段精排**

搜索引擎用「召回 → 粗排 → 精排」多阶段，每阶段有独立模型。
本设计只有一次裁决，因为候选量级不同（40 条 vs 数百万）。
40 条候选不需要粗排。

若未来候选量级上升到数百，可以引入粗排，但那是另一个设计。

**取舍三：前缀扩展的拼音粒度**

在不改 `.bin` 的前提下，前缀扩展段（非精确命中、非补全）只有 `pinyin_len`，
缺失完整拼音字符串，统一归为 L3（按 `pinyin_len - input.len()` 估算 `extent_rank`）。
待 Phase C 确认收益后再决定是否扩展索引值。

---

## 附录 A · 与现行代码的逐项映射

| 现行位置 | 现行作用 | 新结构中 |
|----------|----------|----------|
| 2542–3398 `query_all` | 生成 + 排序混排 | 拆为 `gather()` + `reduce()` + `rank()` |
| 197–206 `default_tier_for` | 来源 → tier | 删除 |
| 213–224 `Candidate` | 5 字段 | 8 字段（+ pinyin/form/cluster/attest，− tier） |
| 3435–3632 `apply_word_len_match` | 8 元组排序 | `match_of` + `rank` |
| 3412–3433 `apply_fuzzy_demote` | Fuzzy 后置 | 删除（L5 天然在后） |
| 3641–3718 `apply_completion_boost` | Completion 插位 | `length_bias` 承担 |
| 3730–3800 `apply_ambiguous_split_boost` | 隔音词搬第 5 位 | 删除（L2） |
| 1712–1801 `apply_context_boost` | bigram 重排 | `context_rank` 维度 |
| 1868–1893 `apply_pin_boost` | 置顶 | `explicit_rank` |
| 1900–1935 `apply_demote` | 降权旋转 | `demoted_rank` |
| 1943–1964 `apply_sort_mode` | 用户偏好 | `pref_rank` |
| 3244 单字母分区 | 单字优先 | `Shape` 的特例 |

## 附录 B · 代码规模预估

| 部分 | 现行 | 新结构 |
|------|------|--------|
| `query_all` | 856 行 | `gather` 约 600 行（去掉排序逻辑） |
| 排序 | 约 700 行（8 个 apply_*） | `reduce` 约 150 行 + `rank` 约 60 行 |
| Match | 无 | 约 120 行（含音节前缀判定） |
| 测试改写 | 55 处 | 约 40 处需要改写（多数改为断言 `match_level`） |

净减少约 400 行，且排序逻辑集中在一处。

---

## 附录 C · Phase A 接口骨架

只给签名与数据结构，不含实现逻辑。
原则：**只增不改**——旧链一行不动，新增物全部是旁路。

### C.1 新增枚举

```rust
/// 匹配关系。全序，声明顺序即优先级（越小越优）。
/// 不带来源语义：由「输入切分」与「候选拼音切分」的关系决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    Exact,       // 切分相等
    Shape,       // 字数命中目标，切分不同
    Extension,   // 输入是候选的音节前缀
    Truncation,  // 候选是输入的音节前缀
    Inferred,    // 需变体规则
    Composed,    // 多段拼凑
    Foreign,     // 拼音规则不适用
}

/// 候选形态。簇内排序用，不参与跨簇比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    Text,
    Emoji,
    Symbol,
}
```

### C.2 输入特征

```rust
pub struct InputShape {
    pub raw: String,
    pub norm: String,
    pub splits_clean: bool,
    pub best_split: Vec<String>,
    pub tail: String,
    pub all_splits: Vec<Vec<String>>,
    pub target_lens: Vec<usize>,
    pub syllable_count: usize,
}

impl InputShape {
    /// 一次性构造。替代现行 4 处重复计算
    /// （2555 / 2575 / 3441 / 3446）。
    pub fn build(raw: &str) -> Self;

    /// L2 判据：候选字数是否命中目标。
    pub fn accepts_len(&self, chars: usize) -> bool;

    /// 切分比较：用于 L1 / L3 / L4 判定。
    pub fn split_eq(&self, cand_syls: &[String]) -> bool;
    pub fn is_prefix_of(&self, cand_syls: &[String]) -> bool;
    pub fn starts_with(&self, cand_syls: &[String]) -> bool;
}
```

### C.3 证据

```rust
pub struct Attestation {
    pub match_level: Match,
    pub extent_rank: u8,
    pub segment_count: u8,
    pub char_count: usize,
    pub freq: Option<u32>,
    pub user_tier: u8,
    pub common_order: Option<u32>,
    pub is_user: bool,
    pub is_common: bool,
    pub has_rare: bool,
    pub context_lift: u8,
    pub explicit_rank: u8,
    pub demoted: bool,
}

impl Attestation {
    pub fn credible(&self) -> bool;
    /// 排序键。返回可比较的元组，不含 I/O。
    pub fn rank_key(&self, pref: u8, form: Form, exact_has_credible: bool) -> impl Ord;
}
```

### C.4 三阶段入口

```rust
impl Engine {
    /// Layer 1 · 生成。只产出候选，不排序、不插位。
    /// 与现行 query_all 的前半段等价，但去掉所有 apply_* / retain / truncate。
    fn gather(&mut self, shape: &InputShape) -> Vec<Candidate>;

    /// Layer 2 · 归约。填充 Attestation。
    /// 不做索引查询（拼音已由生成侧带出），只做纯计算。
    fn reduce(&self, cands: &mut [Candidate], shape: &InputShape);

    /// Layer 3 · 裁决。全链唯一一次排序。
    fn rank(&self, cands: &mut Vec<Candidate>);
}
```

### C.5 词库与整句侧新增（不改 .bin 与解码算法）

```rust
pub struct WordEntry {
    pub word: String,
    /// 精确段为 Some(key)，前缀扩展段为 None（补全段由 completion_candidates_meta 填完整拼音）
    pub pinyin: Option<Arc<str>>,
    pub pinyin_len: usize,
    pub freq: u32,
    pub layer: u8,
}

impl Dictionary {
    /// 与 query_with_split 同语义，额外返回词条元数据。
    /// 旧 API 保持不动（内部薄包装）。
    pub fn query_with_split_meta(&self, prefix: &str) -> (Vec<WordEntry>, usize);
    pub fn completion_candidates_meta(&self, input: &str, limit: usize) -> Vec<WordEntry>;
}

pub struct SentenceEntry {
    pub text: String,
    pub score: f64,
    pub segment_count: u8,
    pub clean_coverage: bool,
}

// sentence.rs
pub fn decode_meta(input: &str, prev: Option<&str>, limit: usize) -> Vec<SentenceEntry>;
```

### C.6 开关与影子探针

```rust
impl Engine {
    pub fn set_ranking_v2(&mut self, enabled: bool);
    pub fn ranking_v2(&self) -> bool;
}

/// 影子模式产物：新旧两套顺序 + 逐条差异归因。
pub struct RankDiff {
    pub input: String,
    pub old_order: Vec<String>,
    pub new_order: Vec<String>,
    /// (词, 旧位次, 新位次)
    pub changed: Vec<(String, usize, usize)>,
    /// 每条差异的归因，如 "match: Extension -> Exact"
    pub explanation: Vec<String>,
}

impl Engine {
    /// 不改变当前候选，只返回对比结果。供离线统计使用。
    pub fn diff_ranking(&self, input: &str) -> Option<RankDiff>;
}
```

### C.7 Candidate 字段变更

```rust
struct Candidate {
    text: String,
    pinyin: Option<String>,   // 新增：生成侧填充
    form: Form,               // 新增
    cluster: String,          // 新增
    source: Source,           // 保留：仅诊断
    reason_mask: u32,         // 保留：仅诊断
    attest: Attestation,      // 新增：替代 tier
    // tier: u8,              // 删除（Phase C 执行）
}
```

### C.8 Phase A 落地顺序

每步完成后都可独立验证，不依赖后续步骤。

| 步 | 内容 | 验证方式 |
|----|------|----------|
| 1 | `InputShape::build` + 三个比较方法 | 单元测试：对 20 个代表性输入断言切分结果 |
| 2 | `Match` + `match_of` | 单元测试：每个层给 2 个正例 + 1 个反例 |
| 3 | `WordEntry` + `query_with_split_meta` | 与 `query_with_split` 逐条比对，词序与词数必须一致 |
| 4 | `Attestation` + `reduce` | 对现有 291 条用例断言 `match_level` 分布合理 |
| 5 | `rank`（不接入） | 与新链并行跑，用 `diff_ranking` 统计差异 |
| 6 | `diff_ranking` 探针 | 离线跑全量，输出差异清单 |

**步 3 是唯一有回归风险的步骤**（改到 `dictionary`），必须做逐条比对。
其余五步都是新增代码，旧链不感知。

### C.9 Phase A 的验收条件

- 旧链行为零变化：三套指标与基线完全一致
- 差异清单 100% 可归因：每条差异能说出是哪个维度导致
- 公理 1（全序）成立：打乱输入数组后 `rank` 结果不变
- 公理 2（相对性）成立：同一词在不同输入下落在不同层

四条全过才进入 Phase B。任一不过，回 Phase A 修 Match 定义。

---

## 结语

这个设计的核心是一句话：**候选的顺序应该由「它和输入的关系」决定，
而不是由「它在代码里是怎么被生成的」决定。**

现行结构的每一个补丁（fuzzy demote / completion boost / ambiguous split）
都是在修正「来源被误当成身份」带来的偏差。把它们统一到 Match，
补丁自然消失——不是因为被合并，而是因为**它们要解决的问题不再存在**。

