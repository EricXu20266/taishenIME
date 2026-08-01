# SPEC: 业务领域层（Root #2）— 中英文切换

> 对应 ARCHITECT.md Root #4「状态管理」+ Root #2「业务领域层」
> 关联 DEV-TRACKER: 0.1.9 中英文切换

---

## 一、需求

输入法支持中文/英文两种输入模式，用户通过切换键实时切换。

**合约**：
- 切换键：**Ctrl+Space**（主流输入法默认，TSF 键盘事件可捕获）
- 中文模式（默认）：字母键累积拼音 → 候选 → 选词上屏（现有链路）
- 英文模式：字母键直接上屏英文字符（不经过拼音/候选/组合）
- 模式状态由引擎保存（Rust 侧 ascii_mode），平台层通过 FFI 读写
- 切换时若存在未完成拼音组合 → 先提交拼音串（保持文档一致）

**不做**：
- Shift 单按切换——与输入大写字母冲突，二期做智能判定
- 快捷键自定义（0.2.x 配置扩展）
- 模式状态持久化（重启后默认中文）

## 二、接口变更

### Rust 引擎（engine/src/lib.rs + ffi.rs）

```rust
pub struct Engine {
    pinyin_buf: String,
    candidates: Vec<String>,
    candidate_limit: usize,
    ascii_mode: bool,  // 新增：英文模式开关，默认 false（中文）
}

// 设置英文模式
pub fn set_ascii_mode(&mut self, enabled: bool);
// 查询英文模式
pub fn ascii_mode(&self) -> bool;
// process_key 逻辑分支：
//   ascii_mode && 字母 → 返回 false（不产生候选，平台层直接上屏字母）
//   或：引擎直接输出字母文本（select_candidate 返回单字母）
```

FFI 新增：
```cpp
int engine_set_ascii_mode(int enabled);  // 0 成功 / -1 未初始化
int engine_get_ascii_mode(void);          // 1=英文 / 0=中文 / -1 未初始化
```

### 平台层（tsf_keyevent.cpp + tsf_module.cpp）

`HandleKeyDown` 增加 Ctrl+Space 检测：
```cpp
// 在 HandleKeyDown 开头：
if (vk == VK_SPACE && (GetKeyState(VK_CONTROL) & 0x8000)) {
    // 切换模式
    const int cur = engine_get_ascii_mode();
    engine_set_ascii_mode(cur ? 0 : 1);
    out.state_changed = true;  // 触发候选窗口刷新（模式变化）
    return true;  // 吞键
}
```

英文模式字母处理：
```cpp
if (vk >= 'A' && vk <= 'Z' && engine_get_ascii_mode()) {
    // 英文模式：字母直接上屏（走 committed 通道，不吞键……需吞键防重复）
    // 方案：吞键 + committed = 字母（TSF 组合提交）
    out.eaten = true;
    out.committed = L"a";  // 对应按键字母
    out.state_changed = false;
    return true;
}
```

**注意**：英文模式下字母直接走 committed 通道（与选词上屏同一路径），由 TSF 组合提交上屏——复用 0.1.7 的 CommitComposition 链路，无需新建提交机制。

### 候选窗口模式指示（可选）

候选窗口背景色/图标区分模式（MVP 可选，无则英文模式无候选窗口弹出，用户可感知）。

## 三、数据流

```
Ctrl+Space 按下
  → HandleKeyDown 检测 Ctrl+Space
    → engine_get_ascii_mode() → engine_set_ascii_mode(!cur)
    → 吞键（应用不收到）
    → 候选窗口刷新（若有未完成拼音先提交）

英文模式字母 'a'
  → HandleKeyDown 检测 ascii_mode
    → committed = "a"（吞键）
    → CTextService::OnKeyDown → RunCompositionOp(Commit, "a")
    → TSF 组合上屏 "a"
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 引擎：ascii_mode + set/get + process_key 分支 | engine/src/lib.rs | cargo test |
| 2 | FFI：engine_set_ascii_mode / engine_get_ascii_mode | engine/src/ffi.rs | cargo test |
| 3 | 引擎测试：模式切换 + 英文模式行为 | engine/src/lib.rs | cargo test |
| 4 | 平台：Ctrl+Space 检测 + 英文字母直通 | tsf_keyevent.cpp | 编译 |
| 5 | 切换时未完成拼音提交处理 | tsf_module.cpp | 编译 |
| 6 | CMake 构建 + 回归 | — | 全链路 |

## 五、风险与依赖

- **Ctrl+Space 冲突**：部分应用（如 IDE 补全）用 Ctrl+Space——输入法激活时优先捕获（吞键），应用行为受限，属于输入法标准行为
- **依赖 0.1.7**：英文直通复用 CommitComposition 链路
- **依赖 0.1.8**：候选数配置已在引擎侧，模式切换不影响
