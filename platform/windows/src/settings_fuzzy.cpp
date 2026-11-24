/// 现代化设置窗体 — 模糊音 9 组独立开关面板（Q2.1）
///
/// 拆出本文件以保持 `settings_window.cpp` 不突破架构行数棘轮上限。

#include "settings_window.h"

namespace taishen {

namespace {

struct FuzzyGroupUiMeta {
    const wchar_t* label;
    bool ImeConfig::* field;
};

constexpr FuzzyGroupUiMeta kFuzzyGroupMeta[9] = {
    { L"z↔zh",   &ImeConfig::fuzzy_z_zh },
    { L"c↔ch",   &ImeConfig::fuzzy_c_ch },
    { L"s↔sh",   &ImeConfig::fuzzy_s_sh },
    { L"an↔ang", &ImeConfig::fuzzy_an_ang },
    { L"en↔eng", &ImeConfig::fuzzy_en_eng },
    { L"in↔ing", &ImeConfig::fuzzy_in_ing },
    { L"n↔l",    &ImeConfig::fuzzy_n_l },
    { L"f↔h",    &ImeConfig::fuzzy_f_h },
    { L"r↔l",    &ImeConfig::fuzzy_r_l },
};

} // namespace

void CSettingsWindow::BuildFuzzyGroupRows(UILayout* parent)
{
    m_chkFuzzy = new UICheckBox(L"模糊音总开关（9 组独立可选，默认全关）");
    m_chkFuzzy->SetSwitchMode(true);
    parent->AddChild(m_chkFuzzy);

    for (int row = 0; row < 3; ++row) {
        auto* hbox = new UILayout(UILayout::Dir::H);
        hbox->SetPadding(0);
        hbox->SetGap(8);
        for (int col = 0; col < 3; ++col) {
            const int idx = row * 3 + col;
            auto* chk = new UICheckBox(kFuzzyGroupMeta[idx].label);
            chk->SetSwitchMode(false);
            chk->SetOnChanged([this](bool on) {
                if (on && m_chkFuzzy != nullptr) {
                    m_chkFuzzy->SetChecked(true);
                }
            });
            m_chkFuzzyGroups[idx] = chk;
            hbox->AddChild(chk);
        }
        parent->AddChild(hbox);
    }
}

void CSettingsWindow::ApplyFuzzyGroupsToUI()
{
    if (m_chkFuzzy != nullptr) {
        m_chkFuzzy->SetChecked(m_cfg.fuzzy_enabled);
    }
    for (int i = 0; i < 9; ++i) {
        if (m_chkFuzzyGroups[i] != nullptr) {
            m_chkFuzzyGroups[i]->SetChecked(m_cfg.*(kFuzzyGroupMeta[i].field));
        }
    }
}

void CSettingsWindow::CollectFuzzyGroupsFromUI()
{
    if (m_chkFuzzy != nullptr) {
        m_cfg.fuzzy_enabled = m_chkFuzzy->IsChecked();
    }
    for (int i = 0; i < 9; ++i) {
        if (m_chkFuzzyGroups[i] != nullptr) {
            m_cfg.*(kFuzzyGroupMeta[i].field) = m_chkFuzzyGroups[i]->IsChecked();
        }
    }
}

} // namespace taishen
