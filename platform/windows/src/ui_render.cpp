/// 自研窗体系统 — D2D 渲染层实现（V0.3.0）

#include "ui_render.h"
#include <algorithm>
#include <cmath>

namespace taishen {

namespace {
inline void HashCombine(size_t& seed, size_t val)
{
    seed ^= val + 0x9e3779b9U + (seed << 6) + (seed >> 2);
}
} // namespace

size_t UIRenderer::LayoutCacheKeyHash::operator()(const LayoutCacheKey& k) const
{
    size_t seed = std::hash<std::wstring>{}(k.text);
    HashCombine(seed, std::hash<std::wstring>{}(k.fontFace));
    HashCombine(seed, std::hash<int>{}(k.sizeTenths));
    HashCombine(seed, std::hash<bool>{}(k.bold));
    return seed;
}

namespace {
/// D2D1_COLOR_F → 32 位 ARGB 缓存 key（8bit 量化，含 Alpha 防半透明碰撞）
DWORD ColorKey(D2D1_COLOR_F c)
{
    const auto to8 = [](float f) -> DWORD {
        const int v = static_cast<int>(f * 255.0f + 0.5f);
        return static_cast<DWORD>(std::clamp(v, 0, 255));
    };
    return (to8(c.a) << 24) | (to8(c.r) << 16) | (to8(c.g) << 8) | to8(c.b);
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

void UIRenderer::Resize(HWND hwnd)
{
    if (m_rt == nullptr) {
        return; // 未创建渲染目标，Ensure 时会按最新窗口尺寸创建
    }
    RECT rc{};
    GetClientRect(hwnd, &rc);
    const D2D1_SIZE_U size = D2D1::SizeU(
        static_cast<UINT32>(rc.right - rc.left),
        static_cast<UINT32>(rc.bottom - rc.top));
    m_rt->Resize(&size);
}

void UIRenderer::ReleaseDeviceResources()
{
    ClearLayoutCache();
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

void UIRenderer::PushClip(const D2D1_RECT_F& rc)
{
    if (m_rt != nullptr) {
        m_rt->PushAxisAlignedClip(rc, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
    }
}

void UIRenderer::PopClip()
{
    if (m_rt != nullptr) {
        m_rt->PopAxisAlignedClip();
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

IDWriteTextFormat* UIRenderer::Format(float size, bool bold, bool noWrap,
                                      const std::wstring& fontFace)
{
    if (m_dwrite == nullptr) {
        HRESULT hr = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED,
                                         __uuidof(IDWriteFactory),
                                         reinterpret_cast<IUnknown**>(&m_dwrite));
        if (FAILED(hr) || m_dwrite == nullptr) {
            return nullptr;
        }
    }
    const std::wstring face = fontFace.empty() ? L"Microsoft YaHei" : fontFace;
    const std::tuple<std::wstring, float, bool, bool> key(face, size, bold, noWrap);
    const auto it = m_formats.find(key);
    if (it != m_formats.end()) {
        return it->second;
    }
    IDWriteTextFormat* fmt = nullptr;
    const HRESULT hr = m_dwrite->CreateTextFormat(
        face.c_str(), nullptr,
        bold ? DWRITE_FONT_WEIGHT_BOLD : DWRITE_FONT_WEIGHT_NORMAL,
        DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
        size, L"zh-CN", &fmt);
    if (FAILED(hr) || fmt == nullptr) {
        return nullptr;
    }
    if (noWrap) {
        fmt->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    }
    m_formats.emplace(key, fmt);
    return fmt;
}

UIRenderer::TextLayoutCacheStats UIRenderer::LayoutCacheStats() const
{
    TextLayoutCacheStats s;
    s.hits = m_layoutHits;
    s.misses = m_layoutMisses;
    s.evictions = m_layoutEvictions;
    s.entries = m_layoutLru.size();
    s.capacity = m_layoutCacheCapacity;
    return s;
}

void UIRenderer::ResetLayoutCacheStats()
{
    m_layoutHits = 0;
    m_layoutMisses = 0;
    m_layoutEvictions = 0;
}

void UIRenderer::ClearLayoutCache()
{
    for (auto& entry : m_layoutLru) {
        if (entry.layout != nullptr) {
            entry.layout->Release();
            entry.layout = nullptr;
        }
    }
    m_layoutIndex.clear();
    m_layoutLru.clear();
}

void UIRenderer::EvictOverflowLayouts()
{
    while (m_layoutLru.size() > m_layoutCacheCapacity) {
        auto tail = std::prev(m_layoutLru.end());
        m_layoutIndex.erase(tail->key);
        if (tail->layout != nullptr) {
            tail->layout->Release();
        }
        m_layoutLru.pop_back();
        ++m_layoutEvictions;
    }
}

void UIRenderer::SetLayoutCacheCapacity(size_t capacity)
{
    m_layoutCacheCapacity = (std::max)(static_cast<size_t>(16), capacity);
    EvictOverflowLayouts();
}

const UIRenderer::LayoutCacheEntry* UIRenderer::GetOrCreateTextLayout(
    const std::wstring& text,
    float size,
    bool bold,
    const std::wstring& fontFace)
{
    if (text.empty()) {
        return nullptr;
    }
    const std::wstring face = fontFace.empty() ? L"Microsoft YaHei" : fontFace;
    LayoutCacheKey key;
    key.text = text;
    key.fontFace = face;
    key.sizeTenths = static_cast<int>(std::lround(size * 10.0f));
    key.bold = bold;

    const auto it = m_layoutIndex.find(key);
    if (it != m_layoutIndex.end()) {
        if (it->second != m_layoutLru.begin()) {
            m_layoutLru.splice(m_layoutLru.begin(), m_layoutLru, it->second);
        }
        ++m_layoutHits;
        return &(*it->second);
    }

    ++m_layoutMisses;
    IDWriteTextFormat* fmt = Format(size, bold, true, face);
    if (fmt == nullptr || m_dwrite == nullptr) {
        return nullptr;
    }
    IDWriteTextLayout* layout = nullptr;
    const HRESULT hr = m_dwrite->CreateTextLayout(
        text.c_str(), static_cast<UINT32>(text.size()), fmt,
        4096.0f, 4096.0f, &layout);
    if (FAILED(hr) || layout == nullptr) {
        return nullptr;
    }
    // 在布局对象自身显式锁定换行与对齐，防止外部 DrawText 改写共享 IDWriteTextFormat 状态
    layout->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    layout->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
    layout->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);

    DWRITE_TEXT_METRICS metrics{};
    if (FAILED(layout->GetMetrics(&metrics))) {
        layout->Release();
        return nullptr;
    }

    LayoutCacheEntry entry;
    entry.key = std::move(key);
    entry.layout = layout;
    entry.width = metrics.width;
    entry.widthIncludingTrailing = metrics.widthIncludingTrailingWhitespace;
    entry.height = metrics.height;

    m_layoutLru.push_front(std::move(entry));
    m_layoutIndex.emplace(m_layoutLru.front().key, m_layoutLru.begin());
    EvictOverflowLayouts();
    return &m_layoutLru.front();
}

void UIRenderer::DrawCachedText(const std::wstring& text, const D2D1_RECT_F& rc, float size,
                                D2D1_COLOR_F color, bool bold,
                                DWRITE_TEXT_ALIGNMENT align,
                                DWRITE_PARAGRAPH_ALIGNMENT valign,
                                const std::wstring& fontFace)
{
    if (m_rt == nullptr || text.empty()) {
        return;
    }
    const float boxW = rc.right - rc.left;
    const float boxH = rc.bottom - rc.top;
    if (boxW <= 0.0f || boxH <= 0.0f) {
        return;
    }
    ID2D1SolidColorBrush* b = Brush(color);
    const LayoutCacheEntry* entry = GetOrCreateTextLayout(text, size, bold, fontFace);
    if (b == nullptr || entry == nullptr || entry->layout == nullptr) {
        return;
    }

    float ox = rc.left;
    if (align == DWRITE_TEXT_ALIGNMENT_CENTER) {
        ox = rc.left + (boxW - entry->width) * 0.5f;
    } else if (align == DWRITE_TEXT_ALIGNMENT_TRAILING) {
        ox = rc.right - entry->width;
    }

    float oy = rc.top;
    if (valign == DWRITE_PARAGRAPH_ALIGNMENT_CENTER) {
        oy = rc.top + (boxH - entry->height) * 0.5f;
    } else if (valign == DWRITE_PARAGRAPH_ALIGNMENT_FAR) {
        oy = rc.bottom - entry->height;
    }

    const float measuredW = (std::max)(entry->width, entry->widthIncludingTrailing);
    const bool needClip = (measuredW > boxW + 0.5f) ||
                          (entry->height > boxH + 0.5f) ||
                          (ox < rc.left - 0.5f) ||
                          (ox + entry->width > rc.right + 0.5f) ||
                          (oy < rc.top - 0.5f) ||
                          (oy + entry->height > rc.bottom + 0.5f);
    if (needClip) {
        m_rt->PushAxisAlignedClip(rc, D2D1_ANTIALIAS_MODE_ALIASED);
    }
    m_rt->DrawTextLayout(D2D1::Point2F(ox, oy), entry->layout, b,
                         D2D1_DRAW_TEXT_OPTIONS_NONE);
    if (needClip) {
        m_rt->PopAxisAlignedClip();
    }
}

void UIRenderer::DrawText(const std::wstring& text, const D2D1_RECT_F& rc, float size,
                          D2D1_COLOR_F color, bool bold,
                          DWRITE_TEXT_ALIGNMENT align,
                          DWRITE_PARAGRAPH_ALIGNMENT valign,
                          bool noWrap,
                          const std::wstring& fontFace)
{
    if (m_rt == nullptr || text.empty()) {
        return;
    }
    ID2D1SolidColorBrush* b = Brush(color);
    IDWriteTextFormat* fmt = Format(size, bold, noWrap, fontFace);
    if (b == nullptr || fmt == nullptr) {
        return;
    }
    fmt->SetTextAlignment(align);
    fmt->SetParagraphAlignment(valign);
    m_rt->DrawText(text.c_str(), static_cast<UINT32>(text.size()),
                   fmt, rc, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
}

D2D1_SIZE_F UIRenderer::MeasureText(const std::wstring& text, float size, bool bold,
                                    const std::wstring& fontFace)
{
    if (text.empty()) {
        return D2D1::SizeF(0.0f, size);
    }
    const LayoutCacheEntry* entry = GetOrCreateTextLayout(text, size, bold, fontFace);
    if (entry == nullptr) {
        return D2D1::SizeF(static_cast<float>(text.size()) * size, size);
    }
    return D2D1::SizeF(entry->widthIncludingTrailing, entry->height);
}

size_t UIRenderer::HitTestTextPosition(const std::wstring& text, float x, float size,
                                       bool bold, const std::wstring& fontFace)
{
    if (text.empty() || x <= 0.0f) {
        return 0;
    }
    const LayoutCacheEntry* entry = GetOrCreateTextLayout(text, size, bold, fontFace);
    if (entry == nullptr || entry->layout == nullptr) {
        return text.size();
    }
    BOOL isTrailingHit = FALSE;
    BOOL isInside = FALSE;
    DWRITE_HIT_TEST_METRICS htm{};
    const HRESULT hr = entry->layout->HitTestPoint(x, size * 0.5f, &isTrailingHit, &isInside, &htm);
    if (FAILED(hr)) {
        return text.size();
    }
    size_t pos = static_cast<size_t>(htm.textPosition) +
                 (isTrailingHit ? static_cast<size_t>(htm.length) : 0);
    return (std::min)(pos, text.size());
}

} // namespace taishen
