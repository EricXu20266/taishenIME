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

#include <algorithm>
#include <commctrl.h>
#include <functional>
#include <shellapi.h>
#include <thread>
#include <windowsx.h>

// DLL 模块句柄（dllmain.cpp 定义于全局命名空间，用于定位 config.ini）
extern HMODULE g_hModule;

namespace taishen {

// 布局常量（P1-12 / P2-13：加宽至 280×36 DIP，去除偏心阴影偏移，补圆角裁剪）
static constexpr int kToolbarWidth = 280;   // 工具栏宽
static constexpr int kToolbarHeight = 36;   // 工具栏高
static constexpr int kCornerRadius = 8;     // 圆角半径
static constexpr int kMargin = 12;          // 距屏幕右下角边距
static constexpr int kBtnGap = 4;           // 按钮间距
static constexpr int kBtnCount = 6;         // 按钮数（中/简/全拼/麦/设置/×）

static void ApplyToolbarRoundedRegion(HWND hwnd)
{
    if (hwnd == nullptr) {
        return;
    }
    RECT rc{};
    GetClientRect(hwnd, &rc);
    const int w = rc.right - rc.left;
    const int h = rc.bottom - rc.top;
    if (w <= 0 || h <= 0) {
        return;
    }
    const UINT dpi = GetDpiForWindow(hwnd);
    const float scale = dpi > 0 ? static_cast<float>(dpi) / 96.0f : 1.0f;
    int rad = static_cast<int>(static_cast<float>(kCornerRadius) * scale + 0.5f);
    rad = (std::min)(rad, (std::min)(w, h) / 2);
    if (rad <= 0) {
        SetWindowRgn(hwnd, nullptr, TRUE);
        return;
    }
    HRGN rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, rad * 2, rad * 2);
    if (rgn != nullptr) {
        SetWindowRgn(hwnd, rgn, TRUE);
    }
}

static const wchar_t* ButtonTooltip(int btn)
{
    switch (btn) {
    case 0: return L"切换中/英文模式";
    case 1: return L"切换简体/繁体";
    case 2: return L"切换全拼/双拼";
    case 3: return L"语音输入（开始/停止）";
    case 4: return L"打开泰深输入法设置";
    case 5: return L"隐藏悬浮工具栏（可在任务栏语言栏右键恢复）";
    default: return L"";
    }
}

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

    /// 同步引擎状态 → 按钮文字/激活态
    void SetState(bool ascii, bool trad, bool shuangpin, bool voiceActive)
    {
        m_texts[0] = ascii ? L"英" : L"中";
        m_texts[1] = trad ? L"繁" : L"简";
        m_texts[2] = shuangpin ? L"双拼" : L"全拼";
        m_texts[3] = voiceActive ? L"🎤" : L"麦";
        m_texts[4] = L"设置";
        m_texts[5] = L"×";   // 隐藏工具栏
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

        // 圆角卡片边框（外围由 SetWindowRgn 裁剪）
        const D2D1_RECT_F cardRc = D2D1::RectF(0.5f, 0.5f, w - 0.5f, h - 0.5f);
        r.FillRoundedRect(cardRc, radius, t.bg);
        r.DrawRoundedRect(cardRc, radius, t.border, 1.0f);

        const float contentX = 6.0f;
        const float contentW = w - 12.0f;
        const float btnW = (contentW - kBtnGap * (kBtnCount - 1)) / kBtnCount;
        const float btnTop = 4.0f;
        const float btnH = h - 8.0f;
        const float btnFont = 13.0f;

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
            r.FillRoundedRect(rc, 5.0f, bg);
            r.DrawText(m_texts[i], rc, btnFont, fg, i != 5,
                       DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                       true);
        }
    }

    // ── 交互 ──
    void OnMouseMove(int x, int y) override
    {
        const int btn = BtnAt(x, y);
        if (btn != m_hoverBtn) {
            m_hoverBtn = btn;
            UpdateTooltip(btn);
            Invalidate();
        }
    }
    void OnMouseLeave() override
    {
        if (m_hoverBtn != -1 || m_pressedBtn != -1) {
            m_hoverBtn = -1;
            m_pressedBtn = -1;
            UpdateTooltip(-1);
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
        const int btn = BtnAt(x, y);
        if (btn >= 0 && m_cmdCb) {
            m_cmdCb(static_cast<ToolbarCmd>(btn));
        }
    }

private:
    void UpdateTooltip(int btn)
    {
        const HWND owner = (Window() != nullptr) ? Window()->Hwnd() : nullptr;
        if (owner == nullptr) {
            return;
        }
        if (m_tooltip == nullptr && btn >= 0) {
            m_tooltip = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                TOOLTIPS_CLASSW, nullptr,
                WS_POPUP | TTS_NOPREFIX | TTS_ALWAYSTIP,
                CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
                owner, nullptr, g_hModule, nullptr);
            if (m_tooltip != nullptr) {
                TOOLINFOW ti{};
                ti.cbSize = sizeof(ti);
                ti.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
                ti.hwnd = owner;
                ti.uId = reinterpret_cast<UINT_PTR>(owner);
                ti.lpszText = const_cast<LPWSTR>(ButtonTooltip(btn));
                SendMessageW(m_tooltip, TTM_ADDTOOLW, 0, reinterpret_cast<LPARAM>(&ti));
                SendMessageW(m_tooltip, TTM_SETDELAYTIME, TTDT_INITIAL, 350);
            }
            return;
        }
        if (m_tooltip != nullptr) {
            TOOLINFOW ti{};
            ti.cbSize = sizeof(ti);
            ti.hwnd = owner;
            ti.uId = reinterpret_cast<UINT_PTR>(owner);
            ti.lpszText = const_cast<LPWSTR>(ButtonTooltip(btn));
            SendMessageW(m_tooltip, TTM_UPDATETIPTEXTW, 0, reinterpret_cast<LPARAM>(&ti));
            if (btn < 0) {
                SendMessageW(m_tooltip, TTM_POP, 0, 0);
            }
        }
    }

    /// x → 按钮索引（-1 未命中），布局与 Draw 一致
    int BtnAt(int x, int y) const
    {
        const float w = static_cast<float>(Width());
        const float h = static_cast<float>(Height());
        if (y < 4 || y > h - 4.0f || x < 6 || x > w - 6.0f) {
            return -1;
        }
        const float contentW = w - 12.0f;
        const float btnW = (contentW - kBtnGap * (kBtnCount - 1)) / kBtnCount;
        const float relX = static_cast<float>(x) - 6.0f;
        const int idx = static_cast<int>(relX / (btnW + kBtnGap));
        if (idx >= 0 && idx < kBtnCount &&
            relX - idx * (btnW + kBtnGap) < btnW) {
            return idx;
        }
        return -1;
    }

    std::wstring m_texts[kBtnCount] = { L"中", L"简", L"全拼", L"麦", L"设置", L"×" };
    bool m_active[kBtnCount] = { true, false, false, false, false, false };
    int m_hoverBtn = -1;
    int m_pressedBtn = -1;
    HWND m_tooltip = nullptr;
    std::function<void(ToolbarCmd)> m_cmdCb;
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
    // 前台判定：优先基于当前持有键盘焦点的线程（m_focusTids）；
    // 若尚未收到焦点事件（如初始激活阶段），兜底参考注册线程集合（m_tids）
    const bool fgTaishen = fgTid != 0 && (m_focusTids.empty() ? (m_tids.count(fgTid) > 0)
                                                              : (m_focusTids.count(fgTid) > 0));
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
    m_focusTids.erase(tid);
    m_tids.erase(tid);
    EvaluateForeground();
}

void CBannerWindow::OnServiceFocus(DWORD tid)
{
    m_focusTids.insert(tid);
    EvaluateForeground();
}

void CBannerWindow::OnServiceLostFocus(DWORD tid)
{
    m_focusTids.erase(tid);
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
        const bool next = engine_get_traditional() != 1;
        engine_set_traditional(next ? 1 : 0);
        // 0.1.68：写回 config.ini，设置页「简繁转换」开关与其他进程据此同步
        wchar_t dllPath[MAX_PATH] = {0};
        if (GetModuleFileNameW(g_hModule, dllPath, MAX_PATH) > 0) {
            std::wstring dllDir(dllPath);
            const size_t slash = dllDir.find_last_of(L"\\/");
            if (slash != std::wstring::npos) {
                dllDir = dllDir.substr(0, slash + 1);
                taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
                cfg.traditional_enabled = next;
                taishen::SaveConfig(dllDir, cfg);
            }
        }
        taishen::DebugLog("Toolbar: traditional -> " +
                          std::to_string(engine_get_traditional()) + " (config updated)");
        break;
    }
    case ToolbarCmd::Shuangpin: {
        const bool next = engine_get_shuangpin() != 1;
        engine_set_shuangpin(next ? 1 : 0);
        // 0.1.68：写回 config.ini——原来只改本进程引擎，设置页读到的还是旧值，
        // 且点设置页保存会把这个未落盘的状态冲回。与「隐」按钮同一处理方式。
        wchar_t dllPath[MAX_PATH] = {0};
        if (GetModuleFileNameW(g_hModule, dllPath, MAX_PATH) > 0) {
            std::wstring dllDir(dllPath);
            const size_t slash = dllDir.find_last_of(L"\\/");
            if (slash != std::wstring::npos) {
                dllDir = dllDir.substr(0, slash + 1);
                taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
                cfg.shuangpin_mode = next;
                taishen::SaveConfig(dllDir, cfg);
            }
        }
        taishen::DebugLog("Toolbar: shuangpin -> " +
                          std::to_string(engine_get_shuangpin()) + " (config updated)");
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
                // P2-21.5：非阻塞弹窗提示用户语音启动失败原因（不卡宿主 UI 线程）
                std::thread([msg = err]() {
                    MessageBoxW(nullptr, msg.c_str(), L"泰深输入法 - 语音输入",
                                MB_OK | MB_ICONINFORMATION | MB_TOPMOST);
                }).detach();
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
        // 0.1.64：点「隐」彻底隐藏工具栏并写回 config.ini，保证所有进程一致。
        // 0.1.67：托盘图标已停用，恢复入口改为语言栏「中/英」按钮右键菜单
        // 的「显示工具栏」。
        SetEnabled(false);
        wchar_t dllPath[MAX_PATH] = {0};
        if (GetModuleFileNameW(g_hModule, dllPath, MAX_PATH) > 0) {
            std::wstring dllDir(dllPath);
            const size_t slash = dllDir.find_last_of(L"\\/");
            if (slash != std::wstring::npos) {
                dllDir = dllDir.substr(0, slash + 1);
                taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
                cfg.toolbar_enabled = false;
                taishen::SaveConfig(dllDir, cfg);
            }
        }
        taishen::DebugLog("Toolbar: hidden via 隐 button (config toolbar=0)");
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
                         true, true, true)) {   // 末参：按 DPI 换算物理尺寸
        taishen::DebugLog("Toolbar: window create failed");
        return false;
    }
    m_window.SetTheme(m_lightTheme ? UIThemeLight() : UIThemeDark());
    m_window.SetRoot(m_panel);
    // UIWindow now owns the root control tree; retain only a non-owning pointer.
    m_panelOwner.release();
    ApplyToolbarRoundedRegion(m_window.Hwnd());
    taishen::DebugLog("Toolbar: initialized hwnd=" +
                      std::to_string(reinterpret_cast<long long>(m_window.Hwnd())));
    return true;
}

void CBannerWindow::PositionBottomRight()
{
    if (m_window.Hwnd() == nullptr) {
        return;
    }
    RECT workArea = {};
    if (SystemParametersInfoW(SPI_GETWORKAREA, 0, &workArea, 0)) {
        // 工作区是物理像素，工具栏尺寸是 DIP → 统一换算后再定位（125% 不至于贴错边）
        const float scale = m_window.DpiScale();
        const int w = static_cast<int>(static_cast<float>(kToolbarWidth) * scale + 0.5f);
        const int h = static_cast<int>(static_cast<float>(kToolbarHeight) * scale + 0.5f);
        const int margin = static_cast<int>(static_cast<float>(kMargin) * scale + 0.5f);
        const int x = workArea.right - w - margin;
        const int y = workArea.bottom - h - margin;
        // 显示由 EvaluateForeground 的 UIWindow::Show 控制（V0.3.5 审查修复）
        SetWindowPos(m_window.Hwnd(), HWND_TOPMOST, x, y, w, h,
                     SWP_NOACTIVATE);
        ApplyToolbarRoundedRegion(m_window.Hwnd());
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
