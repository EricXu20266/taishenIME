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
    const int x0 = X();
    const int y0 = Y();

    // V0.3.6：iOS 风格 toggle 渲染（40×22 胶囊 + 18px 滑块）
    if (m_switchMode) {
        const int trackW = 40, trackH = 22, thumb = 18, margin = 2;
        const int ty = y0 + (Height() - trackH) / 2;
        const D2D1_RECT_F trackRc = D2D1::RectF(static_cast<float>(x0),
                                                static_cast<float>(ty),
                                                static_cast<float>(x0 + trackW),
                                                static_cast<float>(ty + trackH));
        const D2D1_COLOR_F trackColor = !IsEnabled() ? t.switchTrackOff
            : (m_checked ? t.switchTrackOn : t.switchTrackOff);
        r.FillRoundedRect(trackRc, trackH / 2.0f, trackColor);
        // 滑块：开 → 靠右，关 → 靠左
        const float thumbX = m_checked
            ? static_cast<float>(x0 + trackW - thumb - margin)
            : static_cast<float>(x0 + margin);
        const D2D1_RECT_F thumbRc = D2D1::RectF(thumbX,
                                                static_cast<float>(ty + margin),
                                                thumbX + thumb,
                                                static_cast<float>(ty + margin + thumb));
        r.FillRoundedRect(thumbRc, thumb / 2.0f, t.switchThumb);
        // 文字（轨道右侧，垂直居中）
        r.DrawText(m_text,
                   D2D1::RectF(static_cast<float>(x0 + trackW + 8), static_cast<float>(y0),
                               static_cast<float>(X() + Width()), static_cast<float>(Y() + Height())),
                   t.fontSize, !IsEnabled() ? t.textDim : t.text, false,
                   DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        return;
    }

    // 原 checkbox：圆角方块 + 勾 + 文字
    const int box = 16;
    const int cy0 = y0 + (Height() - box) / 2;

    // 勾选框
    const D2D1_RECT_F boxRc = D2D1::RectF(static_cast<float>(x0),
                                          static_cast<float>(cy0),
                                          static_cast<float>(x0 + box),
                                          static_cast<float>(cy0 + box));
    const D2D1_COLOR_F bg = !IsEnabled() ? t.cardBg
        : (m_checked ? t.accent : t.cardBg);
    r.FillRoundedRect(boxRc, 3.0f, bg);
    r.DrawRoundedRect(boxRc, 3.0f, m_checked ? t.accent : t.border, 1.0f);
    // 勾（选中时）
    if (m_checked) {
        const float cx = static_cast<float>(x0) + box / 2.0f;
        const float ccy = static_cast<float>(cy0) + box / 2.0f;
        r.DrawLine(cx - 4.0f, ccy, cx - 1.0f, ccy + 3.0f, t.checkmark, 1.6f);
        r.DrawLine(cx - 1.0f, ccy + 3.0f, cx + 4.0f, ccy - 3.0f, t.checkmark, 1.6f);
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
