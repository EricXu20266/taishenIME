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

#include <algorithm>
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

/// P2-5：拆分候选正文与注释（支持 `\t` 分隔，如 "参差\tcenci"）
void SplitCandidateComment(const std::wstring& raw,
                           std::wstring& word,
                           std::wstring& comment)
{
    const size_t tab = raw.find(L'\t');
    if (tab != std::wstring::npos) {
        word = raw.substr(0, tab);
        comment = raw.substr(tab + 1);
    } else {
        word = raw;
        comment.clear();
    }
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
        m_pageHover = 0;
    }
    void SetCursorPos(int pos)
    {
        if (m_cursorPos != pos) {
            m_cursorPos = pos;
            Invalidate();
        }
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
    void SetRightClickCallback(RightClickCallback cb) { m_rightClickCb = std::move(cb); }
    void SetPageCallback(PageCallback cb) { m_pageCb = std::move(cb); }

    // ── 尺寸（DIP 逻辑坐标系，使用 DirectWrite 精确测量，随字号与 DPI 等比缩放）──
    float MeasureTextWidth(const std::wstring& text, float fontSize) const
    {
        if (text.empty()) {
            return 0.0f;
        }
        if (Window() != nullptr) {
            return Window()->Renderer().MeasureText(text, fontSize, false, m_fontFace).width;
        }
        return static_cast<float>(text.size()) * fontSize * 0.85f;
    }

    float MaxItemWidthDip() const
    {
        return static_cast<float>(kMaxItemWidth) * (m_fontSize / 16.0f);
    }

    float LabelWidthDip(const std::wstring& label) const
    {
        const float labelFont = (std::max)(10.0f, m_fontSize - 2.0f);
        return MeasureTextWidth(label, labelFont) + 4.0f;
    }

    float ItemWidthDip(const std::wstring& label, const std::wstring& raw) const
    {
        std::wstring word, comment;
        SplitCandidateComment(raw, word, comment);
        const float labelW = LabelWidthDip(label);
        const float wordW = MeasureTextWidth(word, m_fontSize);
        const float commentFont = (std::max)(10.0f, m_fontSize - 3.0f);
        const float commentW = comment.empty()
            ? 0.0f
            : (MeasureTextWidth(comment, commentFont) + 6.0f);
        const float w = labelW + wordW + commentW + 10.0f;
        return (std::min)(w, MaxItemWidthDip());
    }

    bool HasPinyinRow() const
    {
        if (m_pinyin.empty()) {
            return false;
        }
        if (!m_inlinePreedit) {
            return true;
        }
        // S2-4：行内预编辑时，光标移回拼音串中部/开头则在候选窗展开拼音行与光标线
        return m_cursorPos >= 0 && m_cursorPos < static_cast<int>(m_pinyin.size());
    }

    struct PageIndicatorLayout {
        bool visible = false;
        std::wstring midText;
        D2D1_RECT_F prevRc{};
        D2D1_RECT_F midRc{};
        D2D1_RECT_F nextRc{};
        float totalW = 0.0f;
    };

    std::wstring FormatPageNumberText() const
    {
        const int cur = (std::max)(1, (std::min)(m_page + 1, m_totalPages));
        return std::to_wstring(cur) + L"/" + std::to_wstring(m_totalPages);
    }

    float PageIndicatorWidthDip() const
    {
        if (m_totalPages <= 1 || m_candidates.empty()) {
            return 0.0f;
        }
        const float labelFont = (std::max)(10.0f, m_fontSize - 2.0f);
        const float arrowW = (std::max)(14.0f, MeasureTextWidth(L"‹", labelFont) + 6.0f);
        const float midW = MeasureTextWidth(FormatPageNumberText(), labelFont) + 6.0f;
        return arrowW * 2.0f + midW;
    }

    PageIndicatorLayout GetPageIndicatorLayout() const
    {
        PageIndicatorLayout out;
        if (m_totalPages <= 1 || m_candidates.empty()) {
            return out;
        }
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding);
        const float pinyinH = 18.0f * fontScale;
        const float candH = 24.0f * fontScale;
        const float labelFont = (std::max)(10.0f, m_fontSize - 2.0f);
        const float arrowW = (std::max)(14.0f, MeasureTextWidth(L"‹", labelFont) + 6.0f);
        out.midText = FormatPageNumberText();
        const float midW = MeasureTextWidth(out.midText, labelFont) + 6.0f;
        out.totalW = arrowW * 2.0f + midW;
        out.visible = true;

        const float rightX = static_cast<float>(Width()) - padF;
        const float leftX = rightX - out.totalW;
        const float rowH = m_multiRow ? pinyinH : candH;
        const float topY = m_multiRow
            ? (static_cast<float>(Height()) - padF - rowH)
            : (padF + (HasPinyinRow() ? pinyinH : 0.0f));
        const float botY = topY + rowH;

        out.prevRc = D2D1::RectF(leftX, topY, leftX + arrowW, botY);
        out.midRc = D2D1::RectF(leftX + arrowW, topY, leftX + arrowW + midW, botY);
        out.nextRc = D2D1::RectF(leftX + arrowW + midW, topY, rightX, botY);
        return out;
    }

    /// 返回页码按钮命中：-1=上一页 ‹，+1=下一页 ›，0=未命中页码区
    int HitPageButton(int x, int y) const
    {
        const PageIndicatorLayout pg = GetPageIndicatorLayout();
        if (!pg.visible) {
            return 0;
        }
        const float fx = static_cast<float>(x);
        const float fy = static_cast<float>(y);
        auto inRect = [fx, fy](const D2D1_RECT_F& rc) {
            return fx >= rc.left && fx < rc.right && fy >= rc.top && fy < rc.bottom;
        };
        if (inRect(pg.prevRc)) {
            return -1;
        }
        if (inRect(pg.nextRc)) {
            return 1;
        }
        return 0;
    }

    void CalculateSize(int& width, int& height)
    {
        const float scale = DpiScale();
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding);
        const float pinyinH = 18.0f * fontScale;
        const float candH = 24.0f * fontScale;

        float dipW = padF * 2.0f;
        float dipH = padF * 2.0f;

        const bool hasPinyin = HasPinyinRow();
        const float pageW = PageIndicatorWidthDip();
        if (hasPinyin) {
            dipH += pinyinH;
        }
        if (!m_candidates.empty()) {
            if (m_multiRow && m_candidates.size() > static_cast<size_t>(kPerRow)) {
                const size_t rows = (m_candidates.size() + kPerRow - 1) / kPerRow;
                dipH += static_cast<float>(rows) * candH;
            } else {
                dipH += candH;
            }
            if (m_multiRow && pageW > 0.0f) {
                dipH += pinyinH;
            }
        }

        float pinyinW = 0.0f;
        if (hasPinyin) {
            const float pinyinFont = (std::max)(10.0f, m_fontSize - 2.0f);
            pinyinW = MeasureTextWidth(Utf8ToWide(m_pinyin), pinyinFont) + 6.0f;
        }

        const size_t widthCount = m_multiRow
            ? (m_candidates.size() < static_cast<size_t>(kPerRow)
                   ? m_candidates.size() : static_cast<size_t>(kPerRow))
            : m_candidates.size();

        float maxItemWidth = 0.0f;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            const std::wstring raw = Utf8ToWide(m_candidates[i]);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const float itemW = ItemWidthDip(label, raw);
            if (itemW > maxItemWidth) {
                maxItemWidth = itemW;
            }
        }

        float contentWidth = 0.0f;
        if (m_multiRow && widthCount > 0) {
            contentWidth = static_cast<float>(widthCount) *
                               (maxItemWidth + static_cast<float>(m_spacing)) -
                           static_cast<float>(m_spacing);
            if (pageW > 0.0f) {
                contentWidth = (std::max)(contentWidth, pageW);
            }
        } else if (!m_candidates.empty()) {
            for (size_t i = 0; i < m_candidates.size(); ++i) {
                const std::wstring raw = Utf8ToWide(m_candidates[i]);
                const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
                contentWidth += ItemWidthDip(label, raw);
            }
            contentWidth += static_cast<float>(m_candidates.size() - 1) *
                            static_cast<float>(m_spacing);
            if (pageW > 0.0f) {
                contentWidth += 8.0f + pageW;
            }
        }
        contentWidth = (std::max)(contentWidth, pinyinW);
        dipW += contentWidth > 0.0f ? contentWidth : 60.0f;
        const float maxWinDip = 800.0f * fontScale;
        if (dipW > maxWinDip) {
            dipW = maxWinDip;
        }
        width = static_cast<int>(dipW * scale + 0.5f);
        height = static_cast<int>(dipH * scale + 0.5f);
    }

    // ── 命中（DIP 逻辑坐标，与 Draw / CalculateSize 一致）──
    int CandidateAt(int x, int y) const
    {
        if (m_candidates.empty()) {
            return -1;
        }
        if (HitPageButton(x, y) != 0) {
            return -1;
        }
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding);
        const float pinyinH = 18.0f * fontScale;
        const float candH = 24.0f * fontScale;
        const bool hasPinyin = HasPinyinRow();

        if (m_multiRow) {
            float maxItemW = 0.0f;
            for (size_t i = 0; i < m_candidates.size(); ++i) {
                const std::wstring raw = Utf8ToWide(m_candidates[i]);
                const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
                const float itemW = ItemWidthDip(label, raw);
                if (itemW > maxItemW) {
                    maxItemW = itemW;
                }
            }
            const int row = static_cast<int>(
                (static_cast<float>(y) - padF - (hasPinyin ? pinyinH : 0.0f)) / candH);
            if (row < 0) {
                return -1;
            }
            const float colW = maxItemW + static_cast<float>(m_spacing);
            const int col = static_cast<int>(
                (static_cast<float>(x) - padF) / colW);
            if (col < 0 || col >= kPerRow) {
                return -1;
            }
            const int index = row * kPerRow + col;
            return (index >= 0 && index < static_cast<int>(m_candidates.size()))
                ? index : -1;
        }
        {
            const float y0 = padF + (hasPinyin ? pinyinH : 0.0f);
            if (static_cast<float>(y) < y0 || static_cast<float>(y) >= y0 + candH) {
                return -1;
            }
            float colX = padF;
            for (size_t i = 0; i < m_candidates.size(); ++i) {
                const std::wstring raw = Utf8ToWide(m_candidates[i]);
                const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
                const float itemW = ItemWidthDip(label, raw);
                if (static_cast<float>(x) >= colX && static_cast<float>(x) < colX + itemW) {
                    return static_cast<int>(i);
                }
                colX += itemW + static_cast<float>(m_spacing);
            }
            return -1;
        }
    }

    // ── 绘制（DIP 逻辑坐标系，传递 m_fontFace + 支持 comment 独立渲染）──
    void Draw(UIRenderer& r, const UITheme& /*t*/) override
    {
        const float fontScale = m_fontSize / 16.0f;
        const float padF = static_cast<float>(m_padding);
        const float pinyinH = 18.0f * fontScale;
        const float candH = 24.0f * fontScale;
        const float labelFont = (std::max)(10.0f, m_fontSize - 2.0f);
        const float commentFont = (std::max)(10.0f, m_fontSize - 3.0f);

        const D2D1_RECT_F rc = D2D1::RectF(
            0.5f, 0.5f,
            static_cast<float>(Width()) - 0.5f,
            static_cast<float>(Height()) - 0.5f);

        r.FillRoundedRect(rc, m_corner, m_theme.bg);
        r.DrawRoundedRect(rc, m_corner, m_theme.border, 1.0f);

        float maxItemW = 0.0f;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            const std::wstring raw = Utf8ToWide(m_candidates[i]);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const float itemW = ItemWidthDip(label, raw);
            if (itemW > maxItemW) {
                maxItemW = itemW;
            }
        }

        float y = padF;
        const bool hasPinyin = HasPinyinRow();
        if (hasPinyin) {
            const std::wstring wPinyin = Utf8ToWide(m_pinyin);
            r.DrawText(wPinyin,
                       D2D1::RectF(padF, y, static_cast<float>(Width()) - padF, y + pinyinH),
                       labelFont, m_theme.dim, false,
                       DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                       true, m_fontFace);
            if (m_cursorPos >= 0) {
                const size_t clamped =
                    (std::min)(static_cast<size_t>(m_cursorPos), wPinyin.size());
                const float prefixW = MeasureTextWidth(wPinyin.substr(0, clamped), labelFont);
                const float cx = padF + prefixW;
                r.DrawLine(cx, y + 2.0f, cx, y + pinyinH - 2.0f, m_theme.highlight_bg, 1.5f);
            }
            y += pinyinH;
        }

        float x = padF;
        for (size_t i = 0; i < m_candidates.size(); ++i) {
            if (m_multiRow) {
                const int row = static_cast<int>(i) / kPerRow;
                const int col = static_cast<int>(i) % kPerRow;
                x = padF + static_cast<float>(col) * (maxItemW + static_cast<float>(m_spacing));
                y = padF + (hasPinyin ? pinyinH : 0.0f) +
                    static_cast<float>(row) * candH;
            }
            const std::wstring raw = Utf8ToWide(m_candidates[i]);
            std::wstring word, comment;
            SplitCandidateComment(raw, word, comment);
            const std::wstring label = FormatLabel(m_labelFormat, static_cast<int>(i));
            const bool selected = (static_cast<int>(i) == m_selected);
            const float itemW = m_multiRow ? maxItemW : ItemWidthDip(label, raw);

            if (selected) {
                r.FillRoundedRect(
                    D2D1::RectF(x - 2.0f, y, x + itemW, y + candH),
                    m_hilite, m_theme.highlight_bg);
            } else if (static_cast<int>(i) == m_hoverIndex && m_hoverIndex >= 0) {
                r.FillRoundedRect(
                    D2D1::RectF(x - 2.0f, y, x + itemW, y + candH),
                    m_hilite, m_theme.mark);
            }

            const float labelW = LabelWidthDip(label);
            r.DrawText(label,
                       D2D1::RectF(x + 2.0f, y, x + 2.0f + labelW, y + candH),
                       labelFont,
                       selected ? m_theme.highlight_label : m_theme.label,
                       false, DWRITE_TEXT_ALIGNMENT_LEADING,
                       DWRITE_PARAGRAPH_ALIGNMENT_CENTER, true, m_fontFace);

            const float wordX = x + 2.0f + labelW;
            const float wordW = MeasureTextWidth(word, m_fontSize);
            const float wordRight = comment.empty()
                ? (x + itemW)
                : (std::min)(wordX + wordW + 2.0f, x + itemW);
            r.DrawText(word,
                       D2D1::RectF(wordX, y, wordRight, y + candH),
                       m_fontSize,
                       selected ? m_theme.highlight_text : m_theme.text,
                       false, DWRITE_TEXT_ALIGNMENT_LEADING,
                       DWRITE_PARAGRAPH_ALIGNMENT_CENTER, true, m_fontFace);

            if (!comment.empty() && wordRight + 4.0f < x + itemW) {
                r.DrawText(comment,
                           D2D1::RectF(wordRight + 4.0f, y, x + itemW, y + candH),
                           commentFont,
                           selected ? m_theme.highlight_label : m_theme.comment,
                           false, DWRITE_TEXT_ALIGNMENT_LEADING,
                           DWRITE_PARAGRAPH_ALIGNMENT_CENTER, true, m_fontFace);
            }
            if (!m_multiRow) {
                x += itemW + static_cast<float>(m_spacing);
            }
        }

        // S2-6 翻页指示：总页数 > 1 时渲染 `‹ N/M ›`，支持点击翻页
        const PageIndicatorLayout pg = GetPageIndicatorLayout();
        if (pg.visible) {
            const bool canPrev = (m_page > 0);
            const bool canNext = (m_page + 1 < m_totalPages);
            const D2D1_COLOR_F prevColor = (m_pageHover == -1 && canPrev)
                ? m_theme.highlight_bg
                : (canPrev ? m_theme.label : m_theme.comment);
            const D2D1_COLOR_F nextColor = (m_pageHover == 1 && canNext)
                ? m_theme.highlight_bg
                : (canNext ? m_theme.label : m_theme.comment);
            r.DrawText(L"‹", pg.prevRc, labelFont, prevColor, false,
                       DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                       true, m_fontFace);
            r.DrawText(pg.midText, pg.midRc, labelFont, m_theme.dim, false,
                       DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                       true, m_fontFace);
            r.DrawText(L"›", pg.nextRc, labelFont, nextColor, false,
                       DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                       true, m_fontFace);
        }
    }

    // ── 交互 ──
    void OnMouseMove(int x, int y) override
    {
        const int pgHit = HitPageButton(x, y);
        const int hit = (pgHit == 0) ? CandidateAt(x, y) : -1;
        if (hit != m_hoverIndex || pgHit != m_pageHover) {
            m_hoverIndex = hit;
            m_pageHover = pgHit;
            Invalidate();
        }
    }
    void OnMouseLeave() override
    {
        if (m_hoverIndex != -1 || m_pageHover != 0) {
            m_hoverIndex = -1;
            m_pageHover = 0;
            Invalidate();
        }
    }
    void OnClick(int x, int y) override
    {
        const int pgHit = HitPageButton(x, y);
        if (pgHit != 0) {
            if (m_pageCb) {
                // delta > 0 上一页（‹），< 0 下一页（›），与滚轮翻页及 server SendInput 约定一致
                m_pageCb(pgHit < 0 ? 120 : -120);
            }
            return;
        }
        if (m_clickCb) {
            const int index = CandidateAt(x, y);
            if (index >= 0) {
                m_clickCb(index);
            }
        }
    }
    /// V0.5.7 右键点击候选：命中候选 → 回调（TSF 层弹菜单）
    void OnRightClick(int x, int y) override
    {
        if (m_rightClickCb) {
            const int index = CandidateAt(x, y);
            if (index >= 0) {
                m_rightClickCb(index);
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
    int m_cursorPos = -1;
    bool m_multiRow = false;
    bool m_inlinePreedit = true;
    int m_hoverIndex = -1;
    int m_pageHover = 0;
    std::wstring m_labelFormat = L"%d.";
    float m_corner = 4.0f;
    float m_hilite = 3.0f;
    int m_padding = 8;
    int m_spacing = 14;
    std::wstring m_fontFace = L"Microsoft YaHei";
    float m_fontSize = 16.0f;
    CandidateTheme m_theme;
    ClickCallback m_clickCb;
    RightClickCallback m_rightClickCb;
    PageCallback m_pageCb;
    static constexpr int kPerRow = 5;
    /// 单列最大宽度（V0.3.6：146→240，长词组完整显示不截断；≈13 字）
    static constexpr int kMaxItemWidth = 240;
};

// ===========================================================================
// CCandidateWindow（薄封装）
// ===========================================================================
CCandidateWindow::CCandidateWindow()
    : m_panelOwner(std::make_unique<CandidatePanel>()),
      m_panel(m_panelOwner.get())
{
}

CCandidateWindow::~CCandidateWindow()
{
    m_window.Destroy();
}

bool CCandidateWindow::Initialize()
{
    if (m_window.Hwnd() != nullptr) {
        return true;
    }
    if (!m_window.Create(L"TaishenCandidateWindow", 200, 60, true, true, true)) {
        taishen::DebugLog("CandidateWindow: window create failed");
        return false;
    }
    m_window.SetRoot(m_panel);
    // UIWindow now owns the root control tree. Keep only the non-owning pointer
    // for rendering updates; the unique_ptr stays empty after the transfer.
    m_panelOwner.release();
    ApplyRoundedRegion();
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

    // V0.4.3-P0A：边界 clamp 从主屏（SM_CXSCREEN）改为光标所在显示器工作区。
    // 修复：副屏输入时窗口被错误 clamp 回主屏；负坐标副屏无 clamp 能力。
    HMONITOR hMon = MonitorFromPoint({ x, y }, MONITOR_DEFAULTTONEAREST);
    MONITORINFO mi{};
    mi.cbSize = sizeof(mi);
    GetMonitorInfo(hMon, &mi);
    const RECT& wa = mi.rcWork;

    if (x + width > wa.right) {
        x = wa.right - width;
    }
    if (y + height > wa.bottom) {
        y = caretRect.top - height - 4;
        if (y < wa.top) {
            y = wa.top;
        }
    }
    if (x < wa.left) {
        x = wa.left;
    }
    SetWindowPos(m_window.Hwnd(), HWND_TOPMOST, x, y, width, height,
                 SWP_NOACTIVATE | SWP_SHOWWINDOW);
    ApplyRoundedRegion();
}

/// V0.3.6：窗口级圆角裁剪——圆角外区域不显示（悬浮卡片效果）。
/// 尺寸每次变化后重建 region；圆角 = config corner_radius × DPI。
void CCandidateWindow::ApplyRoundedRegion()
{
    const HWND hwnd = m_window.Hwnd();
    if (hwnd == nullptr) {
        return;
    }
    RECT rc{};
    GetClientRect(hwnd, &rc);
    const int w = rc.right - rc.left;
    const int h = rc.bottom - rc.top;
    if (w <= 0 || h <= 0) {
        return;
    }
    const UINT dpi = GetDpiForWindow(hwnd);
    const float scale = dpi > 0 ? static_cast<float>(dpi) / 96.0f : 1.0f;
    int rad = static_cast<int>(m_cornerRadius * scale);
    rad = (std::min)(rad, (std::min)(w, h) / 2);
    if (rad <= 0) {
        SetWindowRgn(hwnd, nullptr, TRUE);
        return;
    }
    HRGN rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, rad * 2, rad * 2);
    if (rgn != nullptr) {
        // 成功后系统接管 region 所有权，无需 DeleteObject
        SetWindowRgn(hwnd, rgn, TRUE);
    }
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

    // 0.2.28 复选标点场景 pinyin 为空但候选非空（书名号/大括号候选窗）——
    // 判定只看候选是否为空；pinyin 为空但候选存在时仍须显示
    if (m_candidates.empty()) {
        taishen::DebugLog("CandidateWindow: UpdateState HIDE (pinyin=" +
                          std::to_string(m_pinyin.size()) + " cands=" +
                          std::to_string(m_candidates.size()) + ")");
        Hide();
        return;
    }

    if (m_window.Hwnd() == nullptr) {
        Initialize();
    }
    m_lastCaretRect = caretRect;
    m_hasCaretRect = true;
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

void CCandidateWindow::SetRightClickCallback(RightClickCallback cb)
{
    m_rightClickCb = std::move(cb);
    m_panel->SetRightClickCallback(m_rightClickCb);
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
            if (m_hasCaretRect) {
                PositionWindow(m_lastCaretRect);
            }
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
        if (m_hasCaretRect) {
            PositionWindow(m_lastCaretRect);
        }
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
            if (m_hasCaretRect) {
                PositionWindow(m_lastCaretRect);
            }
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
        if (m_hasCaretRect) {
            PositionWindow(m_lastCaretRect);
        }
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
            if (m_hasCaretRect) {
                PositionWindow(m_lastCaretRect);
            }
            m_window.Invalidate();
        }
    }
}

void CCandidateWindow::SetCursorPos(int pos)
{
    if (m_cursorPos != pos) {
        m_cursorPos = pos;
        m_panel->SetCursorPos(pos);
        if (m_visible) {
            if (m_hasCaretRect) {
                PositionWindow(m_lastCaretRect);
            }
            m_window.Invalidate();
        }
    }
}

} // namespace taishen

