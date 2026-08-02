/// 状态横幅冒烟测试 — 独立 exe
///
/// 验证 CBannerWindow 创建、显示、状态更新、隐藏全链路。
/// 返回 0 = 通过。

#include <windows.h>
#include <objbase.h>
#include <cstdio>
#include <string>

#include "banner_window.h"

int wmain()
{
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

    taishen::CBannerWindow banner;

    // 1. 显示 + 状态文字
    banner.Show(L"中文模式 · 双拼");
    if (!banner.IsVisible()) {
        wprintf(L"FAIL: banner 未显示\n");
        return 1;
    }
    if (banner.StatusText() != L"中文模式 · 双拼") {
        wprintf(L"FAIL: 状态文字错误: %ls\n", banner.StatusText().c_str());
        return 1;
    }
    wprintf(L"STEP1 Show OK (%ls)\n", banner.StatusText().c_str());

    // 2. 状态更新（中英切换场景）
    banner.UpdateStatus(L"英文模式 · 简繁");
    if (banner.StatusText() != L"英文模式 · 简繁") {
        wprintf(L"FAIL: UpdateStatus 失败\n");
        return 1;
    }
    wprintf(L"STEP2 UpdateStatus OK (%ls)\n", banner.StatusText().c_str());

    // 3. 隐藏（切走输入法场景）
    banner.Hide();
    if (banner.IsVisible()) {
        wprintf(L"FAIL: Hide 后仍可见\n");
        return 1;
    }
    wprintf(L"STEP3 Hide OK\n");

    // 4. 重新显示（再次激活场景）——窗口复用
    banner.Show(L"中文模式");
    if (!banner.IsVisible()) {
        wprintf(L"FAIL: 重新 Show 失败\n");
        return 1;
    }
    wprintf(L"STEP4 ReShow OK\n");

    CoUninitialize();
    wprintf(L"BANNER TEST PASSED\n");
    return 0;
}
