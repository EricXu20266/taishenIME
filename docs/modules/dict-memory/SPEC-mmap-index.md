# 词库索引内存改造（mmap 零拷贝）设计稿

> 目标：把词库索引的常驻内存从约 1000 MB 降到 300–400 MB，**不改任何查询语义**。
> 现状：`171 MB` 的 `.bin` 经 `bincode` 反序列化后常驻约 700 MB（system）+ 290 MB（domains）。

## 一、为什么要改

实测账目（`TS_MEM_REPORT=1`）：

```
system_dict   index 232 / short 44 / suffix 65 / full 39 / short_full 30 / trad 21  = 433 MB
domains       dom_index 80 / dom_short 52 / dom_full 20 / dom_exact 18 / word_domain 7 = 179 MB
保守合计      612 MB    实际峰值 1045 MB（哈希桶冗余约 70%）
```

膨胀来自三处，全部由「反序列化成对象图」造成：

1. 每个前缀一个独立 `String` key（107 万个 × 24 字节头 + 堆分配）
2. 每个词条在每个前缀下重复存一份 `String`（344 万前缀实例）
3. 每个 `Vec` 至少 24 字节头，加容量冗余

**关键**：`.bin` 本身只有 171 MB，是紧凑字节流。问题不在数据量，在加载方式。

## 二、不做什么

- **不砍前缀索引**：字符级补全（`niha → 你好`）是泰深与音节组合路线的核心差异，砍掉会退化成另一种产品。
- **不改查询语义**：`get` / `range` 的结果集必须与现在逐字节一致。
- **不动构建端产物的语义**：`system_dict.db` 仍是唯一数据源，`.bin` 只是它的预编译索引。

## 三、新文件格式

替换现有 `.bin`（版本号递增，旧格式自动失效重建）。

```
[头部 32B]
  magic       8B   "TSDICT02"
  version     u32
  shard_count u32
  meta_len    u64
  reserved    u64
[meta 段] meta_len 字节
  bincode 编码的小结构：common/user 派生索引、版本信息（不含大索引）
[分片区] × shard_count，每片自成一个块：
  key_count   u32
  key_offsets u32 × (key_count + 1)     ← 指向 key_pool，末位为总长
  key_pool    bytes                     ← 所有前缀串连续拼接，无长度前缀（偏移已定界）
  entry_count u32
  entries     Entry × entry_count       ← 定长，见下
  word_pool   bytes                     ← 所有词条串连续拼接
    Entry(16B, repr(C)):
      word_off   u32    词串在 word_pool 内的偏移
      word_len   u16    词串字节长
      freq       u32
      pinyin_len u16    音节长度
```

设计要点：

- **key 排序存放**，支持二分查找（`get`）与有序扫描（`range`）。
- **词串池化**：同一个词出现在 N 个前缀下只存一次（构建端去重），`Entry.word_off` 共享。
- **定长 Entry**：无间接指针，`&[Entry]` 可直接指向映射区。
- **分片保留**：沿用现有 8 片，便于并行构建与局部加载。

## 四、读取端

新增 `MappedIndex`，接口与现有 `ShardedMap` / `BTreeMap` 对齐：

```rust
impl MappedIndex {
    fn get(&self, key: &str) -> Option<&[Entry]>;        // 分片定位 → 二分 → 切片
    fn range(&self, from: &str) -> RangeIter<'_>;        // 二分定位 → 顺序读
    fn len(&self) -> usize;
}
```

`Entry` 对外暴露为 `(&str /*word*/, u32 /*freq*/, u16 /*pinyin_len*/)` 的三元组视图，
由 `word_pool` + `Entry.word_off/len` 现算，**不产生分配**。

迁移映射（查询侧仅替换接收者类型，调用点不动）：

```
self.index.get(&key)              →  保持，返回 Option<&[Entry]>
for (pinyin, words) in self.full_index.range(p..)
                                  →  改为 self.full_index.range(p..) 迭代器，逐项产出 (&str, &[Entry])
self.trad_index.values_mut()      →  构建期排序，读取端不再需要
```

## 五、两个难点的处理

**① `range()` 语义**

`full_index.range(prefix..)` 现有 5 处调用，用途是「按拼音前缀扫描所有词」。
新实现：在排序键上二分定位起点，向后顺序读，直到 key 不再以 `prefix` 开头。语义等价。

**② `values_mut()`**

`trad_index` / `domain_short_index` 有构建期排序（按词长、热度）。这些排序移到**构建端**（写盘前排好），读取端直接消费有序数据，不再需要可变访问。

## 六、迁移步骤（每步独立可停）

| 阶段 | 内容 | 预期内存 | 风险 |
|---|---|---|---|
| **P0** | POC：单个分片改成排序键 + 偏移，对比 `get` 延迟与内存 | — | 无（独立 example） |
| **P1** | `index`（232 MB，最大项）切到 mmap，其余索引不动 | ~-180 MB | 中，16 处 `get` 调用点 |
| **P2** | `full_index` / `suffix_index` 切 mmap（含 `range` 改造） | ~-70 MB | 中，8 + 5 处 |
| **P3** | domains 的 `dom_index` / `dom_short` 切 mmap | ~-110 MB | 中 |
| **P4** | `short` / `short_full` / `trad` / `dom_full` / `dom_exact` | ~-80 MB | 低 |

**P0 必须先做**：验证「二分查找在 13 万 key/分片下是否满足单键 < 1ms」。不达标就换成「哈希前置 + 偏移表」（空间略增，查询 O(1)）。

## 七、依赖

需要 `memmap2`（成熟的轻量 mmap 封装，无 unsafe 泄漏到业务代码）。
按项目规矩，加依赖前需要确认。

## 八、验收标准

- 内存：`TS_MEM_REPORT=1` 报告的索引合计下降 ≥ 60%，峰值 ≤ 400 MB
- 语义：主用例集 278/292 不变；长句全拼 36/90、虚词简 31/90 不变
- 延迟：`lat_probe` 单键峰值不高于现状（`zhongguorenmin` 97 ms 为上限）
- 一致性：同一份 `system_dict.db` 用新旧两条加载路径产出的候选**逐条相同**
