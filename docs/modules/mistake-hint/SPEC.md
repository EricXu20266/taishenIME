# SPEC: 错音错字提示（Root #2，V0.2.26）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.26 错音错字提示

---

## 一、需求

输入易错读音时，自动用正确读音查询并追加候选——用户看到正确词写法即自我纠正。

**场景**：参差读成 cancha（正确 cenci）、主角读成 zhujiao（正确 zhujue）、
暖和读成 nuanhe（正确 nuán huo）。

**用户价值**：读错音打不出词，输入法自动补上正确读音的词。

**合约**：
- 词源：内置**易错读音映射表**（手工精选高频易错词，30-50 条，可扩展）
- 映射：错音串 → [(正确拼音, 正确词)]（一词可多错音）
- 触发：正常查询结果**少于 page_size** 时，查映射表补候选
- 候选：显示正确词，选中上屏正确词（不显示注释——候选窗无 comment 机制）
- 追加位置：正常候选之后（不干扰精确命中）
- 追加候选**不学习用户词**
- 仅引擎侧，无 FFI/TSF 改动

**不做**：
- others.dict.yaml 自动生成映射——该词库仅罗列多音无主次，会引入噪声
- 候选带读音注释（需候选窗 comment 机制，后续）
- 动态学习用户错音模式——后续

## 二、数据模型

### 易错读音映射表（内置常量）

```
// 错音 → [(正确拼音, 正确词)]
"cancha"    → [("cenci", "参差")]
"zhujiao"   → [("zhujue", "主角")]
"nuanhe"    → [("nuanhuo", "暖和")]
"guangbo"   → [("guangbo", "广播")]   // 播音类
"jiao"      → [("jue", "角")]         // 角逐/角色场景由词级处理
...
```

### 模块

```
engine/src/mistake.rs
  pub fn lookup(wrong_pinyin: &str) -> Vec<(&'static str, &'static str)>
  // 返回 [(正确拼音, 正确词)]，未命中返回空
```

## 三、接口

### Engine（lib.rs）查询集成（query_all，模糊音后）

```
if candidates.len() < self.page_size:
  for (correct_py, word) in mistake::lookup(&pinyin_str):
    正确词候选（去重追加）
    mistake_candidate_pos 记录首个（选词不学习）
```

### 行为

```
- 输入 cancha → 正常查询无/少候选 → 追加"参差"
- 输入 zhongguo（正确）→ 结果充足，不触发
- 错音词追加后仍可翻页（在 all_candidates 内）
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | mistake.rs：易错读音映射表 + lookup | engine/src/mistake.rs | cargo test |
| 2 | lib.rs：query_all 集成 + 选词不学习 | engine/src/lib.rs | cargo test |
| 3 | 单元测试：错音命中/正确不触发/不学习 | engine/src/lib.rs | cargo test |
| 4 | 全链路验证 | — | build + test |

## 五、测试用例

- 输入 cancha → 候选含"参差"
- 输入 nuanhe → 候选含"暖和"
- 输入 zhongguo → 正常候选（不触发错音）
- 错音候选选中不学习
- 错音候选排在正常候选后
