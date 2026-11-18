# 文章模式考试（Article Exam）

> 用真实文章逐词模拟输入，测「真实语料下候选排序卡在哪」。
> 工具：`engine/examples/article_exam.rs`　用例：`engine/tests/article-real-2026-11-19.json`

---

## 1. 与 exam.rs 的分工

| 工具 | 输入 | 测什么 | 局限 |
|---|---|---|---|
| `exam.rs` | 人造用例（TSV + 断言） | 单条输入的确定性行为、回归通过率 | 用例是人挑的，测不到真实语料的分布 |
| `article_exam` | 真实文章 + LLM 生成拼音 | 连续输入下的排名分布、上下文影响、简拼/混拼的真实命中率 | 只测量不断言，miss 不返回非零 |

人造用例里 `nh`/`zg`/`bj` 这类词都在常用词表里，过滤规则根本碰不到；
真实文章一打「产业链 → cyl」，立刻暴露候选被 `combo_common_only` 过滤掉。

---

## 2. 用法

```powershell
cargo run --release --manifest-path engine/Cargo.toml --example article_exam -- `
  --article engine/tests/article-real-2026-11-19.json `
  --out tmp/article-real-result.jsonl `
  --limit 20
```

| 参数 | 说明 |
|---|---|
| `--article <path>` | 必填，文章 JSON |
| `--out <path>` | 结果 JSONL，默认同目录 `result.jsonl` |
| `--dict <path>` | 词库，默认自动探测 `resources/system_dict.db` |
| `--limit <n>` | 每词取前 n 个候选（默认 20，上限 100） |
| `--no-context` | 关闭上下文联想，做对照组 |
| `--miss-fill` | miss 时上屏首个候选（模拟用户将就选一个，保持上下文连贯） |
| `--no-isolate` | 关闭段间用户词清理（测「上一段教会它没有」时用） |
| `--user-dict <path>` | 用户词库落点，默认 `tmp/article_exam_user_dict.db` |

退出码：0 = 跑完（有 miss 也是 0）；2 = 参数/环境问题。

---

## 3. article.json 格式

```json
{
  "source": {
    "title": "文章标题",
    "url": "https://example.com/xxx",
    "fetched_at": "2026-11-19T01:00:00"
  },
  "segments": [
    {
      "index": 1,
      "mode": "full",
      "text": "因而在这种背景下，如何提升产业链的韧性和安全水平。",
      "words": [
        { "text": "因而", "pinyin": "yiner" },
        { "text": "产业链", "pinyin": "chanyelian" }
      ]
    }
  ]
}
```

`mode` 枚举：`full` 全拼 / `short` 简拼（三字以上也照打，产业链 → cyl）/ `mixed` 混拼 / `typo` 手误。

**拼音由 LLM 生成，工具不做拼音转换**（不引 pypinyin）。拼音写错会算到引擎头上，
所以写 JSON 时要按上下文核多音字（长江 → changjiang，不是 zhangjiang）。

拆词按词不按字。「因而在这种背景下」→ 因而 / 在 / 这种 / 背景下。

---

## 4. result.jsonl 格式

一行一个词：

```json
{"seg":2,"mode":"short","word":"产业链","pinyin":"cyl","rank":null,"status":"miss","top3":["cylinder"],"src3":["English"]}
```

`status` 分级：`ok` 1-3 / `late` 4-9 / `page` >9 / `miss` 前 20 无。

stdout 输出按 segment、按 mode 的分布，外加「问题明细」（ok 不列，含前 3 候选与来源）。

---

## 5. 两个必须知道的设计点

### 5.1 段间必须清用户词（否则复现不出简拼缺陷）

`select_candidate` 内部会 `learn(拼音, 词)`，把选中的词写进用户词表。
于是：

1. 段 1 全拼打 `chanyelian` 选中「产业链」→ 学习表里多了一条；
2. 段 2 简拼打 `cyl` → `combo_common_only` 用 `is_user_word` 放行 → **命中了**。

实测第一版工具就踩了这个坑：`cyl` 拿到 Combo 候选、rank 1，MISS 完全消失。
真实用户当然会享受到这个学习效果，但考试要测的是「简拼本身行不行」，
所以默认每段开始前 `dictionary::clear_user_words()`。

同时工具把用户词库指到 `tmp/article_exam_user_dict.db` 并在启动时删除重建，
避免把测试学习写进 `%APPDATA%/taishen-ime/user_dict.db` 污染真实词库。

### 5.2 必须装 bigram.db，否则上下文联想几乎不生效

`apply_context_boost` 的候选加分来自两路：Engine 内部表（内置几十条 + 学习表）
和全局 `bigram.db`。生产端在 ActivateEx 里装 bigram 表，测试工具必须自己装：

```rust
let table = dictionary::load_bigram(&bigram_path, 2, 64);
dictionary::install_bigram(table);
```

不装时实测「政治 → renwu」位次不变；装上后同一样例从 #2 提到 #1。

---

## 6. 上下文生效验证（对照实验）

同一份 `article-sample.json` 第 3 段「政治人物在社会中坚持社会主义」：

| 运行方式 | 「人物」renwu 排名 |
|---|---|
| 默认（上下文开） | **#1**，top3 = 人物 / 任务 / 人武 |
| `--no-context` | **#2**，top3 = 任务 / 人物 / 人武 |

说明 `select_candidate → last_committed → 下一词排序` 这条链路在工具里真实生效，
不是「每词独立打一遍」的假连续输入。

---

## 7. 实测结果（2026-11-19）

来源：求是网政论 + 博客园技术博客 + 小众软件论坛口语（见 JSON 的 `source`）。
共 5 段 69 词，`--limit 20`，隔离开，上下文开。

| mode | 词数 | ok | late | page | miss | ok 占比 |
|---|---|---|---|---|---|---|
| full（书面语/技术） | 29 | 25 | 2 | 2 | 0 | 86% |
| short（政策简拼） | 15 | 3 | 1 | 2 | 9 | 20% |
| mixed（口语混拼） | 13 | 11 | 0 | 0 | 2 | 85% |
| typo（手误） | 12 | 8 | 1 | 1 | 2 | 67% |
| 合计 | 69 | 47 | 4 | 5 | 13 | 68% |

### 结论

**1. 全拼基本没问题，简拼是最大短板。** 书面语段 29 词里 25 个首选，简拼段 15 词只有 3 个。

简拼 miss 明细（前 3 候选）：

| 词 | 拼音 | 前 3 候选 |
|---|---|---|
| 产业链 | cyl | cylinder(English) |
| 供应链 | gyl | gyl(English)，无中文 |
| 先进制造业 | xjzzy | xjzzy(English)，五字词全无 |
| 现代化 | xdh | xdh(English) |
| 安全水平 | aqsp | aqsp(English) |
| 骨干 | gg | 刚刚(Combo) / 改革(Combo) / 各个(Combo) |
| 体系 | tx | 同学(Combo) / 提醒(Combo) / 天下(Combo) |
| 韧性 | rx | 人形机器人(Combo) / rx(English) |
| 提升 | ts | 1795020523(Datetime)，被时间戳模式吞了 |

**2. `cyl → MISS` 已复现**，与任务书描述一致：产业链在词库里有，但纯声母串走
`combo_common_only`，词不在常用词表就被过滤。

**3. 单字也有缺口。** `jian` 总共只有 5 个候选（见 / 间 / 剑 / 建 / 剪辑），
「键」不在其中——这是词库或单字排序问题，不是简拼问题。

**4. typo 会被纠错抢占。** `jiucup`（纠错，o 打成 p）前 3 是「纠错码(Correction)/
九寸怕/久存怕」，目标词排 #16；`zhinengg`（多打一个 g）首位被「只能(Correction)」占了。
纠正输入被纠错链路抢位，是 typo 场景的主要退化点。

**5. 常用简拼混拼表现正常。** 口语段 `wd → 我的`、`zm → 怎么`、`j → 就` 都是首选，
说明「常用词走简拼」这条路是通的，问题集中在「非高频词 + 纯声母」这一格。

---

## 8. 已知限制

1. **miss 不上屏**：miss 时 `last_committed` 停在上一个成功词，与真实「打错了重打」有偏差。
   需要连续上下文时加 `--miss-fill`（上屏首个候选，模拟将就）。
2. **只看前 `--limit` 个候选**：默认 20，超出算 miss。想分辨「排第 30」还是「根本没有」要调大。
3. **拼音不校验**：LLM 写错拼音（多音字、生僻音）会被记成引擎缺陷，出报告前要复核。
4. **段间隔离是有意的**：测的是「冷启动 + 段内学习」，不是长期用户的词频状态；要测后者用 `--no-isolate`。
5. **同一进程单例**：`DICT` 是全局单例，不能并行跑多篇文章，串行跑即可。
6. **异步加载窗口**：`is_ready` 只等系统词库，工具额外等 500ms 让领域词库合并；
   极慢磁盘上同一篇文章可能仍有轻微浮动。

---

## 9. 写用例的工作流

1. 用 anysearch extract 抓 1-3 篇公开文章（不同文体），记录 URL 与时间；
2. 每种文体摘 1-2 段，按词切分，按上下文写拼音（多音字要核）；
3. 先用 `engine/tests/article-sample.json` 自测工具行为是否正确；
4. 跑真实文章，看 stdout 的问题明细定位缺陷；
5. 需要判断是不是上下文问题时，加 `--no-context` 再跑一遍对比。
