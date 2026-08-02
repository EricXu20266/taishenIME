/// 输入法工具栏冒烟测试 — 独立 exe
///
/// 验证工具栏单例生命周期：托盘开关（SetEnabled）+ 线程注册/注销。
/// 可见性依赖真实前台窗口，此处验证状态与生命周期安全。
/// 返回 0 = 通过。

#include <windows.h>
#include <objbase.h>
#include <cstdio>

#include "banner_window.h"

// 测试 exe 提供 DLL 模块句柄（正式 DLL 由 dllmain.cpp 定义）
HMODULE g_hModule = nullptr;

int wmain()
{
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    auto& tb = taishen::CBannerWindow::Instance();
    const DWORD tid = GetCurrentThreadId();

    // 1. 默认开启
    if (!tb.IsEnabled()) {
        wprintf(L"FAIL: 工具栏默认应开启\n");
        return 1;
    }
    wprintf(L"STEP1 Default Enabled OK\n");

    // 2. 托盘开关：隐藏 → 显示（右键菜单场景）
    tb.SetEnabled(false);
    if (tb.IsEnabled()) {
        wprintf(L"FAIL: SetEnabled(false) 失败\n");
        return 1;
    }
    tb.SetEnabled(true);
    if (!tb.IsEnabled()) {
        wprintf(L"FAIL: SetEnabled(true) 失败\n");
        return 1;
    }
    wprintf(L"STEP2 SetEnabled OK\n");

    // 3. 线程注册/注销（ActivateEx/Deactivate 场景）
    tb.RegisterThread(tid);
    tb.RegisterThread(tid);  // 幂等
    tb.UnregisterThread(tid);
    tb.UnregisterThread(tid);  // 幂等
    wprintf(L"STEP3 Register/Unregister OK\n");

    // 4. 单例唯一性
    auto& tb2 = taishen::CBannerWindow::Instance();
    if (&tb != &tb2) {
        wprintf(L"FAIL: 单例不唯一\n");
        return 1;
    }
    wprintf(L"STEP4 Singleton OK\n");

    CoUninitialize();
    wprintf(L"TOOLBAR TEST PASSED\n");
    return 0;
}
