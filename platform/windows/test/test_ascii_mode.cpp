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
        printf("STEP1 FAIL: initial ascii_mode=%d\n", engine_get_ascii_mode());
        return 1;
    }
    printf("STEP1 OK initial chinese mode\n");

    // 中文模式：字母累积拼音
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('N', 0, r);
        if (!eat || !r.state_changed) {
            printf("STEP2 FAIL: chinese letter not eaten, eat=%d changed=%d\n", eat, r.state_changed);
            return 1;
        }
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        printf("STEP2 pinyin=%s\n", buf);
        if (strcmp(buf, "n") != 0) {
            printf("STEP2 FAIL: pinyin accumulate wrong: %s\n", buf);
            return 1;
        }
    }

    // Ctrl+Space 切换（模拟 Ctrl 按下）
    {
        keybd_event(VK_CONTROL, 0, 0, 0); // Ctrl 按下
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0); // Ctrl 松开
        printf("STEP3 ctrl+space: eat=%d ascii=%d\n", eat, engine_get_ascii_mode());
        if (!eat || engine_get_ascii_mode() != 1) {
            printf("STEP3 FAIL: ctrl+space should switch to english\n");
            return 1;
        }
        printf("STEP3 OK switched to english\n");
    }

    // 英文模式：字母直通（0.1.15 契约：透传给应用，不吞键、不 committed）
    // 修复历史：旧版走 committed 提交但无组合时 CommitComposition 不写文本 → 字母丢失。
    // 透传是输入法英文模式的业界标准——应用直接接收按键上屏。
    {
        taishen::KeyEventResult r;
        const bool eat = taishen::HandleKeyDown('A', 0, r);
        printf("STEP4 english letter: eat=%d committed=%ls changed=%d\n", eat, r.committed.c_str(), r.state_changed);
        if (eat || !r.committed.empty() || r.state_changed) {
            printf("STEP4 FAIL: english letter should pass through (not eaten, no commit)\n");
            return 1;
        }
        // 拼音不累积
        char buf[64] = {0};
        engine_get_pinyin_str(buf, sizeof(buf));
        if (strcmp(buf, "") != 0) {
            printf("STEP4 FAIL: english mode should not accumulate pinyin: %s\n", buf);
            return 1;
        }
        printf("STEP4 OK passed through\n");
    }

    // 再 Ctrl+Space 切回中文
    {
        keybd_event(VK_CONTROL, 0, 0, 0);
        taishen::KeyEventResult r;
        taishen::HandleKeyDown(VK_SPACE, 0, r);
        keybd_event(VK_CONTROL, 0, KEYEVENTF_KEYUP, 0);
        printf("STEP5 switch back: ascii=%d\n", engine_get_ascii_mode());
        if (engine_get_ascii_mode() != 0) {
            printf("STEP5 FAIL: should switch back to chinese\n");
            return 1;
        }
        printf("STEP5 OK back to chinese\n");
    }

    // 多行展开/收起（0.2.14）：↓ 展开请求 / ↑ 收起请求
    {
        engine_process_key('z');
        engine_process_key('h');  // 有候选
        taishen::KeyEventResult r;
        const bool eatDown = taishen::HandleKeyDown(VK_DOWN, 0, r);
        printf("STEP6 down: eat=%d multirow=%d\n", eatDown, r.multirow_requested);
        if (!eatDown || !r.multirow_requested) {
            printf("STEP6 FAIL: down should request multirow expand\n");
            return 1;
        }
        taishen::KeyEventResult r2;
        const bool eatUp = taishen::HandleKeyDown(VK_UP, 0, r2);
        printf("STEP6 up: eat=%d multirow=%d\n", eatUp, r2.multirow_requested);
        if (!eatUp || r2.multirow_requested) {
            printf("STEP6 FAIL: up should request multirow collapse\n");
            return 1;
        }
        engine_reset();
        printf("STEP6 OK multirow toggle\n");
    }

    printf("ALL TESTS PASSED\n");
    engine_destroy();
    return 0;
}
