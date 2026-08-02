/// 右下角状态横幅 — 实现
///
/// 进程级全局单例：显示/隐藏由「前台窗口线程是否激活泰深」驱动。
/// SetWinEventHook(EVENT_SYSTEM_FOREGROUND) 监听前台切换，回调在
/// 注册线程（首个 ActivateEx 的 UI 线程，有消息循环）的消息队列执行。
///
/// 视觉参考 rime-ice/weasel.yaml（purity_of_form_custom）：
/// 深灰底 + 浅字 + 圆角 + 阴影，紧凑卡片式。

#include "banner_window.h"
#include "debug_log.h"

namespace taishen {

// 布局常量
static constexpr int kBannerWidth = 240;   // 横幅宽
static constexpr int kBannerHeight = 46;   // 横幅高
static constexpr int kCornerRadius = 8;    // 圆角半径
static constexpr int kMargin = 12;         // 距屏幕右下角边距
static constexpr int kShadowOffset = 3;    // 阴影偏移

// rime purity_of_form_custom 风格配色
static constexpr COLORREF kBgColor = RGB(84, 85, 84);        // 0x545554 深灰
static constexpr COLORREF kTextColor = RGB(238, 238, 238);   // 0xEEEEEE 浅字
static constexpr COLORREF kDimColor = RGB(128, 128, 128);    // 0x808080 次文字
static constexpr COLORREF kShadowColor = RGB(0, 0, 0);       // 阴影

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
    case WM_MOUSEACTIVATE:
        return MA_NOACTIVATE; // 不抢焦点
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}

// ── 单例 ──

CBannerWindow& CBannerWindow::Instance()
{
    // 静态局部单例（进程内唯一，跨 CTextService 实例共享）
    static CBannerWindow s_instance;
    return s_instance;
}

CBannerWindow::CBannerWindow()
    : m_hwnd(nullptr), m_initialized(false), m_visible(false), m_hook(nullptr) {}

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
    // 前台窗口切换 → 重新评估横幅显示（回调在注册线程消息队列，无需加锁）
    CBannerWindow::Instance().EvaluateForeground();
}

void CBannerWindow::EvaluateForeground()
{
    // 前台窗口所属线程是否激活了泰深
    const HWND fg = GetForegroundWindow();
    DWORD fgTid = 0;
    if (fg != nullptr) {
        fgTid = GetWindowThreadProcessId(fg, nullptr);
    }
    const bool active = (fgTid != 0) && (m_threads.tids.count(fgTid) > 0);
    if (active) {
        if (!m_visible) {
            Show(m_text);  // 前台回到泰深 → 显示（保留最近状态文字）
        }
    } else {
        if (m_visible) {
            Hide();  // 前台离开泰深（如切到游戏英文输入法）→ 立即隐藏
        }
    }
}

void CBannerWindow::RegisterThread(DWORD tid)
{
    m_threads.tids.insert(tid);
    // 首次注册时挂前台监听（每进程一次）
    if (m_hook == nullptr) {
        // OUTOFCONTEXT：回调投递到注册线程的消息队列（ActivateEx 在 UI 线程，有消息循环）
        m_hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND,
                                 nullptr, OnForegroundChanged, 0, 0,
                                 WINEVENT_OUTOFCONTEXT);
        taishen::DebugLog("BannerWindow: SetWinEventHook fg=" +
                          std::to_string(m_hook != nullptr));
    }
    EvaluateForeground();
}

void CBannerWindow::UnregisterThread(DWORD tid)
{
    m_threads.tids.erase(tid);
    EvaluateForeground();
}

void CBannerWindow::UpdateStatus(const std::wstring& text)
{
    m_text = text;
    if (m_visible) {
        InvalidateRect(m_hwnd, nullptr, FALSE);
    }
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
    wc.lpszClassName = kClassName;
    if (RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        taishen::DebugLog("BannerWindow: RegisterClassExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }
    m_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        kClassName, L"Taishen IME Status",
        WS_POPUP,
        0, 0, kBannerWidth, kBannerHeight,
        nullptr, nullptr, wc.hInstance, this);
    if (m_hwnd == nullptr) {
        taishen::DebugLog("BannerWindow: CreateWindowExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }
    m_initialized = true;
    taishen::DebugLog("BannerWindow: initialized hwnd=" +
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
        const int x = workArea.right - kBannerWidth - kMargin;
        const int y = workArea.bottom - kBannerHeight - kMargin;
        SetWindowPos(m_hwnd, HWND_TOPMOST, x, y,
                     kBannerWidth, kBannerHeight,
                     SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

void CBannerWindow::Show(const std::wstring& text)
{
    if (!EnsureWindow()) {
        return;
    }
    m_text = text;
    PositionBottomRight();
    m_visible = true;
    InvalidateRect(m_hwnd, nullptr, FALSE);
}

void CBannerWindow::Hide()
{
    if (m_hwnd != nullptr) {
        ShowWindow(m_hwnd, SW_HIDE);
    }
    m_visible = false;
}

// ── 绘制（GDI 双缓冲，rime 深色风格）──

void CBannerWindow::OnPaint(HDC hdc, const RECT& /*rcPaint*/)
{
    RECT rc = {};
    GetClientRect(m_hwnd, &rc);
    const int w = rc.right - rc.left;
    const int h = rc.bottom - rc.top;

    HDC memDC = CreateCompatibleDC(hdc);
    HBITMAP memBM = CreateCompatibleBitmap(hdc, w, h);
    HGDIOBJ oldBM = SelectObject(memDC, memBM);

    // 阴影：右下偏移的深色圆角
    BeginPath(memDC);
    RoundRect(memDC, kShadowOffset, kShadowOffset, w - 1, h - 1,
              kCornerRadius, kCornerRadius);
    EndPath(memDC);
    HBRUSH shadowBrush = CreateSolidBrush(kShadowColor);
    FillPath(memDC);
    DeleteObject(shadowBrush);

    // 主体：深灰圆角卡片
    BeginPath(memDC);
    RoundRect(memDC, 0, 0, w - 1 - kShadowOffset, h - 1 - kShadowOffset,
              kCornerRadius, kCornerRadius);
    EndPath(memDC);
    HBRUSH bgBrush = CreateSolidBrush(kBgColor);
    FillPath(memDC);
    DeleteObject(bgBrush);

    SetBkMode(memDC, TRANSPARENT);

    // 左侧小 logo：深色圆角方块 + "泰"字（比之前小，紧凑）
    const int logoSize = 26;
    RECT logoRc = {10, (h - logoSize) / 2, 10 + logoSize, (h - logoSize) / 2 + logoSize};
    BeginPath(memDC);
    RoundRect(memDC, logoRc.left, logoRc.top, logoRc.right, logoRc.bottom,
              6, 6);
    EndPath(memDC);
    HBRUSH logoBg = CreateSolidBrush(RGB(46, 46, 46));
    FillPath(memDC);
    DeleteObject(logoBg);

    HFONT logoFont = CreateFontW(16, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                                 DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                 CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                 DEFAULT_PITCH, L"Microsoft YaHei");
    HGDIOBJ oldFont = SelectObject(memDC, logoFont);
    SetTextColor(memDC, kTextColor);
    DrawTextW(memDC, L"泰", -1, &logoRc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    // 右侧：品牌 + 状态 一行式（"泰深 · 中文模式 · 双拼"）
    RECT textRc = {logoRc.right + 8, 0, w - 12, h};
    HFONT textFont = CreateFontW(13, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                                 DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                 CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                 DEFAULT_PITCH, L"Microsoft YaHei");
    SelectObject(memDC, textFont);
    // 品牌（亮）+ 状态（次亮）：分段绘制
    const std::wstring brand = L"泰深";
    RECT brandRc = textRc;
    DrawTextW(memDC, brand.c_str(), static_cast<int>(brand.size()),
              &brandRc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_CALCRECT);
    // 品牌亮色
    SetTextColor(memDC, kTextColor);
    DrawTextW(memDC, brand.c_str(), static_cast<int>(brand.size()),
              &brandRc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
    // 分隔符 + 状态（灰色）
    const int statusX = brandRc.right + 4;
    RECT statusRc = {statusX, 0, w - 12, h};
    SetTextColor(memDC, kDimColor);
    std::wstring statusText = m_text.empty() ? L"" : (L"· " + m_text);
    DrawTextW(memDC, statusText.c_str(), static_cast<int>(statusText.size()),
              &statusRc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    // 清理 + 拷贝
    SelectObject(memDC, oldFont);
    DeleteObject(logoFont);
    DeleteObject(textFont);
    BitBlt(hdc, 0, 0, w, h, memDC, 0, 0, SRCCOPY);
    SelectObject(memDC, oldBM);
    DeleteObject(memBM);
    DeleteDC(memDC);
}

} // namespace taishen
