/// 自研窗体系统 — D2D 渲染层实现（V0.3.0）

#include "ui_render.h"
#include <algorithm>

namespace taishen {

namespace {
/// D2D1_COLOR_F → COLORREF 缓存 key（8bit 量化）
DWORD ColorKey(D2D1_COLOR_F c)
{
    const auto to8 = [](float f) -> DWORD {
        const int v = static_cast<int>(f * 255.0f + 0.5f);
        return static_cast<DWORD>(std::clamp(v, 0, 255));
    };
    return (to8(c.r) << 16) | (to8(c.g) << 8) | to8(c.b);
}
} // namespace

UIRenderer::UIRenderer() = default;

UIRenderer::~UIRenderer()
{
    ReleaseDeviceResources();
}

bool UIRenderer::Ensure(HWND hwnd)
{
    if (m_rt != nullptr) {
        return true;
    }
    m_hwnd = hwnd;

    // DPI（Per-Monitor V2 下按窗口所在监视器）
    const UINT dpi = GetDpiForWindow(hwnd);
    m_dpiScale = dpi > 0 ? static_cast<float>(dpi) / 96.0f : 1.0f;

    if (m_factory == nullptr) {
        HRESULT hr = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED,
                                       &m_factory);
        if (FAILED(hr)) {
            return false;
        }
    }
    if (m_dwrite == nullptr) {
        HRESULT hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,
                                         __uuidof(IDWriteFactory),
                                         reinterpret_cast<IUnknown**>(&m_dwrite));
        if (FAILED(hr)) {
            return false;
        }
    }

    RECT rc{};
    GetClientRect(hwnd, &rc);
    const D2D1_SIZE_U size = D2D1::SizeU(
        static_cast<UINT32>(rc.right - rc.left),
        static_cast<UINT32>(rc.bottom - rc.top));
    const D2D1_PIXEL_FORMAT pf = D2D1::PixelFormat(
        DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED);
    const D2D1_RENDER_TARGET_PROPERTIES props = D2D1::RenderTargetProperties(
        D2D1_RENDER_TARGET_TYPE_DEFAULT, pf, 0.0f, 0.0f, D2D1_RENDER_TARGET_USAGE_NONE);
    HRESULT hr = m_factory->CreateHwndRenderTarget(
        props, D2D1::HwndRenderTargetProperties(hwnd, size), &m_rt);
    if (FAILED(hr)) {
        return false;
    }
    m_rt->SetDpi(static_cast<float>(dpi), static_cast<float>(dpi));
    return true;
}

void UIRenderer::ReleaseDeviceResources()
{
    for (auto& [key, brush] : m_brushes) {
        if (brush != nullptr) {
            brush->Release();
        }
    }
    m_brushes.clear();
    for (auto& [key, fmt] : m_formats) {
        if (fmt != nullptr) {
            fmt->Release();
        }
    }
    m_formats.clear();
    if (m_rt != nullptr) {
        m_rt->Release();
        m_rt = nullptr;
    }
    if (m_dwrite != nullptr) {
        m_dwrite->Release();
        m_dwrite = nullptr;
    }
    if (m_factory != nullptr) {
        m_factory->Release();
        m_factory = nullptr;
    }
}

void UIRenderer::BeginDraw()
{
    if (m_rt != nullptr) {
        m_rt->BeginDraw();
    }
}

void UIRenderer::EndDraw()
{
    if (m_rt != nullptr) {
        const HRESULT hr = m_rt->EndDraw();
        // 设备丢失：释放资源，下次 Ensure 重建
        if (hr == D2DERR_RECREATE_TARGET) {
            ReleaseDeviceResources();
        }
    }
}

void UIRenderer::Clear(D2D1_COLOR_F color)
{
    if (m_rt != nullptr) {
        m_rt->Clear(color);
    }
}

ID2D1SolidColorBrush* UIRenderer::Brush(D2D1_COLOR_F color)
{
    if (m_rt == nullptr) {
        return nullptr;
    }
    const DWORD key = ColorKey(color);
    const auto it = m_brushes.find(key);
    if (it != m_brushes.end()) {
        return it->second;
    }
    ID2D1SolidColorBrush* brush = nullptr;
    if (SUCCEEDED(m_rt->CreateSolidColorBrush(color, &brush))) {
        m_brushes.emplace(key, brush);
    }
    return brush;
}

void UIRenderer::FillRect(const D2D1_RECT_F& rc, D2D1_COLOR_F color)
{
    ID2D1SolidColorBrush* b = Brush(color);
    if (m_rt != nullptr && b != nullptr) {
        m_rt->FillRectangle(rc, b);
    }
}

void UIRenderer::FillRoundedRect(const D2D1_RECT_F& rc, float radius, D2D1_COLOR_F color)
{
    ID2D1SolidColorBrush* b = Brush(color);
    if (m_rt != nullptr && b != nullptr) {
        const float r = (std::min)(radius, (rc.right - rc.left) / 2.0f);
        m_rt->FillRoundedRectangle(
            D2D1::RoundedRect(rc, r, r), b);
    }
}

void UIRenderer::DrawRoundedRect(const D2D1_RECT_F& rc, float radius,
                                 D2D1_COLOR_F color, float strokeWidth)
{
    ID2D1SolidColorBrush* b = Brush(color);
    if (m_rt != nullptr && b != nullptr) {
        const float r = (std::min)(radius, (rc.right - rc.left) / 2.0f);
        m_rt->DrawRoundedRectangle(
            D2D1::RoundedRect(rc, r, r), b, strokeWidth);
    }
}

void UIRenderer::DrawLine(float x1, float y1, float x2, float y2,
                          D2D1_COLOR_F color, float strokeWidth)
{
    ID2D1SolidColorBrush* b = Brush(color);
    if (m_rt != nullptr && b != nullptr) {
        m_rt->DrawLine(D2D1::Point2F(x1, y1), D2D1::Point2F(x2, y2), b, strokeWidth);
    }
}

IDWriteTextFormat* UIRenderer::Format(float size, bool bold)
{
    const std::pair<std::wstring, float> key(L"Microsoft YaHei", size);
    const auto it = m_formats.find(key);
    if (it != m_formats.end()) {
        return it->second;
    }
    IDWriteTextFormat* fmt = nullptr;
    const HRESULT hr = m_dwrite->CreateTextFormat(
        L"Microsoft YaHei", nullptr,
        bold ? DWRITE_FONT_WEIGHT_BOLD : DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
        size, L"zh-CN", &fmt);
    if (FAILED(hr) || fmt == nullptr) {
        return nullptr;
    }
    m_formats.emplace(key, fmt);
    return fmt;
}

void UIRenderer::DrawText(const std::wstring& text, const D2D1_RECT_F& rc, float size,
                          D2D1_COLOR_F color, bool bold,
                          DWRITE_TEXT_ALIGNMENT align,
                          DWRITE_PARAGRAPH_ALIGNMENT valign)
{
    if (m_rt == nullptr || text.empty()) {
        return;
    }
    ID2D1SolidColorBrush* b = Brush(color);
    IDWriteTextFormat* fmt = Format(size, bold);
    if (b == nullptr || fmt == nullptr) {
        return;
    }
    fmt->SetTextAlignment(align);
    fmt->SetParagraphAlignment(valign);
    m_rt->DrawText(text.c_str(), static_cast<UINT32>(text.size()),
                   fmt, rc, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
}

D2D1_SIZE_F UIRenderer::MeasureText(const std::wstring& text, float size, bool bold)
{
    if (text.empty()) {
        return D2D1::SizeF(0.0f, size);
    }
    IDWriteTextFormat* fmt = Format(size, bold);
    if (fmt == nullptr) {
        return D2D1::SizeF(static_cast<float>(text.size()) * size, size);
    }
    IDWriteTextLayout* layout = nullptr;
    HRESULT hr = m_dwrite->CreateTextLayout(
        text.c_str(), static_cast<UINT32>(text.size()), fmt,
        4096.0f, 4096.0f, &layout);
    if (FAILED(hr) || layout == nullptr) {
        return D2D1::SizeF(static_cast<float>(text.size()) * size, size);
    }
    DWRITE_TEXT_METRICS metrics{};
    layout->GetMetrics(&metrics);
    layout->Release();
    return D2D1::SizeF(metrics.width, metrics.height);
}

} // namespace taishen
