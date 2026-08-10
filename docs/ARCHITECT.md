# 泰深输入法 — ARCHITECT.md（v3）

> 架构骨架 · 2026-08-10 更新
> 基于当前代码实现 + 2026-08-10 词库分家（taishen-dict 独立）重写。
> v2（2026-07-28 初版）见 [archive/architecture-v2.md](archive/architecture-v2.md)。

---

## 〇、项目定位（v3 核心变化）

**双仓库分工（2026-08-10 厘清）**：

```
taishen-dict（词库唯一构建方，独立仓库）
    curate/ 人工源 → pipeline.py（构建+校验+版本清单）→ sync_to_ime.py（hash 对账同步）

taishenIME（输入法引擎 + 应用，本仓库）
    只消费 resources/ 词库 + VERSION.json；不构建、不管理词库
    引擎加载 → 候选 → 上屏；部署期编译 .bin 预编译索引
```

本仓库不出现任何词库构建脚本（历史遗留归档于 `tools/archive/`，勿运行）。

## 一、拓扑分析（1+3+3+场）

```
心跳(1): Windows TSF 事件循环 → 注入点 tsf_keyevent.cpp:ITfKeyEventSink::OnKeyDown
         输入链：tsf_module（COM 注册）→ tsf_keyevent（按键）→ tsf_composition（组合/提交）

三角:
  前台 — platform/windows/   TSF 平台层 + Direct2D 自绘 UI（候选窗/设置窗/横幅/工具栏）
  后台 — engine/src/         21 模块拼音引擎 + 词库查询（纯 Rust，平台无关）
  数据库 — resources/system_dict.db + domains.db + common.db + %APPDATA% 用户词库

三条流:
  通道 — C FFI 51 函数（engine_bridge.cpp ↔ ffi.rs，全部 ffi_guard! 防 panic 跨边界）
  跟踪 — engine/src/log.rs（INFO/WARN/ERROR 分级）+ platform debug_log
  呈现 — Direct2D/DirectWrite 全自绘（ui_* 控件库 + theme 主题系统）

场:
  安全 — 输入内容不落盘明文，不联网
  可靠 — FFI panic 守卫（ffi_guard!）、词库损坏降级、DICT 并发 Mutex
  性能 — .bin 预编译索引秒加载、候选查询 <5ms、词长分区稳定首屏
  配置 — engine 级开关（双拼/模糊/简繁/符号/emoji/纠错）+ 平台设置窗口
  生命周期 — NSIS 安装器（package.ps1 打包：校验词库 → CMake → NSIS）
```

## 二、引擎架构（engine/src，21 模块）

```
lib.rs            Engine 状态机：输入模式（拼音/英文/混合）、候选缓冲、模式开关
                    （set_traditional/set_shuangpin/set_fuzzy/set_ascii_mode...）
ffi.rs            C ABI 51 函数：engine_init → process_key → get_candidate → select_candidate
                    → take_char；全部 ffi_guard! 包裹
pinyin/mod.rs     音节表 + 切分算法（to_initial_string 混词简拼兜底 bzhan→bz）
dictionary/mod.rs 词库查询核心（P1→P5 分层，见下）
correction.rs     输入纠错（zhonggou→中国；deletion 候选）
mistake.rs        易错字/音纠错
fuzzy.rs          模糊音（前后鼻音/平翘舌）
radical.rs        部首输入
shuangpin.rs      双拼（多方案）
symbol.rs         符号表（v 前缀，雾凇 symbols_v.yaml 生成）
emoji.rs          emoji 输入（v 前缀扩展）
english.rs        英文候选 / mix_mode 混合输入
calculator.rs     计算器（v 前缀扩展）
datetime.rs       日期时间（v 前缀扩展）
number.rs         数字大写（v 前缀扩展）
unichar.rs        Unicode 输入
context.rs        上下文（context_boost）
trad.rs           简→繁转换 + 歧义词组（系统→系統/关系→關係）+ is_traditional
trad_full.rs      GB2312 一级 1313 字简→繁全表（zhconv 生成）
trad_simp.rs      繁→简映射表（加载层简繁归一化，1698 字）
log.rs            分级日志（FFI panic 捕获写入）
```

### 词库分层查询（dictionary/mod.rs，V0.5.5 重构）

```
P1 用户词库（热 > 温 > 过期，7 天衰减）
P2 common.db（common_words rank 行序 = 人工优先级，592 条）
P3 system_dict.db（37.3 万简体，pinyin 索引，词频降序）
P4 domains.db（16.3 万简体，领域热度 > 词长 > 原序）
P5 联想兜底（纠错/模糊/简拼/组词）
```

- 简繁分集（V0.5.6）：`system_dict_trad` / `domain_words_trad` 繁体原文表，
  繁体模式优先原生繁体（限量 8 前置），无则简体转繁
- 词长匹配分区：完整拼音输入时"字数==N"稳定占前 4 位
- pin 置顶 > 词长分区 > 分层取词

## 三、平台架构（platform/windows，C++17 + TSF）

```
tsf_module.cpp     COM 注册/注销、ITfTextInputProcessor、KLID 注册
tsf_keyevent.cpp   按键链（ITfKeyEventSink），路由给引擎 + 编辑会话
tsf_composition.cpp 组合串管理、文本提交（ITfInsertAtSelection）
engine_bridge.cpp  Rust FFI 51 函数桥接（CStr/字符串编解码）
candidate_window.cpp 候选窗（Direct2D，跟随光标）
banner_window.cpp  状态横幅
settings_window.cpp 设置窗口
theme.cpp          主题系统（浅色/深色 + 自定义色）
ui_* 控件库         window/layout/label/button/edit/checkbox/combobox/
                    colorpicker/scrollbar/tab/render —— 全自绘控件体系
app_state.cpp      应用状态聚合（模式开关/主题/词库路径）
config_reader.cpp  配置读取（安装目录 config.yaml）
debug_log.cpp      平台侧日志
dllmain.cpp        DLL 入口
```

辅助层：`taishen_ime_imm32.ime`（V0.6 IMM32 兼容层，老游戏/老应用，独立 DLL，
复用 TSF 输入链 + 候选窗，KLID E0C00804）。

## 四、数据流

```
用户按键
  ↓
Windows TSF → tsf_keyevent OnKeyDown → engine_bridge.cpp
  ↓
FFI: engine_process_key(char) [ffi_guard!]
  ↓
Engine::process_key() → 拼音缓冲累积（模式路由：拼音/英文/符号/特殊模式）
  ↓
dictionary 分层查询（P1→P5 逐层取词）→ 候选列表
  ↓
FFI: engine_get_candidate() → tsf_composition 组合串更新
  ↓
candidate_window.cpp Direct2D 渲染候选窗（跟随光标）
  ↓
用户选择 → FFI: engine_select_candidate() → take_char()
  ↓
TSF ITfInsertAtSelection 提交到目标应用
  ↓
（选词即学：learn_user_word 写用户词库；领域词命中热度 +1）
```

## 五、词库数据（分家后）

| 词库 | 文件 | 规模 | 来源 | 加载 |
|------|------|------|------|------|
| 系统词库 | resources/system_dict.db | 简体 37.3 万 + 繁体 1.7 万 | taishen-dict（jieba+wiki） | engine_init + .bin 预编译索引 |
| 领域词库 | resources/domains/domains.db | 简体 16.3 万 + 繁体 1.8 万 | taishen-dict（wiki+THUOCL+人工源） | load_domains_from_db |
| 常用词库 | resources/common.db | 592 条（rank 行序） | taishen-dict（curate/common_dict.txt） | P2 层 |
| 用户词库 | %APPDATA%/taishen-ime/user_dict.txt | 动态 | 选词即学 | learn_user_word |
| 版本指针 | resources/VERSION.json | — | sync_to_ime.py 写入 | 打包校验 |

词库版本 = VERSION.json（如 V2026.08.10.1）。缺失/过期时运行 taishen-dict 的
`python tools/sync_to_ime.py`，不在本仓库构建。

## 六、目录结构

```
taishenIME/
├── engine/                        # Rust 引擎（平台无关）
│   ├── Cargo.toml                 # cdylib + staticlib
│   └── src/                       # 21 模块（见第二节）
├── platform/windows/              # C++17 + TSF + Direct2D
│   ├── CMakeLists.txt
│   ├── include/                   # 24 个头文件
│   └── src/                       # 25 个源文件（见第三节）
├── resources/                     # 词库（taishen-dict 同步，本仓库不构建）
│   ├── system_dict.db + .bin      # 系统词库 + 预编译索引
│   ├── domains/                   # 领域 txt（可读源）+ domains.db
│   ├── common_dict.txt + common.db
│   ├── VERSION.json               # 词库版本指针
│   ├── archive/raw_dict.txt       # 早期手工词表（归档）
│   └── rime_ice/                  # 雾凇参考词库（gitignore，非泰深产物）
├── install/                       # NSIS + PowerShell 安装器
├── package.ps1                    # 一键打包（校验词库 → CMake → NSIS）
├── tools/
│   ├── archive/                   # 历史词库构建脚本（已移交 taishen-dict，勿运行）
│   └── （引擎侧工具：符号表生成/FFI 测试等，保留）
└── docs/
    ├── ARCHITECT.md               # ← 本文件（v3）
    ├── archive/architecture-v2.md # v2（2026-07-28 初版）
    ├── business-flow.md / DEV-TRACKER.md / CHANGELOG / index.md
    ├── modules/                   # 模块 SPEC
    └── reference/                 # 参考资料
```

## 七、关键设计决策（v2 → v3 演进）

| 决策 | v2（初版） | v3（现状） | 原因 |
|------|-----------|-----------|------|
| 词库构建 | 本仓库工具（搜狗/raw_dict） | **taishen-dict 独立管线** | 词库迭代周期独立，许可干净（MIT/CC BY-SA），版本可追溯 |
| 词库结构 | 单表 system_dict | **简繁双表**（简体+繁体原文）+ common rank 表 | 简繁双体支持，P2 优先级表 |
| 候选排序 | 词频混排 | **P1→P5 分层 pick** + 词长分区 + pin 置顶 | 低层级永不插队，首屏稳定 |
| FFI | 9 函数 | **51 函数** + ffi_guard! | 功能扩展（双拼/模糊/简繁/符号/emoji/计算器） |
| UI | Skia 预留 | **Direct2D 自绘 ui_* 控件库** | 零依赖，主题系统（浅/深/自定义色） |
| 平台层 | 单 TSF DLL | **TSF + IMM32 双 DLL**（V0.6） | 老游戏/老应用兼容 |
| 词库版本 | 无 | **VERSION.json + hash 对账** | 同步可校验，双边一致可证明 |

## 八、运行时与可靠性

| 维度 | 决策 |
|------|------|
| 启动 | .bin 预编译索引秒加载（engine_build_index 部署期生成） |
| 状态 | 单进程单用户；用户词库持久化，重启恢复 |
| 并发 | DICT 全局 Mutex + TEST_DICT_LOCK（测试串行防竞争） |
| 失败 | FFI panic 守卫返回 fallback；词库损坏降级 |
| 可观测 | log.rs 分级 + debug_log；FFI panic 自动记录 |
| 已知问题 | word2/word3 连续查询 3000+ 次偶发内存损坏（未修，真实部署不触发） |
