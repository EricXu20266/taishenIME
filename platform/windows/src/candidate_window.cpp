/// Direct2D 候选窗口 — 实现
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md
/// 渲染管线：HwndRenderTarget → 圆角背景 → 拼音串 → 候选词（选中高亮）

#include "candidate_window.h"
#include "debug_log.h"
#include "theme.h"

#include <dwmapi.h>
#include <windowsx.h> // GET_X_LPARAM（鼠标坐标解包）

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

/// P0-1：按 label_format 生成候选标签文本（%d / %s 替换为序号数字）。
/// 如 "%d." → "1."；无占位符时若为连续序号字符（如 ①②③…）按索引取第 index 个。
static std::wstring FormatLabel(const std::wstring& fmt, int index)
{
    if (fmt.find(L"%d") != std::wstring::npos ||
        fmt.find(L"%s") != std::wstring::npos) {
        const std::wstring num = std::to_wstring(index + 1);
        const wchar_t token = fmt.find(L"%d") != std::wstring::npos ? L'd' : L's';
        std::wstring out = fmt;
        const std::wstring pat = std::wstring(L"%") + token;
        size_t pos = 0;
        while ((pos = out.find(pat, pos)) != std::wstring::npos) {
            out.replace(pos, 2, num);
            pos += num.size();
        }
        return out;
    }
    // 无占位符：若为连续序号字符（如 ①②③…）则按索引取第 index 个
    if (static_cast<size_t>(index) < fmt.size()) {
        return fmt.substr(index, 1);
    }
    return fmt;
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
    case WM_LBUTTONUP:
        // 鼠标点击选词：命中检测 → 回调（0.1.13 新增）
        if (self != nullptr) {
            const int x = GET_X_LPARAM(lParam);
            const int y = GET_Y_LPARAM(lParam);
            const int index = self->HitTest(x, y);
            if (index >= 0 && self->m_clickCb) {
                self->m_clickCb(index);
            }
        }
        return 0;
    case WM_MOUSEWHEEL:
        // 滚轮翻页（P0-1）：delta>0 上翻 / <0 下翻
        if (self != nullptr && self->m_pageCb) {
            const int delta = GET_WHEEL_DELTA_WPARAM(wParam);
            self->m_pageCb(delta > 0 ? -1 : 1);
        }
        return 0;
    case WM_MOUSEMOVE:
        // 悬停高亮追踪（P0-1）：命中候选 → 高亮，移出 → 清除
        if (self != nullptr) {
            const int x = GET_X_LPARAM(lParam);
            const int y = GET_Y_LPARAM(lParam);
            const int hover = self->HitTest(x, y);
            if (hover != self->m_hoverIndex) {
                self->m_hoverIndex = hover;
                InvalidateRect(hwnd, nullptr, FALSE);
            }
            // 请求 MOUSELEAVE 通知（移出窗口时清除悬停）
            TRACKMOUSEEVENT tme = {};
            tme.cbSize = sizeof(tme);
            tme.dwFlags = TME_LEAVE;
            tme.hwndTrack = hwnd;
            TrackMouseEvent(&tme);
        }
        return 0;
    case WM_MOUSELEAVE:
        if (self != nullptr) {
            self->m_hoverIndex = -1;
            InvalidateRect(hwnd, nullptr, FALSE);
        }
        return 0;
    case WM_MOUSEACTIVATE:
        // 不激活窗口（保持输入焦点在目标应用）
        return MA_NOACTIVATE;
    case WM_SETTINGCHANGE:
        // 系统主题切换（V0.2.20）：应用模式/颜色变化时重检
        if (self != nullptr) {
            self->OnSystemThemeChanged();
        }
        return 0;
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
      m_pHighlightBrush(nullptr), m_pDimBrush(nullptr),
      m_pLabelBrush(nullptr), m_pCommentBrush(nullptr),
      m_pBorderBrush(nullptr), m_pHiliteTextBrush(nullptr),
      m_pHiliteLabelBrush(nullptr), m_pMarkBrush(nullptr),
      m_pDWriteFactory(nullptr), m_pTextFormat(nullptr),
      m_selectedIndex(0), m_visible(false),
      m_page(0), m_totalPages(0), m_dpiScale(1.0f),
      m_theme(CandidateTheme::Default()), m_multiRow(false)
{
}

void CCandidateWindow::SetMultiRow(bool enabled)
{
    if (m_multiRow != enabled) {
        m_multiRow = enabled;
        // 重算尺寸 + 重绘
        if (m_visible && m_hwnd != nullptr) {
            RECT caret = {};
            GetWindowRect(m_hwnd, &caret);
            PositionWindow(caret);
            if (m_pRenderTarget != nullptr) {
                Render();
            }
        }
    }
}

void CCandidateWindow::SetTheme(const CandidateTheme& theme)
{
    m_theme = theme;
    m_followSystemTheme = false; // 显式设置主题 → 不再跟随系统（V0.2.20）
    // 已创建画刷则按新主题重建（未创建则下次 CreateDeviceResources 生效）
    if (m_pRenderTarget != nullptr) {
        // 释放旧画刷（避免泄漏），重建新主题色
        if (m_pDimBrush) { m_pDimBrush->Release(); m_pDimBrush = nullptr; }
        if (m_pHighlightBrush) { m_pHighlightBrush->Release(); m_pHighlightBrush = nullptr; }
        if (m_pTextBrush) { m_pTextBrush->Release(); m_pTextBrush = nullptr; }
        if (m_pBgBrush) { m_pBgBrush->Release(); m_pBgBrush = nullptr; }
        if (m_pLabelBrush) { m_pLabelBrush->Release(); m_pLabelBrush = nullptr; }
        if (m_pCommentBrush) { m_pCommentBrush->Release(); m_pCommentBrush = nullptr; }
        if (m_pBorderBrush) { m_pBorderBrush->Release(); m_pBorderBrush = nullptr; }
        if (m_pHiliteTextBrush) { m_pHiliteTextBrush->Release(); m_pHiliteTextBrush = nullptr; }
        if (m_pHiliteLabelBrush) { m_pHiliteLabelBrush->Release(); m_pHiliteLabelBrush = nullptr; }
        if (m_pMarkBrush) { m_pMarkBrush->Release(); m_pMarkBrush = nullptr; }
        CreateDeviceResources();
        if (m_visible) {
            Render();
        }
    }
}

/// 跟随系统主题设置（V0.2.20）：传入系统当前主题，标记跟随模式
void CCandidateWindow::SetFollowSystemTheme(bool follow)
{
    m_followSystemTheme = follow;
}

/// 系统主题变化（V0.2.20）：跟随模式下重新检测并应用默认主题
void CCandidateWindow::OnSystemThemeChanged()
{
    if (!m_followSystemTheme) {
        return; // 用户显式配置主题 → 不跟随
    }
    ImeConfig cfg;
    cfg.theme = m_theme;
    cfg.userThemeExplicit = !m_followSystemTheme;
    CandidateTheme t;
    if (ApplyThemeWithSystem(t, cfg)) {
        m_theme = t;
        if (m_pRenderTarget != nullptr) {
            if (m_pDimBrush) { m_pDimBrush->Release(); m_pDimBrush = nullptr; }
            if (m_pHighlightBrush) { m_pHighlightBrush->Release(); m_pHighlightBrush = nullptr; }
            if (m_pTextBrush) { m_pTextBrush->Release(); m_pTextBrush = nullptr; }
            if (m_pBgBrush) { m_pBgBrush->Release(); m_pBgBrush = nullptr; }
            if (m_pLabelBrush) { m_pLabelBrush->Release(); m_pLabelBrush = nullptr; }
            if (m_pCommentBrush) { m_pCommentBrush->Release(); m_pCommentBrush = nullptr; }
            if (m_pBorderBrush) { m_pBorderBrush->Release(); m_pBorderBrush = nullptr; }
            if (m_pHiliteTextBrush) { m_pHiliteTextBrush->Release(); m_pHiliteTextBrush = nullptr; }
            if (m_pHiliteLabelBrush) { m_pHiliteLabelBrush->Release(); m_pHiliteLabelBrush = nullptr; }
            if (m_pMarkBrush) { m_pMarkBrush->Release(); m_pMarkBrush = nullptr; }
            CreateDeviceResources();
            if (m_visible) {
                Render();
            }
        }
    }
}

/// 设置候选窗字体与字号（V0.2.21）：空字体/非法字号回退默认，重建 TextFormat
void CCandidateWindow::SetFont(const std::wstring& face, float size)
{
    if (!face.empty()) {
        m_fontFace = face;
    }
    if (size >= 12.0f && size <= 32.0f) {
        m_fontSize = size;
    }
    // 重建 TextFormat（若资源已创建）
    if (m_pTextFormat != nullptr) {
        m_pTextFormat->Release();
        m_pTextFormat = nullptr;
    }
    if (m_pRenderTarget != nullptr) {
        CreateDeviceResources();
        if (m_visible) {
            Render();
        }
    }
}

/// 设置行内预编辑（V0.2.18）：true=拼音在组合（候选窗不画拼音行）；重排窗口
void CCandidateWindow::SetInlinePreedit(bool enable)
{
    if (m_inlinePreedit != enable) {
        m_inlinePreedit = enable;
        if (m_visible && m_hwnd != nullptr) {
            RECT caret = {};
            GetWindowRect(m_hwnd, &caret);
            PositionWindow(caret);
            if (m_pRenderTarget != nullptr) {
                Render();
            }
        }
    }
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
        taishen::DebugLog("CandidateWindow: RegisterClassExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }

    // 创建置顶无边框窗口
    // WS_EX_NOACTIVATE: 不抢焦点；WS_EX_TOOLWINDOW: 不占任务栏；
    // WS_EX_TOPMOST: 置顶
    // 注意：不用 WS_EX_LAYERED——D2D HwndRenderTarget 与 layered 窗口
    // 不兼容，内容不显示（0.1.15 定位到：Initialize OK 但用户看不到窗口）。
    // 非 layered + D2D 渲染不透明背景，是 IME 候选窗口的标准做法。
    m_hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        kClassName, L"Taishen IME Candidate",
        WS_POPUP,
        0, 0, 1, 1, // 初始 1x1，UpdateState 时定位
        nullptr, nullptr, wc.hInstance, this);
    if (m_hwnd == nullptr) {
        taishen::DebugLog("CandidateWindow: CreateWindowExW failed err=" +
                          std::to_string(GetLastError()));
        return false;
    }

    taishen::DebugLog("CandidateWindow: Initialize OK hwnd=" +
                      std::to_string(reinterpret_cast<long long>(m_hwnd)));
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
        m_theme.bg, &m_pBgBrush);      // 背景（主题色，V0.2.4）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.text, &m_pTextBrush);  // 主文本
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.highlight_bg, &m_pHighlightBrush); // 选中高亮背景
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.dim, &m_pDimBrush);    // 页码/次要文字（0.1.13）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.label, &m_pLabelBrush);       // 序号标签（P0-1）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.comment, &m_pCommentBrush);   // 注释（P0-1）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.border, &m_pBorderBrush);     // 边框（P0-1）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.highlight_text, &m_pHiliteTextBrush);   // 选中候选文字（P0-1）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.highlight_label, &m_pHiliteLabelBrush); // 选中候选序号（P0-1）
    m_pRenderTarget->CreateSolidColorBrush(
        m_theme.mark, &m_pMarkBrush);         // 悬停/标记（P0-1）

    // 高 DPI 适配（0.1.13）：字号按 DPI 缩放，避免高分屏上文字过小
    m_dpiScale = 1.0f;
    if (m_hwnd != nullptr) {
        const UINT dpi = GetDpiForWindow(m_hwnd);
        if (dpi > 0) {
            m_dpiScale = static_cast<float>(dpi) / 96.0f;
        }
    }
    const float scaledFontSize = m_fontSize * m_dpiScale;
    hr = m_pDWriteFactory->CreateTextFormat(
        m_fontFace.c_str(), nullptr, DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
        scaledFontSize, L"zh-CN", &m_pTextFormat);
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
    if (m_pDimBrush != nullptr) { m_pDimBrush->Release(); m_pDimBrush = nullptr; }
    if (m_pHighlightBrush != nullptr) { m_pHighlightBrush->Release(); m_pHighlightBrush = nullptr; }
    if (m_pTextBrush != nullptr) { m_pTextBrush->Release(); m_pTextBrush = nullptr; }
    if (m_pBgBrush != nullptr) { m_pBgBrush->Release(); m_pBgBrush = nullptr; }
    if (m_pLabelBrush != nullptr) { m_pLabelBrush->Release(); m_pLabelBrush = nullptr; }
    if (m_pCommentBrush != nullptr) { m_pCommentBrush->Release(); m_pCommentBrush = nullptr; }
    if (m_pBorderBrush != nullptr) { m_pBorderBrush->Release(); m_pBorderBrush = nullptr; }
    if (m_pHiliteTextBrush != nullptr) { m_pHiliteTextBrush->Release(); m_pHiliteTextBrush = nullptr; }
    if (m_pHiliteLabelBrush != nullptr) { m_pHiliteLabelBrush->Release(); m_pHiliteLabelBrush = nullptr; }
    if (m_pMarkBrush != nullptr) { m_pMarkBrush->Release(); m_pMarkBrush = nullptr; }
    if (m_pRenderTarget != nullptr) { m_pRenderTarget->Release(); m_pRenderTarget = nullptr; }
    if (m_pDWriteFactory != nullptr) { m_pDWriteFactory->Release(); m_pDWriteFactory = nullptr; }
    if (m_pD2DFactory != nullptr) { m_pD2DFactory->Release(); m_pD2DFactory = nullptr; }
}

// ---------------------------------------------------------------------------
// 尺寸计算
// ---------------------------------------------------------------------------
void CCandidateWindow::CalculateSize(int& width, int& height)
{
    const float scale = m_dpiScale;
    const int pad = static_cast<int>(m_padding * scale);
    // V0.2.21：行高随字号缩放（font_size 与默认 16 的比例）
    const float fontScale = m_fontSize / 16.0f;
    const int pinyinH = static_cast<int>(18 * fontScale * scale);
    const int candH = static_cast<int>(22 * fontScale * scale);

    // 基础尺寸：内边距
    width = pad * 2;
    height = pad * 2;

    // 拼音行（若非空且非行内预编辑——V0.2.18：行内预编辑时拼音在组合，候选窗不重复）
    bool hasPinyin = !m_pinyin.empty() && !m_inlinePreedit;
    if (hasPinyin) {
        height += pinyinH;
    }

    // 候选行（V0.2.14：多行模式按行数累加）
    if (!m_candidates.empty()) {
        if (m_multiRow && m_candidates.size() > static_cast<size_t>(kPerRow)) {
            const size_t rows = (m_candidates.size() + kPerRow - 1) / kPerRow;
            height += static_cast<int>(rows * candH);
        } else {
            height += candH;
        }
    }

    // 计算内容宽度（字符宽度按 DPI 缩放）
    int contentWidth = 0;
    if (hasPinyin) {
        contentWidth += static_cast<int>(m_pinyin.size() * 14 * scale);
    }

    // 多行模式：宽度 = 前 kPerRow 个候选横排宽（行宽一致）
    const size_t widthCount = m_multiRow
        ? (m_candidates.size() < static_cast<size_t>(kPerRow)
               ? m_candidates.size() : static_cast<size_t>(kPerRow))
        : m_candidates.size();
    for (size_t i = 0; i < widthCount; ++i) {
        const std::wstring word = Utf8ToWide(m_candidates[i]);
        // P0-1：标签宽度按 label_format 实际长度计算
        const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
        const int itemWidth = static_cast<int>(
            (label.size() * 16 + word.size() * 16) * scale);
        contentWidth += itemWidth;
        if (i + 1 < m_candidates.size()) {
            contentWidth += static_cast<int>(m_spacing * scale);
        }
    }

    if (contentWidth > 0) {
        width += contentWidth;
    } else {
        width += static_cast<int>(60 * scale); // 最小宽度
    }

    // 翻页指示 "1/3"（多页时显示，0.1.13；多行模式不放页码）
    if (m_totalPages > 1 && !m_multiRow) {
        const int pageW = static_cast<int>((22 + (m_totalPages >= 10 ? 8 : 0)) * scale);
        width += pageW;
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
    // 非 layered 窗口不透明合成——Clear 用与背景同色深色
    // （0.1.15：去掉 WS_EX_LAYERED 后，透明 Clear 会显示黑色）
    m_pRenderTarget->Clear(D2D1::ColorF(0x2E2E2E, 1.0f));

    const D2D1_SIZE_F size = m_pRenderTarget->GetSize();

    // 圆角背景 + 边框（P0-1：圆角/边框色可配）
    const D2D1_ROUNDED_RECT bgRect = D2D1::RoundedRect(
        D2D1::RectF(0.5f, 0.5f, size.width - 0.5f, size.height - 0.5f),
        m_cornerRadius, m_cornerRadius);
    m_pRenderTarget->FillRoundedRectangle(bgRect, m_pBgBrush);
    m_pRenderTarget->DrawRoundedRectangle(bgRect, m_pBorderBrush, 1.0f);

    const float scale = m_dpiScale;
    const float fontScale = m_fontSize / 16.0f;
    const float padF = static_cast<float>(m_padding) * scale;
    const float pinyinH = 18.0f * fontScale * scale;
    const float candH = 22.0f * fontScale * scale;
    float y = padF;

    // 拼音串（V0.2.18：行内预编辑时不绘制——拼音在组合光标处，候选窗不重复）
    if (!m_pinyin.empty() && !m_inlinePreedit) {
        const std::wstring pinyin = Utf8ToWide(m_pinyin);
        IDWriteTextLayout* pLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                pinyin.c_str(), static_cast<UINT32>(pinyin.size()),
                m_pTextFormat, size.width, pinyinH, &pLayout))) {
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(padF, y),
                pLayout, m_pTextBrush);
            pLayout->Release();
        }
        y += pinyinH;
    }

    // 候选词（V0.2.14：多行网格 / 单行水平排布）
    float x = padF;
    // y 已包含拼音行高度（上面 y += pinyinH）
    for (size_t i = 0; i < m_candidates.size(); ++i) {
        // 多行模式：行列定位（每行 kPerRow 个）
        if (m_multiRow) {
            const int row = static_cast<int>(i) / kPerRow;
            const int col = static_cast<int>(i) % kPerRow;
            // 每列宽度 = 该列最宽候选（简化：按本候选宽度 + 固定列间距）
            x = padF + static_cast<float>(col) * 96.0f * scale;
            const bool hasPinyinRow = !m_pinyin.empty() && !m_inlinePreedit;
            y = padF + static_cast<float>(hasPinyinRow ? pinyinH : 0.0f) +
                static_cast<float>(row) * candH;
        }
        const std::wstring word = Utf8ToWide(m_candidates[i]);
        // P0-1：标签按 label_format 生成（如 "1." / "①"）
        const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
        const bool selected = (static_cast<int>(i) == m_selectedIndex);

        // 选中高亮背景（P0-1：圆角/宽度按标签+正文实际估算）
        if (selected) {
            const D2D1_ROUNDED_RECT highlight = D2D1::RoundedRect(
                D2D1::RectF(x - 2.0f, y,
                            x + static_cast<float>((label.size() + word.size()) * 16 + 20) * scale,
                            y + candH),
                m_hiliteRadius, m_hiliteRadius);
            m_pRenderTarget->FillRoundedRectangle(highlight, m_pHighlightBrush);
        } else if (static_cast<int>(i) == m_hoverIndex && m_hoverIndex >= 0) {
            // 悬停高亮（P0-1）：mark 色半透明背景
            const D2D1_ROUNDED_RECT hover = D2D1::RoundedRect(
                D2D1::RectF(x - 2.0f, y,
                            x + static_cast<float>((label.size() + word.size()) * 16 + 20) * scale,
                            y + candH),
                m_hiliteRadius, m_hiliteRadius);
            m_pRenderTarget->FillRoundedRectangle(hover, m_pMarkBrush);
        }

        // 序号（label 色/选中色） + 正文（正文色/选中色）分画笔（P0-1）
        IDWriteTextLayout* pLabelLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                label.c_str(), static_cast<UINT32>(label.size()),
                m_pTextFormat, 40.0f, candH, &pLabelLayout))) {
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(x, y), pLabelLayout,
                selected ? m_pHiliteLabelBrush : m_pLabelBrush);
            pLabelLayout->Release();
        }
        IDWriteTextLayout* pWordLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                word.c_str(), static_cast<UINT32>(word.size()),
                m_pTextFormat, 300.0f, candH, &pWordLayout))) {
            const float wordX = x + static_cast<float>(label.size() * 16) * scale;
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(wordX, y), pWordLayout,
                selected ? m_pHiliteTextBrush : m_pTextBrush);
            pWordLayout->Release();
        }
        // 单行模式：x 累进（标签+正文+高亮余量+间距）
        if (!m_multiRow) {
            x += static_cast<float>((label.size() + word.size()) * 16 + 20 + m_spacing) * scale;
        }
    }

    // 翻页指示 "1/3"（多页时显示，右侧灰字，0.1.13）
    if (m_totalPages > 1 && m_page >= 0) {
        const std::wstring pageStr =
            std::to_wstring(m_page + 1) + L"/" + std::to_wstring(m_totalPages);
        IDWriteTextLayout* pPageLayout = nullptr;
        if (SUCCEEDED(m_pDWriteFactory->CreateTextLayout(
                pageStr.c_str(), static_cast<UINT32>(pageStr.size()),
                m_pTextFormat, 60.0f, candH, &pPageLayout))) {
            const float pageX = size.width - padF -
                                static_cast<float>(pageStr.size() * 12) * scale;
            m_pRenderTarget->DrawTextLayout(
                D2D1::Point2F(pageX, y), pPageLayout, m_pDimBrush);
            pPageLayout->Release();
        }
    }

    m_pRenderTarget->EndDraw();
}

// ---------------------------------------------------------------------------
// 对外接口
// ---------------------------------------------------------------------------
void CCandidateWindow::UpdateState(const std::string& pinyin,
                                   const std::vector<std::string>& candidates,
                                   const RECT& caretRect,
                                   int page,
                                   int totalPages)
{
    m_pinyin = pinyin;
    m_candidates = candidates;
    m_page = page;
    m_totalPages = totalPages;

    // 拼音为空或候选为空 → 隐藏
    if (m_pinyin.empty() || m_candidates.empty()) {
        taishen::DebugLog("CandidateWindow: UpdateState HIDE (pinyin=" +
                          std::to_string(m_pinyin.size()) + " cands=" +
                          std::to_string(m_candidates.size()) + ")");
        Hide();
        return;
    }

    if (!Initialize()) {
        taishen::DebugLog("CandidateWindow: UpdateState Initialize FAILED");
        return;
    }

    if (!CreateDeviceResources()) {
        taishen::DebugLog("CandidateWindow: UpdateState CreateDeviceResources FAILED");
        return;
    }

    PositionWindow(caretRect);
    m_visible = true;

    // 主动渲染一次（0.1.15 修复）：不依赖 WM_PAINT 消息循环，
    // 确保 layered 窗口内容立即画出
    Render();
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

void CCandidateWindow::SetClickCallback(ClickCallback cb)
{
    m_clickCb = std::move(cb);
}

// ---------------------------------------------------------------------------
// 命中检测
// ---------------------------------------------------------------------------

/// 将窗口内 x 坐标映射为候选索引（0 起）。
/// 命中失败返回 -1。
int CCandidateWindow::HitTest(int x, int y) const
{
    if (m_candidates.empty()) {
        return -1;
    }
    const float scale = m_dpiScale;
    const float fontScale = m_fontSize / 16.0f;
    const float padF = static_cast<float>(m_padding) * scale;
    const float pinyinH = 18.0f * fontScale * scale;
    const float candH = 22.0f * fontScale * scale;
    const bool hasPinyin = !m_pinyin.empty() && !m_inlinePreedit;

    if (m_multiRow) {
        // 多行：行列 → 索引 = row * kPerRow + col
        const int row = static_cast<int>((static_cast<float>(y) - padF - (hasPinyin ? pinyinH : 0.0f)) / candH);
        if (row < 0) {
            return -1;
        }
        const int col = static_cast<int>((static_cast<float>(x) - padF) / (96.0f * scale));
        const int index = row * kPerRow + col;
        if (index >= 0 && index < static_cast<int>(m_candidates.size())) {
            return index;
        }
        return -1;
    }
    // 单行：与 Render 中相同的水平布局逻辑
    float cursorX = padF;
    for (size_t i = 0; i < m_candidates.size(); ++i) {
        const std::wstring word = Utf8ToWide(m_candidates[i]);
        // P0-1：标签按 label_format，宽度与 Render 一致
        const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
        const float itemWidth = static_cast<float>((label.size() + word.size()) * 16 + 20) * scale;
        if (x >= cursorX && x <= cursorX + itemWidth) {
            return static_cast<int>(i);
        }
        cursorX += itemWidth + static_cast<float>(m_spacing) * scale;
    }
    return -1;
}

// ---------------------------------------------------------------------------
// P0-1 新增接口：标签格式 / 布局参数 / 滚轮翻页
// ---------------------------------------------------------------------------
void CCandidateWindow::SetLabelFormat(const std::wstring& fmt)
{
    if (fmt != m_labelFormat) {
        m_labelFormat = fmt;
        if (m_visible && m_hwnd != nullptr) {
            RECT caret = {};
            GetWindowRect(m_hwnd, &caret);
            PositionWindow(caret);
            if (m_pRenderTarget != nullptr) { Render(); }
        }
    }
}

void CCandidateWindow::SetLayout(float cornerRadius, float hiliteRadius,
                                 int padding, int spacing)
{
    m_cornerRadius = cornerRadius;
    m_hiliteRadius = hiliteRadius;
    m_padding = padding;
    m_spacing = spacing;
    if (m_visible && m_hwnd != nullptr) {
        RECT caret = {};
        GetWindowRect(m_hwnd, &caret);
        PositionWindow(caret);
        if (m_pRenderTarget != nullptr) { Render(); }
    }
}

void CCandidateWindow::SetPageCallback(PageCallback cb)
{
    m_pageCb = std::move(cb);
}

} // namespace taishen

