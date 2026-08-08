/// 自研窗体系统 — 下拉框（V0.3.1）
///
/// 收起态：圆角框 + 当前项 + ▾ 箭头。
/// 展开态：下方弹出面板（子控件列表，可超出自身矩形——命中检测子优先已支持），
/// 点击外部自动收起（OnGlobalMouseDown）。行悬停高亮、选中项 accent 底。

#pragma once

#include <functional>
#include <string>
#include <vector>
#include "ui_control.h"

namespace taishen {

class UIComboBox : public UIControl
{
public:
    UIComboBox();

    void SetItems(const std::vector<std::wstring>& items);
    const std::vector<std::wstring>& Items() const { return m_items; }
    /// 当前选中索引（-1 = 无）
    int SelectedIndex() const { return m_selected; }
    void SetSelectedIndex(int idx);
    std::wstring SelectedText() const;

    /// 选择变化回调
    void SetOnSelected(std::function<void(int)> cb) { m_onSelected = std::move(cb); }

    bool IsExpanded() const { return m_expanded; }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnClick(int x, int y) override;
    void OnGlobalMouseDown(int x, int y) override;
    void OnKeyDown(int vk, bool ctrl, bool shift, bool alt) override;

    int PreferredHeight(int /*width*/) const override { return 28; }

private:
    void Expand();
    void Collapse();
    /// 面板矩形（展开时）
    RECT PanelRect() const;

    std::vector<std::wstring> m_items;
    int m_selected = -1;
    bool m_expanded = false;
    std::function<void(int)> m_onSelected;
    static constexpr int kRowHeight = 30;
    static constexpr int kPanelGap = 2;
};

} // namespace taishen
