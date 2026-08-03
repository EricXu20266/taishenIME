# SPEC: 拆字反查（Root #2，V0.2.25）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.25 拆字反查（uU + 部件拼音 → 生僻字）

---

## 一、需求

输入 `u` + 部件拼音 → 候选列出由这些部件组成的汉字。

**用户价值**：不知道读音的生僻字（𣲗/垚/叕）——按字形拆部件打拼音即可找到。

**合约**：
- 触发方式：`u` 开头（与 v 符号、c 计算器同构的字母前缀模式）
- 反查码：`u` + 部件拼音串（可含 `'` 分隔多部件），如：
  - `ushuishou` → 水+手 → 氵扌 相关字
  - `uyanyu` → 言+羽 → 誩/䎽 等
- 词库：rime-ice radical_pinyin.dict.yaml（13.2 万条，下载到 resources/rime_ice/）
- 反查匹配：部件拼音串与词条拼音列匹配（去 `'` 分隔符比对）
- 命中候选按词频降序，显示汉字
- 选中上屏**不学习用户词**
- 与 u 开头的拼音共存：`u` 单独/合法拼音（如 u 无合法音节）→ 无候选时正常走拼音
- 双拼模式 u 是声母 → 自动排除（同 v 模式处理）

**不做**：
- 拆字辅码（rime 的 radical_pinyin 还做辅码）——仅反查
- 笔画/部首反查——词库已含，后续扩展
- 候选带声调注音——候选窗无 comment 机制

## 二、数据模型

```
radical_pinyin.dict.yaml 解析后加载：
  HashMap<String, Vec<(word, freq)>>  // 部件拼音串(去分隔符) → [(字, 频率)]
```

### 词库格式

```
的	bai'shao	40529     # 部件拼音用 ' 分隔
一	heng	18584
```

### 匹配算法

```
输入 u + "bai'shao" → 规范化：去 ' → "baishao"
词库 key 同样去 ' 存储
精确匹配 → 候选
```

## 三、接口

### 模块

```
engine/src/radical.rs
  pub fn init(path: Option<&Path>)     // 加载 radical_pinyin.dict.yaml（内置回退空表）
  pub fn query(parts: &str) -> Vec<(String, u32)>  // 反查，按频率降序
```

### Engine（lib.rs）

```rust
pub fn is_radical_mode(&self) -> bool;  // 'u' 开头且长度>1，非双拼
```

### 查询集成（query_all，v/calc 分支后）

```
若 is_radical_mode():
  反查码 = pinyin_buf[1..]（去 u 前缀，去 ' 分隔符）
  candidates = radical::query(...) → 取前 page_size*max_pages
  radical_candidate_pos = 0（选词不学习）
  return
```

### FFI

```
engine_set_radical_path(*const c_char) -> i32  // 加载词库（NULL = 仅内置空表）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | radical.rs：词库加载 + 反查 | engine/src/radical.rs | cargo test |
| 2 | lib.rs：is_radical_mode + query_all 分支 + 选词不学习 | engine/src/lib.rs | cargo test |
| 3 | ffi.rs：engine_set_radical_path | engine/src/ffi.rs | cargo test |
| 4 | tsf_module：ActivateEx 设置词库路径 | platform/windows | 编译 |
| 5 | 单元测试 + 全链路验证 | — | build + test |

## 五、测试用例

- ushuishou → 候选含氵/扌相关字
- uyanyu → 候选含言羽部件字
- u + 未知部件 → 无候选
- u 单独输入 → 不进入反查模式
- 双拼模式 u → 排除
- 反查候选不学习
