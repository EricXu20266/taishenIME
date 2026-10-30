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

### 修复：热词被同前缀旧词压住（别名/全拼合并丢精确判定）

- `dictionary/mod.rs` `add_user_entry()`：同词在同一前缀键下合并时，「完整拼音长度」
  改为取更短值（此前保留首次插入值）。同一词既有规范全拼学习（huihua→会话）又有
  简拼别名学习（huih→会话）时，别名键下长度仍是 6 → `query` 的精确层判定
  `e.3 == key_len` 失效 → 热用户词掉进前缀扩展层，被精确层里更旧的同前缀词压住。
  真实数据：会话（今天 134 次）被 72 天前的绘画（22 次）压到第 2 位；直接/之间同样
- `user_short_index` 合并分支同步取更短长度，保持两套索引语义一致

### 测试

- 新增 `engine/tests/user_word_alias_exact.rs`：先学全拼、再学别名、并放入同前缀
  竞争词，断言热词回到首位（修复前失败，修复后通过）
- 真实用户词库快照：25 个高频词首位命中 25/25（修复前 23/25）

### 已知

- 候选只有一页时翻页键无视觉反馈（候选窗已移除页码指示），不是配置未生效
