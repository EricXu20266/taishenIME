# 泰深输入法 — 核心数据流（v2）

> 对齐当前代码实现（2026-08-10）。
> 双平台链路（TSF / IMM32）共用同一 Rust 引擎；词库由 taishen-dict 独立管线构建。

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
                       │ C FFI 边界（51 函数，ffi_guard! 防 panic）
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
│ P4 domains.db（16.3 万，领域热度 > 词长 > 原序）                 │
│ P5 联想兜底（纠错/模糊/简拼/组词）                               │
│                                                                   │
│ 叠加规则:                                                         │
│   词长匹配分区（完整拼音 → 字数==N 稳定占前 4 位）               │
│   pin 置顶 > 词长分区 > 分层取词                                  │
│   简拼 → domain_exact_short_index（wb→微博）                     │
│   繁体模式 → trad 表原生繁体前置（限量 8）+ 简体转繁兜底          │
└──────────────────────┬──────────────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────────────┐
│ 阶段 4: 候选窗口（Direct2D 自绘）                                 │
│                                                                   │
│ engine_get_candidate_count() → 数量（部署默认 9）                 │
│ engine_get_candidate(i) → 逐条取候选                              │
│ candidate_window.cpp（UIWindow + CandidatePanel 框架）            │
│   - 跟随光标（MonitorFromPoint 多屏 + dpi_util 坐标对齐）         │
│   - 拼音串 + 候选列表 + 翻页 + 多行展开                           │
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
│   英文候选: apply_cap 大小写还原（Hello/HELLO）                   │
│   符号/计算器/日期/拆字/错音: 不上屏学习、不污染上下文            │
│   ↓                                                               │
│ TSF: ITfInsertAtSelection 提交文本；IMM32: ImeToAsciiEx 上屏      │
│ 候选窗隐藏，引擎 reset()                                          │
└─────────────────────────────────────────────────────────────────┘
```

## 支线链路

### 退格与光标

```
Backspace → engine_backspace()（删尾音节）
           → pinyin_buf.pop() → query_all() → 候选刷新
Backspace（拼音模式）→ engine_backspace_syllable()（删整音节）
←/→        → engine_move_cursor()（光标在拼音串内移动）
删除候选    → engine_delete_candidate()（用户词库条目删除）
```

### 组词模式（V0.5）

```
长串候选不足 → 触发组词（compose_active）
  逐音节显示候选 → 选中字记录 compose_chars → 推进下一音节
  最后音节 → 组合完整词 learn() 用户词库 → 一次性上屏
  例: taishen → 泰深（逐音节选字后组合学习）
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
    resources/ system_dict.db + domains.db + common.db + VERSION.json
    engine_init 加载 → .bin 预编译索引秒加载（ffi 部署期生成）
    运行时: P1 用户词库（%APPDATA%/user_dict.txt，learn 写入）
            领域热度（record_domain_hit，进程内热数据）
```

## 并发与一致性

- 输入事件串行（TSF OnKeyDown 单线程）；DICT 全局 Mutex 保护词库查询
- 用户词库写入即时落盘（选词即学）；7 天热度衰减（P1 排序）
- 词库版本: resources/VERSION.json（如 V2026.08.10.1），打包校验
- FFI 全部 ffi_guard! 包裹：panic 不跨边界，返回 fallback + 日志
