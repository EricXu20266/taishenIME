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

**合约（2026-11-20 统一裁决口径）**：
- 默认开启（快速打字用户收益大），config.ini 可关
- **生成阶段只追加、不插队**；排位由 `apply_word_len_match` 统一裁决：
  - 精确键段存在可信中文词（用户词 ∪ 常用词 ∪ 精确键词频 ≥ 1600）时，`Correction` 强制降组（`group = 3`），绝不抢占精确词首位（`suanfa` / `houxuan`）；
  - 仅当精确键段无可信中文词时（`zhonggou` / `zhongguoo` / `nihaoa` / `xianashi` / `nihap` / `logn`），常用或高频纠错词允许提升（`CORR_PROMOTED`）胜过非词 `Combo`/`Sentence`
- **明确英文词屏蔽**：输入命中英文词典且无精确可信中文词时（`api` / `email` / `app`），跳过全部拼音纠错分支
- 支持模糊音 + 纠错叠加（先纠错变体，再对变体走模糊音查询）

**不做**：
- 跨音节远距字符重排（如 `nihao → nih ai` 复杂错位）——后续
- 用户学习纠错偏好（纠错命中不写入用户词库）
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

四类纠错变体统一产出 `Source::Correction`，并在 `phrase_group_guess` 的 B 阶段复用（B 阶段优先取常用词表，再以系统词库补足，`zhegeweomende → 这个我们的` 已通过 `test_group_weom_to_wom` 守护）：

1. **单键替换**：任一位字符 → 相邻键（覆盖手指误触，如 `nihap → nihao`）
2. **相邻交换**：任两位相邻字符互换（覆盖打字顺序错，如 `logn → long → 龙`）
3. **多打删除**（`deletion_variants`）：删除单字符（如 `zhongguoo → zhongguo`、`nihaoa → nihao`、`xianashi → xianshi`）
4. **漏打插入**（`insertion_variants`）：插入单字符（如 `meiganxi → meiguanxi`）

生成上限：每类最多 N 个变体（防膨胀，N=8），总计 ≤16 个变体。

### 命中条件

```
query_all 流程：
  精确候选 = dictionary::query_with_split（标注 EXACT_PINYIN 与 CREDIBLE）
  若为明确英文词且无可信精确中文词 → 跳过全部拼音纠错
  否则按候选数量 / 可信状态追加拼写、删除、邻键、插入变体（仅追加不插队）
  最终位置由 apply_word_len_match 统一裁决
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
