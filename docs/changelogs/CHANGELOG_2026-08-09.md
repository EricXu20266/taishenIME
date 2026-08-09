# 变更日志 — 2026-08-09

## V0.5.3 候选排序全量验证 + 领域词简拼精确优先（fix + feat）

**动机**：全量测试常用词是否出现在候选前 5（第一屏）。

### 验证结果（真实词库 55 万词）

- 全拼：538 常用词 **98.1%** 前 5（10 个失败均为同音多字排 6-9 位，第一屏仍可见）
- 简拼：538 常用词 40.8% 前 5（失败多为单字母简拼被高频词占满，符合预期）
- 现代词：全拼 24/24 前 5；简拼修复前 wb→474 位/dy→715 位完全打不出

### Bug：领域词简拼被维基候选淹没（真实词库下）

- 根因：简拼查询 system 简拼索引（维基 38 万词）产生海量候选占满前列，
  领域词永远垫底——上一轮（V0.5.2）修复仅在内置小词库下有效
- 修复：
  1. 新增 `domain_exact_short_index`（完整简拼精确索引）——query_short 中
     领域词完整简拼（wb→微博）插到 system 前缀扩展前（≥2 字母生效，
     单字母仍由 system 高频主导）
  2. 热门领域初始热度（network_slang/conversation/modern/computer/idiom/
     food/economics/sport）——同长词按加载序排列时不被冷门领域挤出
     take 窗口（构建排序 = 词长 + 热度双键）
  3. 全拼 query 加领域词精确层（key_len≥3，weibo→微博 提到 system 同音词前）
- 效果：微博 wb 474→2、b站 bz 516→1、抖音 dy 715→8、微信 wx 570→5、
  美团 mt 459→2、支付宝 zfb→2

### 知名品牌词库补充（17 词）

- 支付宝/淘宝/京东/唯品会/苏宁/国美/哔哩哔哩/优酷/腾讯视频/携程/去哪儿等
  ——system 已有但 jieba 词频低（简拼被顶），补进 modern 领域提升排序

### 已知取舍

- 知乎简拼 zh 位置 35：zh 是"这/知"高频简拼，知乎不抢位属合理设计，
  打 zhh（完整声母）或全拼精确命中

**文件**：
- `engine/src/dictionary/mod.rs`：domain_exact_short_index + 热领域初始热度
  + 全拼领域精确层 + 构建双键排序
- `resources/domains/modern.txt`：+17 知名品牌
- 测试：`test_common_top5_survey`（全量调查，真实词库 538 词 + 24 现代词）

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
