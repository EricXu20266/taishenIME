/// 现代化设置窗体 — 公共布局与控件辅助（供 settings_*.cpp 共享）
///
/// 保持各模块源文件简洁清晰，同时不增加 `settings_window.cpp` 行数。

#pragma once

#include <algorithm>
#include <string>

#include "ui_button.h"
#include "ui_checkbox.h"
#include "ui_label.h"
#include "ui_layout.h"

namespace taishen {
namespace settings_ui {

/// 表单行：Label 固定宽 + 控件弹性撑满
class FormRowLayout : public UILayout
{
public:
    explicit FormRowLayout(int labelW)
        : UILayout(UILayout::Dir::H)
        , m_labelW(labelW)
    {
        SetGap(12);
    }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 2) {
            return;
        }
        m_children[0]->SetRect({ x, y, x + m_labelW, y + h });
        m_children[1]->SetRect({ x + m_labelW + m_gap, y, x + w, y + h });
    }

private:
    int m_labelW;
};

/// 表单行辅助：Label + 控件的 HBox
inline UILayout* FormRow(const std::wstring& label, UIControl* control, int labelW = 130)
{
    auto* row = new FormRowLayout(labelW);
    auto* lbl = new UILabel(label);
    row->AddChild(lbl);
    row->AddChild(control);
    return row;
}

/// 按钮内容宽估算（字数 × 15px + 左右内边距）
inline int ButtonWidth(const std::wstring& text)
{
    return static_cast<int>(text.size() * 15) + 32;
}

/// 复选行：默认 toggle 开关
inline UICheckBox* CheckRow(UILayout* page, const std::wstring& text)
{
    auto* chk = new UICheckBox(text);
    chk->SetSwitchMode(true);
    page->AddChild(chk);
    return chk;
}

/// 圆角卡片容器：cardBg 背景 + 可选粗体标题 + 子控件行，高度自适应
class CardLayout : public UILayout
{
public:
    explicit CardLayout(const std::wstring& title = L"")
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(16);
        SetGap(12);
        if (!title.empty()) {
            auto* header = new UILabel(title);
            header->SetBold(true);
            AddChild(header);
        }
    }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()),
                                           static_cast<float>(Y()),
                                           static_cast<float>(X() + Width()),
                                           static_cast<float>(Y() + Height()));
        r.FillRoundedRect(rc, t.cardRadius, t.cardBg);
        UILayout::Draw(r, t);
    }

    int PreferredHeight(int width) const override
    {
        return ContentHeight(width);
    }
};

/// 多行说明文字：左对齐自动换行，高度按行数估算
class HintText : public UILabel
{
public:
    HintText(std::wstring text, int lines)
        : UILabel(std::move(text))
        , m_lines(lines)
    {
        SetWrap(true);
        SetDim(true);
    }

    int PreferredHeight(int /*width*/) const override { return m_lines * 20 + 6; }

private:
    int m_lines;
};

/// 按钮行：水平排列 1~3 个按钮，按文本内容宽度分配
class ActionRowLayout : public UILayout
{
public:
    ActionRowLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(8);
    }

    int PreferredHeight(int /*width*/) const override { return 32; }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.empty()) {
            return;
        }
        int cx = x;
        for (UIControl* c : m_children) {
            if (!c->IsVisible()) {
                continue;
            }
            int cw = 110;
            if (auto* btn = dynamic_cast<UIButton*>(c)) {
                cw = ButtonWidth(btn->Text());
            }
            if (cx + cw > x + w) {
                cw = (std::max)(40, x + w - cx);
            }
            c->SetRect({ cx, y, cx + cw, y + h });
            cx += cw + m_gap;
        }
    }
};

} // namespace settings_ui
} // namespace taishen
