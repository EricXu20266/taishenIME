# SPEC — 上下文联想（P1-1，对标搜狗/微软前文关联候选）

> 模块：engine（Rust 核心）+ platform/windows（C++ 配置传递）
> 日期：2026-08-08
> 状态：已批准实现

## 一、需求

主流输入法（搜狗、微软）的候选排序会参考**已上屏前文**：例如上屏"北京"后，再输入 `da`，"大学"因与前文"北京"构成搭配（北京大学）而前置。泰深当前候选排序完全独立于上下文，输入 `da` 在"北京"后仍按全局词频排。

**目标**：新增上下文联想——引擎维护最近上屏词，候选排序时优先展示能与前文构成搭配的词。

## 二、接口设计

### 引擎层

```rust
// Engine 新增字段
last_committed: String,   // 最近上屏词（供上下文联想，独立于输入态）
context_enabled: bool,    // 上下文联想开关（默认关，config 开启）
context_map: HashMap<String, Vec<String>>,  // 前词 → 后词搭配表

// 新增方法
pub fn set_context_enabled(&mut self, enabled: bool)
pub fn context_enabled(&self) -> bool
pub fn load_contexts(&mut self, entries: Vec<(String, Vec<String>)>)  // 外部加载覆盖内置
```

**上下文状态更新**：`select_candidate` 选中普通中文词时更新 `last_committed`（英文/短语/符号/计算器/日期/拆字等特殊候选不更新）。

**候选前置逻辑**（挂 `apply_sort_mode` 之前）：
1. `last_committed` 非空且 `context_enabled`
2. 查 `context_map[last_committed]` → 得到后词集合
3. 候选列表中命中后词集合的词 → 稳定提到最前（组内相对序不变）
4. 英文候选区（`english_candidate_pos` 之后）不参与

**约束**：
- `reset()` 不清 `last_committed`（上下文跨输入会话）
- 特殊候选（短语/日期/符号）不受影响
- 默认关闭，不改变现有行为

### FFI 层

```rust
#[no_mangle] pub extern "C" fn engine_set_context_assoc(enabled: i32) -> i32
#[no_mangle] pub extern "C" fn engine_get_context_assoc() -> i32
```

### 配置层

config.ini 新增键：
```ini
# 上下文联想（1=开 0=关，默认 0）
context_assoc=0
```

## 三、数据模型

内置搭配表（context.rs），格式：前词 → [后词...]：

```
北京 → 大学、市、时间、人
中国 → 人民、特色、梦、制造
今天 → 天气、晚上、早上、上班
我们 → 的、要、可以、一起
学习 → 能力、成绩、方法、英语
```

外部加载文件（每行 `前词=后词1,后词2`，覆盖内置同前词条目）。

## 四、实施计划

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | context.rs：内置搭配表 + lookup + load | cargo build |
| 2 | lib.rs：last_committed/context_enabled/context_map + 更新逻辑 + 候选前置 | cargo build |
| 3 | 单测：上下文前置/开关/特殊候选不更新/跨 reset 保持 | cargo test |
| 4 | ffi.rs：engine_set/get_context_assoc + config_reader + example + tsf_module | cargo test + cmake |
| 5 | 全量验证 + commit | 全绿 |

## 五、风险

- 默认关闭 → 现有行为零变化，测试无回归风险
- 候选前置是稳定变换，不破坏组内词频序
- 搭配表规模可控（内置精选 + 外部可扩展）
