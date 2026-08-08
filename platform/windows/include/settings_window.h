/// 现代化设置窗体 — 声明（V0.3.4）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.7 + docs/modules/settings-ui/SPEC.md
/// 基于自研窗体系统：UIWindow 模态 + 自绘标题栏（可拖动）+ 左侧导航 + 右侧内容面板。
/// 替代 Win32 对话框（settings.rc / resource.h 的 IDC_ 资源全部移除）。
/// 视觉：左侧 4 项导航（基础/输入/外观/高级），右侧卡片式内容，深浅主题跟随系统。

#pragma once

#include <string>
#include <vector>
#include "config_reader.h"
#include "ui_window.h"
#include "ui_layout.h"
#include "ui_edit.h"
#include "ui_checkbox.h"
#include "ui_combobox.h"
#include "ui_colorpicker.h"

namespace taishen {

/// 现代化设置窗体
class CSettingsWindow : public UIWindow
{
public:
    explicit CSettingsWindow(const std::wstring& dllDir);

    /// 运行模态，返回 IDOK / IDCANCEL
    int Run();

protected:
    LRESULT HandleMessage(UINT msg, WPARAM wp, LPARAM lp) override;
    /// 自绘标题栏 + 内容区
    void OnRender(UIRenderer& r) override;

private:
    // ── UI 构建 ──
    void BuildUI();           // 整体骨架：标题栏/导航/内容区/底部按钮
    void BuildBasicPage();    // 页 0 基础
    void BuildInputPage();    // 页 1 输入
    void BuildAppearancePage(); // 页 2 外观
    void BuildAdvancedPage(); // 页 3 高级

    // ── 交互 ──
    void SwitchPage(int idx);
    void OnThemeModeChanged(int mode);   // 主题下拉 → 同步色板
    void AddAppRow();                    // 应用级列表加行
    void RemoveAppRow(size_t idx);       // 应用级列表删行
    void RebuildAppList();               // 重建应用级列表控件

    // ── 配置 ──
    void ApplyToUI();        // cfg → 控件
    void CollectFromUI();    // 控件 → cfg
    void SaveAndClose();     // Collect + SaveConfig + EndModal

    std::wstring m_dllDir;
    ImeConfig m_cfg;
    int m_currentPage = 0;

    // 页根（右侧面板内，切换可见性）
    UILayout* m_pageRoots[4] = {};

    // ── 基础页控件 ──
    UIEdit* m_editCandidate = nullptr;
    UIEdit* m_editFontFace = nullptr;
    UIEdit* m_editFontSize = nullptr;
    UICheckBox* m_chkInline = nullptr;
    UIEdit* m_editLabelFormat = nullptr;

    // ── 输入页控件 ──
    UICheckBox* m_chkFuzzy = nullptr;
    UICheckBox* m_chkCorrection = nullptr;
    UICheckBox* m_chkMix = nullptr;
    UICheckBox* m_chkTraditional = nullptr;
    UICheckBox* m_chkShuangpin = nullptr;
    UIComboBox* m_comboScheme = nullptr;
    UICheckBox* m_chkPhrase = nullptr;
    UICheckBox* m_chkAsciiPunct = nullptr;
    UICheckBox* m_chkEmoji = nullptr;
    // P0-2/P1-1：候选排序 / 上下文联想（专业词库 v2 自动加载，无需配置）
    UIComboBox* m_comboSortMode = nullptr;
    UICheckBox* m_chkContextAssoc = nullptr;

    // ── 外观页控件 ──
    UIComboBox* m_comboThemeMode = nullptr;
    UIColorSwatch* m_swatches[10] = {};
    UIEdit* m_editCorner = nullptr;
    UIEdit* m_editHilite = nullptr;
    UIEdit* m_editPadding = nullptr;
    UIEdit* m_editSpacing = nullptr;

    // ── 高级页控件 ──
    /// 应用级配置行数据模型
    struct AppRowData {
        std::wstring name;   // 进程名（小写）
        int mode = 0;        // 0=跟随全局 1=默认英文 2=默认中文
        bool inlineOn = false;
        bool vimOn = false;
    };
    std::vector<AppRowData> m_appData;
    UILayout* m_appList = nullptr;     // 应用级行容器
    UIEdit* m_editDict = nullptr;
    UIEdit* m_editUserDict = nullptr;
    UIEdit* m_editPhrase = nullptr;
};

} // namespace taishen
