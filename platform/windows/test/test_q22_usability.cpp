/// Q2.2 易用性补齐无头验证（独立 exe）
///
/// 覆盖：
///   1. CCandidateWindow 竖排候选几何布局与 DIP 命中 / 翻页按钮命中
///   2. EnumWindows 进程拾取器纯函数（文件名小写提取/去重/排自身/截断排序）及实时枚举冒烟
///   3. Rust 用户词库统计 / 导出 / 导入 / 清空 FFI 全链路（隔离于 %TEMP%）
/// 返回 0 = 全部通过。

#include <windows.h>
#include <cstdio>
#include <string>
#include <vector>

#include "candidate_window.h"
#include "engine_bridge.h"
#include "process_picker.h"

HMODULE g_hModule = nullptr;

namespace {

std::string WideToUtf8Test(const std::wstring& w)
{
    if (w.empty()) {
        return std::string();
    }
    const int n = WideCharToMultiByte(
        CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()), nullptr, 0, nullptr, nullptr);
    if (n <= 0) {
        return std::string();
    }
    std::string out(static_cast<size_t>(n), '\0');
    WideCharToMultiByte(
        CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()), &out[0], n, nullptr, nullptr);
    return out;
}

bool TestVerticalCandidateLayout()
{
    taishen::CCandidateWindow wnd;
    const std::vector<std::string> cands = { "中国", "中", "种", "重", "钟" };

    int wHoriz = 0;
    int hHoriz = 0;
    wnd.SetVerticalLayout(false);
    wnd.SetInlinePreedit(false);
    wnd.PrepareLayoutForTest("zhong", cands, 0, 3, wHoriz, hHoriz);

    int wVert = 0;
    int hVert = 0;
    wnd.SetVerticalLayout(true);
    wnd.PrepareLayoutForTest("zhong", cands, 0, 3, wVert, hVert);

    if (hVert <= hHoriz || wVert >= wHoriz) {
        wprintf(L"FAIL: 竖排候选尺寸异常 horiz=(%d,%d) vert=(%d,%d)\n",
                wHoriz, hHoriz, wVert, hVert);
        return false;
    }

    // 默认 padding=10，pinyinH=18（inline_preedit=false），每行高 candH=24：
    // 各行 Y 中心 = 10 + 18 + row * 24 + 12 = 40, 64, 88, 112, 136
    const int xMid = wVert / 2;
    for (int row = 0; row < 5; ++row) {
        const int yMid = 10 + 18 + row * 24 + 12;
        const int hit = wnd.HitTestCandidateForTest(xMid, yMid);
        if (hit != row) {
            wprintf(L"FAIL: 竖排第 %d 行命中异常 got=%d (x=%d, y=%d)\n",
                    row, hit, xMid, yMid);
            return false;
        }
    }

    // 拼音行内与底行以外不可误命中候选
    if (wnd.HitTestCandidateForTest(xMid, 15) != -1) {
        wprintf(L"FAIL: 拼音区不应命中候选\n");
        return false;
    }
    if (wnd.HitTestCandidateForTest(xMid, hVert - 2) != -1) {
        wprintf(L"FAIL: 底部边界外不应命中候选\n");
        return false;
    }

    // totalPages = 3 时底行右端为 ‹ N/M › 翻页按钮（top = hVert - 10 - 18 .. hVert - 10）
    const int pageBtnY = hVert - 19;
    const int hitNext = wnd.HitTestPageButtonForTest(wVert - 14, pageBtnY);
    if (hitNext != 1) {
        wprintf(L"FAIL: 竖排底行下一页按钮命中异常 got=%d\n", hitNext);
        return false;
    }

    wprintf(L"STEP1 Vertical Candidate Layout OK horiz=(%d,%d) vert=(%d,%d)\n",
            wHoriz, hHoriz, wVert, hVert);
    return true;
}

bool TestProcessPicker()
{
    if (taishen::ExtractLowerExeName(L"C:\\Program Files\\App\\Code.EXE") != L"code.exe" ||
        taishen::ExtractLowerExeName(L"/usr/bin/WeChat.Exe") != L"wechat.exe" ||
        !taishen::ExtractLowerExeName(L"").empty()) {
        wprintf(L"FAIL: ExtractLowerExeName 解析异常\n");
        return false;
    }

    const std::wstring longTitle(60, L'A');
    const std::vector<taishen::RunningWindowApp> raw = {
        { L"Notepad.EXE", L"无标题 - 记事本", 10 },
        { L"notepad.exe", L"第二个窗口", 11 },
        { L"taishen_server.exe", L"服务进程", 12 },
        { L"Code.exe", longTitle, 13 },
    };
    const auto dedup = taishen::DeduplicateAndSortApps(raw, L"taishen_server.exe");
    if (dedup.size() != 2 ||
        dedup[0].exeName != L"code.exe" ||
        dedup[1].exeName != L"notepad.exe") {
        wprintf(L"FAIL: DeduplicateAndSortApps 去重/排序结果错误 size=%zu\n", dedup.size());
        return false;
    }
    if (dedup[0].windowTitle.size() != 41 || dedup[0].windowTitle.back() != L'…') {
        wprintf(L"FAIL: 超长窗口标题未截断 size=%zu\n", dedup[0].windowTitle.size());
        return false;
    }

    // 真实桌面枚举冒烟（无头桌面可能返回 0 个，但必须不崩溃且保持小写有序无重）
    const auto live = taishen::EnumerateVisibleWindowApps();
    for (size_t i = 0; i < live.size(); ++i) {
        if (live[i].exeName.empty()) {
            wprintf(L"FAIL: EnumerateVisibleWindowApps 存在空 exeName\n");
            return false;
        }
        if (i > 0 && live[i].exeName <= live[i - 1].exeName) {
            wprintf(L"FAIL: EnumerateVisibleWindowApps 未严格去重排序\n");
            return false;
        }
    }
    wprintf(L"STEP2 Process Picker OK (live windows=%zu)\n", live.size());
    return true;
}

bool TestUserDictFfiRoundtrip()
{
    wchar_t tmpDir[MAX_PATH] = {};
    const DWORD n = GetTempPathW(MAX_PATH, tmpDir);
    if (n == 0 || n >= MAX_PATH) {
        wprintf(L"FAIL: GetTempPathW 失败\n");
        return false;
    }
    const std::wstring dbPath = std::wstring(tmpDir) + L"taishen_q22_test_user.db";
    const std::wstring inPath = std::wstring(tmpDir) + L"taishen_q22_import.txt";
    const std::wstring outPath = std::wstring(tmpDir) + L"taishen_q22_export.txt";
    auto cleanup = [&]() {
        DeleteFileW(dbPath.c_str());
        DeleteFileW((dbPath + L"-wal").c_str());
        DeleteFileW((dbPath + L"-shm").c_str());
        DeleteFileW(inPath.c_str());
        DeleteFileW(outPath.c_str());
    };
    cleanup();

    FILE* fp = _wfopen(inPath.c_str(), L"wb");
    if (fp == nullptr) {
        wprintf(L"FAIL: 无法写入导入测试文件\n");
        return false;
    }
    const char* sample = "泰深输入\ttai'shen shu ru\t8\nni'hao\t你好吗\n";
    fwrite(sample, 1, strlen(sample), fp);
    fclose(fp);

    const std::string dbUtf8 = WideToUtf8Test(dbPath);
    const std::string inUtf8 = WideToUtf8Test(inPath);
    const std::string outUtf8 = WideToUtf8Test(outPath);

    if (engine_init(nullptr) != 0) {
        wprintf(L"FAIL: engine_init 失败\n");
        cleanup();
        return false;
    }
    const int imported = engine_import_user_dict(dbUtf8.c_str(), inUtf8.c_str());
    if (imported != 2) {
        wprintf(L"FAIL: engine_import_user_dict expected 2 got=%d\n", imported);
        engine_destroy();
        cleanup();
        return false;
    }
    int words = -1;
    int pairs = -1;
    if (engine_user_dict_stats(dbUtf8.c_str(), &words, &pairs) != 0 || words != 2 || pairs != 0) {
        wprintf(L"FAIL: engine_user_dict_stats expected (2,0) got=(%d,%d)\n", words, pairs);
        engine_destroy();
        cleanup();
        return false;
    }
    const int exported = engine_export_user_dict(dbUtf8.c_str(), outUtf8.c_str());
    if (exported != 2) {
        wprintf(L"FAIL: engine_export_user_dict expected 2 got=%d\n", exported);
        engine_destroy();
        cleanup();
        return false;
    }
    if (engine_clear_user_dict(dbUtf8.c_str()) != 0) {
        wprintf(L"FAIL: engine_clear_user_dict 失败\n");
        engine_destroy();
        cleanup();
        return false;
    }
    if (engine_user_dict_stats(dbUtf8.c_str(), &words, &pairs) != 0 || words != 0 || pairs != 0) {
        wprintf(L"FAIL: 清空后统计 expected (0,0) got=(%d,%d)\n", words, pairs);
        engine_destroy();
        cleanup();
        return false;
    }
    engine_destroy();
    cleanup();
    wprintf(L"STEP3 User Dict FFI Roundtrip OK\n");
    return true;
}

} // namespace

int wmain()
{
    g_hModule = GetModuleHandleW(nullptr);
    if (!TestVerticalCandidateLayout()) return 1;
    if (!TestProcessPicker()) return 2;
    if (!TestUserDictFfiRoundtrip()) return 3;
    wprintf(L"ALL TESTS PASSED\n");
    return 0;
}
