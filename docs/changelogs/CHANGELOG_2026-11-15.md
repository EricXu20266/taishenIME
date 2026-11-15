# CHANGELOG 2026-11-15

## 纠错：漏打字母容错 + 纠错候选豁免词长分区

### 现象

快速打字漏一个字母时，纠错结果要么不出，要么被冷僻词压在后面：

| 输入 | 应出 | 修复前 |
|------|------|--------|
| `nihap` | 你好 | 第 14 位 |
| `keyyi` | 可以 | 第 11 位 |
| `meiganxi` | 没关系 | 不出 |
| `xinashi` | 显示 | 第 2 位（首位「希腊式」） |

### 根因

1. **词长分区覆盖纠错位置**：`apply_word_len_match` 用输入串估目标词长，`nihap`
   被估成 3 字，2 字的「你好」与 3 字冷僻词分在不同组，被降到第 14 位。
2. **没有插入变体**：变体生成只有邻键替换、相邻换位、删除三种，`gan → guan`
   （漏打 u）生成不出来。
3. **触发判据用候选数量**：前缀命中多的输入（如 `zhongg` 443 条）整块纠错被跳过。

### 改动

- `engine/src/correction.rs`：新增 `insertion_variants`——每个位置插一个字母，
  只保留「插入后能完整切分且首音节不变」的变体，上限 24。
- `engine/src/lib.rs` `query_all`：插入变体分支排在邻键纠错之后，触发条件为
  「输入不像话（切不干净或前 5 位无常用词）+ 前 3 位无常用词」。
- `engine/src/lib.rs` `apply_word_len_match`：`Source::Correction` 且命中常用词表的
  候选不参与词长分组——输入切不干净时，估出来的词长本身是错的。

### 效果

| 输入 | 修复后 |
|------|--------|
| `nihap` | 你好 第 1 |
| `keyyi` | 可以 第 1 |
| `meiganxi` | 没关系 第 1 |
| `xinashi` | 显示 第 1 |

`zhongguo` / `zhongg` / `hello` / `wia` / `shou` 均无回归。

### 性能

插入变体放在邻键纠错之后，并用「前 3 位是否已有常用词」门控：
`xinashi` 的邻键纠错已给出「显示」，不再跑插入（否则 30ms → 180ms）。

实测整串输入累计耗时（release）：`logn` 49.6→41.4ms、`xinashi` 29.6→20.5ms、
`meiganxi` 34.6→24.3ms。

### 测试

`correction` 19、`word_len` 4、`sort_mode` 8、`demote` 4、`compose` 9 全绿；
`candidate` 组 25 通过 / 1 失败（`test_emoji_candidate_appended`，HEAD 上同样失败）。

### 遗留

`mignt → 明天` 需要「换位 + 补两个字母」，单处编辑覆盖不到，属下一档（音节级 typo）。
