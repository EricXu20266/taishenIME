# CHANGELOG 2026-10-29

## 0.1.52 小键盘符号修正 + 托盘图标合并

### 修复：小键盘 `+` 上屏成 `=`（含同类键）
- `tsf_keyevent.h/.cpp`：新增 `IsKeypadKey()` / `KeypadShiftEquivalent()`；
  `HandleKeyDown`、`ShouldEatKey` 增加 `rawVk` 参数，保留未归一化的原始键码
- `HandleKeyDown` 顶部统一 `shiftDown`（真实 Shift 或小键盘 `+`/`*` 的 Shift 等价），
  中文标点映射据此取字符 → 小键盘 `+` 得 `+`、`*` 得 `*`
- 小键盘数字与符号不再参与候选选词、`+/-` 翻页、逗号句号翻页
- 计算器/数字大写模式按原始键码直取小键盘字符（修 `VK_MULTIPLY` 被当成数字 `8`）
- 调用点同步：`tsf_module.cpp`（OnTestKeyDown / OnKeyDown）、`imm32/imm32_ime.cpp`（ImeToAsciiEx）

### 变更：托盘只保留一个图标
- 删除独立的中英状态托盘图标（`kTrayModeGuid` / `GetModeTrayIcon` / `m_modeTrayAdded`）
- 品牌图标本身区分中/英视觉（「泰」/「t ENG」），tooltip 显示当前模式
- 托盘左键切中英、右键弹菜单；沿用用户已固定的品牌图标 GUID，不再被系统归入溢出区

### 验证
- `test_ascii_mode.exe`：STEP8（小键盘 `+ * -`、数字不选词）、STEP9（英文模式不翻页）通过
- `taishen_ime.dll` / `test_tsf_load` / `test_banner_window` / `test_imm32_load` 编译通过
- `test_tsf_load`、`test_imm32_load` 通过；`test_banner_window` STEP1 为既有失败
  （测试期望工具栏默认开启，构造函数默认 `false`），与本次改动无关
- biome check：无错误
