# 文章模式考试工具（Article Exam）

> 给新会话的完整作业指导。读完直接开工。
> 工作目录：`E:/AllinDeepSeek/taishenIME`

---

## 0. 为什么要做这个

现有 `exam.rs` 是**人造用例**（291 条，我挑的场景），跑通过率修 bug。它测不到真实语料的分布。

实测对比（手工验证过一次）：

| 文体 | 全拼 | 简拼 | 混拼 |
|---|---|---|---|
| 书面语 17 词 | 2 条问题 | **10 条** | 8 条 |
| 技术文档 11 词 | 0 条 | **9 条** | 7 条 |
| 口语 7 词 | 1 条 | 4 条 | — |

真实语料立刻暴露了两个类型的缺陷：

1. **代码回归**：`cyl`（产业链）在词库里有、索引也建了，但被上一轮加的 `combo_common_only` 过滤掉了
2. **词库缺口**：`安全水平` `时代课题` `背景下` 词库里根本没有

人造用例完全测不到——因为 `nh`/`zg`/`bj` 这些词都在 `common_words` 表里，过滤不掉。

**目标**：让 LLM 模拟真人打真实文章，看真实使用中卡在哪。

---

## 1. 工具规格

### 1.1 文件位置

新建 `engine/examples/article_exam.rs`（独立于 exam.rs，不要改 exam.rs）。

### 1.2 输入：`article.json`

```json
{
  "source": {
    "title": "新闻标题",
    "url": "https://example.com/xxx",
    "fetched_at": "2026-11-19T10:00:00"
  },
  "segments": [
    {
      "index": 1,
      "mode": "full",
      "text": "因而在这种背景下，如何提升产业链的韧性和安全水平。",
      "words": [
        {"text": "因而", "pinyin": "yiner"},
        {"text": "在", "pinyin": "zai"},
        {"text": "这种", "pinyin": "zhezong"},
        {"text": "背景下", "pinyin": "beijingxia"}
      ]
    }
  ]
}
```

`mode` 枚举：
- `full` 完整全拼
- `short` 简拼（每词首字母，三字词也照打：产业链 → cyl）
- `mixed` 混拼（常用词简拼、生词全拼）
- `typo` 带手误（少打/多打/邻键错）

### 1.3 执行逻辑（关键）

对每个 segment 的每个 word：

```rust
engine.reset();                                    // 清上一词缓冲（不清 last_committed）
for ch in pinyin.chars() { engine.process_key(ch); }
let n = engine.candidate_count();
let mut rank: Option<usize> = None;
for i in 0..n.min(20) {
    if engine.candidate(i) == Some(expected.as_str()) { rank = Some(i + 1); break; }
}
// 上屏：让上下文联想（context_assoc，默认开启）生效
if let Some(r) = rank { engine.select_candidate(r - 1); }
```

**已确认的两个 API 行为**：
- `select_candidate(i)` 会设置 `last_committed = word` 并触发 `learn_context_pair`
- `reset()` 清 `pinyin_buf` / `candidates`，但**不清 `last_committed`**

所以上面这个顺序能正确模拟「打完一个词、上屏、继续打下一个词」。

候选索引语义：`candidate(i)` 是页内索引，`select_candidate(i)` 也是页内。先 `set_candidate_limit(20)` 再遍历。

### 1.4 输出：`result.jsonl`

一行一个词：

```json
{"seg":1,"mode":"full","word":"因而","pinyin":"yiner","rank":1,"status":"ok","top3":["因而","银耳","印儿"],"src3":["Dictionary","Dictionary","Dictionary"]}
{"seg":1,"mode":"full","word":"背景下","pinyin":"beijingxia","rank":8,"status":"late","top3":["北京西","北经希","北江乡"],"src3":["Correction","Correction","Correction"]}
{"seg":2,"mode":"short","word":"产业链","pinyin":"cyl","rank":null,"status":"miss","top3":["cylinder"],"src3":["English"]}
```

`status` 分级：
- `ok` — 排名 1-3
- `late` — 排名 4-9
- `page` — 排名 > 9（需翻页）
- `miss` — 前 20 候选里没有

### 1.5 汇总输出（stdout）

```
【来源】标题 / URL / 时间
【段1 full】 4 词：ok 2 / late 1 / page 0 / miss 1
【段2 short】 5 词：ok 0 / late 0 / page 1 / miss 4
...
【总计】 ok 6 / late 3 / page 2 / miss 5 （共 16 词）

问题明细：
  [miss] 产业链   cyl         前3: cylinder(English)
  [late] 背景下   beijingxia  前3: 北京西(Correction) / 北经希(Correction) / 北江乡(Correction)
```

### 1.6 命令行

```
cargo run --release --example article_exam -- --article <article.json> [--out <result.jsonl>] [--limit 20]
```

---

## 2. LLM 的职责（不是工具的事）

工具只负责执行。**文章和拼音由你（LLM）准备**：

### 2.1 随机摘录

用 `mcp__anysearch__search` / `mcp__anysearch__extract` 或 `webFetch` 抓 3-5 段公开材料，覆盖不同文体：

| 文体 | 来源示例 |
|---|---|
| 新闻（书面语） | 主流媒体时事报道 |
| 技术文档 | 官方文档、技术博客 |
| 口语 | 论坛、社交媒体公开讨论 |
| 文学/散文 | 公开版权作品节选 |

**必须记录 URL 和抓取时间**，写进 `source` 字段。

### 2.2 生成"真人会敲的拼音"

不是机械转写，要模拟真实习惯：

- **full**：完整拼音（多音字按上下文取音）
- **short**：每词首字母，**三字以上词也照打**（`产业链` → `cyl`，`安全水平` → `aqsp`）
- **mixed**：常用词简拼（我们→wm），生僻词/专名全拼（产业链→chanyelian）
- **typo**：模拟手误——少一个字母（`yiner`→`yine`）、多一个（`yiner`→`yinner`）、邻键错（`nihao`→`nihap`）

**注意**：拼音要贴近真实键盘输入，不要用 pypinyin 机械生成后不管多音字（`长` 在「长江」里是 chang）。

### 2.3 拆词

按词切分（不是按字）。例：

> 因而在这种背景下，如何提升产业链的韧性和安全水平。

拆成：因而 / 在 / 这种 / 背景下 / 如何 / 提升 / 产业链 / 的 / 韧性 / 和 / 安全水平

---

## 3. 验收标准

| 项 | 标准 |
|---|---|
| 编译 | `cargo build --release --example article_exam` 零 error |
| 跑通 | 用一篇 3 段以上的真实文章跑出完整 result.jsonl |
| 上屏生效 | 同一 segment 内，后一个词的候选受前一个词影响（`last_committed`） |
| 复现已知问题 | `cyl` 必须 MISS（产业链被 combo_common_only 过滤） |
| 统计完整 | stdout 输出按 segment 和按 status 的分布 |
| 文档 | 写 `docs/modules/exam/ARTICLE-MODE.md`：用法、JSON 格式、已知限制 |

---

## 4. 约束

1. **不改引擎逻辑**。这是纯测试工具。
2. **不改 exam.rs**。它是独立工具。
3. **拼音由 LLM 生成**，工具不做拼音转换（不引入 pypinyin 依赖）。
4. **来源必须标注**，可追溯。
5. 不引入新依赖（只用 std + 现有依赖）。

---

## 5. 交付

1. `engine/examples/article_exam.rs`
2. 示例输入 `engine/tests/article-sample.json`（一小段，用于自测）
3. 文档 `docs/modules/exam/ARTICLE-MODE.md`
4. 实际跑一次真实文章，把结果摘要写进文档
5. commit，message 用 `[AI:泰深] feat(exam): 文章模式考试工具`

---

## 6. 第一次跑的建议

先用一段简短的技术文档（10-15 个词）验证工具正确性，确认：

- 上屏后 `last_committed` 确实影响下一词候选（对比 `set_context_enabled(false)` 的结果）
- `cyl` 能复现 MISS
- status 分级准确

验证通过后再跑长文章。
