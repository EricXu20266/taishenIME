/// 自研窗体系统 — 按钮（V0.3.1）

#pragma once

#include <functional>
#include <string>
#include "ui_control.h"

namespace taishen {

/// 圆角矩形按钮：状态底色（普通=cardBg+border / 悬停=hoverBg / 按下=pressedBg），
/// 主按钮 = accent 底 + accentText 文字。
class UIButton : public UIControl
{
public:
    explicit UIButton(std::wstring text = L"");

    void SetText(const std::wstring& t) { m_text = t; Invalidate(); }
    const std::wstring& Text() const { return m_text; }

    /// 主按钮样式（accent 底）
    void SetPrimary(bool p) { m_primary = p; Invalidate(); }
    bool IsPrimary() const { return m_primary; }

    /// 点击回调
    void SetOnClick(std::function<void()> cb) { m_onClick = std::move(cb); }

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnMouseDown(int x, int y, bool left) override;
    void OnMouseUp(int x, int y, bool left) override;
    void OnClick(int x, int y) override;
    void OnKeyDown(int vk, bool ctrl, bool shift, bool alt) override;

private:
    std::wstring m_text;
    bool m_primary = false;
    std::function<void()> m_onClick;
};

} // namespace taishen
