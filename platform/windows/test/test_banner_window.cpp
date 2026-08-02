/// 状态横幅冒烟测试 — 独立 exe
///
/// 验证全局单例横幅：线程注册/注销生命周期 + 状态文字更新。
/// 可见性依赖真实前台窗口（测试 exe 无前台窗口），此处验证状态与生命周期。
/// 返回 0 = 通过。

#include <windows.h>
#include <objbase.h>
#include <cstdio>
#include <string>

#include "banner_window.h"

int wmain()
{
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    auto& banner = taishen::CBannerWindow::Instance();
    const DWORD tid = GetCurrentThreadId();

    // 1. 状态文字更新（中英切换场景）
    banner.UpdateStatus(L"中文模式");
    if (banner.StatusText() != L"中文模式") {
        wprintf(L"FAIL: 状态文字错误: %ls\n", banner.StatusText().c_str());
        return 1;
    }
    wprintf(L"STEP1 UpdateStatus OK (%ls)\n", banner.StatusText().c_str());

    // 2. 线程注册（ActivateEx 场景）→ 前台评估不崩溃
    banner.RegisterThread(tid);
    banner.UpdateStatus(L"英文模式 · 双拼");
    if (banner.StatusText() != L"英文模式 · 双拼") {
        wprintf(L"FAIL: 更新失败\n");
        return 1;
    }
    wprintf(L"STEP2 RegisterThread OK (%ls)\n", banner.StatusText().c_str());

    // 3. 线程注销（Deactivate 场景）→ 前台评估不崩溃
    banner.UnregisterThread(tid);
    wprintf(L"STEP3 UnregisterThread OK\n");

    // 4. 重复注册/注销安全（多实例场景）
    banner.RegisterThread(tid);
    banner.RegisterThread(tid);  // 幂等
    banner.UnregisterThread(tid);
    banner.UnregisterThread(tid);  // 幂等
    wprintf(L"STEP4 Idempotent OK\n");

    CoUninitialize();
    wprintf(L"BANNER TEST PASSED\n");
    return 0;
}
