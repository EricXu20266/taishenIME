# SPEC: 状态栏/托盘图标（Root #8，V0.2.13）

> 对应 ARCHITECT.md Root #8「呈现层 — 系统集成」
> 关联 DEV-TRACKER: 0.2.13 状态栏/托盘图标（中英状态+菜单）

---

## 一、需求

系统托盘显示输入法状态图标（中/英），右键菜单提供状态切换与快捷操作。

**用户价值**：不打开候选窗口也能看到当前中英状态；托盘右键快速切换。

**合约**：
- 托盘图标显示当前模式：中文（"中"）、英文（"英"），Tooltip 显示状态
- 激活 TSF 时创建托盘图标，Deactivate 时移除
- 右键菜单：切换中英 / 切换简繁 / 退出（禁用输入法）
- 左键单击：切换中英（与 Ctrl+Space 等效）
- 图标动态绘制（D2D 位图，16x16），不依赖 .ico 资源文件

**不做**：
- 托盘菜单打开设置面板 UI——后续
- 多状态图标（大写锁定等）——后续
- 开机自启——后续

## 二、实现

### 图标绘制（D2D 位图）

```
CreateD2DTrayIcon(text: "中"/"英") → HICON
  16x16 D2D 位图 → 深色圆角底 + 白色文字 → CreateIconIndirect
```

### 托盘管理（CTextService 成员）

```cpp
// 新增
bool m_trayAdded;          // 托盘图标是否已添加
HICON m_trayIcon;          // 当前图标（中/英）

// 方法
bool InitTrayIcon();        // 激活时：添加 Shell_NotifyIcon(NIM_ADD)
void UpdateTrayIcon();      // 模式切换时：换图标 + tooltip
void RemoveTrayIcon();      // 停用时：NIM_DELETE
LRESULT OnTrayMessage(...); // WM_APP+1 消息处理：左键/右键
void ShowTrayMenu(HWND);    // 右键菜单：切换中英/切换简繁/退出
```

### 消息流

```
Shell_NotifyIcon(NIM_ADD, {uCallbackMessage: WM_APP+1})
→ 窗口过程收到 WM_APP+1
  → lParam == WM_LBUTTONUP   → 切换中英（ToggleAsciiMode）
  → lParam == WM_RBUTTONUP   → ShowTrayMenu
    → ID_TRAY_TOGGLE_ASCII   → 切换中英
    → ID_TRAY_TOGGLE_TRAD    → 切换简繁
    → ID_TRAY_EXIT           → 通知 TSF 注销（Deactivate）
```

### 模式联动

```
ToggleAsciiMode()（托盘/菜单）：
  engine_get_ascii_mode → engine_set_ascii_mode(反) → UpdateTrayIcon()
```

## 三、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | D2D 托盘图标绘制（中/英） | tsf_module.cpp / 新增 tray_icon.cpp | 冒烟测试 |
| 2 | 托盘添加/更新/移除 | tsf_module.cpp | 冒烟测试 |
| 3 | WM_APP+1 消息处理 + 右键菜单 | tsf_module.cpp | 冒烟测试 |
| 4 | 模式联动（中英切换更新图标） | tsf_module.cpp | 冒烟测试 |
| 5 | 全链路验证 | — | build + 冒烟 |

## 四、测试用例

- 激活后托盘图标存在（NIM_ADD 成功）
- 中文模式图标/英文模式图标切换
- Deactivate 后托盘图标移除
- 右键菜单弹出（含三项）
