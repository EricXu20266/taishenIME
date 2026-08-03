# SPEC: 应用级配置 + per-app 状态记忆（Root #7 #3 #4，V0.2.32/0.2.33）

> 对标 rime-ice weasel.yaml `app_options` 机制。
> 关联 DEV-TRACKER: 0.2.32 应用级配置 app_options（升级 P2-6）/ 0.2.33 per-app 状态记忆

---

## 一、需求

雾凇（Weasel）跨程序使用的核心体验：**每个程序独立维护中英状态，切走再切回不丢状态**；
且可按进程名配置初始行为（`app_options: { firefox.exe: { inline_preedit: true }, cmd.exe: { ascii_mode: true } }`）。

**现状差距**：

| 维度 | 现状（P2-6） | 目标（对标雾凇） |
|------|-------------|-----------------|
| 配置语义 | `app_ascii` 命中进程名 → **每次按键强制**英文 | 初始状态：进入进程时应用，用户手动切换后不被弹回 |
| 默认中文 | 无（只支持英文） | `app_cn` 支持指定程序默认中文 |
| 行内预编辑 | 全局开关，按程序不可覆盖 | `app_inline` 指定程序强制行内预编辑（雾凇 firefox bug 规避场景） |
| 状态隔离 | 引擎全局单例，所有程序共享 | per-app 状态记忆：每个进程独立中英状态 |

**本期目标**：
- 0.2.32：`app_ascii` 语义修正（强制 → 初始）+ 新增 `app_cn` / `app_inline` 配置
- 0.2.33：TSF 平台层 per-app 状态记忆表，焦点切换时应用各进程记忆状态

**用户价值**：终端（cod.exe/cmd.exe）自动英文、聊天（微信）保持中文，切窗口不再手动切输入法。

**不做**：
- per-window 记忆（同进程多窗口独立状态）——TSF DLL 进程内注入，同进程共享状态符合直觉（浏览器多标签通常要一致状态），雾凇亦为 per-app 粒度
- vim_mode（Esc 切 ASCII）——P2 后续可加
- 引擎侧 multi-context 改造——状态决策放平台层，引擎保持单例（FFI 零改动）

## 二、数据模型

### ImeConfig 新增

```cpp
std::vector<std::wstring> app_ascii_list;   // 已有：命中进程 → 默认英文（语义修正）
std::vector<std::wstring> app_cn_list;      // 新增：命中进程 → 默认中文
std::vector<std::wstring> app_inline_list;  // 新增：命中进程 → 强制行内预编辑
```

### config.ini

```
app_ascii=cod.exe,cmd.exe        # 默认英文（保留，语义：初始状态而非强制）
app_cn=notepad.exe               # 默认中文
app_inline=firefox.exe           # 强制行内预编辑
```

### 平台层状态表（tsf_module.cpp 模块级）

```cpp
struct AppState {
    bool ascii = false;   // 记忆的中英状态
    bool init = false;    // 是否已记录（区分「未进入过」与「记忆为中文」）
};
static std::mutex g_appStateMutex;
static std::unordered_map<std::wstring, AppState> g_appStates;  // key: 小写进程名
```

## 三、接口与流程

### 核心函数（tsf_module.cpp）

```cpp
// 应用前台进程的初始/记忆状态；返回是否发生状态变化
static bool ApplyForegroundAppState();
// 设置中英状态并更新当前前台进程的记忆（所有切换入口统一走这里）
static void SetAsciiWithAppState(bool ascii);
// 获取当前前台进程名（已有 GetForegroundProcessName，保留）
```

### ApplyForegroundAppState 逻辑

```
proc = GetForegroundProcessName()
锁 g_appStateMutex
if g_appStates[proc].init:
    ascii = g_appStates[proc].ascii          # 记忆状态（用户切过则用记忆）
else:
    if proc ∈ app_ascii_list:  ascii = true   # 初始英文
    elif proc ∈ app_cn_list:   ascii = false  # 初始中文
    else:                       ascii = false # 全局默认中文
    g_appStates[proc] = { ascii, init=true }
解锁
if ascii != engine_get_ascii_mode():
    engine_set_ascii_mode(ascii)
# inline 覆盖（与全局配置无关，命中即强制 true）
m_candidateWindow.SetInlinePreedit(proc ∈ app_inline_list ? true : cfg.inline_preedit)
```

### 触发时机

| 时机 | 位置 | 动作 |
|------|------|------|
| OnSetFocus（焦点变化） | tsf_module.cpp | ApplyForegroundAppState() |
| OnKeyDown（兜底，B-4 教训：OnSetFocus 不可靠） | tsf_keyevent.cpp | 前台进程缓存变化时 ApplyForegroundAppState()（进程名比较，避免每次按键查表） |
| 手动切换 Shift | tsf_keyevent.cpp:224 | SetAsciiWithAppState(翻转) |
| 手动切换 托盘菜单 | tsf_module.cpp:719 | SetAsciiWithAppState(翻转) |
| 手动切换 工具栏按钮 | banner_window.cpp:235 | SetAsciiWithAppState(翻转) |
| ~~P2-6 强制逻辑~~ | tsf_module.cpp:1305-1318 | **删除**（每次按键强制 → 首次进入应用，被 ApplyForegroundAppState 取代） |

### 引擎侧

**零改动**。FFI `engine_set_ascii_mode` / `engine_get_ascii_mode` 保持单例语义，状态决策完全在平台层。

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | config_reader：解析 app_cn / app_inline，写出时同步 | config_reader.cpp/.h | 编译 |
| 2 | settings_dialog + settings.rc：新增 2 个文本框（默认中文/强制行内） | settings_dialog.cpp settings.rc resource.h | 编译 |
| 3 | tsf_module：状态表 + ApplyForegroundAppState + OnSetFocus 接入 + 删除 P2-6 强制 | tsf_module.cpp | 编译 |
| 4 | 切换入口统一：tsf_keyevent / banner_window 走 SetAsciiWithAppState | tsf_keyevent.cpp banner_window.cpp | 编译 |
| 5 | OnKeyDown 兜底：前台进程变化时应用状态 | tsf_keyevent.cpp | 编译 |
| 6 | 全链路验证 | — | build + 冒烟 |

## 五、测试用例

- **初始状态**：配置 `app_ascii=cod.exe`，进入 cmd → 自动英文；在 cmd 内 Shift 切回中文，不被打回英文（原 P2-6 每次按键强制，此为语义修正验证点）
- **状态记忆**：cmd 切到英文 → 切到微信（中文）→ 切回 cmd 仍是英文；微信中切英文 → 切回微信保持英文（用户上次手动切换的状态）
- **app_cn**：`app_cn=notepad.exe` 且全局默认中文 → 记事本中文
- **app_inline**：`app_inline=firefox.exe` → Firefox 行内预编辑强制开（即使全局 inline_preedit=0）
- **无配置**：不填任何 app_* → 行为与现状一致（全局中文，手动切换全局生效）
