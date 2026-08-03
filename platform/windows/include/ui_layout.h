/// 自研窗体系统 — 布局容器（V0.3.0）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.6
/// 布局也是控件（可嵌套）：VBox 垂直堆叠 / HBox 水平堆叠 / Grid 等宽多列。
/// 子控件通过 AddChild 加入；PreferredHeight() == -1 视为弹性，平分剩余空间。
/// 绝对坐标仍可用（复杂区域直接 SetRect 覆盖布局）。

#pragma once

#include "ui_control.h"

namespace taishen {

/// 布局容器
class UILayout : public UIControl
{
public:
    enum class Dir { V, H, Grid };

    explicit UILayout(Dir dir = Dir::V);

    void SetDir(Dir d) { m_dir = d; Invalidate(); }
    void SetGap(int gap) { m_gap = gap; Invalidate(); }
    void SetPadding(int pad) { m_padding = pad; Invalidate(); }
    /// Grid 列数
    void SetCols(int cols) { m_cols = cols; Invalidate(); }

    /// 重算子控件 Rect（父 Rect 变化/AddChild 后调用）
    void Layout();

    // 布局容器自身不绘制（透明），但必须递归绘制子控件
    void Draw(UIRenderer& r, const UITheme& t) override
    {
        for (UIControl* c : m_children) {
            if (c->IsVisible()) {
                c->Draw(r, t);
            }
        }
    }

    // 布局容器为弹性
    int PreferredHeight(int /*width*/) const override { return -1; }

private:
    void LayoutV();
    void LayoutH();
    void LayoutGrid();

    Dir m_dir;
    int m_gap = 10;
    int m_padding = 0;
    int m_cols = 2;
};

} // namespace taishen
