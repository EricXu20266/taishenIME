/// 自研窗体系统 — 文本标签（V0.3.1）

#pragma once

#include <string>
#include "ui_control.h"

namespace taishen {

/// 只读文本标签（对齐/单行或多行）
class UILabel : public UIControl
{
public:
    enum class Align { Left, Center, Right };

    explicit UILabel(std::wstring text = L"");

    void SetText(const std::wstring& t) { m_text = t; Invalidate(); }
    const std::wstring& Text() const { return m_text; }
    void SetAlign(Align a) { m_align = a; Invalidate(); }
    /// 是否换行（false = 单行，超宽裁剪；true = 自动换行）
    void SetWrap(bool wrap) { m_wrap = wrap; Invalidate(); }
    /// 次要文字颜色（说明文字）
    void SetDim(bool dim) { m_dim = dim; Invalidate(); }
    /// V0.3.6：粗体（卡片分组标题）
    void SetBold(bool bold) { m_bold = bold; Invalidate(); }

    void Draw(UIRenderer& r, const UITheme& t) override;

private:
    std::wstring m_text;
    Align m_align = Align::Left;
    bool m_wrap = false;
    bool m_dim = false;
    bool m_bold = false;
};

} // namespace taishen
