# SPEC: 用户词库（Root #1，V0.2.2）

> 对应 ARCHITECT.md Root #1「数据模型与持久化」
> 关联 DEV-TRACKER: 0.2.2 用户词库（学习+持久化）
> 前置：0.1.3/0.1.4 系统词库 SQLite 加载通道 ✅

---

## 一、需求

用户选词时自动学习：将 (拼音, 选中词) 记入用户词库，词频 +1。下次输入相同拼音时，
用户词优先于系统词（加权排序）。用户词库持久化到独立 SQLite 文件，跨会话保留。

**用户价值**：输入法"越用越懂你"——常用词（人名、专业术语、网络用语）自动靠前。

**合约**：
- 用户词库独立于系统词库存储（不污染发布包内的 system_dict.db）
- 查询时合并：系统词 + 用户词，用户词按加权频率插队
- 学习只在用户**主动选词**时发生（select_candidate），不自动学习候选浏览
- 持久化路径由平台层传入（Windows: %APPDATA%/taishen-ime/user_dict.db）

**不做**：
- 云同步用户词库——V0.3 范围
- 用户词库导入/导出 UI——后续
- 词频衰减（老词条降权）——后续
- 简拼学习的拼音补全（简拼 z 选"中国"时拼音记 z 而非 zhongguo）——已知限制，一期接受

## 二、数据模型

### 用户词库表结构（独立 SQLite 文件）

```sql
CREATE TABLE IF NOT EXISTS user_dict (
    pinyin TEXT NOT NULL,       -- 拼音（小写），如 "zhongguo"
    word TEXT NOT NULL,         -- 汉字词，如 "中国"
    frequency INTEGER DEFAULT 1, -- 学习次数，越大越靠前
    last_used INTEGER DEFAULT 0, -- 最近使用时间戳（unix sec），备用排序
    PRIMARY KEY (pinyin, word)
);

CREATE INDEX IF NOT EXISTS idx_user_pinyin ON user_dict(pinyin);
```

**与系统词库的区别**：PRIMARY KEY(pinyin, word) 去重（同词学习累加频率而非重复行）；
系统词库是 pinyin 前缀索引。用户词库很小（个人积累），无需前缀索引——加载时全量入内存 HashMap。

### 内存合并索引

```
Dictionary 新增 user_index: HashMap<String, Vec<(word, frequency)>>
key = 拼音前缀（与 system index 同构）
查询时 user_index[prefix] 排在 system index 前面（用户词插队）
```

## 三、接口

### Rust 内部（dictionary 模块新增）

```rust
/// 设置用户词库文件路径（NULL = 禁用用户词库）
pub fn set_user_dict_path(path: Option<&Path>);

/// 学习：记录 (pinyin, word)，词频 +1，写回磁盘
pub fn learn(pinyin: &str, word: &str);

/// 查询合并：系统词 + 用户词（用户词加权插队）
// 现有 query() / query_short() 内部自动合并 user_index
```

### FFI（ffi.rs 新增，供平台层调用）

```rust
/// 设置用户词库路径。dict_path 为系统词库路径，user_path 为用户词库路径
#[unsafe(no_mangle)] pub extern "C" fn engine_set_user_dict_path(path: *const c_char) -> i32;

/// 学习用户词（平台层在选词上屏成功后调用）
/// 注意：由 engine_select_candidate 内部自动调用，平台层无需手动调
```

### 学习触发点（引擎内部）

```rust
// lib.rs Engine::select_candidate
pub fn select_candidate(&mut self, index: usize) -> Option<String> {
    let result = self.candidates.get(index).cloned();
    if let Some(word) = &result {
        // 自动学习：当前拼音串 + 选中词 → 用户词库
        if !self.pinyin_buf.is_empty() && !self.ascii_mode {
            dictionary::learn(&self.pinyin_buf, word);
        }
    }
    self.reset();
    result
}
```

**学习时机**：选词成功即学（含数字键/空格/鼠标选词，统一走 select_candidate）。
**拼音补全限制**：简拼/模糊音/联想命中的词，pinyin_buf 可能非完整拼音——一期按 pinyin_buf 原样记录（后续补全）。

### 平台层（C++）

config_reader 解析 user_dict_path（默认 %APPDATA%/taishen-ime/user_dict.db）→ engine_init 后调用 engine_set_user_dict_path。

## 四、加载与合并策略

```
engine_init(dict_path)
  → 加载系统词库（现有逻辑不变）
  → engine_set_user_dict_path(user_path)
    → 存在 → 加载 user_index 到内存
    → 不存在 → 空 user_index（首次运行，select 学习时自动创建文件）
  ↓
query(prefix)
  → user_index[prefix]（加权，frequency 直接作为排序权重）
    → 插在 system index 结果之前，去重（用户词已在系统词中则不重复显示，位置提前）
  → system index[prefix]
```

**加权规则**：用户词 frequency 就是其排序权重，直接与系统词 frequency 比较排序（统一降序）。
用户词库量小（百级），合并排序开销可忽略。

## 五、持久化

| 项 | 值 |
|----|-----|
| 文件 | %APPDATA%/taishen-ime/user_dict.db（Windows），平台层解析 |
| 格式 | SQLite（复用 rusqlite，无需新依赖） |
| 写入 | 每次 learn 立即写（INSERT OR REPLACE ... frequency+1），延迟 < 1ms |
| 容错 | 打开/写入失败静默降级（不阻塞输入）；日志记录 |
| 备份 | 用户数据，卸载不删（uninstall.ps1 保留 %APPDATA%） |

## 六、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | dictionary 模块增加 user_index 字段 + set_user_dict_path/learn | engine/src/dictionary/mod.rs | cargo test |
| 2 | query 合并用户词（加权插队+去重） | 同上 | cargo test（用户词优先） |
| 3 | Engine::select_candidate 自动学习 | engine/src/lib.rs | cargo test |
| 4 | FFI engine_set_user_dict_path | engine/src/ffi.rs | cargo test |
| 5 | config_reader 解析 user_dict_path | platform/windows/src/config_reader.cpp | 冒烟测试 |
| 6 | engine_bridge 调用 set_user_dict_path | platform/windows/src/engine_bridge.cpp | 冒烟测试 |
| 7 | 全链路验证 | — | cargo build + test + 冒烟 + biome |

## 七、测试用例

- 学习后查询：输入 ni 学"昵称"→ 再输 ni 候选含"昵称"且排前
- 重复学习：学 3 次 → frequency=3 → 排最前
- 去重：用户词已在系统词库（如"中国"）→ 不重复显示，位置提前
- 持久化：learn 后重建引擎 → 用户词仍在
- 容错：user_dict.db 路径无效 → 静默降级不崩溃
- 简拼限制：简拼 z 选词后 learn("z", 词)（已知限制，仅验证不崩溃）
