# SPEC — 专业词库热词探测自动匹配（v2）

> 模块：engine（Rust）+ resources/domains（词库）
> 日期：2026-08-08
> 状态：设计定稿（Eric 决策：不靠用户设置，探测热词自动匹配多领域）
> 替代：v1（domain_dicts 手动配置路径）——改为全量加载 + 热度自动激活

## 一、需求（v2 升级）

v1 要求用户在 config/settings 手动指定词库路径——Eric 否掉此交互：**专业词库零配置，引擎探测用户输入热词自动匹配领域，可同时匹配多个**。

**目标**：
1. `resources/domains/*.txt` 启动时全量自动加载（无需 config）
2. 用户选中某领域词 → 该领域热度 +1
3. 查询时热度 > 0 的领域词优先展示（自动感知当前活跃领域）
4. 多领域可同时活跃（写代码 + 聊医学 → computer 与 medical 都提升）

## 二、接口设计

### Dictionary（dictionary/mod.rs）

```rust
// domain 条目带领域 ID（4 元组）：(word, freq, pinyin_len, domain_id)
domain_index: HashMap<String, Vec<(String, u32, usize, usize)>>,
domain_short_index: HashMap<String, Vec<(String, u32, usize, usize)>>,
// 词 → 领域 ID（select_candidate 探测用）
#[serde(skip)]
word_domain: HashMap<String, usize>,
// 领域名列表（索引 = domain_id）
#[serde(skip)]
domain_names: Vec<String>,

pub fn load_domains_from_dir(&mut self, dir: &Path)  // 扫描目录全部 txt 自动加载
```

### Engine（lib.rs）

```rust
// 领域热度（索引 = domain_id），选词命中 +1
domain_heat: Vec<i64>,

pub fn record_domain_hit(&mut self, word: &str)  // select_candidate 调用：word_domain 查命
```

**查询排序**：domain 词追加顺序 = 热度 > 0 的领域词先出（按领域热度降序），热度 0 领域词最后（冷启动仍可命中，但不抢位）。

**热度衰减**：内存版（重启清零），后续可持久化到 user_dict。

## 三、数据模型

- 领域词库：`resources/domains/<name>.txt`，格式 `词 拼音`（已有 9 领域 13.9 万词）
- 领域 ID = 加载顺序（目录排序）
- `word_domain` 一词一域（跨域词取首个加载领域）

## 四、实施计划

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | dictionary：domain 4 元组 + word_domain + load_domains_from_dir | cargo build |
| 2 | engine：domain_heat + record_domain_hit + 查询热度加权 | cargo build |
| 3 | 全量加载接线：init 时扫描 domains 目录 | cargo test |
| 4 | 单测：热度命中提升/多领域/冷启动零污染 | cargo test |
| 5 | 验证 + commit | 全绿 |

## 五、风险

- 13.9 万词全量加载内存开销（预计 <30MB，主词库已更大）
- 查询性能：domain 词追加排序 O(n log n)，候选截断 40 条，可接受
- 冷启动（全 0 热度）：domain 词排在系统词后，不污染首屏
