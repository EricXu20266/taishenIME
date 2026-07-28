# SPEC: 数据模型与持久化（Root #1）

> 对应 ARCHITECT.md Root #1「数据模型与持久化 — 存什么、怎么存」
> 关联 DEV-TRACKER: 0.1.3 系统词库构建 + 0.1.4 词库加载通道

---

## 一、需求

将当前硬编码在 `dictionary/mod.rs` 中的 85 条内置词库替换为 SQLite 外部词库文件。

**合约**：
- 系统词库从 `resources/system_dict.db` 加载，随安装包发布
- 引擎启动时首次查询触发延迟加载（Lazy Init），加载到内存 HashMap 索引
- 查询接口保持 `dictionary::query(pinyin_prefix) -> Vec<String>` 不变
- 内置 85 条硬编码词库作为「最小降级词库」保留——当 SQLite 文件不存在或损坏时，兜底用内置词库

**不做**：
- 用户词库（文本文件）——那是 0.2.2，二期范围
- 词频动态调整——二期范围
- 云输入候选——二期范围

## 二、数据模型

### 系统词库表结构

```sql
CREATE TABLE IF NOT EXISTS system_dict (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pinyin TEXT NOT NULL,       -- 拼音（小写，无空格），如 "zhongguo"
    word TEXT NOT NULL,         -- 汉字词，如 "中国"
    frequency INTEGER DEFAULT 0 -- 词频，越大越靠前
);

CREATE INDEX IF NOT EXISTS idx_pinyin_prefix ON system_dict(pinyin);
```

### 内存索引结构

加载后构建 HashMap<String, Vec<(word, frequency)>>，key 为拼音前缀，value 为候选词列表（按频率降序）。

与当前 `Dictionary::index` 结构完全一致——零 API 变更。

## 三、接口

### 对外接口（不变）

```rust
// 初始化（延迟加载，首次查询时触发）
pub fn ensure_initialized();

// 查询候选词
pub fn query(pinyin_prefix: &str) -> Vec<String>;
```

### 新增内部接口

```rust
// 从 SQLite 文件加载词库到内存
fn load_from_sqlite(path: &str) -> Result<Dictionary, LoadError>;

// 回退到内置最小词库（85条硬编码）
fn load_builtin() -> Dictionary;
```

## 四、加载策略

```
引擎启动（engine_init()）
  → 不加载词库（延迟）
  ↓
首次 dictionary::query("z")
  → ensure_initialized()
    → 尝试加载 resources/system_dict.db
      → 成功 → 构建 HashMap 索引
      → 失败（文件不存在/损坏）→ 回退 builtin_dict()
  → 后续 query() 直接从 HashMap 查
```

## 五、词库构建工具

需要一个工具脚本将原始词表（CSV/TSV）转换为 SQLite 数据库。

```
tools/build_system_dict.py
  输入: resources/raw_dict.txt (格式: pinyin\tword\tfrequency)
  输出: resources/system_dict.db
```

词表来源：可以手工整理高频词（基于现代汉语语料），或从开源项目（雾凇拼音 rime-ice）提取。

## 六、Cargo 依赖变更

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
```

`bundled` feature 将 SQLite 源码编译进 DLL，无需用户安装 SQLite。

## 七、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 添加 rusqlite 依赖 | Cargo.toml | cargo build |
| 2 | 创建词库构建工具 | tools/build_system_dict.py | 生成 system_dict.db |
| 3 | 准备初始词表 | resources/raw_dict.txt | 手工整理 500+ 高频词 |
| 4 | 重构 Dictionary 支持 SQLite 加载 | engine/src/dictionary/mod.rs | cargo test |
| 5 | 降级回退逻辑 | engine/src/dictionary/mod.rs | 测试：删除 db 文件后查询仍工作 |
| 6 | 更新 FFI 接口支持词库路径参数 | engine/src/ffi.rs | cargo test |
| 7 | 全链路验证 | — | cargo build + cargo test + biome |
