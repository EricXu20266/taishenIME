# SPEC — 候选右键管理（置顶/取消置顶/降权）+ 词级简拼自动展开

> 对应 DEV-TRACKER：候选右键管理
> 版本：V0.5.7 目标
> 日期：2026-08-10
> 关联：docs/reference/candidate-ranking-logic.md（V0.5.5 分层模型）
> 竞品参照：雾凇 pin_cand_filter（置顶）+ cold_word_drop（删词/降频）——Eric 2026-08-10 决策：砍掉降频中间档，右键只做「置顶/取消置顶/降权」

---

## 一、需求

### 1.1 候选窗右键菜单（UI + 引擎）

候选窗口（TSF + IMM32 共用 CCandidateWindow）上，**鼠标右键某个候选词**，弹出菜单：

| 菜单项 | 语义 | 触发条件 |
|--------|------|----------|
| 置顶 / 取消置顶 | 该词进入 pin 集合，任何输入方式（全拼/简拼/混合）候选里出现即提到最前；再右键显示「取消置顶」 | 词未置顶 → 置顶；已置顶 → 取消置顶 |
| 降权 / 恢复候选 | 该词压出前 2 屏（第 11 位之后），仍可翻页取到；再右键显示「恢复候选」 | 词未降权 → 降权；已降权 → 恢复 |

**用户决策（2026-08-10）**：
- ✅ 置顶 = **词级**（非编码级）：nihao/nih/nh 任何输入，候选里出现「你好」即置顶。天然覆盖简拼自动展开，无需反查拼音生成展开键
- ✅ 「删除」语义 = **降权**（压出前 2 屏），非物理删除，可恢复
- ❌ 砍掉雾凇式"降频到第 4 位"中间档——用户判定实际应用无用

### 1.2 简拼自动展开（词级置顶的自然结果）

现有 `query_short`/`query_mixed`/`query_abbrev_full` 已能通过简拼索引返回「你好」（nih/nh 输入时候选已含该词）。词级置顶把候选里命中 pin 集合的词提到最前 → nihao/nih/nh/nha 等全部自动生效，**引擎无需为每个词反查完整拼音生成展开键**。

---

## 二、引擎层设计（Rust，engine/src/lib.rs + dictionary/mod.rs + ffi.rs）

### 2.1 数据结构（lib.rs Engine）

```rust
/// 词级置顶集合（V0.5.7）：候选里命中该集合的词提到最前。
/// 替代原 pin_map 的编码→词语义——内置 d→的/m→吗,嘛/hm→后面 也迁移为词集合。
pin_words: std::collections::HashSet<String>,
/// 降权集合（V0.5.7）：候选里命中该集合的词压到第 11 位之后（前 2 屏之外）。
demoted_words: std::collections::HashSet<String>,
```

- 移除 `pin_map: HashMap<String, Vec<String>>`（原编码级），`builtin_pins()` 改为 `builtin_pin_words() -> HashSet<String>`（的/吗/嘛/后面）
- `load_pins(entries)` 签名改为 `load_pin_words(Vec<String>)`，外部加载的词直接并入集合

### 2.2 查询收尾（query_all，插在 apply_context_boost 之后、apply_word_len_match 之前）

```
① apply_pin_boost：候选里命中 pin_words 的词提到最前（保持相对顺序），
   复用 apply_context_boost 同款"收集命中+其余"两分区逻辑
② apply_demote：候选里命中 demoted_words 的词从原位移除，追加到第 11 位
   （page_size*2+1）之后；英文候选区不参与
```

**顺序理由**：pin 置顶在词长分区前（置顶词不受词长分区重排影响，始终第一）；demote 在 pin 之后（被置顶的词不降权，两集合互斥由 UI 保证）。

### 2.3 FFI 新增（ffi.rs，全部带 ffi_guard）

```rust
engine_pin_word(word: *const c_char) -> i32        // 置顶；成功后 query_all
engine_unpin_word(word: *const c_char) -> i32      // 取消置顶
engine_demote_word(word: *const c_char) -> i32     // 降权（压出前 2 屏）
engine_undemote_word(word: *const c_char) -> i32   // 恢复候选
engine_is_pinned(word: *const c_char) -> i32       // 1/0，菜单动态显示
engine_is_demoted(word: *const c_char) -> i32      // 1/0
```

### 2.4 持久化（dictionary/mod.rs，user_dict.db 加表）

```sql
CREATE TABLE IF NOT EXISTS pin_words (
    word TEXT PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS demoted_words (
    word TEXT PRIMARY KEY
);
```

- `load_user_dict(path)`：建表 + 加载两集合（load_user_dict 之后调用 engine 侧注入）
- pin/demote 操作时 INSERT OR REPLACE / DELETE，随 user_dict.db 一起持久化
- 引擎侧提供 `load_pin_words`/`load_demoted_words` 注入入口（对齐 load_pins 现状）

---

## 三、C++ 层设计（platform/windows）

### 3.1 右键事件通路（ui_window + ui_control + candidate_window）

| 文件 | 改动 |
|------|------|
| `ui_control.h` | `UIControl` 增加 `virtual void OnRightClick(int x, int y);`（默认空） |
| `ui_window.cpp` | `HandleMessage` 增加 `WM_RBUTTONDOWN/WM_RBUTTONUP` → `DispatchMouse` 支持右键（left=false 分支）；`WM_RBUTTONDOWN` 记 `m_pressedCtrl`，`WM_RBUTTONUP` 命中同控件 → `OnRightClick` |
| `candidate_window.h/.cpp` | `CandidatePanel` 重写 `OnRightClick`：`CandidateAt(x,y)` 命中 → 回调 `RightClickCallback(index)`；`CCandidateWindow` 增加 `SetRightClickCallback` |

### 3.2 菜单（tsf_module.cpp + imm32_ime.cpp）

- `CTextService` 注册右键回调：`OnCandidateRightClicked(int index)`
  - 取 `m_candidates[index]` 文本 → `engine_is_pinned`/`engine_is_demoted` 判菜单状态
  - `TrackPopupMenu` 原生菜单（WM_APP 模式，候选窗 WS_EX_NOACTIVATE 不抢焦点）：
    - 未置顶 → 「置顶」；已置顶 → 「取消置顶」
    - 未降权 → 「降权（移出前两屏）」；已降权 → 「恢复候选」
  - 选择 → 调对应 FFI → `UpdateCandidateWindow()` 刷新
- `imm32_ime.cpp`：同款右键回调（g_candidateWindow 已共享 CCandidateWindow 类）

---

## 四、验证

| 用例 | 预期 |
|------|------|
| `nihao` 右键「你好」→ 置顶 | nihao/nih/nh/nha 输入，候选里「你好」均第 1 位 |
| 已置顶词再右键 | 菜单显示「取消置顶」，点击后恢复原排序 |
| 右键「侧室」→ 降权 | 输入 ceshi，前 10 位无「侧室」，翻页可见 |
| 已降权词再右键 | 菜单显示「恢复候选」，点击后回到原位置 |
| 重启进程 | pin_words/demoted_words 从 user_dict.db 恢复 |
| 内置置顶回归 | d→的、m→吗/嘛、hm→后面 行为不变（迁移为词集合后） |
| cargo test + cargo build + biome | 零错误 |

---

## 五、风险与取舍

- `pin_map` 编码级→词级迁移：内置 3 条语义不变（d/m/hm 单音节，词级命中等价）。外部 `load_pins` 调用点仅测试代码（FFI 未导出、C++ 未接线），无兼容负担
- 词级置顶副作用：某词被置顶后，任何输入下候选含该词即前置——用户明确接受（"nihao nih nh nha 这种"即此语义）
- demote 只压前 2 屏不物理删除：与用户「降权」定义一致，可恢复
