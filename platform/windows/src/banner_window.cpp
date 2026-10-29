/// 右下角输入法工具栏 — 实现（V0.3.3 迁移：基于自研窗体系统）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.7
/// 结构：CBannerWindow（单例/前台跟踪/命令逻辑）→ UIWindow（置顶/不抢焦点）
///       → ToolbarPanel（自绘：阴影/卡片/4 按钮，激活态 accent 高亮）。
/// 按钮：中/英、简/繁、双拼、语音、设置、收起（0.1.54）。点击直接调引擎 FFI 切换，状态实时高亮。
/// 显示条件：托盘开关(enabled) && 前台窗口线程激活泰深。

#include "banner_window.h"
#include "debug_log.h"
#include "engine_bridge.h"
#include "settings_dialog.h"
#include "theme.h"
#include "app_state.h"
#include "ui_render.h"
#include "voice_manager.h"
#include "config_reader.h"

#include <functional>
#include <shellapi.h>
#include <thread>
#include <windowsx.h>

// DLL 模块句柄（dllmain.cpp 定义于全局命名空间，用于定位 config.ini）
extern HMODULE g_hModule;

namespace taishen {

// 布局常量（与 0.2.x 一致）
static constexpr int kToolbarWidth = 252;   // 工具栏宽（0.1.54：+收起按钮）
static constexpr int kToolbarHeight = 38;   // 工具栏高
static constexpr int kCornerRadius = 8;     // 圆角半径
static constexpr int kMargin = 12;          // 距屏幕右下角边距
static constexpr int kShadowOffset = 4;     // 阴影偏移（D2D 半透明）
static constexpr int kBtnGap = 4;           // 按钮间距
static constexpr int kBtnCount = 6;         // 按钮数（0.1.54：+收起）
static constexpr int kCollapsedWidth = 46;  // 折叠后小图标宽度（0.1.60）

// ===========================================================================
// 工具栏内容面板（自绘）
// ===========================================================================
class CBannerWindow::ToolbarPanel : public UIControl
{
public:
    ToolbarPanel()
    {
        SetRect({ 0, 0, kToolbarWidth, kToolbarHeight });
    }

    /// 按钮命令回调（CBannerWindow::HandleCommand）
    void SetCommandCallback(std::function<void(ToolbarCmd)> cb) { m_cmdCb = std::move(cb); }

    /// 折叠态右键菜单回调（0.1.60）
    void SetMenuCallback(std::function<void()> cb) { m_menuCb = std::move(cb); }

    /// 折叠态（0.1.60）：整条工具栏收缩为一个小方块图标
    void SetCollapsed(bool collapsed)
    {
        m_collapsed = collapsed;
        SetRect({0, 0, collapsed ? kCollapsedWidth : kToolbarWidth,
                 kToolbarHeight});
        Invalidate();
    }

    /// 同步引擎状态 → 按钮文字/激活态
    void SetState(bool ascii, bool trad, bool shuangpin, bool voiceActive)
    {
        m_texts[0] = ascii ? L"英" : L"中";
        m_texts[1] = trad ? L"繁" : L"简";
        m_texts[2] = shuangpin ? L"双拼" : L"全拼";
        m_texts[3] = voiceActive ? L"🎤" : L"麦";
        m_texts[4] = L"设置";
        m_texts[5] = L"隐";   // 收起工具栏
        m_active[0] = !ascii;    // 中文模式时"中"高亮
        m_active[1] = trad;
        m_active[2] = shuangpin;
        m_active[3] = voiceActive; // 录音中高亮（麦克风）
        m_active[4] = false;
        m_active[5] = false;
        Invalidate();
    }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const float w = static_cast<float>(Width());
        const float h = static_cast<float>(Height());
        const float radius = static_cast<float>(kCornerRadius);

        // 折叠态（0.1.60）：整个窗口画成一个小方块，显示当前中/英
        if (m_collapsed) {
            const D2D1_RECT_F card = D2D1::RectF(
                4.0f, 5.0f, w - 8.0f, h - 10.0f - static_cast<float>(kShadowOffset));
            r.FillRoundedRect(card, 6.0f, t.accent);
            r.DrawText(m_texts[0], card, t.fontSize, t.accentText, true,
                       DWRITE_TEXT_ALIGNMENT_CENTER,
                       DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            return;
        }

        // V0.3.x：去掉工具栏背景/阴影/边框（问题 4：工具栏背景去掉），只保留按钮
        const float contentX = 8.0f;
        const float contentW = w - 16.0f - static_cast<float>(kShadowOffset);
        const float btnW = (contentW - kBtnGap * (kBtnCount - 1)) / kBtnCount;
        const float btnTop = 5.0f;
        const float btnH = h - 10.0f - static_cast<float>(kShadowOffset);

        for (int i = 0; i < kBtnCount; ++i) {
            const float bx = contentX + i * (btnW + kBtnGap);
            const D2D1_RECT_F rc = D2D1::RectF(bx, btnTop, bx + btnW, btnTop + btnH);

            // 按钮底色：激活=accent / 按下 / 悬停 / 普通
            D2D1_COLOR_F bg;
            D2D1_COLOR_F fg;
            if (m_active[i]) {
                bg = t.accent;
                fg = t.accentText;
            } else if (i == m_pressedBtn && m_pressedBtn >= 0) {
                bg = t.pressedBg;
                fg = t.text;
            } else if (i == m_hoverBtn && m_hoverBtn >= 0) {
                bg = t.hoverBg;
                fg = t.text;
            } else {
                bg = t.cardBg;
                fg = t.text;
            }
            r.FillRoundedRect(rc, 4.0f, bg);
            r.DrawText(m_texts[i], rc, t.fontSize, fg, true,
                       DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
    }

    // ── 交互 ──
    void OnMouseMove(int x, int y) override
    {
        const int btn = BtnAt(x, y);
        if (btn != m_hoverBtn) {
            m_hoverBtn = btn;
            Invalidate();
        }
    }
    void OnMouseLeave() override
    {
        if (m_hoverBtn != -1 || m_pressedBtn != -1) {
            m_hoverBtn = -1;
            m_pressedBtn = -1;
            Invalidate();
        }
    }
    void OnMouseDown(int x, int y, bool /*left*/) override
    {
        m_pressedBtn = BtnAt(x, y);
        Invalidate();
    }
    void OnMouseUp(int x, int y, bool /*left*/) override
    {
        m_pressedBtn = -1;
        Invalidate();
    }
    void OnClick(int x, int y) override
    {
        // 折叠态（0.1.60）：小图标左键直接切中英（对齐微软拼音状态图标）
        if (m_collapsed) {
            if (m_cmdCb) {
                m_cmdCb(ToolbarCmd::Ascii);
            }
            return;
        }
        const int btn = BtnAt(x, y);
        if (btn >= 0 && m_cmdCb) {
            m_cmdCb(static_cast<ToolbarCmd>(btn));
        }
    }

    /// 折叠态右键（0.1.60）：弹快速菜单（切中英 / 简繁 / 双拼 / 展开工具栏）
    void OnRightClick(int x, int y) override
    {
        (void)x;
        (void)y;
        if (m_collapsed && m_menuCb) {
            m_menuCb();
        }
    }

private:
    /// x → 按钮索引（-1 未命中），布局与 Draw 一致
    int BtnAt(int x, int y) const
    {
        if (m_collapsed) {
            return -1;  // 折叠态不按按钮索引命中（OnClick 走 Ascii 分支）
        }
        const float w = static_cast<float>(Width());
        const float h = static_cast<float>(Height());
        if (y < 5 || y > h - 10.0f - static_cast<float>(kShadowOffset) ||
            x < 8) {
            return -1;
        }
        const float contentW = w - 16.0f - static_cast<float>(kShadowOffset);
        const float btnW = (contentW - kBtnGap * (kBtnCount - 1)) / kBtnCount;
        const int idx = static_cast<int>((x - 8) / (btnW + kBtnGap));
        if (idx >= 0 && idx < kBtnCount &&
            static_cast<float>(x - 8) - idx * (btnW + kBtnGap) < btnW) {
            return idx;
        }
        return -1;
    }

    std::wstring m_texts[kBtnCount] = { L"中", L"简", L"全拼", L"麦", L"设置", L"隐" };
    bool m_active[kBtnCount] = { true, false, false, false, false, false };
    int m_hoverBtn = -1;
    int m_pressedBtn = -1;
    bool m_collapsed = false;
    std::function<void(ToolbarCmd)> m_cmdCb;
    std::function<void()> m_menuCb;
};

// ===========================================================================
// CBannerWindow（单例 + 前台跟踪 + 命令）
// ===========================================================================
CBannerWindow& CBannerWindow::Instance()
{
    static CBannerWindow s_instance;
    return s_instance;
}

CBannerWindow::CBannerWindow()
    : m_panelOwner(std::make_unique<ToolbarPanel>()),
      m_panel(m_panelOwner.get()),
      m_enabled(true),   // 0.1.54：工具栏默认显示（与 config 默认一致）
      m_lightTheme(false),
      m_hook(nullptr)
{
    m_panel->SetCommandCallback([this](ToolbarCmd cmd) { HandleCommand(cmd); });
    m_panel->SetMenuCallback([this]() { ShowCollapsedMenu(); });
}

CBannerWindow::~CBannerWindow()
{
    if (m_hook != nullptr) {
        UnhookWinEvent(m_hook);
        m_hook = nullptr;
    }
    m_window.Destroy();
}

/// 设置主题模式（V0.2.20）：true=浅色，false=深色；重绘生效
void CBannerWindow::SetLightTheme(bool light)
{
    if (m_lightTheme != light) {
        m_lightTheme = light;
        m_window.SetTheme(light ? UIThemeLight() : UIThemeDark());
        if (m_window.Hwnd() != nullptr) {
            m_window.Invalidate();
        }
    }
}

/// 同步引擎状态到按钮并重绘（0.2.26：Shift 切换中英后刷新高亮）
void CBannerWindow::Refresh()
{
    RefreshButtons();
    if (m_window.Hwnd() != nullptr) {
        m_window.Invalidate();
    }
}

void CBannerWindow::RefreshButtons()
{
    if (m_panel == nullptr) {
        return;
    }
    const bool ascii = (engine_get_ascii_mode() == 1);
    const bool trad = (engine_get_traditional() == 1);
    const bool sp = (engine_get_shuangpin() == 1);
    // V0.5.8：语音按钮激活态 = 录音中
    const bool voiceActive = taishen::VoiceManager::Instance().IsListening();
    m_panel->SetState(ascii, trad, sp, voiceActive);
}

// ── 前台跟踪 ──

void CALLBACK CBannerWindow::OnForegroundChanged(HWINEVENTHOOK /*hook*/, DWORD /*event*/,
                                                 HWND /*hwnd*/, LONG /*idObject*/,
                                                 LONG /*idChild*/, DWORD /*idEventThread*/,
                                                 DWORD /*dwmsEventTime*/)
{
    // 前台窗口切换 → 重新评估（回调在注册线程消息队列，无需加锁）
    CBannerWindow::Instance().EvaluateForeground();
}

void CBannerWindow::EvaluateForeground()
{
    const HWND fg = GetForegroundWindow();
    DWORD fgTid = 0;
    if (fg != nullptr) {
        fgTid = GetWindowThreadProcessId(fg, nullptr);
    }
    const bool fgTaishen = fgTid != 0 && (m_tids.count(fgTid) > 0);
    // 托盘图标跟随前台（0.1.54）：状态变化才通知，避免切窗口时反复增删图标
    if (!m_fgKnown || fgTaishen != m_fgTaishen) {
        m_fgKnown = true;
        m_fgTaishen = fgTaishen;
        if (m_fgCb) {
            m_fgCb(fgTaishen);
        }
    }
    const bool active = m_enabled && fgTaishen;
    if (active) {
        if (!m_window.IsVisible() && EnsureWindow()) {
            PositionBottomRight();
            m_window.Show();
            RefreshButtons();
        }
    } else if (m_window.IsVisible()) {
        m_window.Hide();
    }
}

void CBannerWindow::RegisterThread(DWORD tid)
{
    m_tids.insert(tid);
    // 首次注册时挂前台监听（每进程一次）
    if (m_hook == nullptr) {
        m_hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND,
                                 nullptr, OnForegroundChanged, 0, 0,
                                 WINEVENT_OUTOFCONTEXT);
        taishen::DebugLog("Toolbar: SetWinEventHook fg=" +
                          std::to_string(m_hook != nullptr));
    }
    EvaluateForeground();
}

void CBannerWindow::UnregisterThread(DWORD tid)
{
    m_tids.erase(tid);
    EvaluateForeground();
}

void CBannerWindow::SetEnabled(bool enabled)
{
    m_enabled = enabled;
    taishen::DebugLog("Toolbar: SetEnabled=" + std::to_string(m_enabled));
    EvaluateForeground();
}

void CBannerWindow::SetForegroundCallback(std::function<void(bool)> cb)
{
    m_fgCb = std::move(cb);
}

// ── 折叠态右键菜单（0.1.60）──

void CBannerWindow::ShowCollapsedMenu()
{
    if (m_window.Hwnd() == nullptr) {
        return;
    }
    const bool ascii = (engine_get_ascii_mode() == 1);
    const bool trad = (engine_get_traditional() == 1);
    const bool sp = (engine_get_shuangpin() == 1);
    HMENU menu = CreatePopupMenu();
    if (menu == nullptr) {
        return;
    }
    AppendMenuW(menu, MF_STRING, 1001,
                ascii ? L"切换到中文模式" : L"切换到英文模式");
    AppendMenuW(menu, MF_STRING, 1002,
                trad ? L"关闭简繁转换" : L"开启简繁转换");
    AppendMenuW(menu, MF_STRING, 1003,
                sp ? L"切换到全拼输入" : L"切换到双拼输入");
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, 1004, L"显示完整工具栏");

    POINT pt;
    GetCursorPos(&pt);
    SetForegroundWindow(m_window.Hwnd());
    const int cmd = TrackPopupMenu(menu,
                                   TPM_RETURNCMD | TPM_NONOTIFY,
                                   pt.x, pt.y, 0, m_window.Hwnd(), nullptr);
    DestroyMenu(menu);
    if (cmd == 1001) {
        HandleCommand(ToolbarCmd::Ascii);
    } else if (cmd == 1002) {
        HandleCommand(ToolbarCmd::Trad);
    } else if (cmd == 1003) {
        HandleCommand(ToolbarCmd::Shuangpin);
    } else if (cmd == 1004) {
        HandleCommand(ToolbarCmd::Expand);
    }
}

// ── 命令执行 ──

void CBannerWindow::HandleCommand(ToolbarCmd cmd)
{
    switch (cmd) {
    case ToolbarCmd::Ascii: {
        // V0.2.33：走 per-app 记忆，更新当前进程状态
        const int cur = engine_get_ascii_mode();
        taishen::AppStateSetAscii(cur ? false : true);
        break;
    }
    case ToolbarCmd::Trad: {
        const int cur = engine_get_traditional();
        engine_set_traditional(cur ? 0 : 1);
        break;
    }
    case ToolbarCmd::Shuangpin: {
        const int cur = engine_get_shuangpin();
        engine_set_shuangpin(cur ? 0 : 1);
        break;
    }
    case ToolbarCmd::Voice: {
        // V0.5.8：语音输入开关（点 Mic 开始/停止说话转写）
        auto& vm = taishen::VoiceManager::Instance();
        if (vm.IsListening()) {
            vm.Stop();
        } else {
            const std::wstring err = vm.Start();
            if (!err.empty()) {
                taishen::ForceLog("VoiceManager start failed: " +
                                  std::string(err.begin(), err.end()));
            }
        }
        break;
    }
    case ToolbarCmd::Settings: {
        // 弹出设置窗口（SPEC: settings-ui）
        // V0.3.x：独立线程弹窗——TSF 回调运行在宿主进程（如 Notepad++）UI 线程，
        // 同步 RunModal 嵌套消息循环会与宿主冲突（Notepad++ 0xC0000005 崩溃，问题 5）。
        // 独立线程创建窗口 + 自持消息循环，不阻塞宿主 UI 线程。
        wchar_t dllPath[MAX_PATH] = {0};
        if (GetModuleFileNameW(g_hModule, dllPath, MAX_PATH) > 0) {
            std::wstring dllDir(dllPath);
            const size_t slash = dllDir.find_last_of(L"\\/");
            dllDir = dllDir.substr(0, slash + 1);
            std::thread([dllDir]() {
                try {
                    taishen::ShowSettingsDialog(nullptr, dllDir);
                } catch (const std::exception& e) {
                    taishen::ForceLog(std::string("Settings dialog crashed: ") + e.what());
                } catch (...) {
                    taishen::ForceLog("Settings dialog crashed: unknown exception");
                }
            }).detach();
        }
        break;
    }
    case ToolbarCmd::Hide: {
        // 0.1.60：点「隐」折叠成右下角小图标，点小图标可再展开。
        // 彻底隐藏仍走托盘菜单「隐藏工具栏」（SetEnabled(false)）。
        SetCollapsed(true);
        break;
    }
    case ToolbarCmd::Expand: {
        SetCollapsed(false);
        break;
    }
    default:
        break;
    }
    // 切换后刷新按钮状态
    RefreshButtons();
    if (m_window.Hwnd() != nullptr) {
        m_window.Invalidate();
    }
}

// ── 窗口创建与定位 ──

bool CBannerWindow::EnsureWindow()
{
    if (m_window.Hwnd() != nullptr) {
        return true;
    }
    if (!m_window.Create(L"TaishenBannerWindow", kToolbarWidth, kToolbarHeight,
                         true, true)) {
        taishen::DebugLog("Toolbar: window create failed");
        return false;
    }
    m_window.SetTheme(m_lightTheme ? UIThemeLight() : UIThemeDark());
    m_window.SetRoot(m_panel);
    // UIWindow now owns the root control tree; retain only a non-owning pointer.
    m_panelOwner.release();
    taishen::DebugLog("Toolbar: initialized hwnd=" +
                      std::to_string(reinterpret_cast<long long>(m_window.Hwnd())));
    return true;
}

void CBannerWindow::SetCollapsed(bool collapsed)
{
    if (m_collapsed == collapsed) {
        return;
    }
    m_collapsed = collapsed;
    if (m_panel != nullptr) {
        m_panel->SetCollapsed(collapsed);
    }
    if (EnsureWindow()) {
        PositionBottomRight();
    }
    taishen::DebugLog("Toolbar: collapsed=" +
                      std::to_string(m_collapsed));
}

void CBannerWindow::PositionBottomRight()
{
    if (m_window.Hwnd() == nullptr) {
        return;
    }
    RECT workArea = {};
    if (SystemParametersInfoW(SPI_GETWORKAREA, 0, &workArea, 0)) {
        // 折叠时按小图标宽度贴右下角（0.1.60）
        const int width = m_collapsed ? kCollapsedWidth : kToolbarWidth;
        const int x = workArea.right - width - kMargin;
        const int y = workArea.bottom - kToolbarHeight - kMargin;
        // 显示由 EvaluateForeground 的 UIWindow::Show 控制（V0.3.5 审查修复）
        SetWindowPos(m_window.Hwnd(), HWND_TOPMOST, x, y,
                     width, kToolbarHeight,
                     SWP_NOACTIVATE);
    }
}

// ── 状态文字（tooltip 用）──

std::wstring CBannerWindow::StatusText() const
{
    std::wstring text;
    text += (engine_get_ascii_mode() == 1) ? L"英文" : L"中文";
    if (engine_get_traditional() == 1) {
        text += L"·繁";
    }
    if (engine_get_shuangpin() == 1) {
        text += L"·双拼";
    }
    return text;
}

} // namespace taishen
