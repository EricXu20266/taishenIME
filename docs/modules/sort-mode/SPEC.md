# SPEC — 候选排序切换（P0-2，对标微软单字/长词优先）

> 模块：engine（Rust 核心）+ platform/windows（C++ 配置与传递）
> 日期：2026-08-08
> 状态：已批准实现

## 一、需求

微软拼音提供两种候选排序方式：**单字优先**（单字排在词组前面）与**长词优先**（词组排在单字前面），用户可在属性中切换。泰深当前仅有固定的「长词过滤」规则（P2-2 apply_long_word_filter，对标 rime long_word_filter），用户无法切换排序策略。

**目标**：新增用户可配置的候选排序模式，提供三档：
- `default`（默认）：词频序 + 现有长词过滤（现状不变）
- `single_first`（单字优先）：单字候选提到词组前
- `long_first`（长词优先）：长词（2+ 字）提到单字前

## 二、接口设计

### 引擎层（engine/src/lib.rs）

```rust
// Engine 新增字段
sort_mode: i32,  // 0=default 1=single_first 2=long_first

// 新增方法
pub fn set_sort_mode(&mut self, mode: i32)  // 非法值 clamp 到 0..=2
pub fn sort_mode(&self) -> i32
```

排序应用位置：`update_candidates` 末尾（现有 `apply_long_word_filter` 调用之后）：
- mode=0：不动（保留现有行为）
- mode=1：稳定分区，单字在前
- mode=2：稳定分区，多字在前（跳过英文候选区）

**约束**：
- 分区只作用于汉字候选（英文候选恒在末尾，`english_candidate_pos` 之后不参与）
- 短语/日期/符号等特殊候选不参与重排（它们已在前置位）
- 用户词学习、简繁转换、emoji 追加均不受影响

### FFI 层（engine/src/ffi.rs）

```rust
#[no_mangle] pub extern "C" fn engine_set_sort_mode(mode: i32) -> i32
#[no_mangle] pub extern "C" fn engine_get_sort_mode() -> i32
```

### 配置层（platform/windows）

- `config_reader.cpp`：新增 `sort_mode` 键（int，默认 0），读写支持
- `config.ini.example`：新增示例 `sort_mode=0`
- `tsf_module.cpp`：加载配置时 `engine_set_sort_mode(cfg.sort_mode)`

## 三、数据模型

config.ini 新增键：

```ini
# 候选排序：0=默认（词频+长词过滤） 1=单字优先 2=长词优先
sort_mode=0
```

## 四、实施计划

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | lib.rs：Engine 加 sort_mode 字段 + set/get + 排序逻辑 | cargo build |
| 2 | lib.rs：单测（default/single/long 三档排序断言） | cargo test |
| 3 | ffi.rs：engine_set_sort_mode / engine_get_sort_mode + 测试 | cargo test |
| 4 | config_reader.cpp + config.ini.example + tsf_module.cpp 接线 | cargo build + cmake 编译 |
| 5 | 验证 + commit | 全绿 |

## 五、风险

- 排序分区是稳定变换（partition stable），不改变组内相对顺序 → 词频序保持
- 单字优先可能让"中国"类的常用词降位 → 仅限用户显式开启
- 英文候选区隔离，混输不受影响
