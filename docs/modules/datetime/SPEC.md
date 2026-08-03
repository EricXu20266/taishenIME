# SPEC: 日期/时间/农历输入（Root #2，V0.2.19）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.19 日期/时间/农历输入

---

## 一、需求

输入简码 → 候选列出当前日期/时间格式，选中上屏。

| 简码 | 类型 | 候选示例 |
|------|------|---------|
| rq | 日期 | 2026-08-03 / 2026年8月3日 / 8月3日 |
| sj | 时间 | 10:35 / 10:35:20 |
| xq | 星期 | 星期一 / 周一 / Monday |
| nl | 农历 | 七月初一 / 丙午年七月初一（带注释） |

**用户价值**：写文档/聊天时快速插入日期时间，不用看右下角或切系统。

**合约**：
- 简码精确匹配（rq/sj/xq/nl），命中后候选为**多种格式**（2-3 个），选中上屏
- 日期格式至少 3 种（ISO / 中文 / 短格式）
- 农历首期：**公历 → 农历月日**（月 + 日，如"七月初一"），年份干支可选后续
- 农历算法：内置查表（1900-2100 农历数据表），不依赖外部库
- 简码命中不学习用户词（与短语一致）
- 与现有快捷短语的 sj（手机）冲突：**快捷短语 sj=手机 与日期 sj 冲突** →
  处理：日期/时间简码**优先于**短语（rq/sj/xq/nl 是保留码），短语表移除冲突项
  或：日期功能独立判定，sj 在日期功能开启时优先出日期候选，短语仍可翻页获取
  **决策**：短语表 sj=手机 已存在且用户可能依赖 → 日期简码与短语并存，
  日期候选排在短语前（日期意图更明确），短语可通过后续候选页获取。
  实际测试验证行为合理后定案。

**不做**：
- 农历年份干支（丙午年）——后续
- 节日/节气显示——后续
- 时区/自定义格式——后续

## 二、数据模型

```
无新增状态字段——简码走现有短语/拼音候选路径
```

### 模块

```
engine/src/datetime.rs
  pub fn date_candidates() -> Vec<String>     // 日期 3 格式
  pub fn time_candidates() -> Vec<String>     // 时间 2 格式
  pub fn weekday_candidates() -> Vec<String>  // 星期 3 格式
  pub fn lunar_candidates() -> Vec<String>    // 农历 1-2 格式
```

### 农历数据

```
1900-2100 年农历数据表（每月天数 + 闰月，压缩编码）
pub fn lunar_date(year, month, day) -> Option<(u8 月, u8 日, bool 闰)>
```

## 三、接口

### Engine（lib.rs）

```rust
// 保留码表
const DATE_CODES: &[&str] = &["rq", "sj", "xq", "nl"];

// query_all 中，在短语判定后：
if DATE_CODES 含 pinyin_str:
  候选 = datetime 对应候选
  datetime_candidate_pos = 0（选词不学习）
  return
```

### 优先级

```
短语判定 → 日期简码判定（日期优先）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | datetime.rs：日期/时间/星期/农历（含农历查表） | engine/src/datetime.rs | cargo test |
| 2 | lib.rs：DATE_CODES + query_all 分支 + 选词不学习 | engine/src/lib.rs | cargo test |
| 3 | 单元测试：rq/sj/xq/nl 候选内容 + 短语共存 | engine/src/lib.rs | cargo test |
| 4 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- 输入 rq → 3 个日期候选（ISO/中文/短）
- 输入 sj → 2 个时间候选
- 输入 xq → 3 个星期候选
- 输入 nl → 农历候选（月日，闰月正确）
- 日期候选不学习
- sj 与短语"手机"并存（日期优先）
