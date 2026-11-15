# 泰深输入法 — 核心数据流（v3）

> 对齐当前代码实现（2026-11-16）。
> 双平台链路（TSF / IMM32）共用同一 Rust 引擎；词库由 taishen-dict 独立管线构建。
> v3 变更：候选装配顺序按现行 `query_all` 重排；补整句解码、组词三级候选、
> 组合串单一拼装与分段显示属性；bigram 档位表接入主排序。

---

## 主链路：按键 → 上屏

```
┌──────────────────────────────────────────────────────────────────┐
│ 阶段 1: 按键捕获（双链路）                                         │
│                                                                   │
│ 用户按键                                                          │
│   ├─ TSF 应用: ITfKeyEventSink::OnKeyDown                        │
│   │   → tsf_keyevent.cpp（修饰键透传 Ctrl/Alt/Shift）            │
│   │   → tsf_composition.cpp（组合串管理）                        │
│   └─ IMM32 应用（老游戏）: ImeProcessKey → tsf_keyevent 复用       │
│             ↓                                                     │
│ engine_bridge.cpp → FFI: engine_process_key(char)                 │
└──────────────────────┬──────────────────────────────────────────┘
                       │ C FFI 边界（81 函数，ffi_guard! 防 panic）
┌──────────────────────┴──────────────────────────────────────────┐
│ 阶段 2: Engine::process_key 模式路由（lib.rs）                    │
│                                                                   │
│ 字母键:                                                           │
│   ascii_mode（英文）→ 返回 false，平台层直通上屏                  │
│   普通拼音 → pinyin_buf 累积（保留原始大小写 raw_input）          │
│   组词模式中 → 退出组词重新开始                                   │
│ 特殊前缀:                                                         │
│   c + 数字/运算符 → 计算器模式（is_calc_mode）                   │
│   v + 数字 → 符号分类别名 v1-v9（is_symbol_prefix）              │
│   v + 分类码 → 符号模式（is_symbol_mode）                        │
│   r + 数字 → 金额大写（is_number_continuation）                  │
│   u + hex → Unicode 输入（is_unicode_continuation）              │
│   u + 部件拼音 → 拆字反查（is_radical_mode）                     │
│   rq/sj/xq/nl → 日期时间（is_datetime_candidate）                │
│   ↓                                                               │
│ query_all() → 候选组装                                            │
└──────────────────────┬──────────────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────────────┐
│ 阶段 3: 词库分层查询（dictionary/mod.rs，V0.5.5）                 │
│                                                                   │
│ P1 用户词库（热 > 温 > 过期，7 天衰减）                          │
│ P2 common.db（rank 行序 = 人工优先级，592 条）                   │
│ P3 system_dict.db（简体 37.3 万，词频降序）                      │
│ P4 domains.db（16.9 万，领域热度 > 词长 > 原序）                 │
│ P5 联想兜底（纠错/模糊/简拼/组合/锚定拆组）                       │
│                                                                   │
│ 叠加规则:                                                         │
│   词长匹配分区（完整拼音 → 字数==N 稳定占前 4 位）               │
│   pin 置顶 > 词长分区 > 分层取词                                  │
│   简拼 → domain_exact_short_index（wb→微博）                     │
│   繁体模式 → trad 表原生繁体前置（限量 8）+ 简体转繁兜底          │
│   bigram 档位表（1-6 档）参与前文搭配排序                         │
└──────────────────────┬──────────────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────────────┐
│ 阶段 4: 候选窗口（Direct2D 自绘 + TSF UIElement）                 │
│                                                                   │
│ engine_get_candidate_count() → 数量（部署默认 9）                 │
│ engine_get_candidate(i) → 逐条取候选                              │
│ TSF：ITfCandidateListUIElement 注册（宿主/游戏可读）              │
│      宿主 show=FALSE 且未读数据 → CCandidateWindow 兜底自绘       │
│ candidate_window.cpp（UIWindow + CandidatePanel 框架）            │
│   - 跟随光标（MonitorFromPoint 多屏 + dpi_util 坐标对齐）         │
│   - 拼音行与组合串同源：普通输入 = 原始拼音，组词 = 已选+剩余     │
│   - 候选列表 + 翻页 + 多行展开                                    │
└──────────────────────┬──────────────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────────────┐
│ 阶段 5: 选词上屏（select_candidate，lib.rs）                      │
│                                                                   │
│ 用户 Space/数字/鼠标 → engine_select_candidate(index)             │
│   ↓ 候选类型判定                                                  │
│   普通中文词:                                                     │
│     → learn() 写入用户词库（选词即学）                           │
│     → record_domain_hit() 领域热度 +1                             │
│     → last_committed 记录（上下文联想 P1-1）                      │
│     → 繁体模式：简体词 to_traditional 转繁（原生繁体不二次转）    │
│   整句候选（Source::Sentence）: 不学习、不写上下文搭配            │
│   英文候选: apply_cap 大小写还原（Hello/HELLO）                   │
│   符号/计算器/日期/拆字/错音: 不上屏学习、不污染上下文            │
│   ↓                                                               │
│ TSF: ITfInsertAtSelection 提交文本；IMM32: ImeToAsciiEx 上屏      │
│ 候选窗隐藏，引擎 reset()                                          │
└─────────────────────────────────────────────────────────────────┘
```

## 候选装配与排序（阶段 3 内部，按 `query_all` 实际顺序）

| # | 阶段 | 实现 | 关键约束 |
|---|------|------|----------|
| 1 | 特殊模式早退 | `detect_mode` / `apply_mode` | 计算器/符号/拆字/数字/Unicode/双拼/组词各自发布候选 |
| 2 | 日期简码 | `datetime_candidates` | 用户词简拼命中时让位 |
| 3 | 分层主查询 | `dictionary::query` | P1 用户 → P2 common → P3 system → P4 domain |
| 4 | 原生繁体 | `query_trad` | 繁体模式，前 8 条前置，其余追加 |
| 5 | 快捷短语 | `phrase_map` | 插在热/温用户词之后、常用词之前 |
| 6 | 简拼补充 | `query_short` | 声母索引 |
| 7 | 混合简拼 | `query_mixed` | 候选未满时 |
| 8 | 缩写+全拼 | `query_abbrev_full` | 候选未满时 |
| 9 | 模糊音 | `fuzzy_variants` | 仅完整拼音 |
| 10 | 邻键纠错/删除变体 | `correction` | 仅完整拼音，候选不足时 |
| 11 | 插入变体（漏打字母） | `insertion_variants` | 输入不像话 + 前 3 位无常用词；排在邻键之后 |
| 12 | 错音提示 | `mistake::lookup` | 仅完整拼音，候选不足一页 |
| 13 | 逐音节组合 | `query_combo` / `combo_guess` | 仅非完整拼音 |
| 14 | 锚定拆组 | `phrase_group_guess` | 候选不足一页，或输入零命中且前 6 位无常用词 |
| 15 | 排序预算截断 | `RANKING_CANDIDATE_BUDGET = 4096` | 截断后重建去重集合 |
| 16 | 日期/英文混输 | `datetime_pinyin` / `english::query` | 英文恒在中文之后；精确命中且中文无常用词时才抢首位 |
| 17 | 上下文加权 | `apply_context_boost` | 学习表 / 先验 / 内置表 / bigram 四源取 max；权重降序，同权重保持原位置 |
| 18 | 词长匹配分区 | `apply_word_len_match` | 三分区：exact → 短（单字）→ 长；纠错来源的常用词豁免；单字保持词库词频序 |
| 19 | 隔音歧义提升 | `apply_ambiguous_split_boost` | yuan/xian 类键，多音节词提到第 4 位 |
| 20 | 置顶 | `apply_pin_boost` | 词级 pin，任何输入方式命中即前移 |
| 21 | 繁转去重 | `retain` | 转繁后同文本只留原生繁体 |
| 22 | 排序模式 | `apply_sort_mode` | 用户显式开启时生效 |
| 23 | 降权 | `apply_demote` | 必须最后做，否则被分区重新提回 |
| 24 | Emoji | `emoji::lookup` | 插在英文区之前，最多 5 条 |
| 25 | 整句解码 | `apply_sentence_candidates` | 长输入；插到候选最前 3 条 |
| 26 | 长句过滤 | `MAX_CAND_WORD_LEN = 10` | 整句候选豁免；用户短语豁免 |
| 27 | 截断 | `max_pages * page_size` | 展示窗口 |

> 排序细节与历史根因见 [reference/candidate-ranking-logic.md](reference/candidate-ranking-logic.md)。
> 整句解码打分与限制见 [modules/sentence-decode/SPEC.md](modules/sentence-decode/SPEC.md)。

## 支线链路

### 退格与光标

```
Backspace → engine_backspace()（删尾音节）
           → pinyin_buf.pop() → query_all() → 候选刷新
Backspace（拼音模式）→ engine_backspace_syllable()（删整音节）
←/→        → engine_move_cursor()（光标在拼音串内移动）
删除候选    → engine_delete_candidate()（用户词库条目删除）
```

### 组词模式（现行）

```
触发：翻页 3 次仍无目标 → compose_active

候选装配（Mode::Compose，预算 3 页）
  0) 整句层：剩余音节整体转换（sentence_guess，前文已选词参与 bigram）
     门槛：字数 == 剩余音节数；只取 1 条
  1) 逐级前缀：cut 从剩余音节数递减到 1
     query(前缀) 只收「字数 == cut」的词，每级最多 4 条
  2) 当前音节单字兜底：最多 8 个

状态（引擎唯一真相源）
  compose_syllables / compose_idx / compose_chars
  已选前缀 compose_prefix()，平台层不再维护副本

选字
  中间音节 → 只更新组合串，不落文档
  最后音节 → 组合完整词 learn() → 一次提交整词
  例: moshoushijizhenhaowan → 魔兽世界 → 魔兽 → …
退格
  compose_undo() 先回退已选词；无可回退才删拼音字母
结束
  标点/引号/大写字母 → 提交已选前缀后结束组词（不误选第 0 候选）
```

TSF 与 IMM32 共用同一引擎状态，差异只在「怎么写进宿主」：
TSF 走 `BuildCompositionText` + `SyncComposition`，IMM32 走
`GetDisplayPinyinWide` + `GCS_COMPSTR`。两条通道的组词态组合串都是
「已选前缀 + 剩余音节」。

### 整句解码（长输入，2026-11-16）

```
启用条件：≥8 字符、纯小写字母、≥3 音节、不是精确英文单词

sentence::decode(input, prev, limit=3)
  音节图：格子 = 连续 k 个音节（k ≤ 8）命中的词（用户/常用/系统/领域四层）
  打分：词频按词数折算（β=0.6）
        + 成词奖励 2.0
        + 搭配档位按 ln(1+档位) × 0.6
        + 用户词 +1.0
        − 低频词扣分 1.5
  束宽 8；输出前 3 条不同文本
  边界歧义（xian = xian / xi+an）分别求路径后合并排序
  容错切分：整串读不通才启用（替换/删除/插入，总编辑 ≤ 3，带节点预算）

插入位置
  倒序插到候选最前；同文本已存在时只挪位置、保留原 Source
  Source::Sentence 豁免 >10 字过滤
  不写用户词学习、不写上下文搭配
```

### 以词定字（V0.2.24）

```
[ → engine_take_char(true)：取当前候选首字
] → engine_take_char(false)：取末字
（取字不学习）
```

### 中英文切换

```
Shift（tap 检测）→ engine_set_ascii_mode 切换
  英文模式: 字母直通，不累积拼音（保留切换前的拼音缓冲可继续上词）
Shift+字母 → 大写直出（有候选先上屏）
per-app 记忆: app_state 按进程隔离中英状态
```

### 简繁模式（V0.5.6）

```
engine_set_traditional(true)
  查询: query_trad 原生繁体前置（前 8 位）→ 其余简体转繁兜底
  上屏: 原生繁体不二次转繁（trad::is_traditional 判定）
       简体词 to_traditional（trad_full 1313 字全表 + 歧义词组表）
```

### 特殊模式汇总（v 前缀家族）

```
v + 单 v      → 热门符号直选
v + 分类码    → 符号候选（箭头/数学/单位/标点...，183 分类 3585 符号）
v + 数字      → v1 序号 / v2 数学 / ... / v9 部首笔画
c + 算式      → 计算器（四则/括号/幂/取模）
r + 数字      → 金额大写
u + hex       → Unicode 字符
u + 部件拼音  → 拆字反查（radical_pinyin 13.2 万词库）
rq/sj/xq/nl   → 日期/时间/星期/农历
```

## 词库数据流（双仓库分工）

```
taishen-dict（构建）
    curate/ 人工源 → pipeline.py（构建 + 校验 + VERSION.json）
    → sync_to_ime.py（hash 对账同步）
        ↓
taishenIME（消费）
    resources/ system_dict.db + domains.db + common.db + bigram.db
               + context_prior.db（可选）+ VERSION.json
    engine_init 加载 → .bin 预编译索引秒加载（按源库 len+mtime 校验）
    bigram：set_bigram_path 只登记 → 首次按键 ensure_bigram_loaded 后台加载
    运行时: P1 用户词库（%APPDATA%/user_dict.txt，learn 写入）
            领域热度（record_domain_hit，进程内热数据）
            上下文档位（context_pairs 表，上屏共现累积）
```

## 并发与一致性

- 输入事件串行（TSF OnKeyDown 单线程）；DICT 全局 Mutex 保护词库查询
- 用户词库写入即时落盘（选词即学）；7 天热度衰减（P1 排序）
- bigram 表 `RwLock` 保护；同路径 + 同 mtime 幂等（不重读）；
  `bigram_scores` 一次取锁批量查，避免长候选逐条取锁
- 缓存失效：词库换入/领域合并/用户词删除 → 清整句格子缓存；
  bigram `.bin` 签名（源库长度 + mtime）不符 → 自动重建
- 词库版本: resources/VERSION.json（如 V2026.08.10.1），打包校验
- FFI 全部 ffi_guard! 包裹：panic 不跨边界，返回 fallback + 日志
- 有状态 FFI（select/undo/take_char）用单次调用读取，禁止两步读——
  读两遍等于执行两遍
