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

---

## 组词渲染：组合串收敛到单一拼装入口

### 现象

组词中选首字后翻页或退格，已选字从组合区消失；重新选字并输出后旧字又出现。
候选窗拼音行在翻页后也只显示剩余音节。

### 根因

组合串「已选前缀 + 待选后缀」的拼装散落 6 处，其中状态变化（翻页/退格穿透）
与候选窗拼音行直接写剩余音节，把已选字抹掉；`m_composeAccum` 仍在内存里，
下一次选字又拼回来。

### 改动（`platform/windows/src/tsf_module.cpp`）

- 新增 `BuildCompositionText()`：组词态 = 已选前缀 + 待选后缀，普通输入 = 拼音显示串
- 新增 `SyncComposition()`：统一的 Start/Update 入口
- 5 处拼装点改走统一入口；退格不再因引擎回退失败而穿透
- 候选窗拼音行显示已选前缀 + 剩余音节
- `RefreshState` 丢弃组词结束后的残留前缀

### 验证

待 Notepad++ 实机确认（翻页、退格、选后续字三项）。

### IMM32 路径（同轮补齐）

`imm32_ime.cpp` 对齐 TSF：组词态组合串 = 已选前缀 + 剩余音节；中间音节不落文档
（只更新组合串）；最后一个音节提交整词；组词态退格先回退已选词；候选窗拼音行同源。
两条通道共用引擎的 `compose_prefix` / `compose_undo`，差异只在「怎么写进宿主」。

---

## 组词候选重写：从长到短、去掉整句拆组的怪词

原实现只有两层：`phrase_group_guess` 整句拆组（产出「有管腺」类怪词，占首屏）
+ `query(当前音节)`（只查一个音节，剩余拼音的整词从未被查询）。

改为按「本次能消费几个音节」从多到少：

- 剩余整段拼音、词长匹配的词（`zhixianshi` 选「只」→ `显示`）
- 逐级缩短前缀的词（`...zhenhaowan` → `今天天气` → `今天`）
- 当前音节单字兜底（最多 8 个）

每级取 4 个，候选收敛到 3 页；`phrase_group_guess` 退出组词模式。

实测：`moshoushijizhenhaowan` → 没收 / **魔兽** / 魔手…；
`zhixianshi` 选「只」→ **显示** / 限时 / 现实…

---

## 组词整句层：剩余音节整体转换

组词只给「逐段短词」时，用户得一路手选。新增整句层：把剩余音节整体拆成词序列，
选一次吃完整段。

- `dictionary::sentence_guess(input, prev_word, limit)`：同一套拆组递归，
  透传前文已选词参与 bigram 排序（选完「魔兽」后「世界」应压过「时间」）
- **两道质量门槛**：零错（不允许靠单错变体硬凑）+ 常用词过半
  —— 否则 `zhegeweomende` 会出「瓦恶偶闷得」这类胡话
- 组词候选只取 1 条整句，避免挤占后面的短词

实测：`jintiantianqizhenbucuo` → 首位「今天天气真不错」；
`zhegeweomende` → 怪句被挡，首位回到「这个」。

---

## 有副作用的 FFI 被读两遍（组词时选不动后续字）

`ReadEngineUtf8` 是两步读（先查容量、再读正文），只适用于无状态接口。
`engine_select_candidate` / `engine_compose_undo` / `engine_take_char` 每次调用都推进
引擎状态，被读两遍就等于执行两次：按一次数字键选了两个音节，组词直接结束，
之后所有候选键透传。

修复：新增 `ReadEngineActionUtf8`（固定 512 字节缓冲、单次调用），
三个有状态接口改用它；无状态读接口保持不变。

实机日志验证：选首字后组合串 `珐s'w'da'e'wa`、退格 `prefix=珐 undone=珐`、
回退后 `fa's'w'da'e'wa`、最终提交 `发散瓦大额外` 无重复。

---

## 切换输入法卡顿：bigram 表重复加载（2026-11-16）

**现象**：初次切换输入法卡一下，之后正常。

**实测**：`engine.log` 中 **175 次** bigram 表加载，平均 1624ms、P50 1508ms、
最长 6868ms，累计约 **284 秒**。TSF 每次 `ActivateEx` 都调
`engine_set_bigram_path`，原实现无条件重读 20MB / 40 万对。

**三层修复**（`be36004` / `db1c44a` / `0bb8e1e`）：

1. **同进程幂等**：`set_bigram` 按「路径 + mtime」判断，同一文件不重读
2. **推迟到首键**：`set_bigram_path` 只登记路径，首次 `engine_process_key`
   才起后台线程——切换瞬间零开销，加载与打字重叠
3. **.bin 缓存**：bincode 序列化整表（7.2MB），加载 **2760ms → 103ms**（27 倍）；
   按源库 mtime 自动重建，损坏时回退 SQLite

**部署**：4 个 DLL/IME + 预生成 `bigram.db.bin` 已复制到
`%LOCALAPPDATA%\TaishenIME`，ctfmon 已重启。

---

## 审计修复：9 项 P1（2026-11-16）

1. 组词中按标点/引号/大写字母：不再误选第 0 个候选，提交已选前缀后结束组词
2. IMM32 组词回车：新增 Enter 前置分支，不再输出原始拼音 `nihao`
3. IMM32 鼠标点选中间音节：补 `g_inlinePreedit` 门禁，不再向不渲染组合串的宿主写拼音
4. bigram 主排序通路：`merged` 为空但 bigram 表可用时不再早退
5. 隔音歧义：`yue/qie/jie` 的 `yu+e` 切分不再被单字母过滤误伤
6. 上屏文本：Commit 路径不再写 `TF_ATTR_INPUT`，避免属性残留
7. 行内可用性：`StartComposition` 补回读探测，首键不再漏探
8. `install.ps1`：补 32 位 DLL/IMM32 复制与 SysWOW64 注册
9. NSIS / `install_latest.ps1`：补 `imm32_x86` 与 `bigram.db`

完整审计记录：`sessions/b1c7b45f/outputs/audit-2026-11-16.md`。
