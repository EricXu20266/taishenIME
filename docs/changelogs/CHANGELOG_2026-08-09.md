# 变更日志 — 2026-08-09

## V0.5.2 现代词库 LLM 补全 + 领域词简拼修复（feat + fix）

**动机**：用户反馈微博/拼多多/美团/快手/b站/带货/大模型/充电桩/短视频等现代词
"缺乏"。排查发现这些词**实际已在词库中**（network_slang/conversation 领域），
打不出来的真实原因是两个引擎 bug：

### Bug 1：领域词简拼索引截断（简拼打不出微博/拼多多）

- 根因：`query_short` 对 domain 词先 `take(120)` 再排序，而索引按加载顺序
  （agriculture/art 等早期领域先入）排列——热门词如微博在 "wb" 索引的
  120 位之后被截掉，简拼永远查不到
- 修复：加载完成后对 `domain_index` / `domain_short_index` 按词长预排序
  （短词优先进入截断窗口）+ `push_domain_sorted` 增加"精确匹配优先
  （词字符数 == key_len）"排序规则
- 文件：`engine/src/dictionary/mod.rs`

### Bug 2：中英混词简拼为空（b站/C位/up主/emo 简拼全挂）

- 根因：`to_initial_string("bzhan")` 中 `b` 不是合法拼音音节，
  `split_first_syllable` 返回 None → 直接 break → 返回空串 → 混词无简拼索引
- 修复：无法切分时逐字符兜底取字母（bzhan→bz、upzhu→upz、emo→em、
  cwei→cw、yyds→yyds），纯拼音词不受影响
- 文件：`engine/src/pinyin/mod.rs`

### 现代词表 LLM 补全（249 新词）

- 新增 `tools/gen_modern_words.txt`（459 候选词，AI 生成，分类：
  平台App / AI科技 / 短视频直播 / 网络用语 / 新消费生活）+ `tools/gen_modern_dict.py`
  （pypinyin 注音 + 与 system_dict.db 38 万词及 domains 词库去重）
- 生成 `resources/domains/modern.txt` → 重建 domains.db：**35 领域 169,732 词**
  （+249 新词：BOSS直聘/大语言模型/固态电池/直播切片/社区团购/即时零售等）
- 词库规模：系统 38.1 万 + 领域 17.0 万 = **55.1 万词**

### 回归保障

- 新增 `test_modern_words_query_from_db`（9 个点名词全拼+简拼回归断言）
- 新增 `test_to_initial_string` 混词断言
- 全量 lib 测试 **278 passed / 0 failed**

### 已知问题（非本轮引入）

- `test_ascii_mode` smoke test 失败：测试期望英文模式清空拼音缓冲，
  与引擎"切换保留拼音"设计（对标 rime ascii_composer）冲突；平台层真实
  Ctrl+Space 切换会先 engine_reset，实际使用不受影响

**文件**：
- `engine/src/dictionary/mod.rs`：索引预排序 + push_domain_sorted 精确优先
- `engine/src/pinyin/mod.rs`：to_initial_string 混词兜底
- `resources/domains/modern.txt`：249 新词
- `tools/gen_modern_dict.py` / `tools/gen_modern_words.txt`：生成管线
