/// 设置对话框 — 实现
///
/// 对应 SPEC: docs/modules/settings-ui/SPEC.md
/// 主对话框 IDD_SETTINGS：4 个 Tab 页（基础/输入/外观/高级）+ 底部按钮行。
/// 配色子对话框 IDD_THEME_COLORS：10 项主题色 ChooseColorW 逐项调色。
/// 确定 → 校验 → SaveConfig 写回 config.ini（tsf_module 2s 轮询热加载自动生效）。

#include "settings_dialog.h"
#include "config_reader.h"
#include "debug_log.h"
#include "resource.h"

#include <commctrl.h>
#include <commdlg.h>
#include <shellapi.h>
#include <windowsx.h> // ComboBox_* 宏

#include <algorithm>
#include <cstdio>
#include <sstream>

// DLL 模块句柄（dllmain.cpp 定义于全局命名空间，对话框资源从 DLL 加载）
extern HMODULE g_hModule;

namespace taishen {

// ── 日志辅助（DebugLog 只接受 std::string）──

static std::string WToUtf8ForLog(const std::wstring& w)
{
    if (w.empty()) {
        return std::string();
    }
    const int len = WideCharToMultiByte(CP_UTF8, 0, w.c_str(),
                                        static_cast<int>(w.size()),
                                        nullptr, 0, nullptr, nullptr);
    if (len <= 0) {
        return std::string();
    }
    std::string s(static_cast<size_t>(len), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
                        &s[0], len, nullptr, nullptr);
    return s;
}

// ── 上下文（主对话框与配色子对话框共享）──

struct SettingsCtx {
    ImeConfig cfg;              // 编辑中的配置
    std::wstring dllDir;        // DLL 目录（尾分隔符）
    bool themeCustomized = false; // 用户通过配色子对话框改过颜色 → 保存时不覆盖
};

// ── 颜色转换 ──

static COLORREF ColorToRef(const D2D1_COLOR_F& c)
{
    return RGB(static_cast<BYTE>(c.r * 255.0f + 0.5f),
               static_cast<BYTE>(c.g * 255.0f + 0.5f),
               static_cast<BYTE>(c.b * 255.0f + 0.5f));
}

static D2D1_COLOR_F RefToColor(COLORREF ref)
{
    D2D1_COLOR_F c;
    c.r = GetRValue(ref) / 255.0f;
    c.g = GetGValue(ref) / 255.0f;
    c.b = GetBValue(ref) / 255.0f;
    c.a = 1.0f;
    return c;
}

static std::wstring ColorToHexW(const D2D1_COLOR_F& c)
{
    wchar_t buf[16] = {0};
    swprintf(buf, 16, L"%02X%02X%02X",
             static_cast<int>(c.r * 255.0f + 0.5f),
             static_cast<int>(c.g * 255.0f + 0.5f),
             static_cast<int>(c.b * 255.0f + 0.5f));
    return buf;
}

/// 主题配色索引 → ImeConfig.theme 字段（0..9）
static D2D1_COLOR_F* ThemeColorAt(ImeConfig& cfg, int idx)
{
    switch (idx) {
    case 0:  return &cfg.theme.bg;
    case 1:  return &cfg.theme.text;
    case 2:  return &cfg.theme.label;
    case 3:  return &cfg.theme.comment;
    case 4:  return &cfg.theme.border;
    case 5:  return &cfg.theme.highlight_bg;
    case 6:  return &cfg.theme.highlight_text;
    case 7:  return &cfg.theme.highlight_label;
    case 8:  return &cfg.theme.dim;
    case 9:  return &cfg.theme.mark;
    default: return nullptr;
    }
}

/// 应用主题预设（0=深色 1=浅色），标记显式主题
static void ApplyThemePreset(ImeConfig& cfg, bool light)
{
    if (light) {
        cfg.theme.bg             = D2D1::ColorF(0xF5F5F5, 1.0f);
        cfg.theme.text           = D2D1::ColorF(0x333333, 1.0f);
        cfg.theme.label          = D2D1::ColorF(0x666666, 1.0f);
        cfg.theme.comment        = D2D1::ColorF(0x999999, 1.0f);
        cfg.theme.border         = D2D1::ColorF(0xD0D0D0, 1.0f);
        cfg.theme.highlight_bg   = D2D1::ColorF(0x0078D4, 1.0f);
        cfg.theme.highlight_text = D2D1::ColorF(0xFFFFFF, 1.0f);
        cfg.theme.highlight_label= D2D1::ColorF(0xFFFFFF, 1.0f);
        cfg.theme.dim            = D2D1::ColorF(0x999999, 1.0f);
        cfg.theme.mark           = D2D1::ColorF(0x0078D4, 0.22f);
    } else {
        cfg.theme = CandidateTheme::Default();
    }
    cfg.userThemeExplicit = true;
}

// ── Tab 页控件分组（切换显隐）──

static const int kPage0[] = {
    IDC_EDIT_CANDIDATE, IDC_EDIT_FONT_FACE, IDC_EDIT_FONT_SIZE,
    IDC_CHK_INLINE_PREEDIT, IDC_EDIT_LABEL_FORMAT,
};
static const int kPage1[] = {
    IDC_CHK_FUZZY, IDC_CHK_CORRECTION, IDC_CHK_MIX_MODE, IDC_CHK_TRADITIONAL,
    IDC_CHK_SHUANGPIN, IDC_COMBO_SCHEME, IDC_CHK_PHRASE, IDC_CHK_ASCII_PUNCT,
    IDC_CHK_EMOJI,
};
static const int kPage2[] = {
    IDC_COMBO_THEME, IDC_BTN_THEME_COLORS, IDC_EDIT_CORNER,
    IDC_EDIT_HILITE_CORNER, IDC_EDIT_PADDING, IDC_EDIT_SPACING,
};
static const int kPage3[] = {
    IDC_EDIT_APP_ASCII, IDC_EDIT_DICT_PATH, IDC_EDIT_USER_DICT_PATH,
    IDC_EDIT_PHRASE_PATH,
};
static const int* kPages[] = { kPage0, kPage1, kPage2, kPage3 };
static const int kPageCounts[] = { 5, 9, 6, 4 };
static constexpr int kPageNum = 4;

/// 只显示指定页控件，隐藏其余页
static void ShowPage(HWND hDlg, int page)
{
    for (int p = 0; p < kPageNum; ++p) {
        const bool show = (p == page);
        const int* ids = kPages[p];
        for (int i = 0; i < kPageCounts[p]; ++i) {
            ShowWindow(GetDlgItem(hDlg, ids[i]), show ? SW_SHOW : SW_HIDE);
        }
    }
}

// ── 双拼方案表 ──

struct SchemeEntry { const wchar_t* name; const char* key; };
static const SchemeEntry kSchemes[] = {
    { L"微软双拼", "mspy" },
    { L"小鹤双拼", "flypy" },
    { L"搜狗双拼", "sogou" },
    { L"自然码",   "zrm" },
    { L"紫光双拼", "ziguang" },
    { L"加加双拼", "jiajia" },
};
static constexpr int kSchemeCount = 6;

// ── 控件 <-> 配置 ──

static void TrimW(std::wstring& s)
{
    size_t b = 0, e = s.size();
    while (b < e && (s[b] == L' ' || s[b] == L'\t')) ++b;
    while (e > b && (s[e - 1] == L' ' || s[e - 1] == L'\t')) --e;
    s = s.substr(b, e - b);
}

/// 配置 → 控件
static void FillControls(HWND hDlg, const ImeConfig& cfg)
{
    // 页 0 基础
    SetDlgItemInt(hDlg, IDC_EDIT_CANDIDATE, cfg.candidate_count, FALSE);
    SetDlgItemTextW(hDlg, IDC_EDIT_FONT_FACE, cfg.font_face.c_str());
    SetDlgItemInt(hDlg, IDC_EDIT_FONT_SIZE, static_cast<int>(cfg.font_size), FALSE);
    CheckDlgButton(hDlg, IDC_CHK_INLINE_PREEDIT,
                   cfg.inline_preedit ? BST_CHECKED : BST_UNCHECKED);
    SetDlgItemTextW(hDlg, IDC_EDIT_LABEL_FORMAT, cfg.label_format.c_str());

    // 页 1 输入
    CheckDlgButton(hDlg, IDC_CHK_FUZZY, cfg.fuzzy_enabled ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_CORRECTION, cfg.correction_enabled ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_MIX_MODE, cfg.mix_mode_enabled ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_TRADITIONAL, cfg.traditional_enabled ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_SHUANGPIN, cfg.shuangpin_mode ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_PHRASE, cfg.phrase_enabled ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_ASCII_PUNCT, cfg.ascii_punct ? BST_CHECKED : BST_UNCHECKED);
    CheckDlgButton(hDlg, IDC_CHK_EMOJI, cfg.emoji_enabled ? BST_CHECKED : BST_UNCHECKED);

    HWND hScheme = GetDlgItem(hDlg, IDC_COMBO_SCHEME);
    ComboBox_ResetContent(hScheme);
    int sel = 0;
    for (int i = 0; i < kSchemeCount; ++i) {
        ComboBox_AddString(hScheme, kSchemes[i].name);
        if (cfg.shuangpin_scheme == kSchemes[i].key) {
            sel = i;
        }
    }
    ComboBox_SetCurSel(hScheme, sel);

    // 页 2 外观
    HWND hTheme = GetDlgItem(hDlg, IDC_COMBO_THEME);
    ComboBox_ResetContent(hTheme);
    ComboBox_AddString(hTheme, L"深色");
    ComboBox_AddString(hTheme, L"浅色");
    const float lum = 0.299f * cfg.theme.bg.r + 0.587f * cfg.theme.bg.g +
                      0.114f * cfg.theme.bg.b;
    ComboBox_SetCurSel(hTheme, lum > 0.5f ? 1 : 0);

    SetDlgItemInt(hDlg, IDC_EDIT_CORNER, static_cast<int>(cfg.corner_radius), FALSE);
    SetDlgItemInt(hDlg, IDC_EDIT_HILITE_CORNER,
                  static_cast<int>(cfg.hilite_corner_radius), FALSE);
    SetDlgItemInt(hDlg, IDC_EDIT_PADDING, cfg.padding, FALSE);
    SetDlgItemInt(hDlg, IDC_EDIT_SPACING, cfg.candidate_spacing, FALSE);

    // 页 3 高级
    std::wstring joined;
    for (size_t i = 0; i < cfg.app_ascii_list.size(); ++i) {
        if (i > 0) joined += L",";
        joined += cfg.app_ascii_list[i];
    }
    SetDlgItemTextW(hDlg, IDC_EDIT_APP_ASCII, joined.c_str());
    SetDlgItemTextW(hDlg, IDC_EDIT_DICT_PATH, cfg.dict_path.c_str());
    SetDlgItemTextW(hDlg, IDC_EDIT_USER_DICT_PATH, cfg.user_dict_path.c_str());
    SetDlgItemTextW(hDlg, IDC_EDIT_PHRASE_PATH, cfg.phrase_path.c_str());
}

/// 控件 → 配置（带校验）。失败返回 false 并写出错信息。
static bool CollectControls(HWND hDlg, SettingsCtx& ctx, std::wstring& err)
{
    ImeConfig& cfg = ctx.cfg;
    BOOL ok = FALSE;

    int v = GetDlgItemInt(hDlg, IDC_EDIT_CANDIDATE, &ok, FALSE);
    if (!ok || v < 1 || v > 20) { err = L"候选数量需在 1-20 之间"; return false; }
    cfg.candidate_count = v;

    wchar_t buf[1024] = {0};
    GetDlgItemTextW(hDlg, IDC_EDIT_FONT_FACE, buf, 1024);
    cfg.font_face = buf;
    if (cfg.font_face.empty()) { cfg.font_face = L"Microsoft YaHei"; }

    v = GetDlgItemInt(hDlg, IDC_EDIT_FONT_SIZE, &ok, FALSE);
    if (!ok || v < 12 || v > 32) { err = L"字号需在 12-32 之间"; return false; }
    cfg.font_size = static_cast<float>(v);

    cfg.inline_preedit = IsDlgButtonChecked(hDlg, IDC_CHK_INLINE_PREEDIT) == BST_CHECKED;

    GetDlgItemTextW(hDlg, IDC_EDIT_LABEL_FORMAT, buf, 1024);
    cfg.label_format = buf;
    if (cfg.label_format.empty()) { err = L"标签格式不能为空"; return false; }

    cfg.fuzzy_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_FUZZY) == BST_CHECKED;
    cfg.correction_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_CORRECTION) == BST_CHECKED;
    cfg.mix_mode_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_MIX_MODE) == BST_CHECKED;
    cfg.traditional_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_TRADITIONAL) == BST_CHECKED;
    cfg.shuangpin_mode = IsDlgButtonChecked(hDlg, IDC_CHK_SHUANGPIN) == BST_CHECKED;
    cfg.phrase_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_PHRASE) == BST_CHECKED;
    cfg.ascii_punct = IsDlgButtonChecked(hDlg, IDC_CHK_ASCII_PUNCT) == BST_CHECKED;
    cfg.emoji_enabled = IsDlgButtonChecked(hDlg, IDC_CHK_EMOJI) == BST_CHECKED;

    const int schemeSel = ComboBox_GetCurSel(GetDlgItem(hDlg, IDC_COMBO_SCHEME));
    if (schemeSel >= 0 && schemeSel < kSchemeCount) {
        cfg.shuangpin_scheme = kSchemes[schemeSel].key;
    }

    // 主题：仅未自定义配色时应用预设（用户自定义优先）
    if (!ctx.themeCustomized) {
        const int themeSel = ComboBox_GetCurSel(GetDlgItem(hDlg, IDC_COMBO_THEME));
        ApplyThemePreset(cfg, themeSel == 1);
    }

    v = GetDlgItemInt(hDlg, IDC_EDIT_CORNER, &ok, FALSE);
    if (!ok || v < 1 || v > 16) { err = L"窗口圆角需在 1-16 之间"; return false; }
    cfg.corner_radius = static_cast<float>(v);

    v = GetDlgItemInt(hDlg, IDC_EDIT_HILITE_CORNER, &ok, FALSE);
    if (!ok || v < 1 || v > 16) { err = L"高亮圆角需在 1-16 之间"; return false; }
    cfg.hilite_corner_radius = static_cast<float>(v);

    v = GetDlgItemInt(hDlg, IDC_EDIT_PADDING, &ok, FALSE);
    if (!ok || v < 0 || v > 20) { err = L"内边距需在 0-20 之间"; return false; }
    cfg.padding = v;

    v = GetDlgItemInt(hDlg, IDC_EDIT_SPACING, &ok, FALSE);
    if (!ok || v < 0 || v > 40) { err = L"候选间距需在 0-40 之间"; return false; }
    cfg.candidate_spacing = v;

    GetDlgItemTextW(hDlg, IDC_EDIT_APP_ASCII, buf, 1024);
    cfg.app_ascii_list.clear();
    {
        std::wstringstream ss(buf);
        std::wstring item;
        while (std::getline(ss, item, L',')) {
            TrimW(item);
            std::transform(item.begin(), item.end(), item.begin(), ::towlower);
            if (!item.empty()) {
                cfg.app_ascii_list.push_back(item);
            }
        }
    }

    GetDlgItemTextW(hDlg, IDC_EDIT_DICT_PATH, buf, 1024);
    cfg.dict_path = buf;
    GetDlgItemTextW(hDlg, IDC_EDIT_USER_DICT_PATH, buf, 1024);
    cfg.user_dict_path = buf;
    GetDlgItemTextW(hDlg, IDC_EDIT_PHRASE_PATH, buf, 1024);
    cfg.phrase_path = buf;

    return true;
}

// ── 配色子对话框 ──

static INT_PTR CALLBACK ThemeColorsProc(HWND hDlg, UINT msg, WPARAM wParam, LPARAM lParam)
{
    SettingsCtx* ctx = nullptr;
    if (msg == WM_INITDIALOG) {
        ctx = reinterpret_cast<SettingsCtx*>(lParam);
        SetWindowLongPtrW(hDlg, DWLP_USER, reinterpret_cast<LONG_PTR>(ctx));
        for (int i = 0; i < 10; ++i) {
            D2D1_COLOR_F* c = ThemeColorAt(ctx->cfg, i);
            SetDlgItemTextW(hDlg, IDC_CLR_HEX_BASE + i, ColorToHexW(*c).c_str());
        }
        return TRUE;
    }
    ctx = reinterpret_cast<SettingsCtx*>(GetWindowLongPtrW(hDlg, DWLP_USER));
    if (ctx == nullptr) {
        return FALSE;
    }

    switch (msg) {
    case WM_COMMAND: {
        const int id = LOWORD(wParam);
        if (id == IDOK) {
            EndDialog(hDlg, IDOK);
            return TRUE;
        }
        if (id == IDCANCEL) {
            EndDialog(hDlg, IDCANCEL);
            return TRUE;
        }
        if (id >= IDC_CLR_BTN_BASE && id < IDC_CLR_BTN_BASE + 10) {
            const int idx = id - IDC_CLR_BTN_BASE;
            D2D1_COLOR_F* target = ThemeColorAt(ctx->cfg, idx);
            static COLORREF s_custom[16] = {}; // 会话级自定义色记忆
            CHOOSECOLORW cc = {};
            cc.lStructSize = sizeof(cc);
            cc.hwndOwner = hDlg;
            cc.lpCustColors = s_custom;
            cc.rgbResult = ColorToRef(*target);
            cc.Flags = CC_RGBINIT | CC_FULLOPEN;
            if (ChooseColorW(&cc)) {
                *target = RefToColor(cc.rgbResult);
                SetDlgItemTextW(hDlg, IDC_CLR_HEX_BASE + idx,
                                ColorToHexW(*target).c_str());
                ctx->themeCustomized = true;
            }
            return TRUE;
        }
        break;
    }
    default:
        break;
    }
    return FALSE;
}

// ── 主对话框 ──

static INT_PTR CALLBACK SettingsDlgProc(HWND hDlg, UINT msg, WPARAM wParam, LPARAM lParam)
{
    SettingsCtx* ctx = nullptr;
    if (msg == WM_INITDIALOG) {
        ctx = reinterpret_cast<SettingsCtx*>(lParam);
        SetWindowLongPtrW(hDlg, DWLP_USER, reinterpret_cast<LONG_PTR>(ctx));
    } else {
        ctx = reinterpret_cast<SettingsCtx*>(GetWindowLongPtrW(hDlg, DWLP_USER));
    }

    switch (msg) {
    case WM_INITDIALOG: {
        HWND hTab = GetDlgItem(hDlg, IDC_TAB_MAIN);
        TCITEMW ti = {};
        ti.mask = TCIF_TEXT;
        const wchar_t* titles[kPageNum] = { L"基础", L"输入", L"外观", L"高级" };
        for (int i = 0; i < kPageNum; ++i) {
            ti.pszText = const_cast<wchar_t*>(titles[i]);
            TabCtrl_InsertItem(hTab, i, &ti);
        }
        FillControls(hDlg, ctx->cfg);
        ShowPage(hDlg, 0);
        return TRUE;
    }

    case WM_COMMAND:
        switch (LOWORD(wParam)) {
        case IDOK: {
            std::wstring err;
            if (!CollectControls(hDlg, *ctx, err)) {
                MessageBoxW(hDlg, err.c_str(), L"泰深输入法", MB_ICONWARNING);
                return TRUE;
            }
            if (!SaveConfig(ctx->dllDir, ctx->cfg)) {
                MessageBoxW(hDlg, L"配置文件写入失败，请检查目录权限",
                            L"泰深输入法", MB_ICONERROR);
                return TRUE;
            }
            MessageBoxW(hDlg, L"配置已保存，2 秒内自动生效", L"泰深输入法",
                        MB_ICONINFORMATION);
            EndDialog(hDlg, IDOK);
            return TRUE;
        }
        case IDCANCEL:
            EndDialog(hDlg, IDCANCEL);
            return TRUE;
        case IDC_BTN_DEFAULTS:
            ctx->cfg = ImeConfig();
            ctx->themeCustomized = false;
            FillControls(hDlg, ctx->cfg);
            ShowPage(hDlg, TabCtrl_GetCurSel(GetDlgItem(hDlg, IDC_TAB_MAIN)));
            MessageBoxW(hDlg, L"已恢复默认值，点击「确定」保存生效",
                        L"泰深输入法", MB_ICONINFORMATION);
            return TRUE;
        case IDC_BTN_OPEN_CONFIG: {
            const std::wstring path = ctx->dllDir + L"config.ini";
            ShellExecuteW(nullptr, L"open", path.c_str(), nullptr, nullptr, SW_SHOW);
            return TRUE;
        }
        case IDC_BTN_THEME_COLORS:
            DialogBoxParamW(g_hModule, MAKEINTRESOURCEW(IDD_THEME_COLORS), hDlg,
                            ThemeColorsProc, reinterpret_cast<LPARAM>(ctx));
            return TRUE;
        case IDC_COMBO_THEME:
            // 切主题下拉 → 即时预览预设（未自定义配色时）
            if (HIWORD(wParam) == CBN_SELCHANGE && !ctx->themeCustomized) {
                const int t = ComboBox_GetCurSel(reinterpret_cast<HWND>(lParam));
                if (t >= 0) {
                    ApplyThemePreset(ctx->cfg, t == 1);
                }
            }
            return TRUE;
        default:
            break;
        }
        break;

    case WM_NOTIFY: {
        const NMHDR* hdr = reinterpret_cast<NMHDR*>(lParam);
        if (hdr->idFrom == IDC_TAB_MAIN && hdr->code == TCN_SELCHANGE) {
            const int page = TabCtrl_GetCurSel(GetDlgItem(hDlg, IDC_TAB_MAIN));
            ShowPage(hDlg, page);
            return TRUE;
        }
        break;
    }

    default:
        break;
    }
    return FALSE;
}

void ShowSettingsDialog(HWND parent, const std::wstring& dllDir)
{
    INITCOMMONCONTROLSEX icc = { sizeof(icc), ICC_TAB_CLASSES };
    InitCommonControlsEx(&icc);

    SettingsCtx ctx;
    ctx.dllDir = dllDir;
    ctx.cfg = LoadConfig(dllDir);
    taishen::DebugLog("Settings: ShowSettingsDialog dllDir=" +
                      WToUtf8ForLog(dllDir));
    DialogBoxParamW(g_hModule, MAKEINTRESOURCEW(IDD_SETTINGS), parent,
                    SettingsDlgProc, reinterpret_cast<LPARAM>(&ctx));
}

} // namespace taishen
