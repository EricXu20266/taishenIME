# SPEC: 英文自动大写（Root #2，V0.2.23）

> 对应 ARCHITECT.md Root #2「输入引擎」
> 关联 DEV-TRACKER: 0.2.23 英文自动大写

---

## 一、需求

中英混输模式下，英文候选的大小写跟随输入模式：

| 输入 | 候选/上屏 | 规则 |
|------|----------|------|
| hello | hello | 全小写 |
| Hello | Hello | 首字母大写 |
| HELLO | HELLO | 全大写 |

**用户价值**：句首大写（Hello world）、全大写强调（HELLO）不用切英文模式或手动改。

**合约**：
- 仅在**英文候选**（混输追加项）生效——中文候选不受影响
- 判定规则：输入串 `Hello`（首字母大写+其余小写）→ Capitalize；`HE`（前 2 字母大写）→ Upper；其余 → Lower
- 上屏与候选显示一致（不转换学习用户词——英文候选本就不学习）
- 引擎侧实现（候选回调层），TSF 零改动

**不做**：
- 英文模式（ascii_mode）下的大写处理——直通上屏，不经过引擎
- 大小写混合输入（如 `hEllo`）——按 Lower 处理，不特殊转换

## 二、数据模型

```
无新字段——大小写模式由输入历史推导：
  cap_state: Capitalize | Upper | Lower（按输入串字符判定）
```

### 判定算法

```
输入串 = 原始按键序列（保留大小写）
  - 首字符大写 且 其余全小写 → Capitalize
  - 前 2 个字符大写（含全大写）→ Upper
  - 其他 → Lower
```

## 三、接口

### Engine（lib.rs）

```rust
// 新增字段
raw_input: String,          // 原始输入（保留大小写），与 pinyin_buf 同步累积
cap_state: CapState,        // Capitalize | Upper | Lower

// 方法
fn detect_cap_state(&self) -> CapState;   // 从 raw_input 判定
pub fn english_candidate_display(&self, index: usize) -> Option<String>;
  // 英文候选：按 cap_state 转换后返回
  // 非英文候选：原样
```

### 集成点

- `process_key`：`raw_input.push(ch)`（原始字符），`pinyin_buf.push(ch.to_lowercase())`
- `backspace`：`raw_input.pop()` 同步
- `reset`：`raw_input.clear()` 同步
- `candidate_display`：英文候选时按 cap_state 转换
- `select_candidate`：英文候选时按 cap_state 转换输出
- 判定时机：每次 process_key 后重算 cap_state

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | raw_input + cap_state 字段 + 同步累积/清除 | engine/src/lib.rs | cargo test |
| 2 | detect_cap_state + candidate_display/select 转换 | engine/src/lib.rs | cargo test |
| 3 | 单元测试：Hello/HELLO/hello 三态 + 中文不受影响 | engine/src/lib.rs | cargo test |
| 4 | 全链路验证 | — | build + test + biome |

## 五、测试用例

- 输入 hello → 候选 hello（Lower）
- 输入 Hello → 候选/上屏 Hello（Capitalize）
- 输入 HELLO → 候选/上屏 HELLO（Upper）
- 中文候选不受影响（nihao → 你好）
- 退格后大小写模式重算
