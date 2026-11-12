# CHANGELOG 2026-10-31

## 引擎审计优化（A / B / C / D 四批）

> 依据 `docs/modules/ENGINE-AUDIT-20261031.md`。每批独立提交、独立验收。
> 引擎 lib 测试从 341 项增至 358 项，全部通过。

### A1 统一去重集合（P0-3）

- `query_all` 与 `query_all_shuangpin` 各维护一份与候选同步的 `cand_seen`（HashSet）
- 13 处线性 `contains` 改为集合查询，长串输入不再叠加成 O(n²)
- 截断到排序预算后重建 seen，保持与原 `contains` 语义等价

### A2 原地稳定分区（P0-2）

- `apply_pin_boost` / `apply_sort_mode` / `apply_word_len_match` 改稳定排序，不再建新 Vec
- `apply_demote` 改原地分区 + `rotate_left`，**顺带修正原实现丢词与尾部重复**：
  非降权词多于 11 条时，原代码会覆盖掉中间若干词并在尾部留下重复项
- `apply_context_boost` 改下标排序 + `permute_range`（此处排序键仍为 `lift - i`，C1 单独改为权重降序）
- 五段后处理不再克隆候选文本

### A3 排序参数外置（§4.9）

- 新增 `engine/src/tuning.rs`：6 个参数，越界钳制 + 缺省回退 + 不热更新
- 读取优先级：`TAISHEN_TUNING` > 可执行文件同目录 > `%APPDATA%/taishen-ime`
- 新增 5 个单测（默认值 / 覆盖 / 越界 / 缺失 / 非法类型）
- 文档：`docs/modules/ENGINE-TUNING.md` + `tuning.example.json`

### B1 候选来源 side-table（P0-1）

- 新增 `Source` 枚举与 `Engine.candidate_src`，与 `all_candidates` 严格等长
- 新增 `publish` / `publish_with_src`，成为候选产出的唯一发布口
- 删除 `english_candidate_pos` / `phrase_candidate_pos` / `datetime_candidate_pos`
- Emoji 插入不再手动修正位置字段；`is_english_candidate` 等改查来源表
- 五个 `apply_*` 增加来源表参数，`permute_range` 同步重排文本与来源

### B2 模式分发表（P2-1）

- 新增 `Mode` 枚举与 `detect_mode` / `apply_mode`，8 个平铺 if 分支压成一个 match
- 拆字反查无候选返回 false 回落常规路径，语义与原实现一致
- 新增一种输入模式 = 一个变体 + 一行判定 + 一个分支

### B3 汉字判定区间化（P2-3）

- 新增 `is_hanzi`，只认 `4E00-9FFF` / `3400-4DBF` / `F900-FAFF`
- 全角标点、拉丁扩展字母（é/ü）、emoji 不再计入「汉字数」

### C1 上下文权重纯加分（P1-3）

- 排序键从 `(lift - i)` 改为按权重降序、同权重保持原位置
- 原实现里位置项会压过权重：第 30 位命中权重 20 的搭配词排不过首位权重 5 的词
- 更新 3 个基于旧语义的测试；新增长列表权重生效、等权重保序两个用例
- 12 组「前文 + 输入」基线探针改前 / 改后输出逐字节一致

### C2 输入质量分类（P1-1，结论：条件未启用）

- 新增 `InputQuality` 与 `classify_input`（完整拼音 / 混合 / 纯声母 / 前缀 / 需纠错）
- 尝试用质量替代 spelling / deletion 的「候选数量」条件，真实词库实测回归：
  - `xinashi`（Exact，词库命中冷僻词「希腊式」）丢失「显示」
  - `logn` / `nihap` / `rung`（Mixed）丢失「龙」等正确纠错
- 结论：质量判据保留为诊断基础设施，触发条件维持原实现，依据写进代码注释
- 32 例候选探针输出与改动前逐字节一致

### D1 词库路径收敛（P2-2）

- 合并 `DICT_PATH` / `USER_DICT_PATH` / `DOMAIN_DICT_PATH` / `PENDING_DOMAIN` 为一个 `DictPaths`
- 保留 `DICT_READY` / `DICT_GENERATION` 两个 Atomic（无锁高频读，不塞进 Mutex）
- 显式约定锁顺序 **DICT → PATHS**，与领域加载线程一致
- `pending_domain` 先 take 再判断，避免在 if let 块内持有路径锁
- 新增并发 init 两次的测试

### 未落地

- **P1-2 统一打分**：按报告 §4.5.2 的启动条件（三条全不满足）不启动，
  方案留档在审计报告内，条件满足时再立项

### 验证

- `cargo build` 通过；`cargo test -- --test-threads=1` 全绿（358 lib + 集成，1 ignored）
- 32 例候选探针、12 组上下文探针逐条对比
- `rustfmt --check`：改动文件无新增格式差异（剩余差异为既有代码）

## IMM32 游戏内上屏后文字保持选中

### 修复

- `imm32_ime.cpp`：组合结束时，在 `WM_IME_ENDCOMPOSITION` 之前补发一条
  `WM_IME_COMPOSITION(GCS_COMPSTR | GCS_COMPATTR | GCS_CURSORPOS)`（此时组合串已清空）。
  GFxIME（LOL 游戏内聊天）自绘组合串，只在 GCS_COMPSTR 渲染路径上重绘组合区；
  只发 ENDCOMPOSITION 时它不抹除上一次组合内容，上屏文字被继续当作组合串高亮
  （现象：上屏后文字保持选中，继续打字即替换）。
- 键盘选词（`ImeToAsciiEx`）与鼠标点候选（`SetClickCallback`）两条上屏路径同步补齐。

### 诊断

- 提交时记录上屏文本与消息入队数；提交窗口期记录宿主对 `GCS_COMPSTR` /
  `GCS_RESULTSTR` / `GCS_RESULTCLAUSE` 的读取（`%LOCALAPPDATA%\TaishenIME\ime_debug.log`，
  需 `config.ini` 的 `debug_log=1`）。

### 验证

- `test_imm32_load` 新增 2 项断言：存在空组合串刷新消息、顺序为
  RESULT → 空 COMPSTR → ENDCOMPOSITION；全部通过。

### TSF 通道同步修复

- `tsf_composition.cpp`：`CommitComposition` 结束组合后，显式把文档选区折叠到上屏文本末尾
  （`ITfRange::Clone` → `Collapse(TF_ANCHOR_END)` → `ITfContext::SetSelection`）。
  部分宿主（经 msctfime 转发的游戏聊天框）在 `EndComposition` 后不主动移动选区，
  组合 range 继续覆盖上屏文字，产生同样的「文字保持选中、再输入即替换」。
- 调试日志新增 `CommitComposition: collapse selection hr=...`。
- 背景：0.1.70 起 IMM32 默认不注册（登录循环 P0 的处置），系统内只有
  `msctfime.ime` 作为 TSF→IMM32 桥，游戏内聊天很可能走 TSF 通道，
  两条通道因此同步修复。

## 词库增量：AI 术语与口语组合词（词库 V2026.10.29.6）

> 依据：竞品词库实测（QQ 五笔 `trans2.dict` 明文 UTF-16 19.4 万词可检索）
> + 现有 `system_dict` / `domains` / `common` 全库查重。改动落在 taishen-dict 源表，
> 走增量重建（不跑全量 pipeline，不动 `system_dict.db` 与 179 MB 预编译索引）。

### 竞品实测结论

QQ 五笔词库已有 `指的`/`牛的`/`完整的`/`没问题`/`不至于`/`说实话` 等口语组合词，
以及 `grok`/`claude`/`copilot`/`sora`/`llama`/`perplexity`/`cursor` 等英文品牌词；
但 `gpt`/`gemini`/`deepseek`/`chatgpt` 空缺——竞品把新品牌词放在英文候选通道，
不在中文词库。说明「口语组合词收进词库、AI 品牌词走英文通道」是行业通例。

### 四个落点

| 内容 | 落点 | 层级 | 条数 |
|---|---|---|---|
| 中文 AI 术语与国产大模型厂商 | `curate/domains/computer_extra.txt` | 领域词 P4（热域） | +25 |
| `指的` | `curate/domains/conversation.txt` | 领域词 P4 | +1 |
| 口语组合词（牛的/完整的/主要是/看一下/说一下/对比一下/总结一下/的意思） | `curate/common_dict.txt` | 常用词 P2 | +8 |
| 英文 AI 品牌/术语（GPT/Gemini/Grok/Claude/Kimi/Qwen/LLM/RAG/MCP 等） | `resources/english_words.txt` | 英文前缀候选 | +26 |

### 分层理由

`zhide` 在系统词库已有「值得」(2948)/「只得」(2914)/「至德」(1463)。
常用词层 P2 整层压过系统词层 P3，`指的` 若进 P2 会把「值得」挤出首位，
故放领域层 P4（排在系统词之后，翻页可及）。其余口语词同音无强冲突，进 P2 直接置顶。

### 验证

- `domains.db` 26/26 命中（`大模型训练 damoxingxunlian`、`思维链 siweilian`、`指的 zhide`）
- `common.db` 8/8 命中，rank 2036-2043，条数 2036 → 2044
- 英文前缀模拟查询：`gpt`→GPT、`gemi`→Gemini、`grok`→Grok、`rag`→RAG/rage
- `cargo build` 通过（`english_words.txt` 经 `include_str!` 编入，必须重编才生效）
- `sync_to_ime.py` 同步前后哈希对账通过，IME 词库版本 = `2026.10.29.6`

### 覆盖局限（实测已知）

`english_words.txt` 只覆盖 1 万高频英文词 + 手工补齐，前缀输入 `clau` 时
`clause`（google 词表 rank 更高）排在 `Claude` 前；输入完整 `claude` 才 exact 置首。
品牌词若要压过同前缀常用词，需要独立的品牌词权重区，本轮未做。

