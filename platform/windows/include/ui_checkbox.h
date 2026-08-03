/// 自研窗体系统 — 复选框（V0.3.1）

#pragma once

#include <functional>
#include <string>
#include "ui_control.h"

namespace taishen {

/// 圆角方块 + 勾 + 文字；点击切换状态。
class UICheckBox : public UIControl
{
public:
    explicit UICheckBox(std::wstring text = L"");

    void SetText(const std::wstring& t) { m_text = t; Invalidate(); }
    const std::wstring& Text() const { return m_text; }

    void SetChecked(bool c) { m_checked = c; Invalidate(); }
    bool IsChecked() const { return m_checked; }

    void SetOnChanged(std::function<void(bool)> cb) { m_onChanged = std::move(cb); }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnClick(int x, int y) override;
    void OnKeyDown(int vk, bool ctrl, bool shift, bool alt) override;

private:
    std::wstring m_text;
    bool m_checked = false;
    std::function<void(bool)> m_onChanged;
};

} // namespace taishen
