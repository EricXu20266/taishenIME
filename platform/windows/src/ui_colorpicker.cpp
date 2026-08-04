/// 自研窗体系统 — 颜色选择器实现（V0.3.1）

#include "ui_colorpicker.h"
#include "ui_window.h"
#include <cmath>
#include <windows.h>

namespace taishen {

namespace {
/// HSV → RGB（D2D1_COLOR_F）
D2D1_COLOR_F HsvToRgb(float h, float s, float v)
{
    const float c = v * s;
    const float hp = h / 60.0f;
    const float x = c * (1.0f - std::abs(std::fmod(hp, 2.0f) - 1.0f));
    float r = 0, g = 0, b = 0;
    const int i = (static_cast<int>(std::floor(hp)) % 6 + 6) % 6;
    switch (i) {
    case 0: r = c; g = x; break;
    case 1: r = x; g = c; break;
    case 2: g = c; b = x; break;
    case 3: g = x; b = c; break;
    case 4: r = x; b = c; break;
    default: r = c; b = x; break;
    }
    const float m = v - c;
    return D2D1::ColorF(r + m, g + m, b + m, 1.0f);
}

/// 预设色板：8 行（亮度 0.2..1.0 渐变）× 16 列（色环 22.5° 步进）
D2D1_COLOR_F PaletteColor(int row, int col)
{
    const float v = 0.2f + static_cast<float>(row) * 0.114f;
    if (row == 7) { // 最后一行 = 灰阶
        return D2D1::ColorF(v, v, v);
    }
    return HsvToRgb(static_cast<float>(col) * 22.5f, 0.85f, v);
}
} // namespace

UIColorSwatch::UIColorSwatch()
{
}

RECT UIColorSwatch::PanelRect() const
{
    const int pw = kCols * kCell + (kCols + 1) * kPad;
    const int ph = kRows * kCell + (kRows + 1) * kPad;
    return { X(), Y() + Height() + 2,
             X() + pw, Y() + Height() + 2 + ph };
}

void UIColorSwatch::Expand()
{
    if (m_expanded) {
        return;
    }
    m_expanded = true;
    // 色板格（子控件）
    const RECT pr = PanelRect();
    for (int row = 0; row < kRows; ++row) {
        for (int col = 0; col < kCols; ++col) {
            struct Cell : public UIControl {
                D2D1_COLOR_F color;
                std::function<void(D2D1_COLOR_F)> cb;
                void Draw(UIRenderer& r, const UITheme& t) override
                {
                    r.FillRoundedRect(
                        D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                    static_cast<float>(X() + Width()),
                                    static_cast<float>(Y() + Height())),
                        2.0f, color);
                    if (IsHovered()) {
                        r.DrawRoundedRect(
                            D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                        static_cast<float>(X() + Width()),
                                        static_cast<float>(Y() + Height())),
                            2.0f, t.accent, 1.5f);
                    }
                }
                void OnClick(int, int) override
                {
                    if (cb) { cb(color); }
                }
            };
            auto* cell = new Cell();
            cell->color = PaletteColor(row, col);
            const int x = pr.left + kPad + col * (kCell + kPad);
            const int y = pr.top + kPad + row * (kCell + kPad);
            cell->SetRect({ x, y, x + kCell, y + kCell });
            cell->cb = [this](D2D1_COLOR_F c) {
                m_color = c;
                Collapse();
                if (m_onColor) {
                    m_onColor(c);
                }
            };
            cell->SetWindow(m_window);
            cell->SetParent(this);
            m_children.push_back(cell);
        }
    }
    Invalidate();
}

void UIColorSwatch::Collapse()
{
    if (!m_expanded) {
        return;
    }
    m_expanded = false;
    // 删除色块前清空窗口悬停/按下指针（V0.3.5 审查修复：防 use-after-free）
    if (m_window != nullptr) {
        m_window->ClearPointerTracking();
    }
    RemoveAllChildren(true);
    Invalidate();
}

void UIColorSwatch::Draw(UIRenderer& r, const UITheme& t)
{
    // 色板面板背景
    if (m_expanded) {
        const RECT pr = PanelRect();
        r.FillRoundedRect(
            D2D1::RectF(static_cast<float>(pr.left), static_cast<float>(pr.top),
                        static_cast<float>(pr.right), static_cast<float>(pr.bottom)),
            t.cornerRadius, t.cardBg);
        r.DrawRoundedRect(
            D2D1::RectF(static_cast<float>(pr.left), static_cast<float>(pr.top),
                        static_cast<float>(pr.right), static_cast<float>(pr.bottom)),
            t.cornerRadius, t.border, 1.0f);
    }
    // 色块 + HEX
    const D2D1_RECT_F sw = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                       static_cast<float>(X() + 28), static_cast<float>(Y() + Height()));
    r.FillRoundedRect(sw, 3.0f, m_color);
    r.DrawRoundedRect(sw, 3.0f, t.border, 1.0f);
    wchar_t hex[16]{};
    swprintf_s(hex, L"#%02X%02X%02X",
               static_cast<int>(m_color.r * 255.0f + 0.5f),
               static_cast<int>(m_color.g * 255.0f + 0.5f),
               static_cast<int>(m_color.b * 255.0f + 0.5f));
    r.DrawText(hex,
               D2D1::RectF(static_cast<float>(X() + 34), static_cast<float>(Y()),
                           static_cast<float>(X() + Width()), static_cast<float>(Y() + Height())),
               t.fontSize, t.text, false,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    // 色板格（子控件）
    for (UIControl* c : m_children) {
        if (c->IsVisible()) {
            c->Draw(r, t);
        }
    }
}

void UIColorSwatch::OnClick(int /*x*/, int /*y*/)
{
    if (!IsEnabled()) {
        return;
    }
    if (m_expanded) {
        Collapse();
    } else {
        Expand();
    }
}

void UIColorSwatch::OnGlobalMouseDown(int x, int y)
{
    if (!m_expanded) {
        return;
    }
    if (HitTest(x, y)) {
        return;
    }
    const RECT pr = PanelRect();
    if (x >= pr.left && x < pr.right && y >= pr.top && y < pr.bottom) {
        return;
    }
    Collapse();
}

} // namespace taishen
