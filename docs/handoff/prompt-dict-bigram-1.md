# 词库侧任务 · 第 1 轮：搭配主表重建

> 用法：新会话开新对话，把本文件全文作为第一条消息发过去。
> 前置：无（可独立开始）
> 产出：`output/bigram.db`（`context_prior` 表），可用搭配对 ≥ 3 万

---

## 背景与现状（实测数据）

泰深输入法的候选排序依赖一张离线搭配表 `context_prior`（相邻词共现统计），
用于「前文参与排序」：上屏「北京」后再输入 `da`，「大学」应因搭配提前。

现状（2026-11-17 实测）：

- `taishenIME/resources/bigram.db` 共 400,000 行
- 档位分布：count=1 有 **394,547（98.6%）**｜count=2 有 3,085｜count=3 有 1,000｜
  count=4/5/6 各 570/428/370
- 消费侧已把档位 1 排除在主排序之外（视为无证据，2 档起才提升），
  所以**真正可用的只有 count≥2 的 5,453 对 / 1,813 个前词**

结果：真实输入里多数相邻词组合命中不了表，搭配增益有限。

## 目标

把 `count ≥ 2` 的可用搭配对从 **5,453 提升到 30,000 以上**，且档位质量不能退化
（不能靠灌低档位数据凑数）。

## 硬约束

1. **所有改动在 `E:\AllinDeepSeek\taishen-dict` 仓库进行**，不要直接改
   `E:\AllinDeepSeek\taishenIME\resources`（那是单向同步的消费端）。
2. 流程必须走完：改源/脚本 → 重建 output → `tools/verify_build.py` 通过
   → `tools/update_version.py` 更新清单 → `tools/sync_to_ime.py` 单向同步。
   `sync_to_ime.py` 会校验产物哈希与清单一致，**不更新清单会直接失败**（这是设计，不是 bug）。
3. 生成侧脚本是 `tools/build_context_prior.py`，先完整读它再动手。
   它已包含：绝对频次→档位映射、虚词前词跳过（STOP_WORDS）、虚词后词降档、
   结构化噪声对剔除（NOISE_PAIRS）、每前词配额（`--per-prev`）、默认 `--min-count 2`。
4. **文档、代码注释、commit 说明中不得出现任何竞品/其他输入法名称**，用中性表述。
5. 不要改引擎排序逻辑（`taishenIME/engine/src/lib.rs` 的 `apply_context_boost`）。
6. 重建前备份 `output/` 下的旧产物。

## 第一步：诊断语料资产（关键，不要跳过）

`tools/build_context_prior.py` 需要 `--corpus <语料目录>`，但仓库内
`sources/` 只有三个脚本（`jieba.py` / `wiki.py` / `__init__.py`），**没有语料数据**。
说明现存的 40 万行是用某批外部语料统计的，那批语料不在仓库里。

请先明确：

1. 本机是否还能找到那批语料（翻 `taishen-dict/tmp/`、其他盘的备份、历史命令记录）。
2. 若找不到，评估 `sources/wiki.py` 能否重新抓取（读它的实现，确认数据源、耗时、是否需要网络）。
3. 把结论写进 `sources/README.md`（或新建）：**记录语料来源、规模、抓取脚本、
   统计命令的完整参数**——这次必须留下可复现记录，否则下次还要重新找。

**这一步的产出决定后面走哪条路**：语料可获取 → 走「统计」；不可获取 → 走「LLM 生成为主」，
但请如实说明，不要假装有语料。

## 第二步：执行

### 路线 A（语料可获取）

按 `build_context_prior.py` 的文档跑：

```
python tools/build_context_prior.py --corpus <语料目录> \
    --dict output/system_dict.db --out output/bigram.db \
    [--top 700000] [--min-count 2] [--per-prev 8]
```

目标：语料量足够让 `count≥2` 突破 3 万。若语料只有几百万字，这一目标达不到——
请给出「语料字数 → 预估 count≥2 对数」的推算，说明差多少。

### 路线 B（语料不可获取）

用 LLM 生成候选搭配对，但**必须走第 3 道防线的代码校验**（幻觉过滤），
且档位只能落在 2-4 档（5-6 档必须留给真实统计，LLM 无绝对频次校准能力）。

生成规格：

- **前词清单**：从 `output/system_dict.db` 取词频 top 3000，剔除单字虚词
  （的/了/是/在/和/就/都/而/及/与/或/个/这/那/什么/怎么 等）
- **每个前词**：8-15 个惯用后继词，只允许用「已存在于 system_dict.db 的词」
- **标注档位**：极常见→4 / 常见→3 / 一般→2（不允许 5、6）
- **输出格式**：TSV `prev\tnext\ttier`，落在 `tmp/llm_bigram.tsv`

生成后必须过三道代码校验（写在脚本里，不能靠 prompt 自我约束）：

1. **白名单**：`next` 与 `prev` 都必须存在于 `system_dict.db` 词表，否则丢弃
2. **档位封顶**：`tier = min(tier, 4)`
3. **跨行去重**：同一 `(prev, next)` 只保留最小档位（保守）

然后把 TSV 转成 `context_prior` 行，与语料统计结果**并集**（若有），
冲突时取 min。

## 验收标准

- `tools/verify_build.py` 通过
- `count≥2` 的对数 **≥ 30,000**（达不到就如实说明实际能到多少、缺什么）
- 档位分布：`count=1` 占比不得回升（可以更少，不能更多）
- 抽样 20 条搭配对人工确认语义合理（不能出现「的 的」「了 了」这类噪声）
- 在 IME 侧 `engine` 目录跑 `cargo run --release --example probe_context`，
  真实案例提升数 **> 0**（探针会打印每个案例的 off/on 位次差）
- 同步并部署到 `%LOCALAPPDATA%\TaishenIME`，哈希对账一致

## 交付

- commit（taishen-dict + taishenIME 各一次，说明数据来源与档位分布前后对比）
- 一份简短报告：语料来源 / 统计命令 / 前后分布 / 抽样结果 / 未达标的原因

## 参考资料

- 候选体系总设计：`taishenIME\docs\modules\candidate-ranking\DESIGN.md`
  （第七章「搭配证据」是要动的部分；第五章是排序链全貌）
- 词库协作硬约束：`taishenIME\taishenIME.md` →「词库协作（硬约束）」
- 消费侧逻辑：`engine/src/lib.rs` 的 `apply_context_boost`（约 1420 行）
- 验证探针：`engine/examples/probe_context.rs`
