/// Direct2D 候选窗口 — 实现
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md
/// 渲染管线：HwndRenderTarget → 圆角背景 → 拼音串 → 候选词（选中高亮）

#include "candidate_window.h"

#include <dwmapi.h>

namespace taishen {

// UTF-8 → 宽字符串（渲染用）
static std::wstring Utf8ToWide(const std::string& utf8)
{
    if (utf8.empty()) {
        return std::wstring();
    }
    const int len = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                        static_cast<int>(utf8.size()),
                                        nullptr, 0);
    if (len <= 0) {
        return std::wstring();
    }
    std::wstring result(static_cast<size_t>(len), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                        static_cast<int>(utf8.size()), &result[0], len);
    return result;
}

// ---------------------------------------------------------------------------
// 窗口过程
// ---------------------------------------------------------------------------
static LRESULT CALLBACK CandidateWndProc(HWND hwnd, UINT msg,
                                         WPARAM wParam, LPARAM lParam)
{
    CCandidateWindow* self = nullptr;
    if (msg == WM_NCCREATE) {
        const auto* cs = reinterpret_cast<CREATESTRUCT*>(lParam);
        self = static_cast<CCandidateWindow*>(cs->lpCreateParams);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA,
                          reinterpret_cast<LONG_PTR>(self));
    } else {
        self = reinterpret_cast<CCandidateWindow*>(
            GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    }

    switch (msg) {
    case WM_PAINT:
        if (self != nullptr) {
            PAINTSTRUCT ps;
            BeginPaint(hwnd, &ps);
            self->Render();
            EndPaint(hwnd, &ps);
        }
        return 0;
    case WM_ERASEBKGND:
        return 1; // 由 D2D 全量绘制，避免闪烁
    case WM_DESTROY:
        return 0;
    default:
        return DefWindowProcW(hwnd, msg, wParam, lParam);
    }
}

// ---------------------------------------------------------------------------
// 构造/析构
// ---------------------------------------------------------------------------
CCandidateWindow::CCandidateWindow()
    : m_hwnd(nullptr), m_initialized(false),
      m_pD2DFactory(nullptr), m_pRenderTarget(nullptr),
      m_pBgBrush(nullptr), m_pTextBrush(nullptr),
      m_pHighlightBrush(nullptr), m_pDWriteFactory(nullptr),
      m_pTextFormat(nullptr),
      m_selectedIndex(0), m_visible(false)
{
}

CCandidateWindow::~CCandidateWindow()
{
    ReleaseDeviceResources();
    if (m_hwnd != nullptr) {
        DestroyWindow(m_hwnd);
        m_hwnd = nullptr;
    }
}

// ---------------------------------------------------------------------------
// 初始化
// ---------------------------------------------------------------------------
bool CCandidateWindow::Initialize()
{
    if (m_initialized) {
        return true;
    }

    // 注册窗口类
    const wchar_t kClassName[] = L"TaishenCandidateWindow";
    WNDCLASSEXW wc = {};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = CandidateWndProc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, MAKEINTRESOURCEW(IDC_ARROW));
    wc.lpszClassName = kClassName;
    wc.hbrBackground = nullptr;
    if (RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        return false;
    }

    // 创建置顶透明无边框窗口
    // WS_EX_NOACTIVATE: 不抢焦点；WS_EX_TOOLWINDOW: 不占任务栏；
    // WS_EX_LAYERED: 透明混合；WS_EX_TOPMOST: 置顶
    m_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
        kClassName, L"Taishen IME Candidate",
        WS_POPUP,
        0, 0, 1, 1, // 初始 1x1，UpdateState 时定位
        nullptr, nullptr, wc.hInstance, this);
    if (m_hwnd == nullptr) {
        return false;
    }

    m_initialized = true;
    return true;
}

bool CCandidateWindow::CreateDeviceResources()
{
    if (m_pRenderTarget != nullptr) {
        return true;
    }

    HRESULT hr = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED,
                                   &m_pD2DFactory);
    if (FAILED(hr)) {
        return false;
    }

    hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,
                             __uuidof(IDWriteFactory),
                             reinterpret_cast<IUnknown**>(&m_pDWriteFactory));
    if (FAILED(hr)) {
        return false;
    }

    RECT rc = {};
    GetClientRect(m_hwnd, &rc);
    const D2D1_SIZE_U size = D2D1::SizeU(
        static_cast<UINT32>(rc.right - rc.left),
        static_cast<UINT32>(rc.bottom - rc.top));

    hr = m_pD2DFactory->CreateHwndRenderTarget(
        D2D1::RenderTargetProperties(
            D2D1_RENDER_TARGET_TYPE_DEFAULT,
            D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM,
                              D2D1_ALPHA_MODE_PREMULTIPLIED)),
        D2D1::HwndRenderTargetProperties(m_hwnd, size),
        &m_pRenderTarget);
    if (FAILED(hr)) {
        return false;
    }

    m_pRenderTarget->CreateSolidColorBrush(
        D2D1::ColorF(0x2E2E2E, 0.95f), &m_pBgBrush);      // 深色背景
    m_pRenderTarget->CreateSolidColorBrush(
        D2D1::ColorF(0xE8E8E8, 1.0f), &m_pTextBrush);     // 主文本
    m_pRenderTarget->CreateSolidColorBrush(
        D2D1::ColorF(0x1E6FFF, 0.6f), &m_pHighlightBrush); // 选中高亮

    hr = m_pDWriteFactory->CreateTextFormat(
        L"Microsoft YaHei", nullptr, DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
        kFontSize, L"zh-CN", &m_pTextFormat);
    if (FAILED(hr)) {
        return false;
    }
    m_pTextFormat->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    m_pTextFormat->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);

    return true;
}

void CCandidateWindow::ReleaseDeviceResources()
{
    if (m_pTextFormat != nullptr) { m_pTextFormat->Release(); m_pTextFormat = nullptr; }
    if (m_pHighlightBrush != nullptr) { m_pHighlightBrush->Release(); m_pHighlightBrush = nullptr; }
    if (m_pTextBrush != nullptr) { m_pTextBrush->Release(); m_pTextBrush = nullptr; }
    if (m_pBgBrush != nullptr) { m_pBgBrush->Release(); m_pBgBrush = nullptr; }
    if (m_pRenderTarget != nullptr) { m_pRenderTarget->Release(); m_pRenderTarget = nullptr; }
    if (m_pDWriteFactory != nullptr) { m_pDWriteFactory->Release(); m_pDWriteFactory = nullptr; }
    if (m_pD2DFactory != nullptr) { m_pD2DFactory->Release(); m_pD2DFactory = nullptr; }
}

// ---------------------------------------------------------------------------
// 尺寸计算
// ---------------------------------------------------------------------------
void CCandidateWindow::CalculateSize(int& width, int& height)
{
    // 基础尺寸：内边距
    width = kPadding * 2;
    height = kPadding * 2;

    // 拼音行（若非空）
    bool hasPinyin = !m_pinyin.empty();
    if (hasPinyin) {
        height += kPinyinHeight;
    }

    // 候选行
    if (!m_candidates.empty()) {
        height += kCandidateHeight;
    }

    // 计算内容宽度
    int contentWidth = 0;
    if (hasPinyin) {
        contentWidth += static_cast<int>(m_pinyin.size()) * 14;
    }

    for (size_t i = 0; i < m_candidates.size(); ++i) {
        const std::wstring word = Utf8ToWide(m_candidates[i]);
        // "序号.词" 的宽度
        int itemWidth = 0;
        if (i < 9) {
            itemWidth = 24; // "1." 占位
        } else {
            itemWidth = 30; // "10." 占位
        }
        itemWidth += static_cast<int>(word.size()) * 16;
        contentWidth += itemWidth;
        if (i + 1 < m_candidates.size()) {
            contentWidth += kItemGap;
        }
    }

    if (contentWidth > 0) {
        width += contentWidth;
    } else {
        width += 60; // 最小宽度
    }

    // 限制最大宽度
    if (width > 600) {
        width = 600;
    }
}

// ---------------------------------------------------------------------------
// 窗口定位
// ---------------------------------------------------------------------------
void CCandidateWindow::PositionWindow(const RECT& caretRect)
{
    int width = 0;
    int height = 0;
    CalculateSize(width, height);

    // 默认放在光标下方
    int x = caretRect.left;
    int y = caretRect.bottom + 4;

    // 屏幕超界修正
    const int screenW = GetSystemMetrics(SM_CXSCREEN);
    const int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (x + width > screenW) {
        x = screenW - width;
    }
    if (y + height > screenH) {
        // 下方放不下则放上方
        y = caretRect.top - height - 4;
        if (y < 0) {
            y = 0;
        }
    }
    if (x < 0) {
        x = 0;
    }

    SetWindowPos(m_hwnd, HWND_TOPMOST, x, y, width, height,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------
void CCandidateWindow::Render()
{
    if (m_pRenderTarget == nullptr) {
        return;
    }

    // 窗口尺寸可能被 SetWindowPos 改变，同步 render target 尺寸
    RECT rcClient = {};
    GetClientRect(m_hwnd, &rcClient);
    const D2D1_SIZE_U targetSize = D2D1::SizeU(
        static_cast<UINT32>(rcClient.right - rcClient.left),
        static_cast<UINT32>(rcClient.bottom - rcClient.top));
    if (targetSize.width > 0 && targetSize.height > 0) {
        m_pRenderTarget->Resize(targetSize);
    }

    m_pRenderTarget->BeginDraw();
    m_pRenderTarget->Clear(D2D1::ColorF(0, 0));

    const D2D1_SIZE_F size = m_pRenderTarget->GetSize();

    // 圆角背景
    const D2D1_ROUNDED_RECT bgRect = D2D1::RoundedRect(
        D2D1::RectF(0.5f, 0.5f, size.width - 0.5f, size.height - 0.5f),
        4.0f, 4.0f);
    m_pRenderTarget->FillRoundedRectangle(bgRect, m_pBgBrush);

    float y = static_cast<float>(kPadding);

    // 拼音串
    if (!m_pinyin.empty()) {
        const std::wstring pinyin = Utf8ToWide(m_pinyin);
        IDWriteTextLayout* pLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                pinyin.c_str(), static_cast<UINT32>(pinyin.size()),
                m_pTextFormat, size.width, kPinyinHeight, &pLayout))) {
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(static_cast<float>(kPadding), y),
                pLayout, m_pTextBrush);
            pLayout->Release();
        }
        y += kPinyinHeight;
    }

    // 候选词（水平排布）
    float x = static_cast<float>(kPadding);
    for (size_t i = 0; i < m_candidates.size(); ++i) {
        const std::wstring word = Utf8ToWide(m_candidates[i]);
        std::wstring item = std::to_wstring(i + 1) + L"." + word;

        // 选中高亮背景
        if (static_cast<int>(i) == m_selectedIndex) {
            const D2D1_ROUNDED_RECT highlight = D2D1::RoundedRect(
                D2D1::RectF(x - 2.0f, y, x + static_cast<float>(item.size() * 16 + 20),
                            y + kCandidateHeight),
                3.0f, 3.0f);
            m_pRenderTarget->FillRoundedRectangle(highlight, m_pHighlightBrush);
        }

        IDWriteTextLayout* pLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                item.c_str(), static_cast<UINT32>(item.size()),
                m_pTextFormat, 300.0f, kCandidateHeight, &pLayout))) {
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(x, y), pLayout, m_pTextBrush);
            pLayout->Release();
        }
        x += static_cast<float>(item.size() * 16 + 20 + kItemGap);
    }

    m_pRenderTarget->EndDraw();
}

// ---------------------------------------------------------------------------
// 对外接口
// ---------------------------------------------------------------------------
void CCandidateWindow::UpdateState(const std::string& pinyin,
                                   const std::vector<std::string>& candidates,
                                   const RECT& caretRect)
{
    m_pinyin = pinyin;
    m_candidates = candidates;

    // 拼音为空或候选为空 → 隐藏
    if (m_pinyin.empty() || m_candidates.empty()) {
        Hide();
        return;
    }

    if (!Initialize()) {
        return;
    }

    if (!CreateDeviceResources()) {
        return;
    }

    PositionWindow(caretRect);
    m_visible = true;

    // 触发重绘
    InvalidateRect(m_hwnd, nullptr, FALSE);
}

void CCandidateWindow::Hide()
{
    if (m_hwnd != nullptr) {
        ShowWindow(m_hwnd, SW_HIDE);
    }
    m_visible = false;
}

void CCandidateWindow::SetSelectedIndex(int index)
{
    if (index != m_selectedIndex) {
        m_selectedIndex = index;
        if (m_visible && m_hwnd != nullptr) {
            InvalidateRect(m_hwnd, nullptr, FALSE);
        }
    }
}

} // namespace taishen

