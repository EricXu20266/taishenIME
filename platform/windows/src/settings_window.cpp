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
constexpr int kPagePad = 20;      // 页面内边距

// 双拼方案名（与 config_reader shuangpin_scheme 对应）
const wchar_t* const kSchemes[] = { L"微软双拼", L"小鹤", L"搜狗", L"自然码",
                                    L"紫光", L"加加" };
const char* const kSchemeKeys[] = { "mspy", "flypy", "sogou", "zrm",
                                    "ziguang", "jiajia" };

/// 左侧导航项（选中 accent 底）
class NavItem : public UIControl
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
        const D2D1_RECT_F rc = D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                           static_cast<float>(X() + Width()),
                                           static_cast<float>(Y() + Height()));
        D2D1_COLOR_F bg = t.cardBg;
        D2D1_COLOR_F fg = t.text;
        if (m_selected) {
            bg = t.accent;
            fg = t.accentText;
        } else if (IsHovered()) {
            bg = t.hoverBg;
        }
        r.FillRoundedRect(rc, 6.0f, bg);
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

/// 表单行辅助：Label + 控件 的 HBox
UILayout* FormRow(const std::wstring& label, UIControl* control, int labelW = 130)
{
    auto* row = new UILayout(UILayout::Dir::H);
    row->SetGap(10);
    auto* lbl = new UILabel(label);
    lbl->SetRect({ 0, 0, labelW, 28 });
    control->SetRect({ labelW, 0, labelW + 10, 28 });
    row->AddChild(lbl);
    row->AddChild(control);
    return row;
}

/// 复选行：CheckBox 自身含文字
UICheckBox* CheckRow(UILayout* page, const std::wstring& text)
{
    auto* chk = new UICheckBox(text);
    page->AddChild(chk);
    return chk;
}

} // namespace

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
    // 模态窗口：居中显示，标题栏可拖动
    if (!Create(L"TaishenSettingsWindow", 560, 440, false, false)) {
        return IDCANCEL;
    }
    SetFollowSystemTheme(true);
    BuildUI();
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
        nav->AddChild(item);
    }
    content->AddChild(nav);

    // 右侧面板
    auto* panel = new UILayout(UILayout::Dir::V);
    panel->SetPadding(0);
    panel->SetGap(0);
    for (int i = 0; i < 4; ++i) {
        m_pageRoots[i] = new UILayout(UILayout::Dir::V);
        m_pageRoots[i]->SetPadding(kPagePad);
        m_pageRoots[i]->SetGap(14);
        m_pageRoots[i]->SetVisible(i == 0);
        panel->AddChild(m_pageRoots[i]);
    }
    content->AddChild(panel);

    root->AddChild(content);

    // 底部按钮栏
    auto* footer = new UILayout(UILayout::Dir::H);
    footer->SetPadding(12);
    footer->SetGap(8);
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
    m_currentPage = idx;
    m_pageRoots[m_currentPage]->SetVisible(true);
    Invalidate();
}

// ===========================================================================
// 页 0 基础
// ===========================================================================
void CSettingsWindow::BuildBasicPage()
{
    UILayout* page = m_pageRoots[0];
    m_editCandidate = new UIEdit();
    m_editCandidate->SetNumeric(1, 20);
    m_editCandidate->SetPlaceholder(L"1-20");
    page->AddChild(FormRow(L"候选词数量", m_editCandidate));

    m_editFontFace = new UIEdit();
    page->AddChild(FormRow(L"候选窗字体", m_editFontFace));

    m_editFontSize = new UIEdit();
    m_editFontSize->SetNumeric(12, 32);
    m_editFontSize->SetPlaceholder(L"12-32");
    page->AddChild(FormRow(L"正文字号", m_editFontSize));

    m_chkInline = CheckRow(page, L"行内预编辑（拼音写组合，候选窗不重复）");

    m_editLabelFormat = new UIEdit();
    m_editLabelFormat->SetPlaceholder(L"%d. 或 ①");
    page->AddChild(FormRow(L"候选标签格式", m_editLabelFormat));
}

// ===========================================================================
// 页 1 输入
// ===========================================================================
void CSettingsWindow::BuildInputPage()
{
    UILayout* page = m_pageRoots[1];
    m_chkFuzzy = CheckRow(page, L"模糊音（平翘舌/前后鼻音）");
    m_chkCorrection = CheckRow(page, L"智能纠错（相邻键容错）");
    m_chkMix = CheckRow(page, L"中英混输");
    m_chkTraditional = CheckRow(page, L"简繁转换");
    m_chkShuangpin = CheckRow(page, L"双拼模式");
    m_comboScheme = new UIComboBox();
    m_comboScheme->SetItems({ kSchemes[0], kSchemes[1], kSchemes[2],
                              kSchemes[3], kSchemes[4], kSchemes[5] });
    page->AddChild(FormRow(L"双拼方案", m_comboScheme));
    m_chkPhrase = CheckRow(page, L"快捷短语（简码→短语）");
    m_chkAsciiPunct = CheckRow(page, L"英文标点透传");
    m_chkEmoji = CheckRow(page, L"Emoji 候选");
}

// ===========================================================================
// 页 2 外观
// ===========================================================================
void CSettingsWindow::BuildAppearancePage()
{
    UILayout* page = m_pageRoots[2];
    m_comboThemeMode = new UIComboBox();
    m_comboThemeMode->SetItems({ L"跟随系统", L"深色", L"浅色" });
    m_comboThemeMode->SetOnSelected([this](int idx) { OnThemeModeChanged(idx); });
    page->AddChild(FormRow(L"主题模式", m_comboThemeMode));

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
        page->AddChild(row);
    }

    m_editCorner = new UIEdit();
    m_editCorner->SetNumeric(1, 16);
    m_editCorner->SetPlaceholder(L"1-16");
    page->AddChild(FormRow(L"窗口圆角", m_editCorner));
    m_editHilite = new UIEdit();
    m_editHilite->SetNumeric(1, 16);
    m_editHilite->SetPlaceholder(L"1-16");
    page->AddChild(FormRow(L"高亮圆角", m_editHilite));
    m_editPadding = new UIEdit();
    m_editPadding->SetNumeric(0, 20);
    m_editPadding->SetPlaceholder(L"0-20");
    page->AddChild(FormRow(L"内边距", m_editPadding));
    m_editSpacing = new UIEdit();
    m_editSpacing->SetNumeric(0, 40);
    m_editSpacing->SetPlaceholder(L"0-40");
    page->AddChild(FormRow(L"候选间距", m_editSpacing));
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

    // 应用级配置
    auto* appHeader = new UILayout(UILayout::Dir::H);
    appHeader->SetGap(8);
    auto* appTitle = new UILabel(L"应用级配置（进程名，如 cmd.exe）");
    auto* btnAdd = new UIButton(L"+ 添加程序");
    btnAdd->SetOnClick([this]() { AddAppRow(); });
    appHeader->AddChild(appTitle);
    appHeader->AddChild(btnAdd);
    page->AddChild(appHeader);

    m_appList = new UILayout(UILayout::Dir::V);
    m_appList->SetPadding(0);
    m_appList->SetGap(4);
    page->AddChild(m_appList);

    m_editDict = new UIEdit();
    m_editDict->SetPlaceholder(L"空 = 内置词库");
    page->AddChild(FormRow(L"系统词库路径", m_editDict, 120));
    m_editUserDict = new UIEdit();
    m_editUserDict->SetPlaceholder(L"空 = 默认用户词库");
    page->AddChild(FormRow(L"用户词库路径", m_editUserDict, 120));
    m_editPhrase = new UIEdit();
    m_editPhrase->SetPlaceholder(L"空 = 仅内置短语");
    page->AddChild(FormRow(L"短语文件路径", m_editPhrase, 120));
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
        auto* row = new UILayout(UILayout::Dir::H);
        row->SetGap(6);

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
    // 重新布局列表
    m_appList->Layout();
    Invalidate();
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
