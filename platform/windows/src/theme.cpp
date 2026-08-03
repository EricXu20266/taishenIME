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

/// 系统浅色主题（与深色镜像：白底黑字，选中蓝同色）
CandidateTheme LightTheme()
{
    CandidateTheme t;
    t.bg = D2D1::ColorF(0xF5F5F5, 1.0f);
    t.text = D2D1::ColorF(0x1A1A1A, 1.0f);
    t.highlight = D2D1::ColorF(0x1E6FFF, 0.15f); // 浅色下高亮用淡蓝底
    t.dim = D2D1::ColorF(0x8A8A8A, 1.0f);
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
