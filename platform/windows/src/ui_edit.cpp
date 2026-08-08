/// 自研窗体系统 — 单行编辑框实现（V0.3.1）

#include "ui_edit.h"
#include "ui_window.h"
#include <algorithm>
#include <cstring>
#include <windows.h>

namespace taishen {

UIEdit::UIEdit()
{
}

void UIEdit::SetText(const std::wstring& t)
{
    m_text = t;
    m_caret = t.size();
    m_selStart = SIZE_MAX;  // 重置文本时清选中
    m_dragging = false;
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
                      !IsEnabled() ? t.textDim
                                   : (m_focused ? t.accent : t.border),
                      1.0f);

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

    // 滚动偏移：光标超出右边界时左移文本（光标可见）
    const std::wstring prefix = m_text.substr(0, m_caret);
    const D2D1_SIZE_F prefixSz = r.MeasureText(prefix, t.fontSize);
    const float caretX = textX + prefixSz.width;
    float scroll = 0.0f;
    if (caretX - scroll > textX + textW) {
        scroll = caretX - (textX + textW);
    }
    if (caretX - scroll < textX) {
        scroll = (std::max)(0.0f, caretX - textX);
    }
    const float drawX = textX - scroll;

    // V0.3.6：有选中 → 三段绘制（前/选中/后），选中段 accent 实底白字
    if (HasSelection()) {
        const size_t a = (std::min)(m_selStart, m_caret);
        const size_t b = (std::max)(m_selStart, m_caret);
        const std::wstring pre = m_text.substr(0, a);
        const std::wstring sel = m_text.substr(a, b - a);
        const std::wstring post = m_text.substr(b);
        const float preW = r.MeasureText(pre, t.fontSize).width;
        const float selW = r.MeasureText(sel, t.fontSize).width;
        const D2D1_RECT_F selRc = D2D1::RectF(drawX + preW, textTop,
                                              drawX + preW + selW, textTop + static_cast<float>(Height()));
        if (a > 0) {
            r.DrawText(pre, D2D1::RectF(drawX, textTop, drawX + preW, textTop + static_cast<float>(Height())),
                       t.fontSize, t.text, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
        r.FillRoundedRect(selRc, 2.0f, t.accent);
        r.DrawText(sel, selRc, t.fontSize, t.accentText, false,
                   DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        if (!post.empty()) {
            r.DrawText(post, D2D1::RectF(drawX + preW + selW, textTop,
                                         drawX + preW + selW + textW, textTop + static_cast<float>(Height())),
                       t.fontSize, t.text, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        }
    } else {
        r.DrawText(m_text,
                   D2D1::RectF(drawX, textTop,
                               drawX + textW, textTop + static_cast<float>(Height())),
                   t.fontSize, !IsEnabled() ? t.textDim : t.text, false,
                   DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }
    // 光标（聚焦时显示）
    if (m_focused) {
        r.DrawLine(caretX - scroll, textTop + 3.0f,
                   caretX - scroll, textTop + static_cast<float>(Height()) - 3.0f,
                   t.accent, 1.2f);
    }
}

void UIEdit::OnMouseDown(int x, int /*y*/, bool /*left*/)
{
    // V0.3.7：禁用（只读）框不响应鼠标——保持灰色展示
    if (!IsEnabled()) {
        return;
    }
    // V0.3.6：定位光标 + 记录拖选锚点（单击清选中）
    m_caret = CharAt(x);
    m_selStart = SIZE_MAX;
    m_selAnchor = m_caret;
    m_dragging = true;
    Invalidate();
}

void UIEdit::OnMouseMove(int x, int /*y*/)
{
    // V0.3.6：拖选——扩展选中范围 [anchor, caret]
    if (!m_dragging) {
        return;
    }
    m_caret = CharAt(x);
    Invalidate();
}

void UIEdit::OnMouseUp(int x, int y, bool left)
{
    m_dragging = false;
    UIControl::OnMouseUp(x, y, left);
}

size_t UIEdit::CharAt(int x) const
{
    // 按字符均分近似命中（单行文本，够用）
    const int w = Width() > 0 ? Width() : 1;
    const int rel = x - 8; // 文本区左内边距
    if (rel <= 0) {
        return 0;
    }
    return static_cast<size_t>(std::clamp(
        static_cast<int>((static_cast<double>(rel) / w) * m_text.size()),
        0, static_cast<int>(m_text.size())));
}

bool UIEdit::HasSelection() const
{
    return m_selStart != SIZE_MAX && m_selStart != m_caret;
}

void UIEdit::DeleteSelection()
{
    if (!HasSelection()) {
        return;
    }
    const size_t a = (std::min)(m_selStart, m_caret);
    const size_t b = (std::max)(m_selStart, m_caret);
    m_text.erase(a, b - a);
    m_caret = a;
    m_selStart = SIZE_MAX;
    NotifyChanged();
}

void UIEdit::SelectAll()
{
    m_selStart = 0;
    m_caret = m_text.size();
    Invalidate();
}

void UIEdit::Copy()
{
    if (!HasSelection()) {
        return;
    }
    const size_t a = (std::min)(m_selStart, m_caret);
    const size_t b = (std::max)(m_selStart, m_caret);
    const std::wstring sel = m_text.substr(a, b - a);
    const HWND hwnd = m_window != nullptr ? m_window->Hwnd() : nullptr;
    if (hwnd == nullptr || !OpenClipboard(hwnd)) {
        return;
    }
    EmptyClipboard();
    const size_t bytes = (sel.size() + 1) * sizeof(wchar_t);
    if (HGLOBAL hg = GlobalAlloc(GMEM_MOVEABLE, bytes)) {
        if (void* p = GlobalLock(hg)) {
            memcpy(p, sel.c_str(), bytes);
            GlobalUnlock(hg);
            SetClipboardData(CF_UNICODETEXT, hg);
        }
    }
    CloseClipboard();
}

void UIEdit::Paste()
{
    const HWND hwnd = m_window != nullptr ? m_window->Hwnd() : nullptr;
    if (hwnd == nullptr || !IsClipboardFormatAvailable(CF_UNICODETEXT)) {
        return;
    }
    if (!OpenClipboard(hwnd)) {
        return;
    }
    if (HANDLE h = GetClipboardData(CF_UNICODETEXT)) {
        if (wchar_t* p = static_cast<wchar_t*>(GlobalLock(h))) {
            std::wstring ins(p);
            GlobalUnlock(h);
            if (HasSelection()) {
                DeleteSelection();
            }
            if (m_numeric) {
                std::wstring f;
                for (wchar_t c : ins) {
                    if (c >= L'0' && c <= L'9') {
                        f += c;
                    }
                }
                ins = f;
            }
            m_text.insert(m_caret, ins);
            m_caret += ins.size();
            if (m_numeric) {
                ClampNumeric();
            }
            NotifyChanged();
        }
    }
    CloseClipboard();
}

void UIEdit::NotifyChanged()
{
    if (m_numeric) {
        ClampNumeric();
    }
    Invalidate();
    if (m_onChanged) {
        m_onChanged(m_text);
    }
}

void UIEdit::OnKeyDown(int vk, bool ctrl, bool /*shift*/, bool /*alt*/)
{
    if (!IsEnabled()) {
        return;
    }
    // V0.3.6：剪贴板/全选快捷键
    if (ctrl) {
        switch (vk) {
        case 'A': SelectAll(); return;
        case 'C': Copy(); return;
        case 'X': Copy(); DeleteSelection(); return;
        case 'V': Paste(); return;
        default: break;
        }
    }
    // 有选中时：退格/删除/字符替换先删选中
    if (HasSelection() && (vk == VK_BACK || vk == VK_DELETE)) {
        DeleteSelection();
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
        NotifyChanged();
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
    // V0.3.6：有选中 → 输入替换选中段
    if (HasSelection()) {
        DeleteSelection();
    }
    m_text.insert(m_caret, 1, ch);
    ++m_caret;
    NotifyChanged();
}

void UIEdit::OnFocus(bool focused)
{
    // V0.3.7：禁用（只读）框不进焦点态——避免灰色框里闪光标
    if (!IsEnabled() && focused) {
        return;
    }
    UIControl::OnFocus(focused);
    if (!focused) {
        m_dragging = false;
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
