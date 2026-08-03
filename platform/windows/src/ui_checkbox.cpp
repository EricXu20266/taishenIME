/// 自研窗体系统 — 复选框实现（V0.3.1）

#include "ui_checkbox.h"
#include <windows.h>

namespace taishen {

UICheckBox::UICheckBox(std::wstring text)
    : m_text(std::move(text))
{
}

void UICheckBox::Draw(UIRenderer& r, const UITheme& t)
{
    const int box = 16;
    const int x0 = X();
    const int y0 = Y() + (Height() - box) / 2;

    // 勾选框
    const D2D1_RECT_F boxRc = D2D1::RectF(static_cast<float>(x0),
                                          static_cast<float>(y0),
                                          static_cast<float>(x0 + box),
                                          static_cast<float>(y0 + box));
    const D2D1_COLOR_F bg = !IsEnabled() ? t.cardBg
        : (m_checked ? t.accent : t.cardBg);
    r.FillRoundedRect(boxRc, 3.0f, bg);
    r.DrawRoundedRect(boxRc, 3.0f, m_checked ? t.accent : t.border, 1.0f);
    // 勾（选中时）
    if (m_checked) {
        const float cx = static_cast<float>(x0) + box / 2.0f;
        const float cy = static_cast<float>(y0) + box / 2.0f;
        r.DrawLine(cx - 4.0f, cy, cx - 1.0f, cy + 3.0f, t.checkmark, 1.6f);
        r.DrawLine(cx - 1.0f, cy + 3.0f, cx + 4.0f, cy - 3.0f, t.checkmark, 1.6f);
    }
    // 文字
    r.DrawText(m_text,
               D2D1::RectF(static_cast<float>(x0 + box + 8), static_cast<float>(Y()),
                           static_cast<float>(X() + Width()), static_cast<float>(Y() + Height())),
               t.fontSize, !IsEnabled() ? t.textDim : t.text, false,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
}

void UICheckBox::OnClick(int /*x*/, int /*y*/)
{
    if (!IsEnabled()) {
        return;
    }
    m_checked = !m_checked;
    Invalidate();
    if (m_onChanged) {
        m_onChanged(m_checked);
    }
}

void UICheckBox::OnKeyDown(int vk, bool /*ctrl*/, bool /*shift*/, bool /*alt*/)
{
    if (vk == VK_SPACE) {
        OnClick(0, 0);
    }
}

} // namespace taishen
