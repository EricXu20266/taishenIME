# CHANGELOG 2026-10-30

## 0.1.67

### 修复：设置页保存后不生效（配置热加载失效）

- `tsf_module.cpp` `StartConfigWatch()`：`m_configPath` 与基准 mtime 改为无条件初始化。
  0.1.67 停用托盘图标后托盘窗口不再创建（`m_trayHwnd` 恒 `nullptr`），旧写法在
  提前 return 处就退出 → 路径永远为空 → `ReloadConfigIfChanged()` 恒返回 →
  保存设置后必须重启宿主应用。2s 轮询仍在有托盘窗口时启动，无窗口时由按键兜底
- `ApplyConfig()` 增加所选翻页附加键日志（`pageKeys minus_equal=/numpad=/...`）
- `imm32/imm32_ime.cpp` `ImeSelect()`：补 `SetKeyBindings()`，IMM32 进程（老应用的
  `taishen_ime_imm32.ime` 路径）此前没有配置应用点，按键设置在老应用里恒为默认值

### 测试

- `test_config_reader.cpp`：新增 `key_page_extra=minus_equal,numpad` 解析往返断言
- `test_ascii_mode.exe` STEP1–STEP10 全通过；CMake Release 全目标编译通过

### 已知

- 候选只有一页时翻页键无视觉反馈（候选窗已移除页码指示），不是配置未生效
