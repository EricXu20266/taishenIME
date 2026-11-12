# CHANGELOG 2026-11-13

## TSF 候选窗：宿主拒绝 UIElement 时改由自绘兜底（魔兽世界可用）

### 现象

魔兽世界（`wow.exe`）聊天框里输入法「不出候选、看不到打的拼音」，但引擎在工作、上屏其实成功。

### 根因（日志实证）

`%LOCALAPPDATA%\TaishenIME\ime_debug.log`（11:34–12:49 的 wow.exe 会话）：

```
EnsureCandidateUiElement: begun id=0 show=0      ← 宿主 BeginUIElement 返回 show=FALSE
UpdateCandidateWindow: UpdateUIElement 0x00000000
（没有 pinyin= / cands= / gotCaret= 后续行）      ← 被提前 return 掉了
CommitComposition: text=暗 caretOffset=-1 hr=OK   ← 上屏成功
```

链路：

1. `wow.exe` 走 TSF，DLL 已加载，按键全部进引擎（`AppStateSetAscii: wow.exe` 可证）。
2. `ITfUIElementMgr::BeginUIElement` 返回 `show=FALSE`。
3. 旧代码 `tsf_module.cpp` 见 `m_candidateUiSuppressed` 就 `m_candidateWindow.Hide(); return;`——
   **连自己的自绘候选窗也关掉了**，于是屏幕上什么都不显示。

对照正常宿主（同一日志 11:33 段）：`show=1` 后紧跟 `pinyin=f cands=5 / gotCaret=T caret=(1575,434)`。

### 判定依据：show=FALSE 有两种相反含义

| 含义 | 宿主 | 正确处置 |
|---|---|---|
| 宿主自己读候选数据自绘 | 微信等 UILess 宿主 | 尊重宿主，禁自绘 |
| 宿主根本不画、也不读数据 | 魔兽世界等游戏（经 msctfime 桥接） | 输入法必须自绘兜底 |

旧实现把两者混为一谈，一律 Hide。

### 改动

1. **`tsf_module.cpp`**
   - 新增 `m_uiElementDataRead`：在 `ITfCandidateListUIElement` 的
     `GetCount` / `GetSelection` / `GetString` / `GetPageIndex` / `GetCurrentPage` 中置位。
     每次 `BeginUIElement` 之前清零（宿主可能在 Begin 返回前就读数据）。
   - 新增 `ShouldSelfDrawWhileSuppressed()`，`UpdateCandidateWindow` 与
     `OnCandidateUiElementShow` 统一走它；不再无条件 Hide。
2. **`app_state.cpp` / `app_state.h`**
   - 新增 `AppStateShouldSelfDrawWhenSuppressed(cfg, hostReadData)`：
     `app_self_draw`（强制画）> `app_no_self_draw`（禁画）> 自动（宿主没读数据则自绘）。
3. **`config_reader.cpp` / `config_reader.h`**
   - 新增配置项 `app_self_draw` / `app_no_self_draw`（逗号分隔进程名，小写），读取与保存。
4. **不改动 IMM32 兼容层**：`E0C00804` 注册、`enable-imm32.ps1`、`install.nsi` 的历史清理段全部保持原样。
   本次问题在 TSF 通道，不靠 IMM32 解决。

### 附带结论（不进代码，记在文档里）

- 魔兽世界对组合会话挑剔：rime/weasel issue #983 实测
  `inline_preedit=true` 时每次丢字且严重掉帧，`false` 才稳定。游戏内建议 `inline_preedit=0`。
- 独占全屏下任何输入法候选窗都不可见（平台硬限制），游戏请用「窗口化全屏」。

### 验证

（见本轮提交时的构建与回归结果）
