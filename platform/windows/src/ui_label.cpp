/// 自研窗体系统 — 文本标签实现（V0.3.1）

#include "ui_label.h"

namespace taishen {

UILabel::UILabel(std::wstring text)
    : m_text(std::move(text))
{
}

void UILabel::Draw(UIRenderer& r, const UITheme& t)
{
    if (m_text.empty()) {
        return;
    }
    DWRITE_TEXT_ALIGNMENT align = DWRITE_TEXT_ALIGNMENT_LEADING;
    switch (m_align) {
    case Align::Center: align = DWRITE_TEXT_ALIGNMENT_CENTER; break;
    case Align::Right:  align = DWRITE_TEXT_ALIGNMENT_TRAILING; break;
    default: break;
    }
    // 换行模式：限制宽度让 DWrite 自动换行（默认 4096 无限宽）
    const float maxW = m_wrap ? static_cast<float>(Width()) : 4096.0f;
    r.DrawText(m_text,
               D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                           static_cast<float>(X()) + maxW,
                           static_cast<float>(Y() + Height())),
               t.fontSize,
               m_dim ? t.textDim : t.text,
               m_bold, align, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
}

} // namespace taishen
