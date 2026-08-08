# SPEC: 符号输入 v 模式（Root #2，V0.2.17）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.17 符号输入 v 模式
> 对标 rime-ice symbols.schema.yaml（60+ 分类 2000+ 符号），本期覆盖最常用 4 类

---

## 一、需求

输入 `v` + 分类码 → 候选列出该分类常用符号，选中上屏。

**用户价值**：→ ← ≈ ℃ ㎡ 等符号不用切输入法/翻符号面板，盲打直达。

**合约**：
- 触发方式：`v` 开头（拼音中 v 极少作首字母，天然无冲突；微软拼音/搜狗同款）
- 分类码：拼音首字母缩写，如 `vjt`（箭头 jian tou）、`vsx`（数学 shu xue）
- 首期 4 类：箭头 / 数学 / 单位 / 标点，每类 10-20 个高频符号
- 候选显示：符号本身（候选窗口无 comment 机制，符号自解释）
- 选中上屏：**不学习用户词**（符号非拼音词）
- 退格/翻页/Esc 行为与拼音一致（v 是普通字符，走同一状态机）
- v 模式不干扰拼音：v 开头的拼音极少（无合法音节以 v 开头），`v` 单独输入时走拼音查询（大概率无候选，符合预期）

**不做**：
- 60+ 全量符号分类——后续逐步扩展
- 候选带中文注释（如「→ 右箭头」）——需候选窗口 comment 机制，后续
- 符号联想/模糊匹配——精确分类码

## 二、数据模型

### 符号表（内置常量，4 类 × 10-20 个）

```
箭头 jt：→ ← ↑ ↓ ↔ ⇒ ⇐ ⇔ ➜ ↵
数学 sx：≈ ≠ ≤ ≥ ± × ÷ ∞ ∑ ∏ √ ° ′ ″
单位 dw：℃ ℉ ㎡ ㎞ ㎏ ㎝ ㎜ μ Ω § №
标点 bd：· ― … — 『』「」《》〈〉〖〗【】
```

### 内存结构

```
无新增状态字段——v 模式通过 pinyin_buf 首字符 'v' 判定
分类表：static 常量 HashMap<&str, &[&str]>（分类码 → 符号列表）
```

## 三、接口

### Engine（lib.rs）

```rust
// 新增方法（无新字段）
pub fn is_symbol_mode(&self) -> bool;   // pinyin_buf 以 'v' 开头
```

### 查询集成（query_all 最前）

```
若 pinyin_str 以 'v' 开头且长度 > 1：
  category = pinyin_str[1..]
  candidates = symbol::query(category)   // 精确匹配分类码
  symbol_candidate_pos = 记录首个符号位置（选词不学习）
  return（跳过拼音/短语/混输等全部后续逻辑）
```

### 模块

```
engine/src/symbol.rs
  pub fn query(category: &str) -> Vec<&'static str>  // 空分类码返回空
```

### FFI

```
无需新接口——process_key/select_candidate/backspace/page 全部复用
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | symbol.rs：4 类符号表 + query | engine/src/symbol.rs | cargo test |
| 2 | lib.rs：symbol_mode 判定 + query_all 分支 + 选词不学习 | engine/src/lib.rs | cargo test |
| 3 | 单元测试：vjt/vsx/vdw/vbd + 非 v 前缀不受影响 | engine/src/lib.rs | cargo test |
| 4 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- 输入 vjt → 候选含 → ← ↑ ↓
- 输入 vsx → 候选含 ≈ ≠ ≤
- 输入 vdw → 候选含 ℃ ㎡
- 输入 vbd → 候选含 · ―
- 输入 v + 未知分类码 → 无候选
- 输入 zhong → 正常拼音候选（v 前缀不影响）
- 选中符号上屏 → 不学习用户词

## 六、v 前缀即时反馈（0.2.30）

> 关联 DEV-TRACKER: 0.2.30
> 触发场景：用户按 v 期待符号输入，当前只显示英文候选（value/version/vue），无符号提示。

### 需求

按 `v`（单个字符，未输分类码）时，候选窗立即列出**热门符号**（跨分类精选），选中即上屏。
继续输入分类码（`vbd`/`vjt`…）行为不变。

**用户价值**：按 v 即有符号反馈，不用背分类码也能直达最高频符号（→ ← 「」《》≈ ℃）。

**合约**：
- 触发：`pinyin_buf == "v"` 且非双拼（双拼下 v 是 zh 声母，排除）
- 候选内容：`symbol::hot_symbols()` — 箭头/数学/单位/标点四类高频精选（~20 个，翻页可达）
- 选中上屏：**不学习用户词**（与 vbd 符号模式一致，is_symbol 判定扩展覆盖 prefix 态）
- 覆盖英文候选：v 单字符不再出 value/version 等英文候选（v 作拼音首字母价值低，英文混输在真实拼音场景仍可用）
- 退格/翻页/Esc 与拼音一致

**不做**：
- 分类菜单选中展开（需平台层联动刷新候选窗，收益/成本比低）
- v 前缀提示文字（候选窗无 comment 机制）

### 实现

```
engine/src/symbol.rs
  pub fn hot_symbols() -> Vec<&'static str>  // 精选常量表

engine/src/lib.rs
  pub fn is_symbol_prefix(&self) -> bool     // !shuangpin && pinyin_buf == "v"
  query_all() 最前分支：is_symbol_prefix → hot_symbols 候选（跳过英文混输）
  select_candidate()：is_symbol 判定扩展为 is_symbol_mode() || is_symbol_prefix()
```

### 测试

- 输入 v → 候选含 → ℃ 「」等热门符号
- v 前缀选中符号上屏 → 不学习、pinyin_buf 重置
- 输入 vjt → 仍出箭头分类（原行为不变）
- 输入 zhong → 正常拼音（不受影响）
- 双拼模式 v → 走 zh 声母（无热门符号）
