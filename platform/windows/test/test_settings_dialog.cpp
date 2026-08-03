/// 设置对话框预览（独立 exe，无需注册 TSF）
/// 运行 out/test_settings_dialog.exe 直接弹出设置窗口，验证 UI 布局与配置读写。
/// 用 out/ 目录下的 config.ini 作为读写目标（不影响 DLL 行为验证）。
#include "settings_dialog.h"

// exe 无 DllMain：g_hModule 指向 exe 自身（资源已编译进 exe）
#include <windows.h>
HMODULE g_hModule = nullptr;

int main()
{
    g_hModule = GetModuleHandleW(nullptr);
    // 用 out 目录模拟 DLL 目录（config.ini 在此读写）
    const wchar_t* dir = L"E:\\AllinDeepSeek\\taishenIME\\platform\\windows\\out\\";
    taishen::ShowSettingsDialog(nullptr, dir);
    return 0;
}
