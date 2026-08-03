/// 自研窗体系统 — 单行编辑框（V0.3.1）
///
/// 圆角框 + 光标（常显）+ 键盘编辑（字符/退格/删除/方向键/Home/End）。
/// 数字模式：仅接受数字（设置窗体数值项用）。
/// IME 中文组合输入：0.3.4 设置窗体迁移时接入（TSF 自组合），本期支持英文/数字。

#pragma once

#include <functional>
#include <string>
#include "ui_control.h"

namespace taishen {

class UIEdit : public UIControl
{
public:
    UIEdit();

    void SetText(const std::wstring& t);
    std::wstring Text() const { return m_text; }
    /// 内容变化回调（每次编辑触发）
    void SetOnChanged(std::function<void(const std::wstring&)> cb) { m_onChanged = std::move(cb); }
    /// 占位文本（空内容时 dim 显示）
    void SetPlaceholder(const std::wstring& p) { m_placeholder = p; Invalidate(); }
    /// 数字模式（min/max 含；范围在 OnKeyDown 提交时强制收敛）
    void SetNumeric(int min, int max);

    void Draw(UIRenderer& r, const UITheme& t) override;
    void OnMouseDown(int x, int y, bool left) override;
    void OnKeyDown(int vk, bool ctrl, bool shift, bool alt) override;
    void OnChar(wchar_t ch) override;
    void OnFocus(bool focused) override;

private:
    void ClampNumeric();

    std::wstring m_text;
    size_t m_caret = 0;
    std::wstring m_placeholder;
    bool m_numeric = false;
    int m_min = 0;
    int m_max = INT_MAX;
    std::function<void(const std::wstring&)> m_onChanged;
};

} // namespace taishen
