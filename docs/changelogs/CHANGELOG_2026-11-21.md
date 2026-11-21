# CHANGELOG 2026-11-21

## 整句解码改为字符级格子，混拼长句可用

### 背景

整句通道原先「先切音节、再在音节图上走 Viterbi」。全拼长句已经能用（15 条长句 13 条首位命中，纯全拼 13/13），但混拼输入整段崩坏：

| 输入 | 修复前 | 期望 |
|------|--------|------|
| `wmzaizheliganshenme` | 瓦在这里肝肾 | 我们在这里干什么 |
| `zhongguormyinhang` | 中国热银行 | 中国人民银行 |
| `wmytqkdy` | 无候选 | 我们有没有空 |

候选层没有问题：`span_candidates` 对 `jige` 给出「几个」、`zai` 给出「在」。崩坏发生在切分：`wm` / `tq` 不是合法音节，容错切分把它们掰成 `wa`（瓦）、`ti`（提），整句随之失真。

### 根因

音节图里「切分先于查词」。切分可以把 `zhel` 替换成 `zhen`（编辑距离 1），再继续消费后面的字母——解码器于是能把输入重组成任意高频词。实测 `nimjigerenzaizheli` 解成「你们几个认真癌症里」：`认真` 由 `ren`+`zhel→zhen` 拼出，`癌症` 由 `ai`+`zhel→zheng` 拼出，字母根本对不上。

只给音节图加「简拼伪段」不够：伪段惩罚从 2.5 提到 20，错词分数一分未变——因为那条错误路径不经过伪段。

### 改动

- `engine/src/sentence.rs`
  - 新增 `char_graph`：节点是输入串的位置，边覆盖一段连续字母，**词必须对得上这几个字母的读音**，解码器不能跨段重组输入
  - 三档候选来源：整段全拼（`span_candidates`）→ 混拼（`abbrev_span_candidates`，仅短段）→ 单段纠错（`near_syllables`，仅整段读不通时）
  - 混拼段惩罚 `ABBREV_PENALTY = 4`（nats），纠错代价 `EDIT_COST = 8`
  - 旧路径保留在 `TS_SENTENCE_LEGACY=1` 下，供对照回归
  - 容错切分支持伪段（`^` 前缀标记）+ `MAX_SEGMENTATIONS` 配额保护，伪段必须先问词库、无候选不生成边
- `engine/src/dictionary/mod.rs`
  - 新增 `Dictionary::abbrev_span_candidates` / `dictionary::abbrev_span_candidates`：混拼段查词（简拼精确键 → 段匹配区间扫描 → 常用词/用户词遍历），带格子缓存
  - 常用词表的人工 rank 不当词频用：能从系统层取到同一词条真词频就用，取不到给 `COMMON_UNKNOWN_FREQ = 2500`
- `engine/examples/probe_sentence.rs`：新增混拼用例组与单独统计
- `engine/examples/probe_abbrev.rs`：混拼段查词探针

### 指标

| 指标 | 改动前 | 改动后 |
|------|--------|--------|
| 全拼长句（15 条） | 13/15 | 13/15 |
| 混拼用例（4 条） | 0/4 | 3/4 |
| 真实文章考试 | 272/291 | 272/291 |
| 单元测试 | 392 通过 | 392 通过 |

混拼 3/4：`wmzaizheliganshenme` → 我们在这里干什么、`wmmingtianqubeijing` → 我们明天去北京、`nimjigezaizheliganshenme` → 你们几个在这里干什么。
`nimjigerenzaizheli` 仍解成「你面积个人在这里」（`nim` 段与 `ni`+`mji` 段同分区间竞争，靠 bigram 决出），列为已知限制。

### 已知限制

- `nim` 这类「音节前缀 + 声母」混排与「少打一个字母」的纠错在打分上同分，靠 bigram 决出，数据稀疏时会选错
- 全拼段与混拼段不能混在同一条边里（`wmzai` 这种跨词段不生成边），只能靠相邻边拼接
