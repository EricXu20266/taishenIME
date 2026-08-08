/// 自研窗体系统 — 颜色选择器实现（V0.3.1）

#include "ui_colorpicker.h"
#include "ui_window.h"
#include <cmath>
#include <windows.h>

namespace taishen {

namespace {
/// 色板网格常量（V0.3.6 移到匿名命名空间——ColorPanel 弹出层共用）
constexpr int kCols = 16;
constexpr int kRows = 8;
constexpr int kCell = 18;
constexpr int kPad = 2;

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

/// 色板格（内部）
class ColorCell : public UIControl
{
public:
    D2D1_COLOR_F color{};
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
        if (cb) {
            cb(color);
        }
    }
};

/// 色板面板（V0.3.6 窗口弹出层）：背景 + 网格。
/// 背景用 hoverBg（区别于卡片 cardBg——修复色板无背景色/与内容同层）。
class ColorPanel : public UIControl
{
public:
    void Build(std::function<void(D2D1_COLOR_F)> onSelect)
    {
        RemoveAllChildren(true);
        const RECT rc = Rect();
        for (int row = 0; row < kRows; ++row) {
            for (int col = 0; col < kCols; ++col) {
                auto* cell = new ColorCell();
                cell->color = PaletteColor(row, col);
                const int x = rc.left + kPad + col * (kCell + kPad);
                const int y = rc.top + kPad + row * (kCell + kPad);
                cell->SetRect({ x, y, x + kCell, y + kCell });
                cell->cb = onSelect;
                cell->SetParent(this);
                AddChild(cell);
            }
        }
    }

    int PreferredHeight(int /*width*/) const override { return -1; }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                           static_cast<float>(X() + Width()),
                                           static_cast<float>(Y() + Height()));
        r.FillRoundedRect(rc, t.cornerRadius, t.hoverBg);
        r.DrawRoundedRect(rc, t.cornerRadius, t.border, 1.0f);
        for (UIControl* c : m_children) {
            if (c->IsVisible()) {
                c->Draw(r, t);
            }
        }
    }
};

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
    if (m_window == nullptr) {
        return;
    }
    // V0.3.6：色板注册为窗口弹出层（浮最上层、不被裁剪、命中优先）
    auto* panel = new ColorPanel();
    panel->SetRect(PanelRect());
    panel->Build([this](D2D1_COLOR_F c) {
        m_color = c;
        Collapse();
        if (m_onColor) {
            m_onColor(c);
        }
    });
    m_panel = panel;
    m_window->RegisterPopup(panel);
    Invalidate();
}

void UIColorSwatch::Collapse()
{
    if (!m_expanded) {
        return;
    }
    m_expanded = false;
    if (m_window != nullptr) {
        // 删除色板前清空窗口悬停/按下指针（V0.3.5 审查修复：防 use-after-free）
        m_window->ClearPointerTracking();
        m_window->UnregisterPopup(m_panel);
    }
    delete m_panel;
    m_panel = nullptr;
    Invalidate();
}

void UIColorSwatch::Draw(UIRenderer& r, const UITheme& t)
{
    // V0.3.6：只画色块 + HEX——色板由窗口弹出层绘制（浮最上层、不被裁剪）
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
