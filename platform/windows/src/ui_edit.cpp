/// 自研窗体系统 — 单行编辑框实现（V0.3.1）

#include "ui_edit.h"
#include <algorithm>
#include <windows.h>

namespace taishen {

UIEdit::UIEdit()
{
}

void UIEdit::SetText(const std::wstring& t)
{
    m_text = t;
    m_caret = t.size();
    Invalidate();
    if (m_onChanged) {
        m_onChanged(m_text);
    }
}

void UIEdit::SetNumeric(int min, int max)
{
    m_numeric = true;
    m_min = min;
    m_max = max;
}

void UIEdit::Draw(UIRenderer& r, const UITheme& t)
{
    const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                       static_cast<float>(X() + Width()),
                                       static_cast<float>(Y() + Height()));
    // 背景 + 边框（聚焦时强调色）
    r.FillRoundedRect(rc, t.cornerRadius, t.cardBg);
    r.DrawRoundedRect(rc, t.cornerRadius,
                      m_focused ? t.accent : t.border, 1.0f);

    const float textX = static_cast<float>(X()) + 8.0f;
    const float textTop = static_cast<float>(Y());
    const float textW = static_cast<float>(Width()) - 16.0f;
    const D2D1_RECT_F textRc = D2D1::RectF(textX, textTop,
                                           textX + textW,
                                           textTop + static_cast<float>(Height()));

    if (m_text.empty() && !m_placeholder.empty()) {
        r.DrawText(m_placeholder, textRc, t.fontSize, t.textDim, false,
                   DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        return;
    }

    // 文本（右端裁剪到光标右侧，保证光标可见）
    const std::wstring visible = m_text.substr(0, m_caret);
    const D2D1_SIZE_F prefix = r.MeasureText(visible, t.fontSize);
    const float caretX = textX + prefix.width;
    // 滚动偏移：光标超出右边界时左移文本
    float scroll = 0.0f;
    const D2D1_SIZE_F full = r.MeasureText(m_text, t.fontSize);
    if (caretX - scroll > textX + textW) {
        scroll = caretX - (textX + textW);
    }
    if (caretX - scroll < textX) {
        scroll = (std::max)(0.0f, caretX - textX);
    }
    r.DrawText(m_text,
               D2D1::RectF(textX - scroll, textTop,
                           textX - scroll + textW, textTop + static_cast<float>(Height())),
               t.fontSize, !IsEnabled() ? t.textDim : t.text, false,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    // 光标（聚焦时显示）
    if (m_focused) {
        r.DrawLine(caretX - scroll, textTop + 3.0f,
                   caretX - scroll, textTop + static_cast<float>(Height()) - 3.0f,
                   t.accent, 1.2f);
    }
}

void UIEdit::OnMouseDown(int x, int /*y*/, bool /*left*/)
{
    // 简化定位：按字符数均分估算
    const size_t idx = static_cast<size_t>(
        std::clamp(static_cast<int>((static_cast<double>(x) / (Width() > 0 ? Width() : 1)) * m_text.size()),
                   0, static_cast<int>(m_text.size())));
    m_caret = idx;
    Invalidate();
}

void UIEdit::OnKeyDown(int vk, bool ctrl, bool /*shift*/, bool /*alt*/)
{
    if (!IsEnabled()) {
        return;
    }
    bool changed = false;
    switch (vk) {
    case VK_BACK:
        if (ctrl) {
            // Ctrl+Backspace：删到上一个空格/词边界（简化：删前一个词）
            size_t p = m_caret;
            while (p > 0 && m_text[p - 1] == L' ') { --p; }
            while (p > 0 && m_text[p - 1] != L' ') { --p; }
            if (p < m_caret) {
                m_text.erase(p, m_caret - p);
                m_caret = p;
                changed = true;
            }
        } else if (m_caret > 0) {
            m_text.erase(m_caret - 1, 1);
            --m_caret;
            changed = true;
        }
        break;
    case VK_DELETE:
        if (m_caret < m_text.size()) {
            m_text.erase(m_caret, 1);
            changed = true;
        }
        break;
    case VK_LEFT:
        if (m_caret > 0) { --m_caret; changed = true; }
        break;
    case VK_RIGHT:
        if (m_caret < m_text.size()) { ++m_caret; changed = true; }
        break;
    case VK_HOME:
        m_caret = 0;
        changed = true;
        break;
    case VK_END:
        m_caret = m_text.size();
        changed = true;
        break;
    default:
        break;
    }
    if (changed) {
        if (m_numeric) {
            ClampNumeric();
        }
        Invalidate();
        if (m_onChanged) {
            m_onChanged(m_text);
        }
    }
}

void UIEdit::OnChar(wchar_t ch)
{
    if (!IsEnabled() || ch < 32) {
        return;
    }
    // 数字模式：仅接受 0-9
    if (m_numeric && (ch < L'0' || ch > L'9')) {
        return;
    }
    m_text.insert(m_caret, 1, ch);
    ++m_caret;
    if (m_numeric) {
        ClampNumeric();
    }
    Invalidate();
    if (m_onChanged) {
        m_onChanged(m_text);
    }
}

void UIEdit::OnFocus(bool focused)
{
    UIControl::OnFocus(focused);
    if (!focused) {
        // 失焦提交：数字模式收敛范围
        if (m_numeric) {
            ClampNumeric();
            Invalidate();
            if (m_onChanged) {
                m_onChanged(m_text);
            }
        }
    }
}

void UIEdit::ClampNumeric()
{
    if (m_text.empty()) {
        return;
    }
    long long v = 0;
    try {
        v = std::stoll(m_text);
    } catch (...) {
        m_text.clear();
        m_caret = 0;
        return;
    }
    v = std::clamp(v, static_cast<long long>(m_min), static_cast<long long>(m_max));
    m_text = std::to_wstring(v);
    m_caret = m_text.size();
}

} // namespace taishen
