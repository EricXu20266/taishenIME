# SPEC: 全屏/多屏场景候选窗口定位优化（Root #3 #8）

> 对应 ARCHITECT.md Root #3「接口层」+ #8「呈现层」
> 关联 DEV-TRACKER: V0.4.3 P0-A / P1-B / P2-C
> 调研依据: 泰深 vs Weasel vs 微软拼音全屏处理对比（2026-08-08）

---

## 一、需求

**目标**：修正候选窗口在多显示器、非 100% 缩放、DPI-unaware 宿主（老游戏/老应用）场景下的定位错误，使候选窗口始终正确出现在光标附近。

**背景结论**（调研）：
- 独占全屏下候选窗不可见是 Windows 平台硬限制（DWM 不合成桌面层，微软官方确认），输入法无法解决，**不在本次范围**。
- 无边框全屏（现代游戏/全屏视频）DWM 合成生效，候选窗正常——前提是**定位正确**。
- 输入法可控的三个点：多显示器边界、DPI 坐标单位、定位失败兜底。

**三个子方案**：

| 编号 | 问题 | 级别 | 本次实施 |
|------|------|------|---------|
| P0-A | PositionWindow 边界检查基于主屏（SM_CXSCREEN），副屏输入时 clamp 错乱 | P0 | ✅ 实施 |
| P1-B | GetTextExt 坐标单位随宿主 DPI 感知模式变化，未换算导致缩放≠100% 时偏移 | P1 | ✅ 实施 |
| P2-C | 定位失败仅兜底到鼠标位置，无更稳的兜底策略 | P2 | ⏸ SPEC 记录，后续实施 |

## 二、现状代码事实

| 位置 | 现状 | 问题 |
|------|------|------|
| `candidate_window.cpp:415-425` | 边界检查 `GetSystemMetrics(SM_CXSCREEN/SM_CYSCREEN)` | **只认主屏**。副屏输入时：① 副屏坐标 > 主屏宽 → 被错误 clamp 回主屏 ② 副屏无 clamp 能力 |
| `tsf_module.cpp:1551-1565` | GetTextExt 失败 → `GetCursorPos`（物理像素） | 兜底可用，但坐标单位与 GetTextExt 可能不同（见 P1-B） |
| `candidate_window.cpp:347-352` | 渲染 `DpiScale()` 用 `GetDpiForWindow(候选窗 hwnd)` | 渲染缩放已按窗口 DPI ✅（无需改） |
| `ui_window.cpp:75-78` | 窗口创建初始位置用 SM_CXSCREEN 居中 | 初始位置无所谓（子类立刻 MoveWindow），低优先级 |
| 无 manifest | TSF DLL 不声明 DPI 感知 | TSF DLL in-proc，DPI 感知继承宿主进程——**不可改**，只能做坐标换算 |

## 三、P0-A 多显示器定位修正

**位置**：`candidate_window.cpp::PositionWindow`

**改法**：边界 clamp 从"主屏"改为"光标所在显示器的工作区"。

```cpp
void CCandidateWindow::PositionWindow(const RECT& caretRect)
{
    // ... 计算 width/height 不变 ...
    int x = caretRect.left;
    int y = caretRect.bottom + 4;

    // 光标所在显示器（P0-A：替代 SM_CXSCREEN）
    HMONITOR hMon = MonitorFromPoint({ x, y }, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi{ sizeof(mi) };
    GetMonitorInfo(hMon, &mi);
    const RECT& wa = mi.rcWork;   // 工作区（排除任务栏）

    if (x + width > wa.right)  x = wa.right - width;
    if (y + height > wa.bottom) {
        y = caretRect.top - height - 4;
        if (y < wa.top) y = wa.top;
    }
    if (x < wa.left) x = wa.left;

    SetWindowPos(..., HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
}
```

**边界情况**：
- caretRect 在屏幕外（游戏 HUD 越界）→ `MonitorFromPoint` 取最近显示器，clamp 进工作区 ✅
- 负坐标副屏（副屏在主屏左侧）→ `wa.left` 为负，clamp 用 `wa.left` 而非 0 ✅
- 多屏不同 DPI → P1-B 统一换算后坐标即物理像素，此逻辑天然正确

## 四、P1-B 坐标单位对齐（DPI-unaware 宿主）

### 4.1 问题本质

`ITfContextView::GetTextExt` 返回的屏幕坐标，单位取决于**宿主进程 DPI 感知模式**：

| 宿主感知模式 | GetTextExt 单位 | GetCursorPos 单位 |
|-------------|----------------|------------------|
| Per-Monitor V2 / System aware | 物理像素 | 物理像素 |
| **DPI unaware**（老游戏/老应用） | **96-DPI 逻辑像素** | **物理像素** |

DPI unaware 宿主（系统缩放 125% 时）→ GetTextExt 返回的坐标是"以为 96 DPI"的坐标，物理屏幕上实际要乘 1.25。当前代码直接当物理像素用 → **候选窗偏移**。

### 4.2 改法

**新增工具**：`platform/windows/src/dpi_util.h/.cpp`

```cpp
namespace taishen {

/// 检测宿主进程 DPI 感知模式
bool IsHostDpiUnaware(HWND hostWnd);   // GetWindowDpiAwarenessContext + GetAwarenessFromDpiAwarenessContext

/// 把 TSF GetTextExt 返回的屏幕坐标换算为物理像素
/// hostWnd: 目标应用窗口；pt: GetTextExt 原始坐标
POINT CaretToPhysicalPixel(HWND hostWnd, const POINT& pt);

}
```

**换算逻辑**（DPI unaware 时）：
```
物理像素 = 逻辑像素 × (GetDpiForSystem() / 96)
```
- `GetDpiForSystem()` 返回系统 DPI（缩放 125% → 120）
- DPI aware 宿主 → 不换算（已是物理像素）
- 注意：**不能用 GetDpiForWindow(宿主)** —— DPI-unaware 进程的 GetDpiForWindow 返回 96，要用系统级 DPI

**集成点**：`tsf_module.cpp::GetCaretRectFromContext` 成功路径返回前，把 RECT 转物理像素。兜底 `GetCursorPos` 已是物理像素，无需处理。

### 4.3 隐患确认（实施时验证）

- 泰深 DLL 自身无 manifest → 候选窗口的 DPI 感知 = 宿主模式。DPI-unaware 宿主下候选窗口也被系统虚拟化（按 96 DPI 拉伸显示），此时**物理像素坐标 + 系统虚拟化**的组合行为需真机验证（缩放 125%/150% + DPI-unaware 测试程序）。
- 若发现候选窗口自身被虚拟化拉伸，需在候选窗口创建时 `SetThreadDpiAwarenessContext(PerMonitorV2)` 局部提升（Windows 10 1607+ 可用，线程级不改变进程级）。

## 五、P2-C 定位兜底分级（远期，本期不实施）

**记录方案**，对标 weasel FullScreenLayout：

```
第 1 级：GetTextExt（TSF 标准）          — 现有
第 2 级：GetCursorPos（鼠标）            — 现有
第 3 级：屏幕底部居中全屏条              — 新增
         定位失败且无鼠标信息时，候选内容在光标所在显示器
         底部居中显示 + 自适应字号（字号二分逼近不溢出）
         （对标 weasel FullScreenLayout.cpp 的 AdjustFontPoint）
```

触发条件设计（后续细化）：第 3 级仅当 GetTextExt 和 GetCursorPos 都不可用时启用（如远程桌面锁定、特殊游戏）。实施时需新增 CandidatePanel 的"居中布局模式" + 字号自适应逻辑。

## 六、改动文件清单

| 文件 | 改动 | 涉及方案 |
|------|------|---------|
| `platform/windows/src/dpi_util.h` | 新增：DPI 感知检测 + 坐标换算 | P1-B |
| `platform/windows/src/dpi_util.cpp` | 新增：实现 | P1-B |
| `platform/windows/src/tsf_module.cpp` | GetCaretRectFromContext 返回前换算坐标 | P1-B |
| `platform/windows/src/candidate_window.cpp` | PositionWindow 多显示器 clamp | P0-A |
| `platform/windows/CMakeLists.txt` | 接入 dpi_util | P1-B |
| `docs/DEV-TRACKER.md` | 状态更新 | — |

**不做**（本次）：
- 独占全屏候选窗可见性（平台硬限制）
- P2-C 全屏居中条
- app_options 逐应用配置机制
- 候选窗口 WM_DPICHANGED 动态响应（候选窗生命周期短，价值低）

## 七、验证标准

| 方案 | 验证方法 | 通过标准 |
|------|---------|---------|
| P0-A | 双屏（主 1080p + 副 4K 缩放 150%），副屏 Notepad 输入拼音 | 候选窗出现在副屏光标下方，clamp 正确，不跳回主屏 |
| P0-A | 副屏左侧（负坐标）+ 光标贴近屏幕右/下边缘 | 候选窗翻转/收缩不出工作区 |
| P1-B | 系统缩放 125%/150%，DPI-unaware 测试程序（老 Win32/MFC 程序）输入 | 候选窗贴光标，无偏移 |
| P1-B | 系统缩放 100% + 现代应用（PerMonitorV2） | 回归：无偏移（行为不变） |
| 回归 | 常规单屏 100% 缩放 Notepad/Chrome 输入 | 候选窗行为与改动前一致 |
| 全链路 | cargo test + biome check + 安装版实测 | 零错误 |

## 八、实施计划

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | P0-A：PositionWindow 多显示器 clamp | 双屏实测 |
| 2 | P1-B：dpi_util 新建 + GetCaretRectFromContext 换算 | DPI-unaware 实测 |
| 3 | 回归 + 安装版验证 | cargo test + biome |
