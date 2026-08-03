# SPEC: 计算器 cC 模式（Root #2，V0.2.22）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.22 计算器（cC + 算式 → 候选）

---

## 一、需求

输入 `c` + 算式 → 候选显示计算结果，选中上屏。

**用户价值**：聊天/写文档时随手算数（35*12=？），不用切计算器。

**合约**：
- 触发方式：`c` 开头（与 v 模式符号同构，拼音无 c 开头的完整音节冲突场景可控）
  - 注：c 是合法声母（cai/cao/cen...），需要精确匹配才有歧义。
  - 处理：**c 单独输入时走正常拼音**；`c` + 运算符/数字 触发计算器
- 算式支持：四则运算 `+ - * /`、括号 `()`、幂 `^`、取模 `%`
- 支持浮点，结果最多保留 6 位小数，去尾零
- 结果作为**第一个候选**（唯一），选中上屏
- 非法算式（除零/语法错误）→ 无候选，候选显示错误提示
- 计算器候选**不学习用户词**

**不做**：
- 科学函数（sqrt/pow/sin/cos）——后续扩展
- 历史记录/单位换算——后续

## 二、数据模型

```
无新增状态字段——c 模式通过 pinyin_buf 判定 + 独立解析
```

### 触发判定

```
is_calc_mode():
  pinyin_buf 以 'c' 开头 且 长度 > 1 且 后续含数字或运算符（+ - * / ( ) % .）
```

### 模块

```
engine/src/calculator.rs
  pub fn eval(expr: &str) -> Result<f64, String>   // 表达式求值
  pub fn format_result(v: f64) -> String           // 去尾零，≤6 位小数
```

## 三、接口

### Engine（lib.rs）

```rust
pub fn is_calc_mode(&self) -> bool;
```

### 查询集成（query_all，v 模式分支后）

```
若 is_calc_mode():
  expr = pinyin_buf[1..]（去 c 前缀）
  结果 = calculator::eval(expr)
  成功 → 候选 [format_result(结果)]
  失败 → 候选 []（非法算式）
  calc_candidate_pos = 0（选词不学习）
  return
```

### 按键处理（process_key）

```
'c' 之后输入数字/运算符字符（'+' '-' '*' '/' '(' ')' '%' '.' '0'-'9'）
→ 追加到 pinyin_buf 并触发 calc 查询
'c' 之后输入字母 → 回退到正常拼音（如 ca → 擦）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | calculator.rs：表达式求值（递归下降）+ 格式化 | engine/src/calculator.rs | cargo test |
| 2 | lib.rs：is_calc_mode + process_key 特殊字符 + query_all 分支 | engine/src/lib.rs | cargo test |
| 3 | 单元测试：四则/括号/幂/除零/非法表达式/回退拼音 | engine/src/lib.rs | cargo test |
| 4 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- c35*12 → 420
- c(1+2)*3 → 9
- c2^10 → 1024
- c10/4 → 2.5
- c1/0 → 无候选（除零）
- c1+ → 无候选（语法错）
- c1++2 → 3（一元正号合法，与 2*-3 对称）
- ca → 正常拼音（擦）
- 计算器候选不学习
