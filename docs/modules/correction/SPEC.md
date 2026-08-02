# SPEC: 智能纠错（Root #2，V0.2.10）

> 对应 ARCHITECT.md Root #2「业务领域层 — 拼音处理」
> 关联 DEV-TRACKER: 0.2.10 智能纠错（错键纠正，logn→龙）
> 前置：0.1.14 模糊音模块（fuzzy.rs）同构实现

---

## 一、需求

用户快速打字时易错键（相邻键误触），如 logn→long→龙、nihap→nihao→你好。
智能纠错在精确查询无结果（或结果少）时，对输入串生成**键盘相邻键位变体**，变体命中词条补入候选。

**与模糊音的区别**：
- 模糊音 = 发音相近（z↔zh、an↔ang）——读音层面
- 智能纠错 = 键位相邻（l↔k、o↔p）——手指误触层面

**合约**：
- 默认开启（快速打字用户收益大），config.ini 可关
- 变体排在精确命中之后（不干扰正确输入）
- 仅对**无精确候选或候选不足**时生成（避免干扰正常候选）
- 支持模糊音 + 纠错叠加（先纠错变体，再对变体走模糊音查询）

**不做**：
- 前后字互换（如 nihao→nih ai 这种复杂错位）——后续
- 用户学习纠错偏好——后续
- 双拼场景纠错——一期仅全拼（双拼码短且歧义大）

## 二、算法

### QWERTY 相邻键位表

```
每行相邻 + 上下行斜邻（标准键盘相邻判定）
q-w-e-r-t-y-u-i-o-p
 a-s-d-f-g-h-j-k-l
  z-x-c-v-b-n-m
```

生成 `near: HashMap<char, Vec<char>>`：
- 同行左右相邻：`w`→[q,e]，`s`→[a,d]...
- 上下行斜邻：`w`→[s,x]?（w 下方是 s/x 之间——标准判定取下方两键 + 对角）

**简化判定**（与主流输入法一致）：
- 同一行：左右相邻
- 下一行：正下方 + 左下/右下（斜邻）
- 上一行：正上方 + 左上/右上

### 变体生成（与 fuzzy_variants 同构）

```
输入 "logn"（想打 long）
  → 逐位尝试替换为相邻键：
    l→k、l→o、l→p、o→i、o→p、o→l、g→f、g→h、n→b、n→m
  → 单键替换（一次只换一个字符，不递归）：
    "logn" → "long"（o→i？不——是 g 位换成... 实际 logn→long 是 o 与 g 位置错？
    
修正：logn 的错因是 "o" 和 "n" 打反了？不——logn 想打 long，是 g 误触 n（g/n 斜邻）
  → g→n 不在相邻？g 的相邻是 f,h,t,y,v,b... n 在 b 的右侧，g 右下是 b，b 右是 n——g 和 n 是"右右下"两步，不算相邻。
  → 换一种：logn 实际是 n 和 g 换位（交换相邻字符）。纠错支持"相邻字符交换"！
```

### 两类变体

1. **单键替换**：任一位字符 → 相邻键（覆盖误触）
2. **相邻交换**：任两位相邻字符互换（覆盖打字顺序错，logn→long 即 n/g 交换）

生成上限：每类最多 N 个变体（防膨胀，N=8），总计 ≤16 个变体。

### 命中条件

```
query_all 流程：
  精确候选 = 整词 + 简拼
  若精确候选为空 或 数量 < page_size（候选不足）：
    生成纠错变体 → 变体查词库（+ 变体走模糊音）→ 补入候选
```

## 三、接口

### Rust 内部（新模块 engine/src/correction.rs）

```rust
/// 相邻键映射（QWERTY）
pub fn nearby_keys(ch: char) -> Vec<char>;

/// 生成纠错变体（单键替换 + 相邻交换），去重，最多 MAX_VARIANTS 个
pub fn correction_variants(input: &str) -> Vec<String>;

/// 是否需要纠错（快速预判：长度 >= 3 且含可替换字符）
pub fn may_need_correction(input: &str) -> bool;
```

### Engine 状态（lib.rs）

```rust
// 新增字段
correction_enabled: bool,  // 默认 true
```

### FFI

```rust
engine_set_correction(i32) -> i32  // 开关
engine_get_correction() -> i32      // 查询：1 开 / 0 关 / -1 未初始化
```

### config.ini

```
correction=1   # 智能纠错开关（1/true/on 开，0/false/off 关）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | correction.rs：相邻键表 + 变体生成 | engine/src/correction.rs | cargo test（logn→long） |
| 2 | lib.rs：correction_enabled 字段 + query_all 接入 | engine/src/lib.rs | cargo test |
| 3 | FFI engine_set/get_correction | engine/src/ffi.rs | cargo test |
| 4 | config_reader 解析 correction | platform/windows | 冒烟测试 |
| 5 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- logn → long（相邻交换：n/g 换位）→ 龙
- nihap → nihao（单键替换：p/o 相邻）→ 你好
- 精确命中不干扰：nihao 正常出"你好"，不触发纠错
- 开关：关后 logn 无"龙"
- 与模糊音叠加：纠错变体走模糊音
