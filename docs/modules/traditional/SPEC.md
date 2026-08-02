# SPEC: 简繁转换（Root #2，V0.2.11）

> 对应 ARCHITECT.md Root #2「业务领域层 — 词库查询」
> 关联 DEV-TRACKER: 0.2.11 简繁转换
> 前置：0.1.14 词库查询链路（query/query_short/query_all）

---

## 一、需求

输入拼音时，候选词可按需切换为繁体输出（适用港台用户/书面写作）。开启简繁模式后，
候选列表显示对应繁体词（如 输入 zhongguo → 中國）。

**用户价值**：同一套拼音词库，一键切换简体/繁体输出，无需维护两套词库。

**合约**：
- 默认关闭（大陆用户为主），config.ini 可开
- 开启后：候选查询结果做**逐字简→繁转换**（简体候选 → 繁体显示）
- 转换是**显示层/输出层**转换，词库数据不变（只存简体）
- 支持简→繁映射表（覆盖常用字 + 常用词组）
- 用户词库学习的词同样转换输出
- FFI 提供开关；平台层选词上屏时用转换后的文本

**不做**：
- 繁→简（繁体输入场景，词库本身是简体，无需求）——后续
- 上下文相关转换（"头发/發" 多音多义处理）——一期用字符映射，词组表补充
- 转换词库动态维护——静态表

## 二、数据模型

### 简繁映射表（内置静态）

```
trad_map: HashMap<char, char>  // 常用简→繁单字映射（约 2000+ 常用字）
trad_phrase: HashMap<&str, &str>  // 词组级映射（处理单字映射歧义，如 头发→頭髮、发现→發現）
```

来源：OpenCC 简繁映射子集（开源），一期取常用 2000 字 + 常用词组。
单字映射覆盖 GB2312 一级汉字常用繁体对应。

### 转换算法

```
fn to_traditional(text: &str) -> String:
  // 先词组匹配（最长优先），剩余逐字查表
  // 无映射的字保持原样（简繁同形字）
```

## 三、接口

### Engine 状态（lib.rs）

```rust
// 新增字段
traditional_mode: bool,  // 简繁转换开关，默认 false

// 方法
pub fn set_traditional(&mut self, enabled: bool);  // 开关（变化时重查）
pub fn traditional(&self) -> bool;
```

### 候选输出转换

select_candidate 返回前转换；FFI engine_get_candidate 返回前转换。
（转换只影响输出，不影响内部候选/学习——learn 存简体，输出转繁体）

### FFI

```rust
engine_set_traditional(i32) -> i32   // 开关
engine_get_traditional() -> i32       // 查询：1 开 / 0 关 / -1 未初始化
```

### config.ini

```
traditional=0   # 简繁转换（0/1，默认 0）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | trad.rs：简繁映射表 + to_traditional | engine/src/trad.rs | cargo test（中→中國） |
| 2 | lib.rs：traditional_mode + 输出转换 | engine/src/lib.rs | cargo test |
| 3 | FFI engine_set/get_traditional | engine/src/ffi.rs | cargo test |
| 4 | config_reader 解析 traditional | platform/windows | 冒烟测试 |
| 5 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- zhongguo → 中國（开启后候选）
- 头发 → 頭髮（词组映射）
- 关闭 → 正常简体
- 无映射字保持原样（如 我 → 我）
