# SPEC: 常用词分层 + 用户词热度学习（Root #1，V0.2.30）

> 对应 ARCHITECT.md Root #1「词库」
> 关联 DEV-TRACKER: 0.2.30 常用词库分层（选词优先级重排）
> 2026-08-03 Eric 需求：常用词优先出现，词库区分常用词库；频繁词（你/我/他/好/好的/这个/那么/嗯/没）应在首位可直接空格上词；用户词需热度学习——单位时间打过 N 次才提优先级，非"打过一次即首位"。

---

## 一、需求与现状诊断

### 现状痛点

system_dict.db 62 万词条，词频全部落在 1369–9999 的**粗糙分档值**（无低频词），同频词大量存在，排序依赖 SQLite 返回顺序，**常用字可能被生僻字压住**：

| 输入 | 现状候选顺序 | 问题 |
|------|-------------|------|
| `en` | 奀、嗯、恩、摁、蒽（全 2000 频） | 「嗯」被生僻字「奀」压在第 2 位 |
| `wo` | 我(9983)、握、窝、卧… | 「我」靠词频侥幸首位，非设计保证 |

### 目标

1. **常用词显式分层**：人工维护高频词表，命中即优先出候选，不受词频分档影响
2. **用户词热度学习**：用户词按"近期使用频率"分热/温两档，单位时间（7 天）累计 ≥ 3 次才升热词压过常用词；偶然打过一次只排系统词前，不干扰常用词首位

## 二、设计

### 2.1 常用词层（common）

新增资源文件 `resources/common_dict.txt`（人工维护，行序 = 优先级）：

```
# 格式: pinyin<TAB>word，行序越靠前越先出候选
wo	我
ni	你
en	嗯
...
```

- 加载为独立 `common_index`（与 user_index 同构：prefix → [(word, rank)]），命中即出候选，**不依赖 system_dict 是否有该词条**
- 同步构建 `common_short_index`（声母索引，简拼 `hd`→好的 生效）
- **不参与 .bin 序列化**（`#[serde(skip)]`）：运行时从 txt 读，用户改词表下次启动生效；旧 .bin 完全兼容，无需重建
- 无 txt 文件时用内置降级词表（Rust 常量兜底）

### 2.2 用户词热度学习（hot/warm）

现有 `user_dict` 表已有 `frequency` + `last_used` 字段，**数据模型零改动**；仅内存 `user_index` 元组补 `last_used`：

```
(String, u32, usize)  →  (String, u32, i64, usize)   // (word, frequency, last_used, pinyin_len)
```

热度判定（常量，二期进 config）：

```rust
const HOT_THRESHOLD: u32 = 3;             // 7 天内累计 ≥ 3 次
const HOT_WINDOW_SECS: i64 = 7 * 24 * 3600;

fn is_hot(freq: u32, last_used: i64, now: i64) -> bool {
    freq >= HOT_THRESHOLD && now - last_used <= HOT_WINDOW_SECS
}
```

| 档位 | 条件 | 排序位置 |
|------|------|---------|
| 🔥 热词 | 7 天内 ≥ 3 次 | 压过常用词（最高） |
| 🌡️ 温词 | 其余（打过 <3 次 或 超 7 天） | 常用词后、系统词前 |

7 天不碰 → 自动降温回温词，常用词夺回首位；再打即恢复热词。

### 2.3 查询排序（query 升级）

```
层1 精确：热词 > 常用词 > 温词 > 系统词
层2 前缀：热词 > 常用词 > 温词 > 系统词
```

- 双拼走 query() 自动继承
- 简拼 query_short：常用词声母索引命中优先（在系统简拼前）
- pin_map（d→的 等硬置顶）优先级保持最高，不受影响
- 现有"用户词只在同层插队"语义保留（精确用户词不压非精确系统词等）

## 三、数据模型

### common_dict.txt

```
pinyin<TAB>word（UTF-8 无 BOM，行首 # 注释，空行忽略）
```

### Dictionary 结构变更

```rust
pub struct Dictionary {
    index: HashMap<String, Vec<(String, u32, usize)>>,
    short_index: HashMap<String, Vec<(String, u32, usize)>>,
    full_index: BTreeMap<String, Vec<(String, u32)>>,
    #[serde(skip)]
    common_index: HashMap<String, Vec<(String, u32)>>,      // 新增：prefix → [(word, rank)]
    #[serde(skip)]
    common_short_index: HashMap<String, Vec<(String, u32)>>, // 新增：声母 prefix → [(word, rank)]
    #[serde(skip)]
    user_index: HashMap<String, Vec<(String, u32, i64, usize)>>, // 变更：+last_used
    #[serde(skip)]
    user_dict_path: Option<PathBuf>,
}
```

## 四、接口

无 FFI 改动。新增内部方法：

```
load_common(dir)      — 从 <db 目录>/common_dict.txt 解析构建 common 索引（无文件用内置兜底）
builtin_common()      — 内置常用词降级表
is_hot(freq, last, now) — 热度判定
```

`from_sqlite` / `from_bin` / `from_builtin` 构造后统一调用 `load_common`。

## 五、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 常用词表初稿（~150 条：Eric 指定 9 词 + 高频口语单/双字） | resources/common_dict.txt | 词表格式检查 |
| 2 | Dictionary 结构 + common 加载 + 热度判定 + query 排序升级 | engine/src/dictionary/mod.rs | cargo build |
| 3 | 单元测试（en→嗯首位、wo→我、简拼 hd、热/温词切换） | engine/src/dictionary/mod.rs | cargo test |
| 4 | 全链路验证 + commit | — | build + test + biome + commit |

## 六、测试用例

- `en` → 候选首位「嗯」（当前为「奀」）
- `wo` → 候选首位「我」；`hao` → 「好」；`haode` → 「好的」
- `zhege` → 「这个」；`name` → 「那么」；`mei` → 「没」
- 简拼 `hd` → 「好的」在候选前列
- 用户词：学一次「恩」→ 温词，`en` 首位仍「嗯」；模拟 7 天内 3 次 → 热词，`en` 首位变「恩」；超 7 天（模拟时间推进）→ 降温回「嗯」
- 双拼模式：常用词层生效
- 旧 .bin 兼容：不带 common 字段的 .bin 反序列化正常 + 运行时补 load_common
