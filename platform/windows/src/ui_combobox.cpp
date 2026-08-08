/// 自研窗体系统 — 下拉框实现（V0.3.1）

#include "ui_combobox.h"
#include "ui_window.h"
#include <algorithm>
#include <windows.h>

namespace taishen {

namespace {

constexpr int kComboRowH = 30;     // 下拉面板行高（V0.3.6：24→30 更舒适）
constexpr int kComboPanelGap = 2;  // 面板与主体间距

/// 下拉面板中的行项（内部）
class ComboRow : public UIControl
{
public:
    explicit ComboRow(std::wstring text)
        : m_text(std::move(text))
    {
    }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                           static_cast<float>(X() + Width()),
                                           static_cast<float>(Y() + Height()));
        const D2D1_RECT_F trc = D2D1::RectF(static_cast<float>(X()) + 8.0f, static_cast<float>(Y()),
                                            static_cast<float>(X() + Width()),
                                            static_cast<float>(Y() + Height()));
        if (m_selected) {
            // 选中项 accent 底白字（V0.3.6）
            r.FillRoundedRect(rc, 3.0f, t.accent);
            r.DrawText(m_text, trc, t.fontSize, t.accentText, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        } else if (IsHovered()) {
            r.FillRoundedRect(rc, 3.0f, t.hoverBg);
            r.DrawText(m_text, trc, t.fontSize, t.text, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        } else {
            r.DrawText(m_text, trc, t.fontSize, t.text, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
    }

    void OnClick(int /*x*/, int /*y*/) override
    {
        if (m_onClick) {
            m_onClick(m_index);
        }
    }

    void SetIndex(int i) { m_index = i; }
    void SetSelected(bool s) { m_selected = s; }
    void SetOnClick(std::function<void(int)> cb) { m_onClick = std::move(cb); }

private:
    std::wstring m_text;
    int m_index = -1;
    bool m_selected = false;
    std::function<void(int)> m_onClick;
};

/// 下拉面板（V0.3.6 窗口弹出层）：背景 + 行。
/// 背景用 hoverBg（区别于卡片 cardBg——修复面板无背景色/与内容同层）。
class ComboPanel : public UIControl
{
public:
    void Build(const std::vector<std::wstring>& items, int selected,
               std::function<void(int)> onSelect)
    {
        RemoveAllChildren(true);
        const RECT rc = Rect();
        for (int i = 0; i < static_cast<int>(items.size()); ++i) {
            auto* row = new ComboRow(items[i]);
            row->SetIndex(i);
            row->SetSelected(i == selected);
            row->SetRect({ rc.left, rc.top + 2 + i * kComboRowH,
                           rc.right, rc.top + 2 + (i + 1) * kComboRowH });
            row->SetParent(this);
            row->SetOnClick(onSelect);
            AddChild(row);
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

UIComboBox::UIComboBox()
{
}

void UIComboBox::SetItems(const std::vector<std::wstring>& items)
{
    m_items = items;
    if (m_selected >= static_cast<int>(m_items.size())) {
        m_selected = -1;
    }
    Collapse();
    Invalidate();
}

void UIComboBox::SetSelectedIndex(int idx)
{
    m_selected = idx;
    Invalidate();
}

std::wstring UIComboBox::SelectedText() const
{
    if (m_selected >= 0 && m_selected < static_cast<int>(m_items.size())) {
        return m_items[m_selected];
    }
    return L"";
}

RECT UIComboBox::PanelRect() const
{
    const int n = static_cast<int>(m_items.size());
    const int ph = n > 0 ? n * kComboRowH + 4 : 0;
    RECT rc{ X(), Y() + Height() + kComboPanelGap,
             X() + Width(), Y() + Height() + kComboPanelGap + ph };
    // V0.3.6：下方空间不足（滚动容器内面板超出可视区会被裁剪）→ 向上展开
    if (m_window != nullptr) {
        RECT client{};
        GetClientRect(m_window->Hwnd(), &client);
        if (rc.bottom > client.bottom) {
            rc.top = Y() - ph - kComboPanelGap;
            rc.bottom = Y() - kComboPanelGap;
            if (rc.top < client.top) {
                rc.top = client.top;
                rc.bottom = rc.top + ph;
            }
        }
    }
    return rc;
}

void UIComboBox::Expand()
{
    if (m_expanded) {
        return;
    }
    m_expanded = true;
    if (m_window == nullptr) {
        return;
    }
    // V0.3.6：面板注册为窗口弹出层（浮最上层、不被 ScrollPanel 裁剪、命中优先）
    auto* panel = new ComboPanel();
    panel->SetRect(PanelRect());
    panel->Build(m_items, m_selected, [this](int idx) {
        SetSelectedIndex(idx);
        Collapse();
        if (m_onSelected) {
            m_onSelected(idx);
        }
    });
    m_panel = panel;
    m_window->RegisterPopup(panel);
    Invalidate();
}

void UIComboBox::Collapse()
{
    if (!m_expanded) {
        return;
    }
    m_expanded = false;
    if (m_window != nullptr) {
        // 删除面板前清空窗口悬停/按下指针（V0.3.5 审查修复：防 use-after-free）
        m_window->ClearPointerTracking();
        m_window->UnregisterPopup(m_panel);
    }
    delete m_panel;
    m_panel = nullptr;
    Invalidate();
}

void UIComboBox::Draw(UIRenderer& r, const UITheme& t)
{
    // V0.3.6：只画主体——面板由窗口弹出层绘制（浮最上层、不被裁剪）
    const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                       static_cast<float>(X() + Width()),
                                       static_cast<float>(Y() + Height()));
    r.FillRoundedRect(rc, t.cornerRadius, IsHovered() ? t.hoverBg : t.cardBg);
    r.DrawRoundedRect(rc, t.cornerRadius, m_expanded ? t.accent : t.border, 1.0f);
    // 当前项文本（V0.3.6：noWrap——长项单行，不换行挤扁）
    const std::wstring cur = SelectedText();
    r.DrawText(cur,
               D2D1::RectF(static_cast<float>(X()) + 8.0f, static_cast<float>(Y()),
                           static_cast<float>(X() + Width()) - 22.0f, static_cast<float>(Y() + Height())),
               t.fontSize, !IsEnabled() ? t.textDim : t.text, false,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
               true);
    // ▾ 箭头（画在右侧）
    const float ax = static_cast<float>(X() + Width()) - 14.0f;
    const float ay = static_cast<float>(Y() + Height() / 2);
    r.DrawLine(ax - 4.0f, ay - 2.0f, ax, ay + 2.0f, t.textDim, 1.4f);
    r.DrawLine(ax, ay + 2.0f, ax + 4.0f, ay - 2.0f, t.textDim, 1.4f);
}

void UIComboBox::OnClick(int /*x*/, int /*y*/)
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

void UIComboBox::OnGlobalMouseDown(int x, int y)
{
    if (!m_expanded) {
        return;
    }
    // 点击自身或面板内 → 不收起（面板行由行自身处理）
    if (HitTest(x, y)) {
        return;
    }
    const RECT pr = PanelRect();
    if (x >= pr.left && x < pr.right && y >= pr.top && y < pr.bottom) {
        return;
    }
    Collapse();
}

void UIComboBox::OnKeyDown(int vk, bool /*ctrl*/, bool /*shift*/, bool /*alt*/)
{
    if (!IsEnabled()) {
        return;
    }
    if (vk == VK_RETURN || vk == VK_SPACE) {
        OnClick(0, 0);
    } else if (vk == VK_UP || vk == VK_DOWN) {
        const int n = static_cast<int>(m_items.size());
        if (n == 0) {
            return;
        }
        int next = m_selected < 0 ? 0 : m_selected;
        next += (vk == VK_DOWN) ? 1 : -1;
        next = std::clamp(next, 0, n - 1);
        SetSelectedIndex(next);
        if (m_onSelected) {
            m_onSelected(next);
        }
    }
}

} // namespace taishen
