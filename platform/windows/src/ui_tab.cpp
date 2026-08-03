/// 自研窗体系统 — 标签页实现（V0.3.1）

#include "ui_tab.h"
#include "ui_layout.h"
#include <algorithm>
#include <windows.h>

namespace taishen {

UITab::UITab()
{
}

void UITab::AddPage(const std::wstring& title, UIControl* content)
{
    if (content == nullptr) {
        return;
    }
    content->SetParent(this);
    content->SetWindow(m_window);
    m_children.push_back(content);
    m_pages.push_back({ title, content });
    content->SetVisible(m_current == static_cast<int>(m_pages.size()) - 1);
    Invalidate();
}

void UITab::SetWindow(UIWindow* w)
{
    UIControl::SetWindow(w);
    for (UIControl* c : m_children) {
        c->SetWindow(w);
    }
}

void UITab::SetCurrent(int idx)
{
    if (idx < 0 || idx >= static_cast<int>(m_pages.size()) || idx == m_current) {
        return;
    }
    if (m_current >= 0 && m_current < static_cast<int>(m_pages.size())) {
        m_pages[m_current].content->SetVisible(false);
    }
    m_current = idx;
    m_pages[m_current].content->SetVisible(true);
    Invalidate();
    if (m_onChanged) {
        m_onChanged(idx);
    }
}

RECT UITab::ContentRect() const
{
    return { X(), Y() + kTabBarHeight,
             X() + Width(), Y() + Height() };
}

int UITab::TabFromX(int x) const
{
    const int n = static_cast<int>(m_pages.size());
    if (n == 0) {
        return -1;
    }
    const int tabW = Width() / n;
    const int idx = (x - X()) / tabW;
    return (idx >= 0 && idx < n) ? idx : -1;
}

void UITab::Draw(UIRenderer& r, const UITheme& t)
{
    const int n = static_cast<int>(m_pages.size());
    if (n == 0) {
        return;
    }
    // 内容区背景
    const RECT cr = ContentRect();
    r.FillRect(D2D1::RectF(static_cast<float>(cr.left), static_cast<float>(cr.top),
                           static_cast<float>(cr.right), static_cast<float>(cr.bottom)),
               t.bg);
    // 当前页内容（子控件）
    for (UIControl* c : m_children) {
        if (c->IsVisible()) {
            // 同步内容页 rect
            c->SetRect(cr);
            c->Draw(r, t);
        }
    }
    // 标签行
    const int tabW = Width() / n;
    for (int i = 0; i < n; ++i) {
        const float tx = static_cast<float>(X() + i * tabW);
        const float ty = static_cast<float>(Y());
        const bool selected = (i == m_current);
        // 标签文字
        r.DrawText(m_pages[i].title,
                   D2D1::RectF(tx, ty, tx + static_cast<float>(tabW),
                               ty + static_cast<float>(kTabBarHeight) - 2.0f),
                   t.fontSize, selected ? t.text : t.textDim, selected,
                   DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        // 选中下划线
        if (selected) {
            r.FillRect(D2D1::RectF(tx + 10.0f, ty + static_cast<float>(kTabBarHeight) - 2.0f,
                                   tx + static_cast<float>(tabW) - 10.0f,
                                   ty + static_cast<float>(kTabBarHeight)),
                       t.accent);
        }
    }
    // 标签行分隔线
    r.DrawLine(static_cast<float>(X()), static_cast<float>(Y() + kTabBarHeight - 1),
               static_cast<float>(X() + Width()), static_cast<float>(Y() + kTabBarHeight - 1),
               t.border, 1.0f);
}

void UITab::OnClick(int x, int y)
{
    if (y - Y() < kTabBarHeight) {
        const int idx = TabFromX(x);
        if (idx >= 0) {
            SetCurrent(idx);
        }
    }
}

} // namespace taishen
