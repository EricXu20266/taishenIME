/// 现代化设置窗体 — 迁移预设与 EnumWindows 进程拾取器（Q2.2）
///
/// 拆出本文件以保持 `settings_window.cpp` 不突破架构行数棘轮上限。

#include "settings_window.h"
#include "process_picker.h"
#include "settings_ui_helpers.h"

#include <algorithm>
#include <cwctype>

namespace taishen {

namespace {

using settings_ui::ButtonWidth;
using settings_ui::CardLayout;
using settings_ui::HintText;

/// 预设行：下拉框占满左侧 + 应用按钮固定按文本宽
class PresetRowLayout : public UILayout
{
public:
    PresetRowLayout()
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
        if (w <= 0 || m_children.size() < 2) {
            return;
        }
        const int btnW = ButtonWidth(L"应用预设");
        const int comboW = (std::max)(80, w - btnW - m_gap);
        m_children[0]->SetRect({ x, y, x + comboW, y + h });
        m_children[1]->SetRect({ x + comboW + m_gap, y, x + w, y + h });
    }
};

std::wstring ToLowerCopy(std::wstring s)
{
    for (wchar_t& ch : s) {
        ch = static_cast<wchar_t>(towlower(ch));
    }
    return s;
}

} // namespace

void CSettingsWindow::BuildPresetCard(UILayout* page)
{
    auto* card = new CardLayout(L"迁移预设（一键对齐常用输入法习惯）");
    auto* row = new PresetRowLayout();

    m_comboPreset = new UIComboBox();
    m_comboPreset->SetItems({
        L"泰深默认（横排 · 行内预编辑 · -/= 翻页）",
        L"微软拼音风格（横排 · 行内预编辑 · 微软双拼）",
        L"搜狗拼音风格（横排 · 窗口拼音 · ,/. 与 -/= 翻页）",
        L"小狼毫 / RIME 风格（竖排 · 行内预编辑 · [/] 定字）",
    });
    m_comboPreset->SetSelectedIndex(0);

    auto* btnApply = new UIButton(L"应用预设");
    btnApply->SetOnClick([this]() {
        const int idx = m_comboPreset != nullptr ? m_comboPreset->SelectedIndex() : 0;
        CollectFromUI();
        ApplyMigrationPreset(m_cfg, static_cast<MigrationPreset>(std::clamp(idx, 0, 3)));
        ApplyToUI();
        Invalidate();
    });

    row->AddChild(m_comboPreset);
    row->AddChild(btnApply);
    card->AddChild(row);
    card->AddChild(new HintText(
        L"仅调整候选布局、翻页键与输入习惯，保留应用级配置与词库路径；点击「确定」后保存生效。",
        2));
    page->AddChild(card);
}

bool CSettingsWindow::AddAppRowWithName(const std::wstring& exeName)
{
    const std::wstring lower = ToLowerCopy(exeName);
    if (lower.empty()) {
        return false;
    }
    const bool dup = std::any_of(m_appData.begin(), m_appData.end(),
                                 [&](const AppRowData& d) {
                                     return ToLowerCopy(d.name) == lower;
                                 });
    if (dup) {
        return false;
    }
    m_appData.push_back(AppRowData{ lower, 0, 0, 0, false });
    RebuildAppList();
    return true;
}

void CSettingsWindow::ShowAppPickerMenu()
{
    const std::vector<RunningWindowApp> apps = EnumerateVisibleWindowApps();
    HMENU menu = CreatePopupMenu();
    if (menu == nullptr) {
        return;
    }

    if (apps.empty()) {
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, L"（未检测到其他可见窗口）");
    } else {
        for (size_t i = 0; i < apps.size(); ++i) {
            const bool exists = std::any_of(
                m_appData.begin(), m_appData.end(),
                [&](const AppRowData& d) {
                    return ToLowerCopy(d.name) == apps[i].exeName;
                });
            std::wstring label = apps[i].exeName;
            if (!apps[i].windowTitle.empty()) {
                label += L"  —  " + apps[i].windowTitle;
            }
            UINT flags = MF_STRING | (exists ? MF_CHECKED : 0);
            AppendMenuW(menu, flags, static_cast<UINT_PTR>(i + 1), label.c_str());
        }
    }

    POINT pt{};
    GetCursorPos(&pt);
    const int cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTBUTTON,
                                   pt.x, pt.y, 0, Hwnd(), nullptr);
    DestroyMenu(menu);

    if (cmd > 0 && static_cast<size_t>(cmd - 1) < apps.size()) {
        AddAppRowWithName(apps[static_cast<size_t>(cmd - 1)].exeName);
    }
}

} // namespace taishen
