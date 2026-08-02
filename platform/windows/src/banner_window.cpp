/// 右下角状态横幅 — 实现
///
/// GDI 双缓冲绘制：深色圆角背景 + 左侧"泰"字 Logo + 右侧品牌/状态两行文字。
/// 窗口：置顶、不抢焦点、不占任务栏（WS_EX_TOPMOST|TOOLWINDOW|NOACTIVATE）。

#include "banner_window.h"
#include "debug_log.h"

namespace taishen {

// 布局常量
static constexpr int kBannerWidth = 260;   // 横幅宽
static constexpr int kBannerHeight = 56;   // 横幅高
static constexpr int kLogoSize = 40;       // 左侧 Logo 方块尺寸
static constexpr int kMargin = 8;          // 窗口内边距 / 距屏幕右下角边距
static constexpr int kCornerRadius = 12;   // 圆角半径

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
        return 1; // 由 OnPaint 全量绘制，避免闪烁
    case WM_MOUSEACTIVATE:
        return MA_NOACTIVATE; // 不抢焦点（保持输入焦点在目标应用）
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}

// ── 构造/析构 ──

CBannerWindow::CBannerWindow()
    : m_hwnd(nullptr), m_initialized(false), m_visible(false) {}

CBannerWindow::~CBannerWindow()
{
    if (m_hwnd != nullptr) {
        DestroyWindow(m_hwnd);
        m_hwnd = nullptr;
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

// ── 对外接口 ──

void CBannerWindow::Show(const std::wstring& text)
{
    m_text = text;
    if (!EnsureWindow()) {
        return;
    }
    PositionBottomRight();
    m_visible = true;
    InvalidateRect(m_hwnd, nullptr, FALSE);
    taishen::DebugLog("BannerWindow: Show text=" + std::string(text.begin(), text.end()));
}

void CBannerWindow::UpdateStatus(const std::wstring& text)
{
    if (!m_visible || m_hwnd == nullptr) {
        m_text = text; // 未显示时仅记录，Show 时使用
        return;
    }
    m_text = text;
    InvalidateRect(m_hwnd, nullptr, FALSE);
    taishen::DebugLog("BannerWindow: UpdateStatus text=" + std::string(text.begin(), text.end()));
}

void CBannerWindow::Hide()
{
    if (m_hwnd != nullptr) {
        ShowWindow(m_hwnd, SW_HIDE);
    }
    m_visible = false;
    taishen::DebugLog("BannerWindow: Hide");
}

// ── 绘制（GDI 双缓冲）──

void CBannerWindow::OnPaint(HDC hdc, const RECT& /*rcPaint*/)
{
    RECT rc = {};
    GetClientRect(m_hwnd, &rc);
    const int w = rc.right - rc.left;
    const int h = rc.bottom - rc.top;

    // 双缓冲位图
    HDC memDC = CreateCompatibleDC(hdc);
    HBITMAP memBM = CreateCompatibleBitmap(hdc, w, h);
    HGDIOBJ oldBM = SelectObject(memDC, memBM);

    // 圆角深色背景（0x2E2E2E，与候选窗口主题一致）
    BeginPath(memDC);
    RoundRect(memDC, 0, 0, w - 1, h - 1, kCornerRadius, kCornerRadius);
    EndPath(memDC);
    HBRUSH bgBrush = CreateSolidBrush(RGB(46, 46, 46));
    FillPath(memDC);
    DeleteObject(bgBrush);

    // 左侧 Logo：深色方块 + 白色"泰"字
    RECT logoRc = {kMargin, kMargin, kMargin + kLogoSize, kMargin + kLogoSize};
    HBRUSH logoBg = CreateSolidBrush(RGB(38, 38, 38));
    FillRect(memDC, &logoRc, logoBg);
    DeleteObject(logoBg);

    SetBkMode(memDC, TRANSPARENT);
    HFONT logoFont = CreateFontW(24, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                                 DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                 CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                 DEFAULT_PITCH, L"Microsoft YaHei");
    HGDIOBJ oldFont = SelectObject(memDC, logoFont);
    SetTextColor(memDC, RGB(232, 232, 232));
    DrawTextW(memDC, L"泰", -1, &logoRc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    // 品牌行：泰深输入法（白色粗体）
    const int textX = kMargin + kLogoSize + 10;
    RECT brandRc = {textX, 8, w - kMargin, 26};
    HFONT brandFont = CreateFontW(14, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                                  DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                  CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                  DEFAULT_PITCH, L"Microsoft YaHei");
    SelectObject(memDC, brandFont);
    SetTextColor(memDC, RGB(232, 232, 232));
    DrawTextW(memDC, L"泰深输入法", -1, &brandRc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    // 状态行：状态文字（灰色）
    RECT statusRc = {textX, 30, w - kMargin, h - 6};
    HFONT statusFont = CreateFontW(11, 0, 0, 0, FW_NORMAL, FALSE, FALSE, FALSE,
                                   DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                   CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                   DEFAULT_PITCH, L"Microsoft YaHei");
    SelectObject(memDC, statusFont);
    SetTextColor(memDC, RGB(154, 154, 154));
    DrawTextW(memDC, m_text.c_str(), static_cast<int>(m_text.size()),
              &statusRc, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    // 清理字体
    SelectObject(memDC, oldFont);
    DeleteObject(logoFont);
    DeleteObject(brandFont);
    DeleteObject(statusFont);

    // 拷贝到窗口（必须在 DeleteDC 之前）
    BitBlt(hdc, 0, 0, w, h, memDC, 0, 0, SRCCOPY);

    SelectObject(memDC, oldBM);
    DeleteObject(memBM);
    DeleteDC(memDC);
}

} // namespace taishen
