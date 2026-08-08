/// 现代化设置窗体 — 实现（V0.3.4）
///
/// 结构：UIWindow 模态（自绘标题栏 36px 可拖动）+ 左侧导航（4 项）+ 右侧内容面板。
/// 配置：LoadConfig → 控件；控件 → SaveConfig（热加载自动生效）。
/// 替代 settings.rc / resource.h 的 Win32 对话框资源。

#include "settings_window.h"
#include "debug_log.h"
#include "theme.h"
#include "ui_button.h"
#include "ui_label.h"
#include "ui_render.h"

#include <algorithm>
#include <shellapi.h>
#include <windowsx.h>

namespace taishen {

namespace {

// 布局常量
constexpr int kTitleBarH = 36;    // 标题栏高
constexpr int kNavW = 120;        // 左侧导航宽
constexpr int kFooterH = 44;      // 底部按钮栏高
constexpr int kDefaultRowH = 32;  // 默认行高（弹性项估算值，P2-3 收敛；V0.3.6 28→32 更舒展）

// 双拼方案名（与 config_reader shuangpin_scheme 对应）
const wchar_t* const kSchemes[] = { L"微软双拼", L"小鹤", L"搜狗", L"自然码",
                                    L"紫光", L"加加" };
const char* const kSchemeKeys[] = { "mspy", "flypy", "sogou", "zrm",
                                    "ziguang", "jiajia" };

/// 表单行：Label 固定宽 + 控件弹性撑满（审计 P2-4：原 HBox 把 label/edit
/// 都按 28px 宽布局导致输入框极窄——手动布局 label 定宽、control 占满剩余）。
class FormRowLayout : public UILayout
{
public:
    FormRowLayout(int labelW)
        : UILayout(UILayout::Dir::H)
        , m_labelW(labelW)
    {
        SetGap(12);  // V0.3.6：10→12
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
        UIControl* lbl = m_children[0];
        UIControl* ctrl = m_children[1];
        lbl->SetRect({ x, y, x + m_labelW, y + h });
        ctrl->SetRect({ x + m_labelW + m_gap, y, x + w, y + h });
    }

private:
    int m_labelW;
};

/// 表单行辅助：Label + 控件 的 HBox（P2-4：控件弹性宽）
UILayout* FormRow(const std::wstring& label, UIControl* control, int labelW = 130)
{
    auto* row = new FormRowLayout(labelW);
    auto* lbl = new UILabel(label);
    row->AddChild(lbl);
    row->AddChild(control);
    return row;
}

/// 应用级配置行：Edit 弹性 + 模式下拉/行内/vim/删除固定宽。
/// （审计 P2-4：原 HBox 把子控件都按 28px 宽布局——edit 极窄无法输入）
class AppRowLayout : public UILayout
{
public:
    AppRowLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(6);
    }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 5) {
            return;
        }
        // 固定项宽度（顺序：0=edit 弹性 / 1=combo / 2=行内 / 3=vim / 4=×）
        constexpr int kComboW = 110;
        constexpr int kInlineW = 55;
        constexpr int kVimW = 45;
        constexpr int kDelW = 26;
        const int fixed = kComboW + kInlineW + kVimW + kDelW + 4 * m_gap;
        int editW = w - fixed;
        if (editW < 60) {
            editW = 60;
        }
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + editW, y + h });
        cx += editW + m_gap;
        m_children[1]->SetRect({ cx, y, cx + kComboW, y + h });
        cx += kComboW + m_gap;
        m_children[2]->SetRect({ cx, y, cx + kInlineW, y + h });
        cx += kInlineW + m_gap;
        m_children[3]->SetRect({ cx, y, cx + kVimW, y + h });
        cx += kVimW + m_gap;
        m_children[4]->SetRect({ cx, y, cx + kDelW, y + h });
    }
};

/// 按钮内容宽估算（文字数 ×15px + 左右内边距）。
/// 布局层无渲染器可测量，用估算保证按钮不换行挤扁。
int ButtonWidth(const std::wstring& text)
{
    return static_cast<int>(text.size() * 15) + 32;
}

/// 底部按钮栏：左组（打开配置文件/恢复默认）靠左，右组（取消/确定）靠右，
/// 全部按内容宽——修复 LayoutH 把按钮压成 28px 导致文字换行挤扁。
class FooterLayout : public UILayout
{
public:
    FooterLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetPadding(12);
        SetGap(8);
    }

    void Layout() override
    {
        const int x = m_rect.left + m_padding;
        const int y = m_rect.top + m_padding;
        const int w = m_rect.right - m_rect.left - 2 * m_padding;
        const int h = m_rect.bottom - m_rect.top - 2 * m_padding;
        if (w <= 0 || m_children.size() < 4) {
            return;
        }
        // 子控件：0=打开配置文件 1=恢复默认 2=确定(主) 3=取消
        const int w0 = ButtonWidth(L"打开配置文件");
        const int w1 = ButtonWidth(L"恢复默认");
        const int w2 = ButtonWidth(L"确定");
        const int w3 = ButtonWidth(L"取消");
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + w0, y + h });
        cx += w0 + m_gap;
        m_children[1]->SetRect({ cx, y, cx + w1, y + h });
        // 右组：确定最右，取消在左
        int rx = x + w;
        m_children[2]->SetRect({ rx - w2, y, rx, y + h });
        rx -= w2 + m_gap;
        m_children[3]->SetRect({ rx - w3, y, rx, y + h });
    }
};

/// 卡片头部：标题弹性 + 右侧按钮按内容宽（修复 "+ 添加程序" 挤扁）
class HeaderLayout : public UILayout
{
public:
    HeaderLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(8);
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
        const int btnW = ButtonWidth(L"+ 添加程序");
        m_children[0]->SetRect({ x, y, x + w - btnW - m_gap, y + h });
        m_children[1]->SetRect({ x + w - btnW, y, x + w, y + h });
    }
};

/// 复选行：CheckBox 自身含文字（V0.3.6：默认 toggle 开关）
UICheckBox* CheckRow(UILayout* page, const std::wstring& text)
{
    auto* chk = new UICheckBox(text);
    chk->SetSwitchMode(true);
    page->AddChild(chk);
    return chk;
}

// ===========================================================================
// V0.3.6：ScrollPanel（可滚动内容面板）+ CardLayout（圆角卡片）
// ===========================================================================

/// 可滚动内容面板：垂直排列子控件，内容超高时滚轮滚动 + 裁剪。
/// 子控件 Y 坐标随滚动整体偏移（重 Layout），绘制时 PushClip 到面板矩形。
class ScrollPanel : public UILayout
{
public:
    explicit ScrollPanel()
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(16);
        SetGap(14);
    }

    void Layout() override
    {
        // P2-2：面板宽度无效时不布局（防御——避免子控件保留旧滚动偏移污染 m_maxScroll）
        if (Width() <= 0) {
            return;
        }
        UILayout::Layout();
        // 内容总高 = 子控件底部 - 面板顶（弹性子控件占满时按实际内容算）
        int maxBottom = m_rect.top;
        for (const UIControl* c : m_children) {
            if (c->IsVisible()) {
                maxBottom = (std::max)(maxBottom, static_cast<int>(c->Rect().bottom));
            }
        }
        m_contentHeight = maxBottom - m_rect.top;
        m_maxScroll = (std::max)(0, m_contentHeight - Height());
        if (m_scrollY > m_maxScroll) {
            m_scrollY = m_maxScroll;
        }
        ApplyScroll();
    }

    void OnMouseWheel(int delta) override
    {
        if (m_maxScroll <= 0) {
            // P2-5：无滚动内容时向上冒泡（父级可滚动容器仍能响应）
            UILayout::OnMouseWheel(delta);
            return;
        }
        const int target = m_scrollY - delta / 2; // 120/格 → 60px
        const int clamped = (std::clamp)(target, 0, m_maxScroll);
        if (clamped != m_scrollY) {
            m_scrollY = clamped;
            ApplyScroll();
            Invalidate();
        }
    }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        // 裁剪前验证矩形有效（Rect 未分配/零尺寸时跳过，避免 D2D clip 异常）
        const float x = static_cast<float>(X());
        const float y = static_cast<float>(Y());
        const float w = static_cast<float>(Width());
        const float h = static_cast<float>(Height());
        if (w > 0.0f && h > 0.0f) {
            r.PushClip(D2D1::RectF(x, y, x + w, y + h));
            UILayout::Draw(r, t);
            r.PopClip();
        } else {
            UILayout::Draw(r, t);
        }
    }

    UIControl* HitTestTree(int x, int y) override
    {
        if (!IsVisible() || !HitTest(x, y)) {
            return nullptr;
        }
        return UILayout::HitTestTree(x, y);
    }

private:
    /// 将滚动偏移应用到子控件（重排 + 递归子布局）。
    /// P2-1：dy==0（无滚动）直接返回——UILayout::Layout 已完整布局，
    /// 避免对整棵子布局重复 Layout（scrollY==0 时纯冗余计算）。
    void ApplyScroll()
    {
        const int dy = -m_scrollY;
        if (dy == 0) {
            return;
        }
        for (UIControl* c : m_children) {
            if (!c->IsVisible()) {
                continue;
            }
            const RECT r = c->Rect();
            c->SetRect({ r.left, r.top + dy, r.right, r.bottom + dy });
            if (auto* sub = dynamic_cast<UILayout*>(c)) {
                sub->Layout();
            }
        }
    }

    int m_scrollY = 0;
    int m_maxScroll = 0;
    int m_contentHeight = 0;
};

/// 圆角卡片容器：cardBg 背景 + 可选粗体标题 + 子控件行。
/// 高度由内容自适应（PreferredHeight 累加子项）。
class CardLayout : public UILayout
{
public:
    explicit CardLayout(const std::wstring& title = L"")
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(16);
        SetGap(12);  // V0.3.6：10→12 内容更舒展
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

    /// 卡片高 = 内容高（含 padding/gap），不弹性。
    /// 复用 UILayout::ContentHeight——按子项递归累计（审计 P1-1：
    /// 动态列表 m_appList 子项数变化时高度实时正确，卡片随行数增长）。
    int PreferredHeight(int width) const override
    {
        return ContentHeight(width);
    }
};

} // namespace

// ===========================================================================
// 左侧导航项（V0.3.6：accent 左边界条 + 浅色底 + accent 文字）
// 定义在 taishen 命名空间（settings_window.h forward-declare 成员指针）
// ===========================================================================
class CSettingsWindow::NavItem : public UIControl
{
public:
    explicit NavItem(std::wstring text)
        : m_text(std::move(text))
    {
    }
    void SetSelected(bool s) { m_selected = s; Invalidate(); }
    bool IsSelected() const { return m_selected; }
    int PreferredHeight(int /*width*/) const override { return 34; }
    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const float x = static_cast<float>(X());
        const float y = static_cast<float>(Y());
        const D2D1_RECT_F rc = D2D1::RectF(x, y, x + static_cast<float>(Width()),
                                           y + static_cast<float>(Height()));
        D2D1_COLOR_F bg = t.cardBg;
        D2D1_COLOR_F fg = t.text;
        if (m_selected) {
            bg = t.hoverBg;      // 选中：浅色底
            fg = t.accent;       // accent 文字
        } else if (IsHovered()) {
            bg = t.hoverBg;
        }
        r.FillRoundedRect(rc, 6.0f, bg);
        if (m_selected) {
            // accent 左边界条（4px 竖条，垂直居中）
            r.FillRoundedRect(
                D2D1::RectF(x, y + 6.0f, x + 4.0f, y + static_cast<float>(Height()) - 6.0f),
                2.0f, t.accent);
        }
        r.DrawText(m_text, rc, t.fontSize, fg, m_selected,
                   DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }
    void OnClick(int /*x*/, int /*y*/) override
    {
        if (m_onClick) {
            m_onClick();
        }
    }

    void SetOnClick(std::function<void()> cb) { m_onClick = std::move(cb); }

private:
    std::wstring m_text;
    bool m_selected = false;
    std::function<void()> m_onClick;
};

// ===========================================================================
// 构造 / 运行
// ===========================================================================
CSettingsWindow::CSettingsWindow(const std::wstring& dllDir)
    : m_dllDir(dllDir)
{
    m_cfg = LoadConfig(dllDir);
    SetTitleBarHeight(kTitleBarH);
}

int CSettingsWindow::Run()
{
    // 模态窗口：居中显示，标题栏可拖动（V0.3.6：560×440 → 640×520）
    if (!Create(L"TaishenSettingsWindow", 640, 520, false, false)) {
        return IDCANCEL;
    }
    SetFollowSystemTheme(true);
    BuildUI();
    // V0.3.6 fix：原代码漏调页面构建函数——控件全是 nullptr，ApplyToUI 空指针崩溃。
    // 页面内容必须在 ApplyToUI 前构建。
    BuildBasicPage();
    BuildInputPage();
    BuildAppearancePage();
    BuildAdvancedPage();
    ApplyToUI();
    return RunModal();
}

// ===========================================================================
// 标题栏 + 关闭按钮
// ===========================================================================
void CSettingsWindow::OnRender(UIRenderer& r)
{
    r.Clear(m_theme.bg);
    RECT rc{};
    GetClientRect(m_hwnd, &rc);
    const float w = static_cast<float>(rc.right - rc.left);
    // 标题栏
    r.FillRect(D2D1::RectF(0, 0, w, static_cast<float>(kTitleBarH)),
               m_theme.cardBg);
    r.DrawText(L"泰深输入法设置",
               D2D1::RectF(12, 0, 300, static_cast<float>(kTitleBarH)),
               m_theme.fontSize, m_theme.text, true,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    // 关闭按钮 ✕（右侧）
    r.DrawText(L"✕",
               D2D1::RectF(w - 46, 0, w - 8, static_cast<float>(kTitleBarH)),
               m_theme.fontSize, m_theme.textDim, false,
               DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    r.DrawLine(0, static_cast<float>(kTitleBarH), w, static_cast<float>(kTitleBarH),
               m_theme.border, 1.0f);
    // 内容区
    if (m_root != nullptr) {
        m_root->Draw(r, m_theme);
    }
}

LRESULT CSettingsWindow::HandleMessage(UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_LBUTTONUP: {
        // 标题栏右侧 ✕ 点击 → 取消关闭
        const int x = GET_X_LPARAM(lp);
        const int y = GET_Y_LPARAM(lp);
        RECT rc{};
        GetClientRect(m_hwnd, &rc);
        if (y < kTitleBarH && x > (rc.right - rc.left) - 50) {
            EndModal(IDCANCEL);
            return 0;
        }
        break;
    }
    default:
        break;
    }
    return UIWindow::HandleMessage(msg, wp, lp);
}

// ===========================================================================
// 整体骨架
// ===========================================================================
void CSettingsWindow::BuildUI()
{
    auto* root = new UILayout(UILayout::Dir::V);
    root->SetPadding(0);
    root->SetGap(0);

    // 内容区（标题栏下方）：左侧导航 + 右侧面板
    auto* content = new UILayout(UILayout::Dir::H);
    content->SetPadding(0);
    content->SetGap(0);

    // 左侧导航
    auto* nav = new UILayout(UILayout::Dir::V);
    nav->SetPadding(8);
    nav->SetGap(6);
    const wchar_t* kNavNames[] = { L"基础", L"输入", L"外观", L"高级" };
    for (int i = 0; i < 4; ++i) {
        auto* item = new NavItem(kNavNames[i]);
        item->SetOnClick([this, i]() { SwitchPage(i); });
        item->SetSelected(i == 0);
        m_navItems[i] = item;
        nav->AddChild(item);
    }
    content->AddChild(nav);

    // 右侧面板（V0.3.6：页面根 = ScrollPanel，内容超高可滚动）
    auto* panel = new UILayout(UILayout::Dir::V);
    panel->SetPadding(0);
    panel->SetGap(0);
    for (int i = 0; i < 4; ++i) {
        m_pageRoots[i] = new ScrollPanel();
        m_pageRoots[i]->SetVisible(i == 0);
        panel->AddChild(m_pageRoots[i]);
    }
    content->AddChild(panel);

    root->AddChild(content);

    // 底部按钮栏（V0.3.6：FooterLayout——按钮按内容宽，确定/取消靠右）
    auto* footer = new FooterLayout();
    auto* btnOpen = new UIButton(L"打开配置文件");
    btnOpen->SetOnClick([this]() {
        const std::wstring ini = m_dllDir + L"config.ini";
        ShellExecuteW(nullptr, L"open", ini.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    });
    auto* btnDefaults = new UIButton(L"恢复默认");
    btnDefaults->SetOnClick([this]() {
        m_cfg = ImeConfig();
        ApplyToUI();
    });
    auto* btnOk = new UIButton(L"确定");
    btnOk->SetPrimary(true);
    btnOk->SetOnClick([this]() { SaveAndClose(); });
    auto* btnCancel = new UIButton(L"取消");
    btnCancel->SetOnClick([this]() { EndModal(IDCANCEL); });
    footer->AddChild(btnOpen);
    footer->AddChild(btnDefaults);
    footer->AddChild(btnOk);
    footer->AddChild(btnCancel);
    root->AddChild(footer);

    SetRoot(root);
}

void CSettingsWindow::SwitchPage(int idx)
{
    if (idx < 0 || idx >= 4 || idx == m_currentPage) {
        return;
    }
    m_pageRoots[m_currentPage]->SetVisible(false);
    if (m_navItems[m_currentPage] != nullptr) {
        m_navItems[m_currentPage]->SetSelected(false);
    }
    m_currentPage = idx;
    m_pageRoots[m_currentPage]->SetVisible(true);
    if (m_navItems[m_currentPage] != nullptr) {
        m_navItems[m_currentPage]->SetSelected(true);
    }
    // 新页此前未布局（LayoutV 跳过不可见子控件）——整树重排
    Relayout();
    Invalidate();
}

/// V0.3.6：重排当前页（卡片高度变化后更新滚动范围）
void CSettingsWindow::ReflowPage()
{
    if (m_currentPage >= 0 && m_currentPage < 4 && m_pageRoots[m_currentPage] != nullptr) {
        m_pageRoots[m_currentPage]->Layout();
    }
    Invalidate();
}

// ===========================================================================
// 页 0 基础
// ===========================================================================
void CSettingsWindow::BuildBasicPage()
{
    UILayout* page = m_pageRoots[0];

    // 卡片 1：候选设置
    auto* card1 = new CardLayout(L"候选设置");
    m_editCandidate = new UIEdit();
    m_editCandidate->SetNumeric(1, 20);
    m_editCandidate->SetPlaceholder(L"1-20");
    card1->AddChild(FormRow(L"候选词数量", m_editCandidate));

    m_editFontFace = new UIEdit();
    card1->AddChild(FormRow(L"候选窗字体", m_editFontFace));

    m_editFontSize = new UIEdit();
    m_editFontSize->SetNumeric(12, 32);
    m_editFontSize->SetPlaceholder(L"12-32");
    card1->AddChild(FormRow(L"正文字号", m_editFontSize));

    m_editLabelFormat = new UIEdit();
    m_editLabelFormat->SetPlaceholder(L"%d. 或 ①");
    card1->AddChild(FormRow(L"候选标签格式", m_editLabelFormat));
    page->AddChild(card1);

    // 卡片 2：输入行为
    auto* card2 = new CardLayout(L"输入行为");
    m_chkInline = CheckRow(card2, L"行内预编辑（拼音写组合，候选窗不重复）");
    page->AddChild(card2);
}

// ===========================================================================
// 页 1 输入
// ===========================================================================
void CSettingsWindow::BuildInputPage()
{
    UILayout* page = m_pageRoots[1];

    // 卡片 1：输入模式
    auto* card1 = new CardLayout(L"输入模式");
    m_chkFuzzy = CheckRow(card1, L"模糊音（平翘舌/前后鼻音）");
    m_chkCorrection = CheckRow(card1, L"智能纠错（相邻键容错）");
    m_chkMix = CheckRow(card1, L"中英混输");
    m_chkTraditional = CheckRow(card1, L"简繁转换");
    m_chkShuangpin = CheckRow(card1, L"双拼模式");
    m_comboScheme = new UIComboBox();
    m_comboScheme->SetItems({ kSchemes[0], kSchemes[1], kSchemes[2],
                              kSchemes[3], kSchemes[4], kSchemes[5] });
    card1->AddChild(FormRow(L"双拼方案", m_comboScheme));
    m_chkPhrase = CheckRow(card1, L"快捷短语（简码→短语）");
    m_chkAsciiPunct = CheckRow(card1, L"英文标点透传");
    m_chkEmoji = CheckRow(card1, L"Emoji 候选");
    page->AddChild(card1);

    // 卡片 2：智能候选（P0-2 排序 / P1-1 联想；专业词库 v2 自动加载，无需配置）
    auto* card2 = new CardLayout(L"智能候选");
    m_comboSortMode = new UIComboBox();
    m_comboSortMode->SetItems({ L"默认（词频+长词过滤）", L"单字优先", L"长词优先" });
    card2->AddChild(FormRow(L"候选排序", m_comboSortMode));
    m_chkContextAssoc = CheckRow(card2, L"上下文联想（前文搭配词前置）");
    page->AddChild(card2);
}

// ===========================================================================
// 页 2 外观
// ===========================================================================
void CSettingsWindow::BuildAppearancePage()
{
    UILayout* page = m_pageRoots[2];

    // 卡片 1：主题
    auto* card1 = new CardLayout(L"主题");
    m_comboThemeMode = new UIComboBox();
    m_comboThemeMode->SetItems({ L"跟随系统", L"深色", L"浅色" });
    m_comboThemeMode->SetOnSelected([this](int idx) { OnThemeModeChanged(idx); });
    card1->AddChild(FormRow(L"主题模式", m_comboThemeMode));

    // 主题色 10 项（候选窗配色）
    const wchar_t* const kColorNames[10] = {
        L"背景", L"正文", L"序号", L"注释", L"边框",
        L"选中背景", L"选中文字", L"选中序号", L"页码/次要", L"选中标记",
    };
    for (int i = 0; i < 10; ++i) {
        auto* row = new UILayout(UILayout::Dir::H);
        row->SetGap(10);
        auto* lbl = new UILabel(kColorNames[i]);
        lbl->SetRect({ 0, 0, 90, 24 });
        m_swatches[i] = new UIColorSwatch();
        m_swatches[i]->SetRect({ 100, 0, 100 + 160, 24 });
        row->AddChild(lbl);
        row->AddChild(m_swatches[i]);
        card1->AddChild(row);
    }
    page->AddChild(card1);

    // 卡片 2：窗口
    auto* card2 = new CardLayout(L"窗口");
    m_editCorner = new UIEdit();
    m_editCorner->SetNumeric(1, 16);
    m_editCorner->SetPlaceholder(L"1-16");
    card2->AddChild(FormRow(L"窗口圆角", m_editCorner));
    m_editHilite = new UIEdit();
    m_editHilite->SetNumeric(1, 16);
    m_editHilite->SetPlaceholder(L"1-16");
    card2->AddChild(FormRow(L"高亮圆角", m_editHilite));
    m_editPadding = new UIEdit();
    m_editPadding->SetNumeric(0, 20);
    m_editPadding->SetPlaceholder(L"0-20");
    card2->AddChild(FormRow(L"内边距", m_editPadding));
    m_editSpacing = new UIEdit();
    m_editSpacing->SetNumeric(0, 40);
    m_editSpacing->SetPlaceholder(L"0-40");
    card2->AddChild(FormRow(L"候选间距", m_editSpacing));
    page->AddChild(card2);
}

void CSettingsWindow::OnThemeModeChanged(int mode)
{
    // 切换深/浅预设时同步 10 个色板（跟随系统不动色板）。
    // 显式成员数组避免依赖 CandidateTheme 字段内存顺序（V0.3.5 审查加固）
    const auto fill = [this](const CandidateTheme& t) {
        const D2D1_COLOR_F colors[10] = {
            t.bg, t.text, t.label, t.comment, t.border,
            t.highlight_bg, t.highlight_text, t.highlight_label, t.dim, t.mark,
        };
        for (int i = 0; i < 10; ++i) {
            m_swatches[i]->SetColor(colors[i]);
        }
    };
    if (mode == 1) {
        fill(CandidateTheme::Default());
    } else if (mode == 2) {
        fill(LightTheme());
    }
}

// ===========================================================================
// 页 3 高级
// ===========================================================================
void CSettingsWindow::BuildAdvancedPage()
{
    UILayout* page = m_pageRoots[3];

    // 卡片 1：应用级配置
    auto* card1 = new CardLayout(L"应用级配置");
    auto* appHeader = new HeaderLayout();
    auto* appTitle = new UILabel(L"进程名（如 cmd.exe），独立中英/vim/行内行为");
    auto* btnAdd = new UIButton(L"+ 添加程序");
    btnAdd->SetOnClick([this]() { AddAppRow(); });
    appHeader->AddChild(appTitle);
    appHeader->AddChild(btnAdd);
    card1->AddChild(appHeader);

    m_appList = new UILayout(UILayout::Dir::V);
    m_appList->SetPadding(0);
    m_appList->SetGap(4);
    card1->AddChild(m_appList);
    page->AddChild(card1);

    // 卡片 2：词库路径
    auto* card2 = new CardLayout(L"词库路径");
    m_editDict = new UIEdit();
    m_editDict->SetPlaceholder(L"空 = 内置词库");
    card2->AddChild(FormRow(L"系统词库", m_editDict, 120));
    m_editUserDict = new UIEdit();
    m_editUserDict->SetPlaceholder(L"空 = 默认用户词库");
    card2->AddChild(FormRow(L"用户词库", m_editUserDict, 120));
    m_editPhrase = new UIEdit();
    m_editPhrase->SetPlaceholder(L"空 = 仅内置短语");
    card2->AddChild(FormRow(L"短语文件", m_editPhrase, 120));
    page->AddChild(card2);
}

void CSettingsWindow::AddAppRow()
{
    m_appData.push_back({});
    RebuildAppList();
}

void CSettingsWindow::RemoveAppRow(size_t idx)
{
    if (idx < m_appData.size()) {
        m_appData.erase(m_appData.begin() + static_cast<ptrdiff_t>(idx));
        RebuildAppList();
    }
}

void CSettingsWindow::RebuildAppList()
{
    if (m_appList == nullptr) {
        return;
    }
    // 清理旧行控件（行控件由本函数 new，安全 delete）
    m_appList->RemoveAllChildren(true);

    const wchar_t* const kModes[] = { L"跟随全局", L"默认英文", L"默认中文" };
    for (size_t i = 0; i < m_appData.size(); ++i) {
        // V0.3.6：AppRowLayout——edit 弹性撑满，固定项定宽（P2-4）
        auto* row = new AppRowLayout();

        auto* edit = new UIEdit();
        edit->SetText(m_appData[i].name);
        edit->SetPlaceholder(L"进程名.exe");
        edit->SetOnChanged([this, i](const std::wstring& v) { m_appData[i].name = v; });

        auto* combo = new UIComboBox();
        combo->SetItems({ kModes[0], kModes[1], kModes[2] });
        combo->SetSelectedIndex(m_appData[i].mode);
        combo->SetOnSelected([this, i](int m) { m_appData[i].mode = m; });

        auto* chkInline = new UICheckBox(L"行内");
        chkInline->SetChecked(m_appData[i].inlineOn);
        chkInline->SetOnChanged([this, i](bool on) { m_appData[i].inlineOn = on; });

        auto* chkVim = new UICheckBox(L"vim");
        chkVim->SetChecked(m_appData[i].vimOn);
        chkVim->SetOnChanged([this, i](bool on) { m_appData[i].vimOn = on; });

        auto* btnDel = new UIButton(L"×");
        btnDel->SetOnClick([this, i]() { RemoveAppRow(i); });

        row->AddChild(edit);
        row->AddChild(combo);
        row->AddChild(chkInline);
        row->AddChild(chkVim);
        row->AddChild(btnDel);
        m_appList->AddChild(row);
    }
    // 空列表提示
    if (m_appData.empty()) {
        auto* hint = new UILabel(L"未配置。可添加：终端（cmd.exe/powershell.exe）默认英文、nvim-qt.exe 开 vim 模式等。");
        hint->SetDim(true);
        hint->SetWrap(true);
        m_appList->AddChild(hint);
    }
    // 重新布局列表 + 整页重排（卡片高度变化 → 滚动范围更新）
    m_appList->Layout();
    ReflowPage();
}

// ===========================================================================
// 配置读写
// ===========================================================================
void CSettingsWindow::ApplyToUI()
{
    // 基础
    m_editCandidate->SetText(std::to_wstring(m_cfg.candidate_count));
    m_editFontFace->SetText(m_cfg.font_face);
    m_editFontSize->SetText(std::to_wstring(static_cast<int>(m_cfg.font_size)));
    m_chkInline->SetChecked(m_cfg.inline_preedit);
    m_editLabelFormat->SetText(m_cfg.label_format);

    // 输入
    m_chkFuzzy->SetChecked(m_cfg.fuzzy_enabled);
    m_chkCorrection->SetChecked(m_cfg.correction_enabled);
    m_chkMix->SetChecked(m_cfg.mix_mode_enabled);
    m_chkTraditional->SetChecked(m_cfg.traditional_enabled);
    m_chkShuangpin->SetChecked(m_cfg.shuangpin_mode);
    int schemeIdx = 0;
    for (int i = 0; i < 6; ++i) {
        if (m_cfg.shuangpin_scheme == kSchemeKeys[i]) {
            schemeIdx = i;
            break;
        }
    }
    m_comboScheme->SetSelectedIndex(schemeIdx);
    m_chkPhrase->SetChecked(m_cfg.phrase_enabled);
    m_chkAsciiPunct->SetChecked(m_cfg.ascii_punct);
    m_chkEmoji->SetChecked(m_cfg.emoji_enabled);
    // P0-2：候选排序模式
    m_comboSortMode->SetSelectedIndex(m_cfg.sort_mode >= 0 && m_cfg.sort_mode <= 2
                                           ? m_cfg.sort_mode
                                           : 0);
    // P1-1：上下文联想
    m_chkContextAssoc->SetChecked(m_cfg.context_assoc);

    // 外观
    int themeMode = 0; // 跟随系统
    if (m_cfg.userThemeExplicit) {
        // 显式主题：按背景明暗判断深/浅
        const float lum = m_cfg.theme.bg.r * 0.299f + m_cfg.theme.bg.g * 0.587f +
                          m_cfg.theme.bg.b * 0.114f;
        themeMode = lum > 0.5f ? 2 : 1;
    }
    m_comboThemeMode->SetSelectedIndex(themeMode);
    m_swatches[0]->SetColor(m_cfg.theme.bg);
    m_swatches[1]->SetColor(m_cfg.theme.text);
    m_swatches[2]->SetColor(m_cfg.theme.label);
    m_swatches[3]->SetColor(m_cfg.theme.comment);
    m_swatches[4]->SetColor(m_cfg.theme.border);
    m_swatches[5]->SetColor(m_cfg.theme.highlight_bg);
    m_swatches[6]->SetColor(m_cfg.theme.highlight_text);
    m_swatches[7]->SetColor(m_cfg.theme.highlight_label);
    m_swatches[8]->SetColor(m_cfg.theme.dim);
    m_swatches[9]->SetColor(m_cfg.theme.mark);
    m_editCorner->SetText(std::to_wstring(static_cast<int>(m_cfg.corner_radius)));
    m_editHilite->SetText(std::to_wstring(static_cast<int>(m_cfg.hilite_corner_radius)));
    m_editPadding->SetText(std::to_wstring(m_cfg.padding));
    m_editSpacing->SetText(std::to_wstring(m_cfg.candidate_spacing));

    // 高级：4 列表 → 行模型（并集，mode 冲突时 ascii 优先）
    m_appData.clear();
    for (const auto& p : m_cfg.app_ascii_list) {
        m_appData.push_back({ p, 1, false, false });
    }
    for (const auto& p : m_cfg.app_cn_list) {
        const bool dup = std::any_of(m_appData.begin(), m_appData.end(),
                                     [&](const AppRowData& d) { return d.name == p; });
        if (!dup) {
            m_appData.push_back({ p, 2, false, false });
        }
    }
    for (const auto& p : m_cfg.app_inline_list) {
        auto it = std::find_if(m_appData.begin(), m_appData.end(),
                               [&](const AppRowData& d) { return d.name == p; });
        if (it != m_appData.end()) {
            it->inlineOn = true;
        } else {
            m_appData.push_back({ p, 0, true, false });
        }
    }
    for (const auto& p : m_cfg.app_vim_list) {
        auto it = std::find_if(m_appData.begin(), m_appData.end(),
                               [&](const AppRowData& d) { return d.name == p; });
        if (it != m_appData.end()) {
            it->vimOn = true;
        } else {
            m_appData.push_back({ p, 0, false, true });
        }
    }
    RebuildAppList();

    m_editDict->SetText(m_cfg.dict_path);
    m_editUserDict->SetText(m_cfg.user_dict_path);
    m_editPhrase->SetText(m_cfg.phrase_path);
}

void CSettingsWindow::CollectFromUI()
{
    // 基础
    m_cfg.candidate_count = std::clamp(_wtoi(m_editCandidate->Text().c_str()), 1, 20);
    m_cfg.font_face = m_editFontFace->Text();
    m_cfg.font_size = static_cast<float>(std::clamp(_wtoi(m_editFontSize->Text().c_str()), 12, 32));
    m_cfg.inline_preedit = m_chkInline->IsChecked();
    m_cfg.label_format = m_editLabelFormat->Text();
    if (m_cfg.label_format.empty()) {
        m_cfg.label_format = L"%d.";
    }

    // 输入
    m_cfg.fuzzy_enabled = m_chkFuzzy->IsChecked();
    m_cfg.correction_enabled = m_chkCorrection->IsChecked();
    m_cfg.mix_mode_enabled = m_chkMix->IsChecked();
    m_cfg.traditional_enabled = m_chkTraditional->IsChecked();
    m_cfg.shuangpin_mode = m_chkShuangpin->IsChecked();
    const int si = m_comboScheme->SelectedIndex();
    m_cfg.shuangpin_scheme = (si >= 0 && si < 6) ? kSchemeKeys[si] : "mspy";
    m_cfg.phrase_enabled = m_chkPhrase->IsChecked();
    m_cfg.ascii_punct = m_chkAsciiPunct->IsChecked();
    m_cfg.emoji_enabled = m_chkEmoji->IsChecked();
    // P0-2：候选排序模式
    const int sm = m_comboSortMode->SelectedIndex();
    m_cfg.sort_mode = (sm >= 0 && sm <= 2) ? sm : 0;
    // P1-1：上下文联想
    m_cfg.context_assoc = m_chkContextAssoc->IsChecked();

    // 外观
    const int tm = m_comboThemeMode->SelectedIndex();
    m_cfg.userThemeExplicit = (tm != 0);
    m_cfg.theme.bg = m_swatches[0]->Color();
    m_cfg.theme.text = m_swatches[1]->Color();
    m_cfg.theme.label = m_swatches[2]->Color();
    m_cfg.theme.comment = m_swatches[3]->Color();
    m_cfg.theme.border = m_swatches[4]->Color();
    m_cfg.theme.highlight_bg = m_swatches[5]->Color();
    m_cfg.theme.highlight_text = m_swatches[6]->Color();
    m_cfg.theme.highlight_label = m_swatches[7]->Color();
    m_cfg.theme.dim = m_swatches[8]->Color();
    m_cfg.theme.mark = m_swatches[9]->Color();
    m_cfg.corner_radius = static_cast<float>(std::clamp(_wtoi(m_editCorner->Text().c_str()), 1, 16));
    m_cfg.hilite_corner_radius = static_cast<float>(std::clamp(_wtoi(m_editHilite->Text().c_str()), 1, 16));
    m_cfg.padding = std::clamp(_wtoi(m_editPadding->Text().c_str()), 0, 20);
    m_cfg.candidate_spacing = std::clamp(_wtoi(m_editSpacing->Text().c_str()), 0, 40);

    // 高级：行模型 → 4 列表
    m_cfg.app_ascii_list.clear();
    m_cfg.app_cn_list.clear();
    m_cfg.app_inline_list.clear();
    m_cfg.app_vim_list.clear();
    for (const auto& d : m_appData) {
        std::wstring name = d.name;
        for (auto& ch : name) {
            ch = static_cast<wchar_t>(towlower(ch));
        }
        if (name.empty()) {
            continue;
        }
        if (d.mode == 1) {
            m_cfg.app_ascii_list.push_back(name);
        } else if (d.mode == 2) {
            m_cfg.app_cn_list.push_back(name);
        }
        if (d.inlineOn) {
            m_cfg.app_inline_list.push_back(name);
        }
        if (d.vimOn) {
            m_cfg.app_vim_list.push_back(name);
        }
    }
    m_cfg.dict_path = m_editDict->Text();
    m_cfg.user_dict_path = m_editUserDict->Text();
    m_cfg.phrase_path = m_editPhrase->Text();
}

void CSettingsWindow::SaveAndClose()
{
    CollectFromUI();
    if (SaveConfig(m_dllDir, m_cfg)) {
        taishen::DebugLog("Settings: config saved");
    } else {
        taishen::DebugLog("Settings: SaveConfig FAILED");
    }
    EndModal(IDOK);
}

} // namespace taishen

// ===========================================================================
// 入口（settings_dialog.h 兼容：工具栏「设置」按钮调用）
// ===========================================================================
#include "settings_dialog.h"

namespace taishen {

void ShowSettingsDialog(HWND /*parent*/, const std::wstring& dllDir)
{
    CSettingsWindow wnd(dllDir);
    wnd.Run();
}

} // namespace taishen
