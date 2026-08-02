# SPEC: 快捷短语（Root #7，V0.2.12）

> 对应 ARCHITECT.md Root #7「配置系统」
> 关联 DEV-TRACKER: 0.2.12 快捷短语/剪贴板
> 本期实现**快捷短语**；**剪贴板历史**为平台层功能（剪贴板监听/托盘 UI），标注后续

---

## 一、需求

输入特定简码 → 上屏预设长文本（高频回复/常用语一键输出）。
如输入 `bq` → 候选"不客气"，选中上屏；输入 `dz` → "地址：深圳市南山区..."。

**用户价值**：长句/常用语不用逐字打，简码直达。

**合约**：
- 短语表：内置常用短语 + `config.ini` 可指定自定义短语文件
- 简码全字母（小写），如 bq/dz/gs（公司）/wm（我们）
- 命中短语加入候选，**排在候选最前**（简码意图明确，优先级最高）
- 选中短语上屏：**不学习用户词**（短语非拼音词）
- 短语命中不干扰正常拼音（短语表小，冲突少；简码 2-4 字母）
- 默认开，config.ini 可关

**不做**：
- 剪贴板历史（平台层：剪贴板监听 + 托盘 UI + 快捷键粘贴）——0.2.12 后续拆分
- 短语分类/分组管理 UI——后续
- 短语模糊匹配——精确简码

## 二、数据模型

### 短语表（内置 + 外部文件）

```
内置（rust 常量，常用 20+ 条）：
  bq → 不客气
  wm → 我们
  dz → 地址：深圳市南山区科技园
  ...

外部文件（config.ini phrase_path 指定，UTF-8，每行）：
  # 注释
  bq=不客气
  dz=地址：...
```

### 内存结构

```
phrase_map: HashMap<String, String>  // 简码(小写) → 短语文本
```

## 三、接口

### Engine（lib.rs）

```rust
// 新增字段
phrase_enabled: bool,          // 开关，默认 true
phrase_map: HashMap<String, String>,  // 简码 → 短语

// 方法
pub fn set_phrase_enabled(&mut self, enabled: bool);
pub fn phrase_enabled(&self) -> bool;
pub fn load_phrases(&mut self, entries: Vec<(String, String)>);  // 外部加载
```

### 查询集成（query_all）

```
若 phrase_enabled && phrase_map 含 pinyin_str（简码精确匹配）：
  候选插入位置：用户词之后、系统词之前
  phrase_candidate_pos = 插入位置  // 记录，选词不学习
```

### FFI

```rust
engine_set_phrase_enabled(i32) -> i32   // 开关
engine_get_phrase_enabled() -> i32       // 查询
engine_set_phrase_path(*const c_char) -> i32  // 加载外部短语文件（NULL = 仅内置）
```

### config.ini

```
phrase=1                 # 快捷短语开关
phrase_path=             # 自定义短语文件（空 = 仅内置）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | lib.rs：phrase 字段 + 内置短语表 + query 集成 | engine/src/lib.rs | cargo test |
| 2 | 外部短语文件解析（UTF-8 key=value） | engine/src/ffi.rs | cargo test |
| 3 | FFI 三接口 | engine/src/ffi.rs | cargo test |
| 4 | config_reader 解析 phrase/phrase_path | platform/windows | 冒烟测试 |
| 5 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- 输入 bq → 候选含"不客气"，选中上屏不学习
- 短语排在用户词后、系统词前
- 关闭短语 → 无短语候选
- 外部短语文件加载（bq=自定义）覆盖内置
- 简码与拼音共存不冲突（wm → 我们 + 拼音候选）
