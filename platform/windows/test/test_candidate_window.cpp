/// 候选窗口冒烟测试 — 独立 exe
///
/// 验证 CCandidateWindow 能创建窗口、渲染候选词、定位、隐藏。
/// 用法：test_candidate_window.exe
///   显示 3 秒后自动退出。返回 0 = 通过。

#include <windows.h>
#include <vector>
#include <string>

#include "candidate_window.h"

int WINAPI wWinMain(HINSTANCE, HINSTANCE, LPWSTR, int)
{
    taishen::CCandidateWindow wnd;

    // 1. 初始化
    if (!wnd.Initialize()) {
        MessageBoxW(nullptr, L"Initialize failed", L"Test", MB_OK);
        return 1;
    }

    // 2. 模拟输入 "zhong" 有候选
    RECT caret = {100, 200, 120, 220};
    std::vector<std::string> candidates = {"中国", "中", "种", "重", "钟", "终"};
    wnd.UpdateState("zhong", candidates, caret);
    wnd.SetSelectedIndex(0);

    // 3. 停留 2 秒观察
    Sleep(2000);

    // 4. 隐藏
    wnd.Hide();
    Sleep(500);

    // 5. 再次显示（模拟连续输入）
    RECT caret2 = {300, 400, 320, 420};
    std::vector<std::string> candidates2 = {"你好", "你", "拟", "妮"};
    wnd.UpdateState("nihao", candidates2, caret2);
    Sleep(2000);

    return 0;
}
