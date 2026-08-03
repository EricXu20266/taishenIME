/// 右下角输入法工具栏 — 实现
///
/// 按钮：中/英、简/繁、双拼、设置。点击直接调引擎 FFI 切换，
/// 状态从引擎实时读取并高亮。设置按钮用默认关联程序打开 config.ini。
///
/// 显示条件：托盘开关(enabled) && 前台窗口线程激活泰深。
/// SetWinEventHook(EVENT_SYSTEM_FOREGROUND) 监听前台切换，
/// 回调在注册线程（首个 ActivateEx 的 UI 线程，有消息循环）消息队列执行。

#include "banner_window.h"
#include "debug_log.h"
#include "engine_bridge.h"
#include "settings_dialog.h"
#include "theme.h"
#include "app_state.h"

#include <shellapi.h>
#include <windowsx.h> // GET_X_LPARAM

// DLL 模块句柄（dllmain.cpp 定义于全局命名空间，用于定位 config.ini）
extern HMODULE g_hModule;

namespace taishen {

// 布局常量
static constexpr int kToolbarWidth = 216;  // 工具栏宽
static constexpr int kToolbarHeight = 38;  // 工具栏高
static constexpr int kCornerRadius = 8;    // 圆角半径
static constexpr int kMargin = 12;         // 距屏幕右下角边距
static constexpr int kShadowOffset = 3;    // 阴影偏移
static constexpr int kBtnGap = 4;          // 按钮间距
static constexpr int kBtnPadX = 10;        // 按钮内边距

// 按钮数（Ascii/Trad/Shuangpin/Settings）
static constexpr int kBtnCount = 4;

// rime purity_of_form_custom 风格配色（深色）
static constexpr COLORREF kBgColor = RGB(84, 85, 84);        // 0x545554 深灰
static constexpr COLORREF kTextColor = RGB(238, 238, 238);   // 0xEEEEEE 浅字
static constexpr COLORREF kDimColor = RGB(128, 128, 128);    // 0x808080
static constexpr COLORREF kHiliteBg = RGB(227, 227, 227);    // 0xE3E3E3 激活按钮底
static constexpr COLORREF kHiliteText = RGB(76, 76, 76);     // 0x4C4C4C 激活按钮字
static constexpr COLORREF kShadowColor = RGB(0, 0, 0);       // 阴影

// 浅色主题配色（V0.2.20，与深色镜像）
static constexpr COLORREF kLightBg = RGB(245, 245, 245);     // 0xF5F5F5 白底
static constexpr COLORREF kLightText = RGB(26, 26, 26);      // 0x1A1A1A 黑字
static constexpr COLORREF kLightDim = RGB(138, 138, 138);    // 0x8A8A8A 灰
static constexpr COLORREF kLightHiliteBg = RGB(30, 111, 255); // 0x1E6FFF 蓝底
static constexpr COLORREF kLightHiliteText = RGB(255, 255, 255); // 白字

// 按主题模式取色（V0.2.20）
static COLORREF ThemeBg(bool light)   { return light ? kLightBg   : kBgColor; }
static COLORREF ThemeText(bool light) { return light ? kLightText  : kTextColor; }
static COLORREF ThemeDim(bool light)  { return light ? kLightDim   : kDimColor; }
static COLORREF ThemeHiliteBg(bool light)   { return light ? kLightHiliteBg   : kHiliteBg; }
static COLORREF ThemeHiliteText(bool light) { return light ? kLightHiliteText : kHiliteText; }

// ── 窗口过程 ──

static LRESULT CALLBACK BannerWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    CBannerWindow* self = nullptr;
    if (msg == WM_NCCREATE) {
        const auto* cs = reinterpret_cast<CREATESTRUCT*>(lParam);
        self = static_cast<CBannerWindow*>(cs->lpCreateParams);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(self));
    } else {
        self = reinterpret_cast<CBannerWindow*>(
            GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    }

    switch (msg) {
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC hdc = BeginPaint(hwnd, &ps);
        if (self != nullptr) {
            self->OnPaint(hdc, ps.rcPaint);
        }
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_ERASEBKGND:
        return 1; // 由 OnPaint 全量绘制
    case WM_SETTINGCHANGE:
        // 系统主题切换（V0.2.20）：重检并切换工具栏配色
        if (self != nullptr) {
            const int sysTheme = taishen::GetSystemAppTheme();
            if (sysTheme >= 0) {
                self->SetLightTheme(sysTheme == 1);
            }
        }
        return 0;
    case WM_MOUSEACTIVATE:
        return MA_NOACTIVATE; // 不抢焦点
    case WM_SETCURSOR:
        // 0.3.x 修复：窗口类 hCursor 缺失时系统给了错误光标（左右箭头）。
        // 按钮上显示小手（IDC_HAND），其余区域箭头（IDC_ARROW）。
        if (self != nullptr && LOWORD(lParam) == HTCLIENT) {
            POINT pt = {};
            GetCursorPos(&pt);
            ScreenToClient(hwnd, &pt);
            const HCURSOR cur = (self->HitTest(pt.x) >= 0)
                ? LoadCursorW(nullptr, MAKEINTRESOURCEW(IDC_HAND))
                : LoadCursorW(nullptr, MAKEINTRESOURCEW(IDC_ARROW));
            SetCursor(cur);
            return TRUE;
        }
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    case WM_LBUTTONDOWN:
        if (self != nullptr) {
            self->m_pressedBtn = self->HitTest(GET_X_LPARAM(lParam));
            SetCapture(hwnd);
        }
        return 0;
    case WM_LBUTTONUP:
        if (self != nullptr) {
            ReleaseCapture();
            const int btn = self->HitTest(GET_X_LPARAM(lParam));
            if (btn >= 0 && btn == self->m_pressedBtn && btn < kBtnCount) {
                self->HandleCommand(static_cast<ToolbarCmd>(btn));
            }
            self->m_pressedBtn = -1;
        }
        return 0;
    case WM_MOUSELEAVE:
        if (self != nullptr) {
            self->m_pressedBtn = -1;
        }
        return 0;
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}

// ── 单例 ──

CBannerWindow& CBannerWindow::Instance()
{
    static CBannerWindow s_instance;
    return s_instance;
}

CBannerWindow::CBannerWindow()
    : m_hwnd(nullptr), m_initialized(false), m_visible(false),
      m_enabled(true), m_pressedBtn(-1), m_lightTheme(false), m_hook(nullptr) {}

/// 设置主题模式（V0.2.20）：true=浅色，false=深色；重绘生效
void CBannerWindow::SetLightTheme(bool light)
{
    if (m_lightTheme != light) {
        m_lightTheme = light;
        if (m_hwnd != nullptr) {
            InvalidateRect(m_hwnd, nullptr, TRUE);
        }
    }
}

CBannerWindow::~CBannerWindow()
{
    if (m_hook != nullptr) {
        UnhookWinEvent(m_hook);
        m_hook = nullptr;
    }
    if (m_hwnd != nullptr) {
        DestroyWindow(m_hwnd);
        m_hwnd = nullptr;
    }
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
    const bool active = m_enabled && fgTid != 0 && (m_tids.count(fgTid) > 0);
    if (active) {
        if (!m_visible && EnsureWindow()) {
            PositionBottomRight();
            m_visible = true;
            InvalidateRect(m_hwnd, nullptr, FALSE);
        }
    } else if (m_visible) {
        if (m_hwnd != nullptr) {
            ShowWindow(m_hwnd, SW_HIDE);
        }
        m_visible = false;
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
    EvaluateForeground();
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
    case ToolbarCmd::Settings: {
        // 弹出设置窗口（SPEC: settings-ui，替代直接打开 config.ini）
        wchar_t dllPath[MAX_PATH] = {0};
        if (GetModuleFileNameW(g_hModule, dllPath, MAX_PATH) > 0) {
            std::wstring dllDir(dllPath);
            const size_t slash = dllDir.find_last_of(L"\\/");
            dllDir = dllDir.substr(0, slash + 1);
            taishen::ShowSettingsDialog(m_hwnd, dllDir);
        }
        break;
    }
    default:
        break;
    }
    // 切换后刷新按钮状态
    if (m_visible && m_hwnd != nullptr) {
        InvalidateRect(m_hwnd, nullptr, FALSE);
    }
}

// ── 命中检测 ──

int CBannerWindow::HitTest(int x) const
{
    if (x < 10) {
        return -1;
    }
    const int btnW = (kToolbarWidth - 20 - kBtnGap * (kBtnCount - 1)) / kBtnCount;
    const int btnX = 10 + x - 10; // x 相对内容区
    const int idx = (x - 10) / (btnW + kBtnGap);
    if (idx >= 0 && idx < kBtnCount && (x - 10) % (btnW + kBtnGap) < btnW) {
        return idx;
    }
    return -1;
}

// ── 窗口创建与定位 ──

bool CBannerWindow::EnsureWindow()
{
    if (m_initialized) {
        return true;
    }
    const wchar_t kClassName[] = L"TaishenBannerWindow";
    WNDCLASSEXW wc = {};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = BannerWndProc;
    wc.hInstance = GetModuleHandleW(nullptr);
    // 0.3.x 修复：hCursor 缺失导致鼠标悬停显示错误光标（左右箭头），
    // 与候选窗口对齐设置 IDC_ARROW（按钮 hover 由 WM_SETCURSOR 换 IDC_HAND）
    wc.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(IDC_ARROW));
    wc.lpszClassName = kClassName;
    if (RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        taishen::DebugLog("Toolbar: RegisterClassExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }
    m_hwnd = CreateWindowExW(
        // 0.3.x：补 WS_EX_NOACTIVATE（与候选窗口一致）——无该样式时
        // 窗口可能被点击激活抢焦点，导致前台线程判定错乱、工具栏闪烁/消失
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        kClassName, L"Taishen IME Toolbar",
        WS_POPUP,
        0, 0, kToolbarWidth, kToolbarHeight,
        nullptr, nullptr, wc.hInstance, this);
    if (m_hwnd == nullptr) {
        taishen::DebugLog("Toolbar: CreateWindowExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }
    m_initialized = true;
    taishen::DebugLog("Toolbar: initialized hwnd=" +
                      std::to_string(reinterpret_cast<long long>(m_hwnd)));
    return true;
}

void CBannerWindow::PositionBottomRight()
{
    if (m_hwnd == nullptr) {
        return;
    }
    RECT workArea = {};
    if (SystemParametersInfoW(SPI_GETWORKAREA, 0, &workArea, 0)) {
        const int x = workArea.right - kToolbarWidth - kMargin;
        const int y = workArea.bottom - kToolbarHeight - kMargin;
        SetWindowPos(m_hwnd, HWND_TOPMOST, x, y,
                     kToolbarWidth, kToolbarHeight,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

// ── 绘制 ──

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

void CBannerWindow::OnPaint(HDC hdc, const RECT& /*rcPaint*/)
{
    RECT rc = {};
    GetClientRect(m_hwnd, &rc);
    const int w = rc.right - rc.left;
    const int h = rc.bottom - rc.top;

    HDC memDC = CreateCompatibleDC(hdc);
    HBITMAP memBM = CreateCompatibleBitmap(hdc, w, h);
    HGDIOBJ oldBM = SelectObject(memDC, memBM);

    // 阴影
    BeginPath(memDC);
    RoundRect(memDC, kShadowOffset, kShadowOffset, w - 1, h - 1,
              kCornerRadius, kCornerRadius);
    EndPath(memDC);
    HBRUSH shadowBrush = CreateSolidBrush(kShadowColor);
    FillPath(memDC);
    DeleteObject(shadowBrush);

    // 主体：深灰圆角卡片（V0.2.20 浅色主题用浅色）
    BeginPath(memDC);
    RoundRect(memDC, 0, 0, w - 1 - kShadowOffset, h - 1 - kShadowOffset,
              kCornerRadius, kCornerRadius);
    EndPath(memDC);
    HBRUSH bgBrush = CreateSolidBrush(ThemeBg(m_lightTheme));
    FillPath(memDC);
    DeleteObject(bgBrush);

    SetBkMode(memDC, TRANSPARENT);

    // 按钮布局
    const int contentX = 8;
    const int contentW = w - 16 - kShadowOffset;
    const int btnW = (contentW - kBtnGap * (kBtnCount - 1)) / kBtnCount;
    const int btnTop = 5;
    const int btnH = h - 10 - kShadowOffset;

    // 按钮文字（实时读引擎状态）
    const bool ascii = (engine_get_ascii_mode() == 1);
    const bool trad = (engine_get_traditional() == 1);
    const bool sp = (engine_get_shuangpin() == 1);
    const wchar_t* texts[kBtnCount] = {
        ascii ? L"英" : L"中",
        trad ? L"繁" : L"简",
        sp ? L"双拼" : L"全拼",
        L"设置",
    };
    // 激活状态：中英=当前模式高亮；简繁/双拼=开关高亮；设置=恒不高亮
    const bool active[kBtnCount] = {
        !ascii,       // 中文模式时"中"高亮
        trad,         // 简繁开启时高亮
        sp,           // 双拼开启时高亮
        false,
    };

    HFONT font = CreateFontW(13, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                             CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                             DEFAULT_PITCH, L"Microsoft YaHei");
    HGDIOBJ oldFont = SelectObject(memDC, font);

    for (int i = 0; i < kBtnCount; ++i) {
        const int bx = contentX + i * (btnW + kBtnGap);
        const bool pressed = (i == m_pressedBtn);
        RECT btnRc = {bx, btnTop, bx + btnW, btnTop + btnH};

        // 按钮底：激活态亮色 / 普通深灰 / 按下微暗（V0.2.20 浅色主题适配）
        COLORREF btnBg;
        if (active[i]) {
            btnBg = ThemeHiliteBg(m_lightTheme);
        } else if (pressed) {
            btnBg = m_lightTheme ? RGB(220, 220, 220) : RGB(70, 70, 70);
        } else {
            btnBg = m_lightTheme ? RGB(225, 225, 225) : RGB(58, 58, 58);
        }
        HBRUSH btnBrush = CreateSolidBrush(btnBg);
        FillRect(memDC, &btnRc, btnBrush);
        DeleteObject(btnBrush);

        // 按钮文字：激活态深字 / 普通浅字（V0.2.20 浅色主题适配）
        SetTextColor(memDC, active[i] ? ThemeHiliteText(m_lightTheme)
                                      : ThemeText(m_lightTheme));
        DrawTextW(memDC, texts[i], -1, &btnRc,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }

    SelectObject(memDC, oldFont);
    DeleteObject(font);

    BitBlt(hdc, 0, 0, w, h, memDC, 0, 0, SRCCOPY);
    SelectObject(memDC, oldBM);
    DeleteObject(memBM);
    DeleteDC(memDC);
}

} // namespace taishen
