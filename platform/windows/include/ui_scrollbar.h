/// 自研窗体系统 — 滚动条（V0.3.1）
///
/// 细轨道 + 圆角 thumb，拖拽/点击轨道翻页/滚轮。
/// 垂直版（设置页内容区用）；水平版后续需要时扩展。

#pragma once

#include <functional>
#include "ui_control.h"

namespace taishen {

class UIScrollBar : public UIControl
{
public:
    UIScrollBar();

    /// 设置滚动范围：total = 内容总长（px），view = 可视长度（px）
    void SetRange(int total, int view);
    void SetPos(int pos);
    int Pos() const { return m_pos; }
    int MaxPos() const { return m_maxPos; }

    void SetOnScroll(std::function<void(int)> cb) { m_onScroll = std::move(cb); }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnMouseDown(int x, int y, bool left) override;
    void OnMouseUp(int x, int y, bool left) override;
    void OnMouseMove(int x, int y) override;
    void OnMouseWheel(int delta) override;

    int PreferredHeight(int /*width*/) const override { return -1; } // 弹性

private:
    /// thumb 矩形（相对控件）
    RECT ThumbRect() const;
    /// y（相对控件）→ pos
    int PosFromY(int y) const;

    int m_total = 0;
    int m_view = 0;
    int m_pos = 0;
    int m_maxPos = 0;
    bool m_dragging = false;
    int m_dragOffset = 0; // 按下时 thumb 顶与光标差
    std::function<void(int)> m_onScroll;
    static constexpr int kThumbMin = 24;
    static constexpr int kTrackW = 8;
};

} // namespace taishen
