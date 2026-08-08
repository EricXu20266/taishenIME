# SPEC — 专业词库分类（对标微软/搜狗分类词库）

> 模块：engine（Rust 核心）+ resources（分类词库文件）+ platform/windows（config 传递）
> 日期：2026-08-08
> 状态：已批准实现

## 一、需求

微软拼音内置 43 套专业词库（计算机/电子/生化/自动化等，默认启用 4 套），搜狗提供细胞词库分类导入。泰深当前单一词库，专业领域词汇（医学/法律/编程术语）命中率依赖主词库覆盖。

**目标**：新增分类词库支持——独立分类词库文件，用户可启用/停用，启用后该领域的词在查询时追加到系统词之后（不抢占常用位）。

## 二、接口设计

### 词库层（dictionary/mod.rs）

```rust
// Dictionary 新增字段
domain_index: HashMap<String, Vec<(String, u32, usize)>>,  // 全拼前缀 → [(word, freq, pinyin_len)]
#[serde(default)]
domain_short_index: HashMap<String, Vec<(String, u32, usize)>>,  // 简拼声母

// 新增方法
pub fn load_domain_dict(&mut self, path: &Path)  // 解析分类词库 txt 并入索引
pub fn clear_domain(&mut self)                    // 清空分类词索引（停用）
```

**分类词库文件格式**（每行 `词 拼音`，空格分隔，与 build_dict.py 派生格式一致）：
```
神经网络 shenjingwangluo
深度学习 shenduxuexi
```

**查询合并**（query / query_short）：系统词之后、英文候选之前追加 domain 命中（专业词不抢常用词位置）。

### FFI 层

```rust
#[no_mangle] pub extern "C" fn engine_load_domain_dict(path: *const c_char) -> i32  // 0 成功 / -1 失败
```

### 配置层

config.ini 新增键（逗号分隔多个分类词库文件，留空 = 不启用）：
```ini
# 专业词库（逗号分隔多个文件路径，留空=关闭）
domain_dicts=
```

## 三、资源文件

resources/domains/ 下预置分类（可选）：
- computer.txt（计算机/编程术语）
- medical.txt（医学词汇）
- law.txt（法律词汇）

首次先做机制 + 1 个示例分类（computer），后续可扩展。

## 四、实施计划

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | dictionary：domain_index 字段 + load_domain_dict + clear_domain | cargo build |
| 2 | query/query_short 合并 domain 命中 | cargo build |
| 3 | FFI：engine_load_domain_dict | cargo build |
| 4 | resources/domains/computer.txt 示例 + config 接线 | cargo test + cmake |
| 5 | 单测 + 全量验证 + commit | 全绿 |

## 五、风险

- domain 词追加在系统词后 → 不影响常用词排序，零回归
- 分类文件格式错误行静默跳过
- 与 .bin 预编译索引兼容：domain_index 运行时加载（serde skip），不入 .bin
