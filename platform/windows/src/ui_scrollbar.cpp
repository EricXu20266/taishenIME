/// 自研窗体系统 — 滚动条实现（V0.3.1）

#include "ui_scrollbar.h"
#include <algorithm>
#include <windows.h>

namespace taishen {

UIScrollBar::UIScrollBar()
{
}

void UIScrollBar::SetRange(int total, int view)
{
    m_total = (std::max)(0, total);
    m_view = (std::max)(1, view);
    m_maxPos = (std::max)(0, m_total - m_view);
    SetPos(m_pos);
}

void UIScrollBar::SetPos(int pos)
{
    const int p = std::clamp(pos, 0, m_maxPos);
    if (p != m_pos) {
        m_pos = p;
        Invalidate();
        if (m_onScroll) {
            m_onScroll(m_pos);
        }
    }
}

RECT UIScrollBar::ThumbRect() const
{
    if (m_maxPos <= 0) {
        return { X(), Y(), X() + kTrackW, Y() + Height() };
    }
    const int trackH = Height();
    const int thumbH = (std::max)(kThumbMin,
        trackH * m_view / (std::max)(1, m_total));
    const int maxThumbY = trackH - thumbH;
    const int ty = Y() + (m_maxPos > 0 ? m_pos * maxThumbY / m_maxPos : 0);
    const int tx = X() + (Width() - kTrackW) / 2;
    return { tx, ty, tx + kTrackW, ty + thumbH };
}

int UIScrollBar::PosFromY(int y) const
{
    if (m_maxPos <= 0 || Height() <= 0) {
        return 0;
    }
    const int trackH = Height();
    const int thumbH = (std::max)(kThumbMin,
        trackH * m_view / (std::max)(1, m_total));
    const int maxThumbY = trackH - thumbH;
    const int rel = std::clamp(y - m_dragOffset - Y(), 0, maxThumbY);
    return m_maxPos * rel / (std::max)(1, maxThumbY);
}

void UIScrollBar::Draw(UIRenderer& r, const UITheme& t)
{
    // 轨道
    const float tx = static_cast<float>(X() + (Width() - kTrackW) / 2);
    r.FillRoundedRect(
        D2D1::RectF(tx, static_cast<float>(Y()),
                    tx + static_cast<float>(kTrackW), static_cast<float>(Y() + Height())),
        4.0f, t.border);
    // thumb
    if (m_maxPos > 0) {
        const RECT tr = ThumbRect();
        r.FillRoundedRect(
            D2D1::RectF(static_cast<float>(tr.left), static_cast<float>(tr.top),
                        static_cast<float>(tr.right), static_cast<float>(tr.bottom)),
            4.0f, t.accent);
    }
}

void UIScrollBar::OnMouseDown(int x, int y, bool /*left*/)
{
    const RECT tr = ThumbRect();
    if (x >= tr.left && x < tr.right && y >= tr.top && y < tr.bottom) {
        m_dragging = true;
        m_dragOffset = y - tr.top;
    } else {
        // 点击轨道：翻页（thumb 上方 = 上一页，下方 = 下一页）
        const int delta = (y < tr.top) ? -m_view : m_view;
        SetPos(m_pos + delta);
    }
}

void UIScrollBar::OnMouseUp(int /*x*/, int /*y*/, bool /*left*/)
{
    m_dragging = false;
}

void UIScrollBar::OnMouseMove(int x, int y)
{
    if (m_dragging) {
        SetPos(PosFromY(y));
    }
}

void UIScrollBar::OnMouseWheel(int delta)
{
    const int step = (std::max)(16, m_view / 8);
    SetPos(m_pos - (delta > 0 ? step : -step));
}

} // namespace taishen
