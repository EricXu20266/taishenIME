/// 自研窗体系统 — 颜色选择器（V0.3.1）
///
/// 色块 + HEX 文本；点击展开预设色板（8×16 网格，可超出自身矩形）。
/// 选色后收起并回调 OnColorChanged。

#pragma once

#include <functional>
#include <d2d1.h>
#include "ui_control.h"

namespace taishen {

class UIColorSwatch : public UIControl
{
public:
    UIColorSwatch();

    void SetColor(D2D1_COLOR_F c) { m_color = c; Invalidate(); }
    D2D1_COLOR_F Color() const { return m_color; }
    void SetOnColorChanged(std::function<void(D2D1_COLOR_F)> cb) { m_onColor = std::move(cb); }

    bool IsExpanded() const { return m_expanded; }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnClick(int x, int y) override;
    void OnGlobalMouseDown(int x, int y) override;

    int PreferredHeight(int /*width*/) const override { return 24; }

private:
    void Expand();
    void Collapse();
    RECT PanelRect() const;

    D2D1_COLOR_F m_color = D2D1::ColorF(0xFF4C8DFF);
    bool m_expanded = false;
    std::function<void(D2D1_COLOR_F)> m_onColor;
    static constexpr int kCols = 16;
    static constexpr int kRows = 8;
    static constexpr int kCell = 18;
    static constexpr int kPad = 2;
};

} // namespace taishen
