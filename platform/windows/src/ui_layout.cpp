/// 自研窗体系统 — 布局容器实现（V0.3.0）

#include "ui_layout.h"
#include <algorithm>

namespace taishen {

UILayout::UILayout(Dir dir)
    : m_dir(dir)
{
}

/// 内容总高：padding×2 + 可见子项累计高 + gaps。弹性子项按默认行高估算。
/// （P2-3：默认行高收敛为具名常量，与设置页 FormRow 估算一致）
int UILayout::ContentHeight(int width) const
{
    constexpr int kDefaultRowH = 28;
    const int innerW = width - 2 * m_padding;
    if (innerW <= 0) {
        return 2 * m_padding;
    }
    int total = 2 * m_padding;
    int gapSum = 0;
    int n = 0;
    for (const UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        int h = c->PreferredHeight(innerW);
        total += (h < 0) ? kDefaultRowH : h;
        if (n > 0) {
            gapSum += m_gap;
        }
        ++n;
    }
    return total + gapSum;
}

void UILayout::Layout()
{
    switch (m_dir) {
    case Dir::V:    LayoutV();    break;
    case Dir::H:    LayoutH();    break;
    case Dir::Grid: LayoutGrid(); break;
    }
    // 递归子布局（V0.3.5 审查修复）：子布局的 Layout 依赖自己的 rect，
    // 必须在本层分配完成后对嵌套 UILayout 递归展开——否则深层控件
    // 停留在 {0,0,0,0}（设置窗体 root→content→panel→page→row 深达 4 层）。
    for (UIControl* c : m_children) {
        if (auto* sub = dynamic_cast<UILayout*>(c)) {
            sub->Layout();
        }
    }
}

void UILayout::LayoutV()
{
    const int x = m_rect.left + m_padding;
    const int w = (m_rect.right - m_rect.left) - 2 * m_padding;
    if (w <= 0) {
        return;
    }

    int fixedTotal = 0;
    int flexCount = 0;
    for (UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        const int ph = c->PreferredHeight(w);
        if (ph < 0) {
            ++flexCount;
        } else {
            fixedTotal += ph;
        }
    }
    const int visible = static_cast<int>(std::count_if(
        m_children.begin(), m_children.end(),
        [](UIControl* c) { return c->IsVisible(); }));
    if (visible == 0) {
        return;
    }
    const int gaps = (visible - 1) * m_gap;
    const int avail = (m_rect.bottom - m_rect.top) - 2 * m_padding - gaps;
    const int flexH = flexCount > 0
        ? (std::max)(0, (avail - fixedTotal) / flexCount)
        : 0;

    int y = m_rect.top + m_padding;
    for (UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        int h = c->PreferredHeight(w);
        if (h < 0) {
            h = flexH;
        }
        c->SetRect({ x, y, x + w, y + h });
        y += h + m_gap;
    }
}

void UILayout::LayoutH()
{
    const int y = m_rect.top + m_padding;
    const int h = (m_rect.bottom - m_rect.top) - 2 * m_padding;
    if (h <= 0) {
        return;
    }

    // HBox 中子控件宽度 = PreferredHeight（近似内容宽），弹性者分剩余
    int fixedTotal = 0;
    int flexCount = 0;
    for (UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        const int pw = c->PreferredHeight(h);
        if (pw < 0) {
            ++flexCount;
        } else {
            fixedTotal += pw;
        }
    }
    const int visible = static_cast<int>(std::count_if(
        m_children.begin(), m_children.end(),
        [](UIControl* c) { return c->IsVisible(); }));
    if (visible == 0) {
        return;
    }
    const int gaps = (visible - 1) * m_gap;
    const int avail = (m_rect.right - m_rect.left) - 2 * m_padding - gaps;
    const int flexW = flexCount > 0
        ? (std::max)(0, (avail - fixedTotal) / flexCount)
        : 0;

    int x = m_rect.left + m_padding;
    for (UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        int cw = c->PreferredHeight(h);
        if (cw < 0) {
            cw = flexW;
        }
        c->SetRect({ x, y, x + cw, y + h });
        x += cw + m_gap;
    }
}

void UILayout::LayoutGrid()
{
    const int visible = static_cast<int>(std::count_if(
        m_children.begin(), m_children.end(),
        [](UIControl* c) { return c->IsVisible(); }));
    if (visible == 0 || m_cols <= 0) {
        return;
    }
    const int rows = (visible + m_cols - 1) / m_cols;
    const int contentW = (m_rect.right - m_rect.left) - 2 * m_padding;
    const int contentH = (m_rect.bottom - m_rect.top) - 2 * m_padding;
    const int cellW = (contentW - (m_cols - 1) * m_gap) / m_cols;
    const int cellH = (contentH - (rows - 1) * m_gap) / rows;

    int idx = 0;
    for (UIControl* c : m_children) {
        if (!c->IsVisible()) {
            continue;
        }
        const int row = idx / m_cols;
        const int col = idx % m_cols;
        const int x = m_rect.left + m_padding + col * (cellW + m_gap);
        const int y = m_rect.top + m_padding + row * (cellH + m_gap);
        c->SetRect({ x, y, x + cellW, y + cellH });
        ++idx;
    }
}

} // namespace taishen
