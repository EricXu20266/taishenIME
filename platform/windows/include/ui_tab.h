/// 自研窗体系统 — 标签页（V0.3.1）
///
/// 顶部标签行（选中下划线 accent）+ 内容区（只画当前页）。
/// 页内容作为子控件，由 Tab 维护 rect；内容为布局容器时自动重排。

#pragma once

#include <functional>
#include <string>
#include <vector>
#include "ui_control.h"

namespace taishen {

class UITab : public UIControl
{
public:
    UITab();

    /// 添加一页（title + 内容控件；内容的所有权归调用方，Tab 不释放）
    void AddPage(const std::wstring& title, UIControl* content);
    void SetCurrent(int idx);
    int Current() const { return m_current; }
    size_t PageCount() const { return m_pages.size(); }
    void SetOnChanged(std::function<void(int)> cb) { m_onChanged = std::move(cb); }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnClick(int x, int y) override;
    void SetWindow(UIWindow* w) override;

    /// 标签行高度
    static constexpr int kTabBarHeight = 32;

private:
    struct Page {
        std::wstring title;
        UIControl* content = nullptr;
    };
    std::vector<Page> m_pages;
    int m_current = 0;
    std::function<void(int)> m_onChanged;

    /// 内容区矩形
    RECT ContentRect() const;
    /// 标签命中 → 索引（-1 未命中）
    int TabFromX(int x) const;
};

} // namespace taishen
