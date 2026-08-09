# 变更日志 — 2026-08-09

## V0.5.6 简繁分集：简体/繁体字库隔离 + 全表转换（Eric 2026-08-09 决策）

**动机**：Eric 指出前两轮"清理转换"方向错误——泰深支持简繁双体，繁体词条不该
被转简丢弃。正确做法是**简繁隔离**：简体/繁体两个词库分集，繁体模式优先走
繁体字库，没有的词用简体转换兜底。

### 数据：system_dict 简繁分集

- `system_dict`（简体）：372,733 条（已清理，无繁体）
- `system_dict_trad`（繁体）：16,761 条（原生繁体词条原文，从原始词库提取，
  如 系統控制臺/我們的出口/臺灣簇蟲）
- 同步 taishenIME `resources/system_dict.db`（双表）+ 重建 .bin
- 原始 381,336 单表备份：`tmp/system_dict_orig_381k.db`

### 引擎：繁体索引 + 模式取词

1. `Dictionary.trad_index`：加载 `system_dict_trad` 表 → 原生繁体索引（前缀+词频降序）
2. `query_trad()`：繁体字库查询（顶层暴露）
3. `query_all` 繁体模式：`query_trad` 前 8 个原生繁体前置（第一屏优先），
   其余追加末尾（避免 40+ 繁体词条挤占 truncate 窗口）；无原生繁体时
   简体词条由输出层转繁兜底
4. `candidate_display`/`select_candidate`：原生繁体词条（`trad::is_traditional`）
   不再二次转繁——避免多音歧义（系統控制臺 → 繫統控製臺）
5. `trad_full.rs`：GB2312 一级 1313 常用字简→繁全表（zhconv 生成），
   替换 trad.rs 旧"常用高频字子集"——此前 测/试/们/系 等不在表，繁体模式
   "我们→我们"转不出；现在 我們/測試/系統 全对
6. `trad.rs` 补多音字歧义词组（系统→系統、关系→關係、控制→控制、制造→製造、
   以后→以後、干部→幹部、台湾→臺灣 等）

### 验证（release 真实词库）

| 场景 | 候选 |
|------|------|
| 简体 women/ceshi | 我们/测试（无繁体）✅ |
| 繁体 xitongkongzhitai | **系統控制臺**（原生繁体 第1）✅ |
| 繁体 taiwan | **臺灣**（台湾转繁 第1）/太彎/太晚/臺網 ✅ |
| 繁体 women | **我們**/澳門/我們仨/奧蒙德/沃克蘭 ✅ |
| 繁体 ceshi | **側視**/測試/側室/測時 ✅ |

### 已知问题

- 4 个 lib 测试 flaky（V0.5.3 已记录，stash 复现，非本轮引入）
- trad_full 词组歧义表按需补充（新词组如转换错误再补）

**文件**：
- `engine/src/dictionary/mod.rs`：trad_index + load_trad_from_db + query_trad
- `engine/src/lib.rs`：繁体模式 trad 前置（限量 8）+ is_traditional 跳过二次转繁
- `engine/src/trad.rs`：to_traditional 全表 + 歧义词组 + is_traditional
- `engine/src/trad_full.rs`：1313 简→繁映射（zhconv 生成）
- `resources/system_dict.db`：简繁双表（gitignore）+ .bin 重建
- `docs/reference/candidate-ranking-logic.md`：简繁分集章节

## V0.5.5 候选 pick 重构：词库优先级分层（Eric 2026-08-09 决策）

**动机**：Eric 指出 V0.5.4 仍是「混排 + 阈值补丁」——所有词库的词进同一个池子
按词频/热度/阈值排序。要求改为**词库优先级分层 pick**：先定义词库优先级，
输入拼音后逐层取词，低层级永不插队。

### 设计：词库优先级分层（P1→P5）

| 层级 | 词库 | 内部排序 | 规模 |
|------|------|----------|------|
| P1 | 用户词库 | 热 > 温 > 过期 | 动态 |
| P2 | 常用词库 common | 人工 rank 行序 | 568 条（+30 专名） |
| P3 | 系统词库 system | 词频降序 | 38.1 万 |
| P4 | 领域词库 domains | 热度 > 词长 > 原序 | 16.9 万 |
| P5 | 联想兜底 | 纠错/模糊/简拼/组合 | — |

### 改动

1. **query()/query_short() 重构为分层**：精确层 + 前缀层都按 P1→P4 逐层取词，
   每层取完才进入下一层。删除 V0.5.4 的 `SYSTEM_HIGH_FREQ` 阈值夹层
   （system 拆高/低频夹 domain）、query_short 高/低频拆分、domain 重复追加
2. **删除 apply_domain_boost**（领域热度前置，分层后冗余——热度排序已并入 P4
   层内 `push_domain_sorted`）；删除对应 2 个测试
3. **高频专名进 common（方案 A）**：微博/微信/抖音/美团/支付宝/b站/腾讯视频/
   哔哩哔哩等 30 个专名补进 `resources/common_dict.txt`（P2 层）——专名本质是
   常用词，V0.5.2 的「领域词前置打微博」特例不再需要（domain_exact_short 降为
   P4 内部排序）
4. 保留：词长匹配分区（apply_word_len_match，引擎层兜底）、繁体归一化、
   phrase_group_guess 兜底

### 验证（release 真实词库）

| 输入 | 候选 |
|------|------|
| ceshi | **测试**/侧室/侧视/测时/策士（P3 测试 > P4 侧视）|
| weibo / wb | **微博**/微波/微薄/韦伯（P2 专名第 1）|
| women | **我们**/澳门/我们仨/…（无短句抢位）|
| zg | 这个/**中国**/最高/整个（中国 P3 第 2）|
| tengxunshipin / bilibili | **腾讯视频**/**哔哩哔哩**（专名全拼第 1）|
| xiexie / xihuan / de | 谢谢/喜欢/的 全部第 1 |

### 已知问题（非本轮引入）

- 4 个 lib 测试 flaky + first_char_verify 10 缺词（V0.5.3/0.5.4 已记录，stash 复现）

**文件**：
- `engine/src/dictionary/mod.rs`：query/query_short 分层重构，删 SYSTEM_HIGH_FREQ
- `engine/src/lib.rs`：删 apply_domain_boost + 2 测试
- `resources/common_dict.txt`：+30 高频专名
- `resources/common.db`：重建（568 条，gitignore）
- `docs/reference/candidate-ranking-logic.md`：分层模型文档

## V0.5.4 候选排序修复三件套：繁体归一化 + 词长匹配 + 过度联想收敛（fix）

**动机**：Eric 实测反馈 ①简体模式候选出现繁体 ②women/womenceshi 过度联想出短句/
拼接怪词 ③常用字词优先级异常。真实词库实测复现全部三项。

### Bug 1：简体模式候选出现繁体字（词库数据混入）

- 根因：词库构建无简繁归一化——system_dict.db 38.1 万词中 **16,858 条**含繁体
  独有字，domains.db 16.9 万词中 **19,955 条**（维基繁体词条直接入库），
  另有 GBK 乱码残留（紝/鐨/剉 类 mojibake）。查询层无繁→简处理，
  trad.rs 只做输出层简→繁（繁体模式），方向相反
- 修复：加载层简繁归一化——新增 `trad_simp.rs`（词库实际出现频次≥2 的
  1698 个繁体字→简体映射表，zhconv 生成）+ `trad::to_simplified`（逐字查表，
  繁→简方向多对一收敛无需词组匹配）；`from_sqlite`/`load_domains_from_db`
  加载循环转简体 + 同拼音去重（ORDER BY frequency DESC 保留高频）
- 效果：domains 16.97 万→16.28 万词（去重 6,901 条）；women 不再出「我們的出口」、
  ceshi 不再出「側視」

### Bug 2：过度联想（women 第 1 位短句、womenceshi 拼接怪词）

- 根因：① `phrase_guess` 多音节切分联想在候选为空时**无条件触发**，把音节
  top 单字笛卡尔积拼接（wo→我、men→们、ce→测、shi→是 → "我们测是"），
  而 combo_guess/phrase_group_guess 有 `!is_full_pinyin` 条件——条件不一致；
  ② 领域词热度前置（sport「我们是冠军」前缀命中）+ `apply_long_word_filter`
  把 3-4 字短语从第 4 位起提前，压过 2 字双字词「我们」
- 修复：① 删除 `phrase_guess` 调用（拼接怪词来源），`phrase_group_guess`
  （词库锚定拆分，出真实词组「我们测试」）条件放宽为"候选不足一页时触发"；
  ② 删除 `apply_long_word_filter`（长词提前方向与需求相反），新增
  `apply_word_len_match`：输入 N 个完整音节 → 候选按「字数 == N」稳定分区
  在前（占前 4 位），N+1 次之，其余靠后。仅完整拼音输入生效（简拼/英文
  目标词长不可知不干预），英文候选区不参与

### Bug 3：常用字词优先级异常（领域词压常用词）

- 根因：V0.5.2 的领域词前置（全拼精确层 key_len≥3 + 简拼精确层 wb→微博）
  没有 system 高频保护——「侧视」(astronomy) 压过「测试」(2848)、
  「之卦」(conversation) 压过「中国」(4298)、「雪鞋」(sport) 压过「谢谢」(2553)
- 修复：新增 `SYSTEM_HIGH_FREQ=2500` 阈值——system 超高频词（≥2500，实测
  测试 2848/喜欢 3355/我们 4199/中国 4298 全达标）优先于领域词；中频 system
  词（微波 2365）仍让位给领域热门专名（微博，V0.5.2 效果保留）；
  `apply_domain_boost` 改为候选不足一页时才重排（领域热度不越级）

### 验证（release 真实词库，诊断测试全过）

| 输入 | 修复前 | 修复后 |
|------|--------|--------|
| wo | 我/握/窝/卧/我国 | 我/握/窝/卧/斡 ✅ |
| women | 我们是冠军/我们/.../我們的出口 | 我们/澳门/我们仨/... ✅ |
| womenceshi | 我们测是（拼接怪词） | 我们测试/我们侧室/... ✅ |
| ceshi | 测试用例/側視/测试/... | 测试/侧视/侧室/测时/策士 ✅ |
| xihuan | 喜欢编程/喜欢/... | 喜欢/修函/绣房/... ✅ |
| weibo | — | 微博/苇箔/韦伯/微波/微薄 ✅ |
| zg | 之卦/只改/...（无中国） | 这个/中国/最高/整个/职工 ✅ |
| xiexie | 雪鞋/谢谢/... | 谢谢/歇歇/泄泻/写写 ✅ |

### 已知问题（非本轮引入）

- 4 个 lib 测试 flaky（mistake_nuanhe/mix_mode_english_candidate_appended/
  mix_mode_select_english_no_learn/traditional_english_not_converted）：
  `test_compose_entry_after_2_pages` 加载真实词库污染全局 DICT，串行全量必失败
  （stash 对比复现，既有问题）
- `first_char_verify` 10 个缺词（值/基/己/使/事/始/制/执/志/机）：common 词表
  同音单字 >5 个时前 5 数学不可全达（V0.5.3 已记录的已知取舍）

**文件**：
- `engine/src/trad.rs`：to_simplified（繁→简）+ 测试
- `engine/src/trad_simp.rs`：繁→简映射表（1698 对，词库频次≥2 自动生成）
- `engine/src/dictionary/mod.rs`：加载层简繁归一化 + SYSTEM_HIGH_FREQ 保护
  + query_short 高/低频拆分
- `engine/src/lib.rs`：词长匹配分区（取代 long_word_filter）+ phrase 逻辑调整
  + domain_boost 条件收敛
- `docs/reference/candidate-ranking-logic.md`：候选排序全景 + 根因 + 目标规则
- `docs/DEV-TRACKER.md`：登记 B-22/23/24

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
