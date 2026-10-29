# CHANGELOG 2026-10-29

## 0.1.52

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

### 追加（同日）：翻页键可配置 + Shift 快速点按上屏英文

### 翻页键
- 设置 → 按键 → 候选词导航：PgUp / PgDn 固定生效；新增勾选项
  「主键盘 + / −」「小键盘 + / −」「逗号 / 句号」「左 / 右方括号」
- 默认全部不勾（只有 PgUp/PgDn 生效）
- `KeyBindings` 新增 `page_numpad`；`page_minus_equal` / `page_brackets` 默认改为 false
- config.ini：`key_page_extra` 新增 `numpad` 取值
- **修复翻页优先级**：翻页判定移到中文标点之前——此前 `pageMinusEqual` 在中文模式下
  永远被标点分支先吃掉，实际是死配置

### Shift 快速点按上屏英文
- 中文模式已输入拼音时快速点按 Shift（<300ms、期间无其它键）→ 拼音原文作为英文
  直接上屏，并切到英文模式（对齐微软拼音）
- 无拼音时保持原行为（仅切中英）

### 验证
- `test_ascii_mode.exe` 新增 STEP10（翻页优先级），STEP1-10 全绿
- `taishen_ime.dll` / `test_ascii_mode` / `test_settings_dialog` 编译通过
- biome check：无错误
- 待人工验证：Shift 上屏、设置页勾选落 config.ini、真实多页候选翻页
  （测试内置词库只有 4 个候选，无法断言真实换页）

## 0.1.54

### 变更
- 托盘恢复两个图标：品牌「泰」图标左右键都弹菜单（「显示 / 隐藏工具栏」在菜单里），
  「中 / 英」状态图标左键一键切中英、右键菜单——单个图标无法同时承载两种语义
- 工具栏默认显示（config.ini 的 `toolbar` 缺省值改为 1）
- 工具栏新增「×」收起按钮：点击隐藏并写回 config.ini，从托盘菜单「显示工具栏」恢复

### 验证
- `test_ascii_mode` STEP1-10 全过
- `test_banner_window` 全过（此前 STEP1 因测试期望「默认开启」与构造函数默认 false 冲突而失败）

## 0.1.56

### 变更
- 品牌托盘图标固定「泰」：不再随中英切换变成「t ENG」（与「泰」字号不一致、切换时视觉跳动）。
  中英状态只由「中 / 英」状态图标表达，两个图标各司其职
- 托盘图标跟随前台：当前窗口切到微软拼音等其它输入法时图标自动隐藏，切回泰深再出现
  （此前只要有程序仍把泰深保留为输入法，图标就一直挂着）
- 工具栏收起按钮文案由「×」改为「隐」

### 验证
- `test_ascii_mode` STEP1-10 全过
- `test_banner_window` 全过

## 0.1.58

### 修复
- **IMM32 应用（LOL 等）内选词不上屏**：新增 `ImeGetCompositionStringW/A` 导出，
  上屏改走 `WM_IME_COMPOSITION(GCS_RESULTSTR)`。旧实现逐字符发 `WM_IME_CHAR`，
  GFxIME 不处理该消息 → 候选能选、字不进输入框
- **候选列表选完不消失**：补发 `IMN_OPENCANDIDATE` / `IMN_CLOSECANDIDATE`。
  LOL 的候选由游戏自己绘制，只隐藏我们自绘的窗口宿主是看不到的
- 鼠标点候选的上屏路径同步改为结果串 + 关闭通知

### 验证
- `test_imm32_load` 全过：25 个导出符号、`ImeGetCompositionString(GCS_RESULTSTR)`
  能读回上屏文本、选词后 `IMN_CLOSECANDIDATE` + `ENDCOMPOSITION`
- `test_ascii_mode` / `test_banner_window` 全过

## 0.1.62

### 变更
- 工具栏「隐」改为折叠：整条工具栏收缩为右下角一个小方块，方块上显示当前「中」/「英」
- 折叠小图标与中英状态托盘图标行为一致：**左键切换中英文**，
  **右键弹出快速菜单**（切换中英文 / 简繁 / 双拼、显示完整工具栏）
- 彻底隐藏仍走托盘菜单「隐藏工具栏」（toolbar=0），与折叠区分开
- 折叠状态不写入 config.ini：重启后回到展开状态

### 验证
- `test_banner_window` 全过

## 0.1.63

### 修复（code-auditor 审计结论，d1cfbe5..HEAD）
- 托盘：前台回调改为全程持递归锁遍历实例，RemoveTrayIcon 持同一把锁 ——
  消除跨线程 use-after-free
- 托盘：DestroyWindow 只在窗口所属线程执行，避免跨线程销毁失败后残留窗口引用
- 托盘：GUID 图标进程级唯一 —— 任一实例删除后同步清空其它实例的 added/visible 标志，
  修「图标删掉后再也回不来」
- 托盘：explorer 重启后按重建前的可见性恢复，不再把图标"复活"
- IMM32：清理组合串/结果串/候选通知三处重复块
- 工具栏：折叠态按钮命中返回 -1（点击走切中英文分支），修正注释与实现矛盾
- 设置页：「左/右方括号」文案更正为「取候选首 / 末字（非翻页）」

### 验证
- `test_banner_window` / `test_ascii_mode` / `test_imm32_load` 全过

## 0.1.64

### 修复：工具栏「隐」恢复彻底隐藏（撤销 0.1.60 折叠小图标）
- 用户实测：折叠后的小方块贴在屏幕右下角工作区，不在系统托盘；隐藏后找不到恢复入口
- `banner_window.cpp/.h`：删除折叠态全套实现 —— `kCollapsedWidth`、`ToolbarPanel::SetCollapsed`、
  折叠绘制/命中/左键切中英/右键快速菜单、`CBannerWindow::SetCollapsed`、
  折叠态右键菜单命令 `ToolbarCmd::Expand`
- 点「隐」= `SetEnabled(false)` + 写回 `config.ini` 的 `toolbar=0`（全进程一致），
  恢复入口为托盘「泰」图标右键菜单「显示工具栏」
- 编辑过程中误改 `PositionBottomRight` 函数头（临时变成 `EnsureWindow`），已当场修正并编译验证

### 验证
- `taishen_ime` / `taishen_ime_imm32` Release 编译通过
- `test_banner_window` 全过（STEP1-4 + TOOLBAR TEST PASSED）

## 0.1.65

### 工具栏隐藏方式确认（不改动托盘跟随前台）
- 「隐」按钮：点它隐藏工具栏并写回 `config.ini` 的 `toolbar=0`，恢复入口为托盘菜单「显示工具栏」
- 托盘图标保持 0.1.54 的跟随前台规则不变（切到非泰深宿主窗口自动隐藏）
- 工具栏默认显示（config 缺省 `true`，构造函数 `m_enabled=true`）
- 本版无托盘逻辑改动，上一版误加的「图标常驻」已 revert

### 验证
- `taishen_ime` / `taishen_ime_imm32` Release 编译通过
