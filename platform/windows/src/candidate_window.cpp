/// Direct2D 候选窗口 — 实现（V0.3.2 迁移：基于自研窗体系统）
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md + docs/modules/ui-framework/SPEC.md
/// 结构：CCandidateWindow（薄封装，接口不变）→ UIWindow（置顶/不抢焦点窗口）
///       → CandidatePanel（自绘内容：圆角背景/拼音/候选/高亮/悬停/翻页指示）。
/// 布局算法与 0.1.x 完全一致（字号比例估算字宽），保证视觉零回归。

#include "candidate_window.h"
#include "debug_log.h"
#include "theme.h"
#include "ui_render.h"

#include <windowsx.h>

namespace taishen {

namespace {

/// UTF-8 → 宽字符串（渲染用）
std::wstring Utf8ToWide(const std::string& utf8)
{
    if (utf8.empty()) {
        return std::wstring();
    }
    const int len = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                        static_cast<int>(utf8.size()),
                                        nullptr, 0);
    if (len <= 0) {
        return std::wstring();
    }
    std::wstring result(static_cast<size_t>(len), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                        static_cast<int>(utf8.size()), &result[0], len);
    return result;
}

/// P0-1：按 label_format 生成候选标签文本（%d / %s 替换为序号数字）。
/// 如 "%d." → "1."；无占位符时若为连续序号字符（如 ①②③…）按索引取第 index 个。
std::wstring FormatLabel(const std::wstring& fmt, int index)
{
    if (fmt.find(L"%d") != std::wstring::npos ||
        fmt.find(L"%s") != std::wstring::npos) {
        const std::wstring num = std::to_wstring(index + 1);
        const wchar_t token = fmt.find(L"%d") != std::wstring::npos ? L'd' : L's';
        std::wstring out = fmt;
        const std::wstring pat = std::wstring(L"%") + token;
        size_t pos = 0;
        while ((pos = out.find(pat, pos)) != std::wstring::npos) {
            out.replace(pos, 2, num);
            pos += num.size();
        }
        return out;
    }
    // 无占位符：若为连续序号字符（如 ①②③…）则按索引取第 index 个
    if (static_cast<size_t>(index) < fmt.size()) {
        return fmt.substr(index, 1);
    }
    return fmt;
}

} // namespace

// ===========================================================================
// 候选内容面板（自绘）
// ===========================================================================
class CCandidateWindow::CandidatePanel : public UIControl
{
public:
    CandidatePanel()
    {
        SetRect({ 0, 0, 200, 60 });
    }

    // ── 数据（CCandidateWindow 转发）──
    void SetData(const std::string& pinyin,
                 const std::vector<std::string>& candidates,
                 int selected, int page, int totalPages)
    {
        m_pinyin = pinyin;
        m_candidates = candidates;
        m_selected = selected;
        m_page = page;
        m_totalPages = totalPages;
        m_hoverIndex = -1;
    }
    void SetTheme(const CandidateTheme& t) { m_theme = t; Invalidate(); }
    void SetFont(const std::wstring& face, float size)
    {
        m_fontFace = face;
        m_fontSize = size;
        Invalidate();
    }
    void SetLayout(float corner, float hilite, int padding, int spacing)
    {
        m_corner = corner;
        m_hilite = hilite;
        m_padding = padding;
        m_spacing = spacing;
        Invalidate();
    }
    void SetMultiRow(bool b) { m_multiRow = b; Invalidate(); }
    void SetInlinePreedit(bool b) { m_inlinePreedit = b; Invalidate(); }
    void SetLabelFormat(const std::wstring& fmt) { m_labelFormat = fmt; Invalidate(); }
    void SetClickCallback(ClickCallback cb) { m_clickCb = std::move(cb); }
    void SetPageCallback(PageCallback cb) { m_pageCb = std::move(cb); }

    // ── 尺寸（与 0.1.x CalculateSize 一致）──
    /// 单列宽度（V0.3.x2 clamp）：超长词（专名/长句）不撑爆窗口——上限 ≈ 8 字
    static int ItemWidth(const std::wstring& label, const std::wstring& word, float scale)
    {
        const int w = static_cast<int>((label.size() * 16 + word.size() * 16 + 20) * scale);
        return (std::min)(w, kMaxItemWidth);
    }
    void CalculateSize(int& width, int& height)
    {
        const float scale = DpiScale();
        const int pad = static_cast<int>(m_padding * scale);
        const float fontScale = m_fontSize / 16.0f;
        const int pinyinH = static_cast<int>(18 * fontScale * scale);
        const int candH = static_cast<int>(22 * fontScale * scale);

        width = pad * 2;
        height = pad * 2;

        const bool hasPinyin = !m_pinyin.empty() && !m_inlinePreedit;
        if (hasPinyin) {
            height += pinyinH;
        }
        if (!m_candidates.empty()) {
            if (m_multiRow && m_candidates.size() > static_cast<size_t>(kPerRow)) {
                const size_t rows = (m_candidates.size() + kPerRow - 1) / kPerRow;
                height += static_cast<int>(rows * candH);
            } else {
                height += candH;
            }
        }

        int contentWidth = 0;
        if (hasPinyin) {
            contentWidth += static_cast<int>(m_pinyin.size() * 14 * scale);
        }
        const size_t widthCount = m_multiRow
            ? (m_candidates.size() < static_cast<size_t>(kPerRow)
                   ? m_candidates.size() : static_cast<size_t>(kPerRow))
            : m_candidates.size();
        // V0.3.x：多行模式每列等宽（max 项宽）——修复 ↓ 展开后
        // 窗口宽度按实际文本、绘制按固定 96px 列宽的错位（问题 1：一列半/裁切）
        // V0.3.x2：列宽 clamp 到 kMaxItemWidth——超长词（专名/长句）不撑爆窗口
        int maxItemWidth = 0;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            const std::wstring word = Utf8ToWide(m_candidates[i]);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const int itemWidth = ItemWidth(label, word, scale);
            if (itemWidth > maxItemWidth) {
                maxItemWidth = itemWidth;
            }
        }
        if (m_multiRow) {
            // 多行：列数 × 等宽 + 间距
            contentWidth = static_cast<int>(
                widthCount * (maxItemWidth + m_spacing * scale) - m_spacing * scale);
        } else {
            // V0.4：单行等宽列（对标搜狗/微软横排）——每列取 max 项宽，
            // 5 个候选等宽对齐，窗口宽度稳定（此前按词宽累加，长短不一）。
            contentWidth = static_cast<int>(
                widthCount * (maxItemWidth + m_spacing * scale) - m_spacing * scale);
        }
        width += contentWidth > 0
            ? contentWidth : static_cast<int>(60 * scale);
        if (width > 800) {
            width = 800;
        }
    }

    // ── 命中（与 0.1.x HitTest 一致）──
    int CandidateAt(int x, int y) const
    {
        if (m_candidates.empty()) {
            return -1;
        }
        const float scale = DpiScale();
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding) * scale;
        const float pinyinH = 18.0f * fontScale * scale;
        const float candH = 22.0f * fontScale * scale;
        const bool hasPinyin = !m_pinyin.empty() && !m_inlinePreedit;

        if (m_multiRow) {
            // V0.3.x：列宽与 Draw/CalculateSize 一致（每列等宽 max 项宽）
            float maxItemW = 0.0f;
            for (size_t i = 0; i < m_candidates.size(); ++i) {
                const std::wstring word = Utf8ToWide(m_candidates[i]);
                const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
                const float itemW = static_cast<float>(ItemWidth(label, word, scale));
                if (itemW > maxItemW) {
                    maxItemW = itemW;
                }
            }
            const int row = static_cast<int>(
                (static_cast<float>(y) - padF - (hasPinyin ? pinyinH : 0.0f)) / candH);
            if (row < 0) {
                return -1;
            }
            const float colW = maxItemW + static_cast<float>(m_spacing) * scale;
            const int col = static_cast<int>(
                (static_cast<float>(x) - padF) / colW);
            const int index = row * kPerRow + col;
            return (index >= 0 && index < static_cast<int>(m_candidates.size()))
                ? index : -1;
        }
        // V0.4：单行等宽列（与 Draw/CalculateSize 一致）——每列 max 项宽
        {
            float maxItemW = 0.0f;
            for (size_t i = 0; i < m_candidates.size(); ++i) {
                const std::wstring word = Utf8ToWide(m_candidates[i]);
                const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
                const float itemW = static_cast<float>(ItemWidth(label, word, scale));
                if (itemW > maxItemW) {
                    maxItemW = itemW;
                }
            }
            const float colW = maxItemW + static_cast<float>(m_spacing) * scale;
            const int col = static_cast<int>((static_cast<float>(x) - padF) / colW);
            if (col >= 0 && col < static_cast<int>(m_candidates.size())) {
                return col;
            }
            return -1;
        }
    }

    // ── 绘制（与 0.1.x Render 内容一致）──
    void Draw(UIRenderer& r, const UITheme& /*t*/) override
    {
        const float scale = DpiScale();
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding) * scale;
        const float pinyinH = 18.0f * fontScale * scale;
        const float candH = 22.0f * fontScale * scale;

        const D2D1_RECT_F rc = D2D1::RectF(
            0.5f, 0.5f,
            static_cast<float>(Width()) - 0.5f,
            static_cast<float>(Height()) - 0.5f);

        // V0.3.x：去掉背景填充（问题 4：选词窗口背景去掉），仅保留边框
        r.DrawRoundedRect(rc, m_corner * scale, m_theme.border, 1.0f);

        // 等宽列（单行/多行一致）：每列 max 项宽（与 CalculateSize 一致，问题 1 修复）
        // V0.4.1 fix：单行模式也必须计算 maxItemW（此前仅多行计算→单行 itemW=0→候选挤在一起）
        float maxItemW = 0.0f;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            const std::wstring word = Utf8ToWide(m_candidates[i]);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const float itemW = static_cast<float>(ItemWidth(label, word, scale));
            if (itemW > maxItemW) {
                maxItemW = itemW;
            }
        }

        float y = padF;
        // 拼音串（行内预编辑时不绘制）
        if (!m_pinyin.empty() && !m_inlinePreedit) {
            r.DrawText(Utf8ToWide(m_pinyin),
                       D2D1::RectF(padF, y, static_cast<float>(Width()) - padF, y + pinyinH),
                       m_fontSize, m_theme.dim);
            y += pinyinH;
        }

        // 候选词（单行水平 / 多行网格）
        float x = padF;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            if (m_multiRow) {
                const int row = static_cast<int>(i) / kPerRow;
                const int col = static_cast<int>(i) % kPerRow;
                x = padF + static_cast<float>(col) * (maxItemW + m_spacing * scale);
                const bool hasPinyinRow = !m_pinyin.empty() && !m_inlinePreedit;
                y = padF + static_cast<float>(hasPinyinRow ? pinyinH : 0.0f) +
                    static_cast<float>(row) * candH;
            }
            const std::wstring word = Utf8ToWide(m_candidates[i]);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const bool selected = (static_cast<int>(i) == m_selected);
            // V0.4：单行也等宽列（与 CalculateSize/CandidateAt 一致）
            const float itemW = maxItemW;

            // 高亮背景（选中/悬停）
            if (selected) {
                r.FillRoundedRect(
                    D2D1::RectF(x - 2.0f, y, x + itemW, y + candH),
                    m_hilite * scale, m_theme.highlight_bg);
            } else if (static_cast<int>(i) == m_hoverIndex && m_hoverIndex >= 0) {
                r.FillRoundedRect(
                    D2D1::RectF(x - 2.0f, y, x + itemW, y + candH),
                    m_hilite * scale, m_theme.mark);
            }

            // 序号 + 正文（分色）
            r.DrawText(label,
                       D2D1::RectF(x, y, x + static_cast<float>(label.size() * 16) * scale, y + candH),
                       m_fontSize,
                       selected ? m_theme.highlight_label : m_theme.label);
            r.DrawText(word,
                       D2D1::RectF(x + static_cast<float>(label.size() * 16) * scale, y,
                                   x + itemW, y + candH),
                       m_fontSize,
                       selected ? m_theme.highlight_text : m_theme.text);
            if (!m_multiRow) {
                x += itemW + static_cast<float>(m_spacing) * scale;
            }
        }
        // V0.3.x：翻页指示 "1/3" 已移除（问题 3：xx/xxx 计数不需要）
    }

    // ── 交互 ──
    void OnMouseMove(int x, int y) override
    {
        const int hit = CandidateAt(x, y);
        if (hit != m_hoverIndex) {
            m_hoverIndex = hit;
            Invalidate();
        }
    }
    void OnMouseLeave() override
    {
        if (m_hoverIndex != -1) {
            m_hoverIndex = -1;
            Invalidate();
        }
    }
    void OnClick(int x, int y) override
    {
        if (m_clickCb) {
            const int index = CandidateAt(x, y);
            if (index >= 0) {
                m_clickCb(index);
            }
        }
    }
    void OnMouseWheel(int delta) override
    {
        if (m_pageCb) {
            m_pageCb(delta);
        }
    }

private:
    float DpiScale() const
    {
        const HWND hwnd = (Window() != nullptr) ? Window()->Hwnd() : nullptr;
        const UINT dpi = GetDpiForWindow(hwnd);
        return dpi > 0 ? static_cast<float>(dpi) / 96.0f : 1.0f;
    }

    // 数据（渲染/命中/尺寸）
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    int m_selected = 0;
    int m_page = 0;
    int m_totalPages = 0;
    bool m_multiRow = false;
    bool m_inlinePreedit = true;
    int m_hoverIndex = -1;
    std::wstring m_labelFormat = L"%d.";
    float m_corner = 4.0f;
    float m_hilite = 3.0f;
    int m_padding = 8;
    int m_spacing = 14;
    std::wstring m_fontFace = L"Microsoft YaHei";
    float m_fontSize = 16.0f;
    CandidateTheme m_theme;
    ClickCallback m_clickCb;
    PageCallback m_pageCb;
    static constexpr int kPerRow = 5;
    /// 单列最大宽度（V0.3.x2）：≈ 8 字词宽（label 2字 + 正文 8字 + padding）
    static constexpr int kMaxItemWidth = 146;
};

// ===========================================================================
// CCandidateWindow（薄封装）
// ===========================================================================
CCandidateWindow::CCandidateWindow()
    : m_panel(new CandidatePanel())
{
}

CCandidateWindow::~CCandidateWindow()
{
    m_window.Destroy();
    delete m_panel;
}

bool CCandidateWindow::Initialize()
{
    if (m_window.Hwnd() != nullptr) {
        return true;
    }
    if (!m_window.Create(L"TaishenCandidateWindow", 200, 60, true, true)) {
        taishen::DebugLog("CandidateWindow: window create failed");
        return false;
    }
    m_window.SetRoot(m_panel);
    taishen::DebugLog("CandidateWindow: Initialize OK hwnd=" +
                      std::to_string(reinterpret_cast<long long>(m_window.Hwnd())));
    return true;
}

void CCandidateWindow::PositionWindow(const RECT& caretRect)
{
    int width = 0;
    int height = 0;
    m_panel->CalculateSize(width, height);

    int x = caretRect.left;
    int y = caretRect.bottom + 4;
    const int screenW = GetSystemMetrics(SM_CXSCREEN);
    const int screenH = GetSystemMetrics(SM_CYSCREEN);
    if (x + width > screenW) {
        x = screenW - width;
    }
    if (y + height > screenH) {
        y = caretRect.top - height - 4;
        if (y < 0) {
            y = 0;
        }
    }
    if (x < 0) {
        x = 0;
    }
    SetWindowPos(m_window.Hwnd(), HWND_TOPMOST, x, y, width, height,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
}

void CCandidateWindow::UpdateState(const std::string& pinyin,
                                   const std::vector<std::string>& candidates,
                                   const RECT& caretRect,
                                   int page,
                                   int totalPages)
{
    m_pinyin = pinyin;
    m_candidates = candidates;
    m_page = page;
    m_totalPages = totalPages;

    if (m_pinyin.empty() || m_candidates.empty()) {
        taishen::DebugLog("CandidateWindow: UpdateState HIDE (pinyin=" +
                          std::to_string(m_pinyin.size()) + " cands=" +
                          std::to_string(m_candidates.size()) + ")");
        Hide();
        return;
    }

    if (m_window.Hwnd() == nullptr) {
        Initialize();
    }
    m_panel->SetData(m_pinyin, m_candidates, m_selectedIndex, m_page, m_totalPages);
    PositionWindow(caretRect);
    m_visible = true;
    // 主动渲染（0.1.15 修复）：不依赖 WM_PAINT 消息循环时序
    m_window.Invalidate();
    UpdateWindow(m_window.Hwnd());
}

void CCandidateWindow::Hide()
{
    if (!m_visible) {
        return;
    }
    m_visible = false;
    if (m_window.Hwnd() != nullptr) {
        ShowWindow(m_window.Hwnd(), SW_HIDE);
    }
}

void CCandidateWindow::SetSelectedIndex(int index)
{
    m_selectedIndex = index;
    m_panel->SetData(m_pinyin, m_candidates, m_selectedIndex, m_page, m_totalPages);
    if (m_visible) {
        m_window.Invalidate();
    }
}

void CCandidateWindow::SetClickCallback(ClickCallback cb)
{
    m_clickCb = std::move(cb);
    m_panel->SetClickCallback(m_clickCb);
}

void CCandidateWindow::SetPageCallback(PageCallback cb)
{
    m_pageCb = std::move(cb);
    m_panel->SetPageCallback(m_pageCb);
}

void CCandidateWindow::SetLabelFormat(const std::wstring& fmt)
{
    if (fmt != m_labelFormat) {
        m_labelFormat = fmt;
        m_panel->SetLabelFormat(fmt);
        if (m_visible) {
            RECT caret = {};
            GetWindowRect(m_window.Hwnd(), &caret);
            PositionWindow(caret);
            m_window.Invalidate();
        }
    }
}

void CCandidateWindow::SetLayout(float cornerRadius, float hiliteRadius,
                                 int padding, int spacing)
{
    m_cornerRadius = cornerRadius;
    m_hiliteRadius = hiliteRadius;
    m_padding = padding;
    m_spacing = spacing;
    m_panel->SetLayout(cornerRadius, hiliteRadius, padding, spacing);
    if (m_visible) {
        RECT caret = {};
        GetWindowRect(m_window.Hwnd(), &caret);
        PositionWindow(caret);
        m_window.Invalidate();
    }
}

void CCandidateWindow::SetTheme(const CandidateTheme& theme)
{
    m_theme = theme;
    m_followSystemTheme = false; // 显式设置主题 → 不再跟随系统（V0.2.20）
    m_panel->SetTheme(theme);
    m_window.Invalidate();
}

void CCandidateWindow::SetMultiRow(bool enabled)
{
    if (m_multiRow != enabled) {
        m_multiRow = enabled;
        m_panel->SetMultiRow(enabled);
        if (m_visible) {
            RECT caret = {};
            GetWindowRect(m_window.Hwnd(), &caret);
            PositionWindow(caret);
            m_window.Invalidate();
        }
    }
}

void CCandidateWindow::SetFont(const std::wstring& face, float size)
{
    if (!face.empty()) {
        m_fontFace = face;
    }
    if (size >= 12.0f && size <= 32.0f) {
        m_fontSize = size;
    }
    m_panel->SetFont(m_fontFace, m_fontSize);
    if (m_visible) {
        RECT caret = {};
        GetWindowRect(m_window.Hwnd(), &caret);
        PositionWindow(caret);
        m_window.Invalidate();
    }
}

void CCandidateWindow::OnSystemThemeChanged()
{
    if (!m_followSystemTheme) {
        return; // 用户显式配置主题 → 不跟随
    }
    // 跟随模式：按系统模式取默认主题（V0.2.20）
    const int mode = GetSystemAppTheme();
    SetTheme(mode == 1 ? LightTheme() : CandidateTheme::Default());
    m_followSystemTheme = true; // SetTheme 内部会置 false，这里恢复跟随语义
}

void CCandidateWindow::SetFollowSystemTheme(bool follow)
{
    m_followSystemTheme = follow;
}

void CCandidateWindow::SetInlinePreedit(bool enable)
{
    if (m_inlinePreedit != enable) {
        m_inlinePreedit = enable;
        m_panel->SetInlinePreedit(enable);
        if (m_visible) {
            RECT caret = {};
            GetWindowRect(m_window.Hwnd(), &caret);
            PositionWindow(caret);
            m_window.Invalidate();
        }
    }
}

} // namespace taishen

