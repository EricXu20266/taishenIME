# SPEC: 中英混输（Root #2 #4，V0.2.8）

> 对应 ARCHITECT.md Root #2「业务领域层」+ Root #4「状态管理」
> 关联 DEV-TRACKER: 0.2.8 中英混输（不切换直接输英文候选）
> 前置：0.1.9 中英文切换（ascii_mode）

---

## 一、需求

中文模式下，输入英文单词不需要切换模式：拼音候选列表**末尾追加当前输入串的英文候选**，
选中直接上屏英文。如输入 `hello`，候选末尾出现 `hello`，选它上屏 `hello`。

**与 ascii_mode（0.1.9）的区别**：
- ascii_mode = 整个输入会话切英文模式，字母直通上屏
- 中英混输 = 保持中文模式，遇到英文词临时选英文上屏，之后继续打中文

**用户价值**：中英混排场景（"用 hello 打招呼"）不用来回 Ctrl+Space 切换。

**合约**：
- 中文模式下生效（ascii_mode=0）
- 英文候选恒排在候选列表**末尾**（不干扰汉字候选排序）
- 选中英文候选：上屏英文原文，**不学习用户词**（英文不是拼音）
- 输入串含非字母（数字/标点）时不追加英文候选
- 默认开，config.ini 可关

**不做**：
- 英文单词联想补全（输 hel 联想 hello）——需英文词典，后续
- 大小写跟随（首字母大写/全大写）——后续
- 空格断词后继续混输——后续

## 二、算法

```
query_all 流程末尾：
  若 mix_mode_enabled && !ascii_mode && pinyin_buf 全为字母 && pinyin_buf 非空：
    all_candidates.push(pinyin_buf)  // 英文候选恒末尾
    english_candidate_pos = all_candidates.len() - 1  // 记录位置
```

select_candidate(index)：
```
  若 Some(english_candidate_pos) == index：
    返回 pinyin_buf（英文原文），不学习用户词，reset
  否则：
    正常汉字候选 + 学习用户词，reset
```

**位置随翻页变化**：english_candidate_pos 在 repage 后需要重算——
英文候选在 all_candidates 末尾，repage 只切分窗口，pos 指向 all_candidates 索引。
select_candidate 用 all_candidates 索引判断，再映射当前页显示。

## 三、接口

### Engine 状态（lib.rs）

```rust
// 新增字段
mix_mode_enabled: bool,        // 中英混输开关，默认 true
english_candidate_pos: Option<usize>,  // 英文候选在 all_candidates 中的位置

// 方法
pub fn set_mix_mode(&mut self, enabled: bool);  // 开关（变化时重查）
pub fn mix_mode(&self) -> bool;
```

### FFI

```rust
engine_set_mix_mode(i32) -> i32   // 开关
engine_get_mix_mode() -> i32       // 查询：1 开 / 0 关 / -1 未初始化
```

### config.ini

```
mix_mode=1   # 中英混输（0/1，默认 1）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | lib.rs：mix_mode 字段 + query_all 追加英文候选 + select 判断 | engine/src/lib.rs | cargo test |
| 2 | FFI engine_set/get_mix_mode | engine/src/ffi.rs | cargo test |
| 3 | config_reader 解析 mix_mode | platform/windows | 冒烟测试 |
| 4 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- 中文模式输入 hello → 候选末尾含 "hello"，选它上屏 hello
- 上屏英文不学习用户词（用户词库无 "hello" 条目）
- 关闭混输 → 无英文候选
- 英文模式（ascii_mode=1）→ 不追加英文候选（字母直通）
- 输入串含数字 → 无英文候选
