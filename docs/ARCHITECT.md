# 泰深输入法 — ARCHITECT.md（v3.1）

> 架构骨架。基于 app-project-architect 方法论：拓扑总纲（1+3+3+场）+ Root 裁剪（四条标准）+ 结构映射 + 运行时设计。
> v3 更新：2026-08-10 词库分家（taishen-dict 独立）+ 当前代码实现。
> v3.1 更新（2026-11-16）：按实际代码校准规模（FFI 81 / 平台层 32 源文件）；
> 补整句解码（sentence）、bigram 档位主排序、组词候选三级重写、
> 组合串单一拼装点与分段显示属性、TSF UIElement 候选协商。
> v2（2026-07-28 初版）见 [archive/architecture-v2.md](archive/architecture-v2.md)。

---

## 〇、双仓库分工（v3 核心前提）

```
taishen-dict（词库唯一构建方，独立仓库）
    curate/ 人工源 → pipeline.py（构建+校验+VERSION.json 版本清单）
    → sync_to_ime.py（hash 对账同步）

taishenIME（输入法引擎 + 应用，本仓库）
    只消费 resources/ 词库；不构建、不管理词库
    引擎加载 → 候选 → 上屏；部署期编译 .bin 预编译索引
```

本仓库不出现任何词库构建脚本（历史遗留归档于 `tools/archive/`，勿运行）。

## 一、拓扑分析（1+3+3+场）

四层四个动词：识别声明 / 建 / 铺 / 罩。

```
心跳(1): 双引擎 → 识别 + 声明
  框架自带（隐式）：Windows TSF 事件循环 —— 注入点 tsf_keyevent.cpp:ITfKeyEventSink::OnKeyDown
  自研（显式）：Engine 状态机（lib.rs:process_key）—— 业务节律（拼音累积/候选/提交）
  （散落形态：无 —— 引擎职责单一，无渗漏）

三角: 建
  前台 — platform/windows/   TSF + IMM32 双入口 + Direct2D 自绘 UI（候选窗/设置窗/横幅）
  后台 — engine/src/         24 个顶层 Rust 模块拼音引擎（纯 Rust，平台无关）
  数据库 — resources/system_dict.db + domains.db + common.db + bigram.db（只读，taishen-dict 产出）
          + %APPDATA%/taishen-ime/user_dict.txt（运行时生长）

三条流: 铺
  通道 — C FFI 81 函数（engine_bridge.cpp ↔ ffi.rs，全部 ffi_guard! 防 panic 跨边界）
  跟踪 — engine/src/log.rs 分级（INFO/WARN/ERROR）+ 平台 debug_log + FFI panic 自动捕获
  呈现 — Direct2D/DirectWrite 全自绘（ui_* 控件库 + theme 主题系统，浅/深/自定义色）
          组合串统一拼装 + 已选前缀/待选拼音分段显示属性；TSF UIElement 候选数据通道

场: 罩
  安全 — 输入内容不落盘明文，不联网
  可靠 — FFI panic 守卫、词库损坏降级、DICT 并发 Mutex
  性能 — .bin 预编译索引秒加载、候选查询 <5ms、词长分区稳定首屏
  配置 — 引擎级开关（双拼/模糊/简繁/符号/emoji）+ config.yaml + 设置窗口
  生命周期 — NSIS 安装器；package.ps1 打包（校验词库 → CMake → NSIS）
```

## 二、Root 裁剪

从参考 12 Root 出发，用四条判定标准（根本问题 / 正交 / 独立演化 / 缺位可证伪）裁剪。
砍掉 #5 身份与权限（桌面单用户）；#6 安全边界为场约束不建实体。

| # | Root | 白话 | 物理位置 | 状态 |
|---|------|------|---------|------|
| 1 | 数据模型与持久化 | 存什么、怎么存 | `resources/*.db`（taishen-dict 产出）+ `%APPDATA%/user_dict.txt` | 🟢 词库分家完成，版本化 |
| 2 | 业务领域层 | 应用到底做什么 | `engine/src/` 24 个顶层模块（拼音/词库/纠错/整句/联想/简繁/特殊模式/语音） | 🟢 全量实现 |
| 3 | 接口层 | 怎么跟它打交道 | `ffi.rs` 81 函数 + `platform/windows/` TSF + IMM32 双 DLL | 🟢 双链路 |
| 4 | 状态管理 | 此刻输入什么 | `lib.rs` Engine 状态机 + `app_state.cpp` | 🟢 实现 |
| 5 | ~~身份与权限~~ | ~~谁能干什么~~ | — | ❌ 砍掉（单用户桌面） |
| 6 | 安全边界 | 不能发生什么 | 场覆盖，不建实体 | 🟢 场约束 |
| 7 | 配置系统 | 不改代码怎么改行为 | `config_reader.cpp` + 引擎 set_* 开关 + 设置窗口 | 🟢 实现 |
| 8 | 呈现层 | 长什么样、在哪儿呈现 | Direct2D `ui_*` 控件库 + `theme.cpp` | 🟢 全自绘 |
| 9 | 可观测性 | 发生了什么 | `log.rs` 分级 + `debug_log` + FFI panic 捕获 | 🟢 实现 |
| 10 | 可靠性 | 错了能回退 | `ffi_guard!` + 词库损坏降级 + 配置回退 | 🟢 实现 |
| 11 | 性能 | 跑得快用得省 | `.bin` 预编译索引 + P1→P5 分层查询 + 词长分区 + bigram 延迟/幂等加载 | 🟢 体检 99.84% |
| 12 | 生命周期 | 怎么部署、升级、回滚 | NSIS + `package.ps1`（校验词库版本） | 🟢 打包闭环 |

### 引擎领域层内部（Root #2 的 Branch 生长，非新 Root）

24 个顶层模块按功能族归属，不单独立 Root（正交性约束）：

```
拼音核心      pinyin（音节表/切分） · dictionary（P1→P5 分层查询 + 整句格子）
             correction（纠错） · mistake（易错字） · fuzzy（模糊音）
             radical（部首） · shuangpin（双拼）
整句/上下文   sentence（词图 Viterbi + 束搜索） · context（前文搭配）
简繁系统      trad（简→繁+歧义词组） · trad_full（1313 字全表） · trad_simp（繁→简归一化）
特殊模式      symbol · emoji · english · calculator · datetime · number · unichar
语音          voice（VAD 分段 + 语音转写）
基础设施      lib（Engine 状态机） · ffi（C ABI） · log（分级+panic 捕获）
             tuning（排序参数外置 tuning.json）
```

> `dictionary/write_back.rs`（用户词异步落盘）与 `bin/build_index.rs`（预编译索引生成）
> 是子模块/工具入口，不计入顶层模块。

## 三、结构映射

### 配置三个家（不混）

| 类型 | 位置 | 生效时机 |
|------|------|---------|
| 代码库配置 | 引擎默认开关（lib.rs 常量）、`resources/` 词库 | 随 git，构建/同步更新，重启生效 |
| 环境配置 | 安装目录 `config.yaml` | 安装器写入，重启生效 |
| 用户数据 | `%APPDATA%/taishen-ime/` | 运行时读写，即时生效（用户词库选词即学） |

### 数据分库（量级不同，不混）

| 库 | 位置 | 量级 | 保留期 |
|----|------|------|--------|
| 系统词库 | `resources/system_dict.db`（简体 37.3 万 + 繁体 1.7 万） | 20MB 只读 | 随词库版本更新 |
| 领域词库 | `resources/domains/domains.db`（简体 16.9 万 + 繁体 1.8 万） | 20MB 只读 | 随词库版本更新 |
| 常用词库 | `resources/common.db`（592 条 rank 行序） | 20KB 只读 | 随词库版本更新 |
| 搭配表 | `resources/bigram.db`（prev → next，1-6 档，约 40 万对） | 20MB 只读 | 随词库版本更新 |
| 上下文先验 | `resources/context_prior.db`（可选，相邻词共现档位） | 数 MB 只读 | 随词库版本更新 |
| 用户词库 | `%APPDATA%/taishen-ime/user_dict.txt` | 持续增长 | 永久，7 天热度衰减 |
| 日志 | `%APPDATA%/taishen-ime/logs/` + debug_log | ~1MB/天 | 滚动保留 |

**预编译缓存（`.bin`）**：`system_dict.db.bin`、`domains.db.bin`、`bigram.db.bin`、
`context_prior.db.bin` 均由引擎首次加载时生成，按源库 `(长度, mtime)` 校验后复用。
bigram 冷读 2.7s → `.bin` 热读 ~103ms；文件缺失或签名不符时自动回退 SQLite 并重建。

### 词库版本控制（v3 新增）

词库版本 = `resources/VERSION.json`（如 V2026.08.10.1）。缺失/过期时运行
taishen-dict 的 `python tools/sync_to_ime.py`（同步前后 hash 对账），不在本仓库构建。

### 呈现层结构（v3.1）

```
输入状态（引擎唯一真相源）
  ├─ 已选前缀 compose_prefix（组词态）
  └─ 待选音节 syllable_display（组词态为剩余音节）
        ↓
组合串拼装（各入口唯一实现）
  TSF   BuildCompositionText(pending) + SyncComposition(op)
  IMM32 GetDisplayPinyinWide() + SyncComposition(himc, pinyin)
  规则：组词态 = 已选前缀 + 待选音节；普通输入 = 原始拼音显示串
        ↓
宿主呈现
  TSF
    · 组合串：ITfComposition + ITfRange::SetText
    · 分段属性：GUID_TAISHEN_ATTR_CONFIRMED（已选，无额外样式）
                GUID_TAISHEN_ATTR_PENDING（待选，虚线下划线）
    · 候选数据：ITfCandidateListUIElement（宿主/游戏可读）
    · 自绘兜底：宿主 show=FALSE 且未读过候选数据 → CCandidateWindow
  IMM32
    · 组合串：GCS_COMPSTR + WM_IME_COMPOSITION
    · 候选通知：IMN_OPENCANDIDATE + GCS_CANDIDATELIST
    · 自绘：CCandidateWindow（与 TSF 同一控件库）
```

**归属判定**（`ShouldSelfDrawWhileSuppressed`）：per-app 配置
（`app_self_draw` / `app_no_self_draw`）优先；未配置时看宿主是否调用过
`GetCount` / `GetString` 等候选数据接口——读过说明宿主自己会画，不再叠窗。

## 四、数据流

```
用户按键
  ↓
TSF OnKeyDown / IMM32 ImeProcessKey → tsf_keyevent 预判 → engine_bridge.cpp
  ↓
FFI: engine_process_key(char) [ffi_guard!]（首次按键顺带 ensure_bigram_loaded）
  ↓
Engine::process_key() 模式路由：计算器/符号/拆字/数字/Unicode/双拼/组词
  ↓（常规拼音）
query_all() 候选装配
  ① 词库分层 query（P1 用户 → P2 common → P3 system → P4 domains）
  ② 补充源：短语/简拼/混合简拼/模糊/邻键纠错/插入纠错/错音/组合/锚定拆组
  ③ 中英混输 + 日期
  ④ 排序：context_boost（学习表/先验/内置表/bigram 四源取大）
        → word_len_match（三分区 + 纠错豁免 + 单字词频序）
        → ambiguous_split_boost → pin_boost → sort_mode
        → demote → emoji → sentence（整句插前三）
  ⑤ >10 字过滤（整句豁免）→ 截断到 max_pages
  ↓
FFI: engine_get_candidate() → 平台层
  ↓
渲染：
  TSF：BuildCompositionText = 已选前缀 + 待选音节 → SyncComposition
       + ApplyCompositionDisplayAttributes（已选/待选分段）
       + ITfCandidateListUIElement 供宿主/游戏读取；宿主不画时自绘兜底
  IMM32：GCS_COMPSTR 写同一组合串，IMN_OPENCANDIDATE 通知候选
       + CandidatePanel 自绘
  ↓
用户选择 → engine_select_candidate()
  ↓
TSF ITfInsertAtSelection / IMM32 GCS_RESULTSTR 提交
  ↓
选词即学：learn_user_word + 上下文档位累积 + 领域热度 +1
```

> 详细候选装配顺序见 [business-flow.md](business-flow.md)；
> 排序规则全景见 [reference/candidate-ranking-logic.md](reference/candidate-ranking-logic.md)；
> 整句解码见 [modules/sentence-decode/SPEC.md](modules/sentence-decode/SPEC.md)。

## 五、目录结构

```
taishenIME/
├── engine/                        # Root #2 业务领域 + #4 状态管理（平台无关）
│   ├── Cargo.toml                 # cdylib + staticlib
│   └── src/                       # 24 个顶层模块（见第二节功能族）
│       ├── lib.rs                 # Engine 状态机（查询装配/排序/组词/选词）
│       ├── ffi.rs                 # Root #3 C ABI 81 函数
│       ├── pinyin/mod.rs          # 音节表 + 切分
│       ├── sentence.rs            # 整句解码（Viterbi + 束搜索）
│       ├── tuning.rs              # 排序参数外置（tuning.json）
│       └── dictionary/mod.rs      # Root #1 词库查询（P1→P5 分层）
├── platform/windows/              # Root #3 (TSF/IMM32) + #8 (呈现)
│   ├── CMakeLists.txt
│   ├── include/                   # 平台层头文件（tsf_* / ui_* / imm32）
│   └── src/                       # 32 个源文件（tsf_* 三件套 + ui_* 控件库 + imm32/ + engine_bridge）
├── resources/                     # Root #1 词库（taishen-dict 同步，本仓库不构建）
│   ├── system_dict.db + .bin      # 系统词库 + 预编译索引（引擎 ffi 部署期生成）
│   ├── domains/                   # 领域 txt（可读源）+ domains.db
│   ├── common_dict.txt + common.db
│   ├── bigram.db + bigram.db.bin  # 搭配档位表（1-6 档）+ 预编译缓存
│   ├── context_prior.db           # 上下文先验（可选）
│   ├── VERSION.json               # 词库版本指针
│   ├── archive/raw_dict.txt       # 早期手工词表（归档）
│   └── rime_ice/                  # 雾凇参考词库（gitignore）
├── install/                       # Root #12 NSIS + PowerShell 安装器
├── package.ps1                    # 一键打包（校验词库 → CMake → NSIS）
├── tools/
│   ├── archive/                   # 历史词库构建脚本（已移交 taishen-dict，勿运行）
│   └── （引擎侧工具：符号表生成/FFI 测试等，保留）
└── docs/
    ├── ARCHITECT.md               # ← 本文件（v3.1）
    ├── archive/architecture-v2.md # v2（2026-07-28 初版）
    ├── business-flow.md / DEV-TRACKER.md / CHANGELOG / index.md
    ├── modules/                   # 模块 SPEC（45 份，含审计与验证文档）
    └── reference/                 # 参考资料
```

## 六、运行时设计（九问）

| # | 问 | IME 现状 |
|---|----|---------|
| 1 | 启动 | `.bin` 预编译索引秒加载；bigram 表登记路径、首键后台加载；词库缺失时降级到内置最小词表 |
| 2 | 状态 | 单机单用户；用户词库持久化，重启恢复学习结果；组词已选前缀只存引擎一处 |
| 3 | 数据增长 | 用户词库文本追加，7 天热度衰减；词库随版本整体更新 |
| 4 | 并发 | 输入事件串行；DICT 全局 Mutex + TEST_DICT_LOCK（测试串行防竞争）；bigram 用 RwLock，批量查询减少取锁 |
| 5 | 失败 | FFI panic 守卫返回 fallback；词库损坏/`.bin` 签名不符降级重读；外部依赖（TSF）由系统提供 |
| 6 | 可观测 | log.rs 分级 + debug_log + FFI panic 自动捕获；组合串每次推送落日志 |
| 7 | 可回退 | 配置回退（删用户配置即默认）；词库回退（VERSION.json 版本可追溯）；降权词可恢复 |
| 8 | 多端一致性 | TSF + IMM32 双入口共用同一 Engine（ffi.rs）与同一组合串拼装规则——业务层单一 |
| 9 | 版本兼容 | 词库 VERSION.json 版本化；`.bin` 由引擎从 db 重建（头部含源库长度+mtime 签名），格式不兼容即重建 |

## 七、决策记录（v2 → v3）

| 决策 | 选项 A | 选项 B | 选择 | 原因 |
|------|--------|--------|------|------|
| 双仓库分工 | 单一仓库（词库构建 + 引擎应用耦合） | **taishen-dict / taishenIME 双仓库** | B | 词库有自己的迭代周期（月度），不绑 IME 发布；IME 只专注引擎与应用，职责边界清晰（2026-08-10 分家） |
| 词库构建 | 本仓库工具（搜狗/raw_dict） | **taishen-dict 独立管线** | B | 词库迭代周期独立，许可干净（MIT/CC BY-SA），版本可追溯（2026-08-10 分家） |
| 词库结构 | 单表 system_dict | **简繁双表 + common rank 表** | B | 简繁双体支持；P2 优先级表人工可控 |
| 候选排序 | 词频混排 + 阈值补丁 | **P1→P5 分层 pick** | B | 低层级永不插队，首屏稳定（V0.5.5） |
| 简繁处理 | 引擎运行时转换 | **构建时源头分集** | B | 词库层面隔离，引擎查询不背转换包袱（V0.5.6） |
| 呈现 | Skia 跨平台（预留） | **Direct2D 自绘 ui_* 控件库** | B | 零依赖，主题系统成熟（V0.3.6） |
| 平台层 | 单 TSF DLL | **TSF + IMM32 双 DLL** | B | 老游戏/老应用兼容（V0.6） |
| 词库版本 | 无 | **VERSION.json + hash 对账** | B | 同步可校验，双边一致可证明（V2026.08.10.1） |

### 决策记录（v3.1 增补，2026-11-15 / 11-16）

| 决策 | 选项 A | 选项 B | 选择 | 原因 |
|------|--------|--------|------|------|
| 长句解码 | DFS 拆段 + 段数少优先 | **音节图 Viterbi + 束搜索（`sentence.rs`）** | B | 分词正确性依赖整句联合打分；局部贪心会把「几个人 / 在」拼成「几个认栽」 |
| bigram 用途 | 只服务拆组 | **同时接入主候选排序（与学习/先验/内置表取 max）** | B | 装了搭配表主排序无感；取 max 而不再相加，避免同一共现被计两遍 |
| 组词候选 | 整句拆组 + 当前音节 | **三级：剩余整句 → 逐级前缀整词 → 当前音节单字** | B | 旧实现产出「有管腺」类怪词，而剩余拼音里真实存在的词从未被查询 |
| 组词状态归属 | 平台层维护已选前缀副本 | **引擎唯一真相源（`compose_prefix` / `compose_undo`）** | B | 双份状态不同步直接导致已选字消失与重复上屏 |
| 组合串拼装 | 6 处分别拼 | **`BuildCompositionText` + `SyncComposition` 单一入口** | B | 翻页/退格/提交各走各的拼装，必然有路径漏掉已选前缀 |
| 候选 UIElement | 宿主 `show=FALSE` 即不自绘 | **无条件注册；按「宿主是否读过候选数据」决定是否兜底自绘** | B | `show=FALSE` 只表示不让系统画，不代表宿主自己会画（游戏经 msctfime 读数据） |

### 已知问题（非本轮引入）

- 引擎在 word2/word3 区间连续查询 3000+ 次后偶发崩溃（累积型内存损坏，哈希布局相关，
  无 panic 日志）——真实部署（逐次选词上屏）不触发；后续单独排查。
