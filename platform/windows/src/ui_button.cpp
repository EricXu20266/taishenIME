/// 自研窗体系统 — 按钮实现（V0.3.1）

#include "ui_button.h"
#include <windows.h>

namespace taishen {

UIButton::UIButton(std::wstring text)
    : m_text(std::move(text))
{
}

void UIButton::Draw(UIRenderer& r, const UITheme& t)
{
    const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                       static_cast<float>(X() + Width()),
                                       static_cast<float>(Y() + Height()));
    D2D1_COLOR_F bg;
    D2D1_COLOR_F fg;
    if (m_primary) {
        bg = t.accent;
        fg = t.accentText;
        if (!IsEnabled()) {
            bg.a *= 0.5f;
        } else if (IsPressed()) {
            bg = t.pressedBg;
        }
    } else {
        switch (State()) {
        case UIState::Disabled: bg = t.cardBg; fg = t.textDim; break;
        case UIState::Hover:    bg = t.hoverBg; fg = t.text; break;
        case UIState::Pressed:  bg = t.pressedBg; fg = t.text; break;
        default:                bg = t.cardBg; fg = t.text; break;
        }
    }
    r.FillRoundedRect(rc, t.cornerRadius, bg);
    r.DrawRoundedRect(rc, t.cornerRadius, t.border, 1.0f);
    // V0.3.6：noWrap——按钮文字单行，宽度不足时裁剪而非换行挤扁
    r.DrawText(m_text, rc, t.fontSize, fg, false,
               DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
               true);
}

void UIButton::OnMouseDown(int x, int y, bool left)
{
    UIControl::OnMouseDown(x, y, left);
}

void UIButton::OnMouseUp(int x, int y, bool left)
{
    UIControl::OnMouseUp(x, y, left);
}

void UIButton::OnClick(int /*x*/, int /*y*/)
{
    if (IsEnabled() && m_onClick) {
        m_onClick();
    }
}

void UIButton::OnKeyDown(int vk, bool /*ctrl*/, bool /*shift*/, bool /*alt*/)
{
    if ((vk == VK_SPACE || vk == VK_RETURN) && IsEnabled() && m_onClick) {
        m_onClick();
    }
}

} // namespace taishen
