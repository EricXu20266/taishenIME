/// 系统主题检测与应用策略 — 实现（V0.2.20 深色模式跟随系统）

#include "theme.h"

namespace taishen {

/// 检测系统应用模式：0=深色，1=浅色，-1=未知
/// 注册表键：HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize
///   AppsUseLightTheme: 0=深色, 1=浅色（DWORD）
int GetSystemAppTheme()
{
    HKEY hKey = nullptr;
    const LSTATUS status = RegOpenKeyExW(
        HKEY_CURRENT_USER,
        L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        0, KEY_READ, &hKey);
    if (status != ERROR_SUCCESS) {
        return -1;
    }
    DWORD value = 0;
    DWORD size = sizeof(DWORD);
    const LSTATUS result = RegQueryValueExW(hKey, L"AppsUseLightTheme",
                                            nullptr, nullptr,
                                            reinterpret_cast<LPBYTE>(&value), &size);
    RegCloseKey(hKey);
    if (result != ERROR_SUCCESS || size != sizeof(DWORD)) {
        return -1;
    }
    // 0 = 深色，1 = 浅色
    return value == 1 ? 1 : 0;
}

/// 系统浅色主题（极简扁平：白卡片 + 细边框 + 蓝胶囊高亮）
CandidateTheme LightTheme()
{
    CandidateTheme t;
    t.bg = D2D1::ColorF(0xFFFFFF, 1.0f);
    t.text = D2D1::ColorF(0x1A1A1A, 1.0f);
    t.label = D2D1::ColorF(0x8A8A8A, 1.0f);
    t.comment = D2D1::ColorF(0x999999, 1.0f);
    t.border = D2D1::ColorF(0xE5E5E5, 1.0f);
    t.highlight_bg = D2D1::ColorF(0x0078D4, 1.0f); // 胶囊实底
    t.highlight_text = D2D1::ColorF(0xFFFFFF, 1.0f);
    t.highlight_label = D2D1::ColorF(0xFFFFFF, 1.0f);
    t.dim = D2D1::ColorF(0x9A9A9A, 1.0f);
    t.mark = D2D1::ColorF(0xE8F0FE, 1.0f); // 悬停浅蓝灰
    return t;
}

/// 应用主题策略：用户显式配置 theme_* → 固定用户主题；
/// 否则跟随系统（深色 → 默认深色，浅色 → LightTheme，未知 → 默认深色）
bool ApplyThemeWithSystem(CandidateTheme& out, const ImeConfig& cfg)
{
    if (cfg.userThemeExplicit) {
        out = cfg.theme;
        return false;
    }
    const int sys = GetSystemAppTheme();
    if (sys == 1) {
        out = LightTheme();
    } else {
        out = CandidateTheme::Default(); // 深色或未知 → 深色默认
    }
    return true;
}

} // namespace taishen
