/// 中英文切换冒烟测试 — 独立 exe
///
/// 验证：
///   1. Ctrl+Space 切换模式
///   2. 英文模式下字母直通 committed
///   3. 中文模式下字母累积拼音
/// 返回 0 = 通过。

#include <windows.h>
#include <cstdio>

#include "tsf_keyevent.h"
#include "engine_bridge.h"

int wmain()
{
    engine_init(nullptr);
    engine_set_candidate_count(9);

    // 初始中文模式
    if (engine_get_ascii_mode() != 0) {
        wprintf(L"FAIL: 初始应为中文模式\n");
        return 1;
    }
    wprintf(L"初始中文模式 OK\n");

    // 中文模式：字母累积拼音
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('N', 0, r);
        if (!eat || !r.state_changed) {
            wprintf(L"FAIL: 中文模式字母应吞键且状态变化\n");
            return 1;
        }
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        wprintf(L"拼音=%hs\n", buf);
        if (strcmp(buf, "n") != 0) {
            wprintf(L"FAIL: 拼音累积错误\n");
            return 1;
        }
    }

    // Ctrl+Space 切换（模拟 Ctrl 按下）
    {
        keybd_event(VK_CONTROL, 0, 0, 0); // Ctrl 按下
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0); // Ctrl 松开
        if (!eat || engine_get_ascii_mode() != 1) {
            wprintf(L"FAIL: Ctrl+Space 应切换到英文模式\n");
            return 1;
        }
        wprintf(L"Ctrl+Space 切换到英文模式 OK\n");
    }

    // 英文模式：字母直通 committed
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('A', 0, r);
        if (!eat || r.committed != L"a" || r.state_changed) {
            wprintf(L"FAIL: 英文模式字母应直通 committed=a\n");
            return 1;
        }
        wprintf(L"英文模式字母直通 OK (committed=%ls)\n", r.committed.c_str());
        // 拼音不累积
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        if (strcmp(buf, "") != 0) {
            wprintf(L"FAIL: 英文模式不应累积拼音\n");
            return 1;
        }
    }

    // 再 Ctrl+Space 切回中文
    {
        keybd_event(VK_CONTROL, 0, 0, 0);
        taishen::KeyEventResult r;
        taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        if (engine_get_ascii_mode() != 0) {
            wprintf(L"FAIL: 再次 Ctrl+Space 应切回中文模式\n");
            return 1;
        }
        wprintf(L"切回中文模式 OK\n");
    }

    wprintf(L"ALL TESTS PASSED\n");
    engine_destroy();
    return 0;
}
