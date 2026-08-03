/// 自研窗体系统 — 控件基类实现（V0.3.0）

#include "ui_control.h"
#include "ui_window.h"

namespace taishen {

UIState UIControl::State() const
{
    if (!m_enabled) {
        return UIState::Disabled;
    }
    if (m_pressed) {
        return UIState::Pressed;
    }
    if (m_hovered) {
        return UIState::Hover;
    }
    return UIState::Normal;
}

void UIControl::AddChild(UIControl* c)
{
    if (c == nullptr) {
        return;
    }
    c->SetParent(this);
    c->SetWindow(m_window);
    m_children.push_back(c);
    Invalidate();
}

void UIControl::SetWindow(UIWindow* w)
{
    m_window = w;
    for (UIControl* c : m_children) {
        c->SetWindow(w);
    }
}

bool UIControl::HitTest(int x, int y) const
{
    return x >= m_rect.left && x < m_rect.right &&
           y >= m_rect.top && y < m_rect.bottom;
}

UIControl* UIControl::HitTestTree(int x, int y)
{
    if (!m_visible) {
        return nullptr;
    }
    // 子控件优先（后加入的在上层；子可弹出超出父矩形）
    for (auto it = m_children.rbegin(); it != m_children.rend(); ++it) {
        if (UIControl* c = (*it)->HitTestTree(x, y)) {
            return c;
        }
    }
    return HitTest(x, y) ? this : nullptr;
}

void UIControl::OnMouseMove(int /*x*/, int /*y*/) {}

void UIControl::OnMouseLeave()
{
    if (m_hovered) {
        m_hovered = false;
        Invalidate();
    }
}

void UIControl::OnMouseDown(int /*x*/, int /*y*/, bool /*left*/)
{
    if (!m_pressed) {
        m_pressed = true;
        Invalidate();
    }
}

void UIControl::OnMouseUp(int /*x*/, int /*y*/, bool /*left*/)
{
    if (m_pressed) {
        m_pressed = false;
        Invalidate();
    }
}

void UIControl::OnClick(int /*x*/, int /*y*/) {}

void UIControl::OnKeyDown(int /*vk*/, bool /*ctrl*/, bool /*shift*/, bool /*alt*/) {}

void UIControl::OnChar(wchar_t /*ch*/) {}

void UIControl::OnFocus(bool focused)
{
    if (m_focused != focused) {
        m_focused = focused;
        Invalidate();
    }
}

void UIControl::Invalidate()
{
    if (m_window != nullptr) {
        m_window->Invalidate();
    }
}

} // namespace taishen
