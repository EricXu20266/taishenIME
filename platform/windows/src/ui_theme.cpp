/// 自研窗体系统 — 主题 token 实现（V0.3.0）

#include "ui_theme.h"

namespace taishen {

UITheme UIThemeDark()
{
    UITheme t;
    t.bg          = D2D1::ColorF(0xFF1E1E1E);
    t.cardBg      = D2D1::ColorF(0xFF2A2A2A);
    t.text        = D2D1::ColorF(0xFFE8E8E8);
    t.textDim     = D2D1::ColorF(0xFF9A9A9A);
    t.accent      = D2D1::ColorF(0xFF4C8DFF);
    t.accentText  = D2D1::ColorF(0xFFFFFFFF);
    t.border      = D2D1::ColorF(0xFF3A3A3A);
    t.hoverBg     = D2D1::ColorF(0xFF3A3A3A);
    t.pressedBg   = D2D1::ColorF(0xFF454545);
    t.checkmark   = D2D1::ColorF(0xFFFFFFFF);
    t.cornerRadius = 4.0f;
    t.cardRadius   = 8.0f;
    t.padding      = 16;
    t.gap          = 10;
    t.fontSize     = 14.0f;
    t.fontSizeTitle = 15.0f;
    return t;
}

UITheme UIThemeLight()
{
    UITheme t;
    t.bg          = D2D1::ColorF(0xFFF5F5F5);
    t.cardBg      = D2D1::ColorF(0xFFFFFFFF);
    t.text        = D2D1::ColorF(0xFF1A1A1A);
    t.textDim     = D2D1::ColorF(0xFF6B6B6B);
    t.accent      = D2D1::ColorF(0xFF0078D4);
    t.accentText  = D2D1::ColorF(0xFFFFFFFF);
    t.border      = D2D1::ColorF(0xFFE0E0E0);
    t.hoverBg     = D2D1::ColorF(0xFFEAEAEA);
    t.pressedBg   = D2D1::ColorF(0xFFDADADA);
    t.checkmark   = D2D1::ColorF(0xFFFFFFFF);
    t.cornerRadius = 4.0f;
    t.cardRadius   = 8.0f;
    t.padding      = 16;
    t.gap          = 10;
    t.fontSize     = 14.0f;
    t.fontSizeTitle = 15.0f;
    return t;
}

int UIThemeSystemMode()
{
    HKEY hKey = nullptr;
    const LPCWSTR kPath =
        L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
    if (RegOpenKeyExW(HKEY_CURRENT_USER, kPath, 0, KEY_READ, &hKey) != ERROR_SUCCESS) {
        return -1;
    }
    DWORD value = 0, size = sizeof(value);
    const LONG ret = RegQueryValueExW(hKey, L"AppsUseLightTheme", nullptr, nullptr,
                                      reinterpret_cast<LPBYTE>(&value), &size);
    RegCloseKey(hKey);
    if (ret != ERROR_SUCCESS) {
        return -1;
    }
    return value != 0 ? 1 : 0;
}

UITheme UIThemeCurrent()
{
    return UIThemeSystemMode() == 1 ? UIThemeLight() : UIThemeDark();
}

} // namespace taishen
