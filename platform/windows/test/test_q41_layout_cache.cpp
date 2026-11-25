/// Q4.1 候选窗 IDWriteTextLayout LRU 缓存与布局快照自动化验证（独立 exe）
///
/// 覆盖：
///   1. 14 键 zhongguorenmin 首轮冷态可控未命中 + 次轮热态 0 新增 CreateTextLayout
///   2. 选中高亮切换（SetSelectedIndex 0..8）与主题切换帧 0 新增布局创建
///   3. 横排 / 多行 / 竖排 + 拼音光标行的几何测量、候选命中与 UIA 矩形一致性
///   4. 字号/粗体缓存键隔离 + 设备丢失 ReleaseDeviceResources 清零并自愈恢复
///   5. LRU 容量上限有界淘汰验证 + 50 帧暖态重绘耗时采集
/// 返回 0 = 全部通过。

#include <windows.h>
#include <cstdio>
#include <string>
#include <vector>

#include "candidate_window.h"
#include "theme.h"

namespace {

bool Check(bool cond, const wchar_t* msg)
{
    if (!cond) {
        std::wprintf(L"FAIL: %ls\n", msg);
        return false;
    }
    return true;
}

void PumpMessages()
{
    MSG msg{};
    for (int i = 0; i < 32 && PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE); ++i) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

struct KeyStep {
    std::string pinyin;
    std::vector<std::string> cands;
};

std::vector<KeyStep> BuildZhongguorenminSteps()
{
    const std::vector<std::string> base1 = {
        "在\tzai", "中\tzh", "着", "这", "只", "找", "真", "早", "知"
    };
    const std::vector<std::string> base2 = {
        "中\tzh", "重\tchong", "种", "终", "众", "钟", "忠", "周", "主"
    };
    const std::vector<std::string> base3 = {
        "中国\tzhg", "中兴\tzhx", "中午", "中学", "中心", "重点", "重要", "周围", "主要"
    };
    const std::vector<std::string> base4 = {
        "中国人\tzhgr", "中国梦\tzhgm", "中国心", "中国队", "中华人民共和国", "中科院", "中学生", "中老年人", "中长期"
    };
    const std::vector<std::string> base5 = {
        "中国人民\tzhgrm", "中国人民大学\tzhgrmdx", "中国人民银行", "中国日报", "中国文化", "中国传统", "中国制造", "中国式", "中国风"
    };
    const char* prefixes[] = {
        "z", "zh", "zho", "zhon", "zhong",
        "zhongg", "zhonggu", "zhongguo", "zhongguor", "zhongguore",
        "zhongguoren", "zhongguorenm", "zhongguorenmi", "zhongguorenmin"
    };
    std::vector<KeyStep> steps;
    steps.reserve(14);
    for (int i = 0; i < 14; ++i) {
        const std::vector<std::string>* pool = &base5;
        if (i == 0) pool = &base1;
        else if (i <= 4) pool = &base2;
        else if (i <= 7) pool = &base3;
        else if (i <= 10) pool = &base4;
        steps.push_back({ prefixes[i], *pool });
    }
    return steps;
}

bool TestKeySequenceCaching()
{
    taishen::CCandidateWindow wnd;
    if (!Check(wnd.Initialize(), L"STEP1 CandidateWindow Initialize 失败")) return false;
    wnd.SetInlinePreedit(false);
    wnd.SetFont(L"Microsoft YaHei", 16.0f);
    wnd.ResetLayoutCacheStatsForTest();

    const auto steps = BuildZhongguorenminSteps();
    const RECT caret{ 120, 120, 122, 140 };

    // 第 1 轮（冷态）
    for (const auto& st : steps) {
        wnd.UpdateState(st.pinyin, st.cands, caret, 0, 3);
        PumpMessages();
    }
    const auto s1 = wnd.LayoutCacheStatsForTest();
    const double avgMiss = static_cast<double>(s1.misses) / static_cast<double>(steps.size());
    std::wprintf(L"STEP1 Round1(cold): misses=%llu (avg=%.2f/key) hits=%llu entries=%zu/%zu\n",
                 static_cast<unsigned long long>(s1.misses), avgMiss,
                 static_cast<unsigned long long>(s1.hits), s1.entries, s1.capacity);
    if (!Check(s1.misses > 0 && s1.misses <= steps.size() * 25,
               L"STEP1 Round1 misses 超出受控上界")) return false;
    if (!Check(s1.hits > s1.misses,
               L"STEP1 Round1 同一轮内 Measure+DrawCachedText 应使 hits > misses")) return false;
    if (!Check(s1.entries > 0 && s1.entries <= s1.capacity,
               L"STEP1 缓存条目数越界")) return false;

    // 第 2 轮（热态重放同一 14 键，必须零新增 CreateTextLayout）
    wnd.ResetLayoutCacheStatsForTest();
    for (const auto& st : steps) {
        wnd.UpdateState(st.pinyin, st.cands, caret, 0, 3);
        PumpMessages();
    }
    const auto s2 = wnd.LayoutCacheStatsForTest();
    std::wprintf(L"STEP1 Round2(warm): misses=%llu hits=%llu entries=%zu\n",
                 static_cast<unsigned long long>(s2.misses),
                 static_cast<unsigned long long>(s2.hits), s2.entries);
    if (!Check(s2.misses == 0 && s2.hits > 0,
               L"STEP1 Round2 热态重放应当零 CreateTextLayout 未命中且全命中")) return false;

    // STEP 2：选词移动（SetSelectedIndex 0..8）+ 主题切换重绘 → 0 新增布局
    wnd.ResetLayoutCacheStatsForTest();
    for (int idx = 0; idx < 9; ++idx) {
        wnd.SetSelectedIndex(idx);
        UpdateWindow(wnd.Hwnd());
        PumpMessages();
    }
    wnd.SetTheme(taishen::LightTheme());
    UpdateWindow(wnd.Hwnd());
    PumpMessages();
    const auto sSel = wnd.LayoutCacheStatsForTest();
    std::wprintf(L"STEP2 Selection+Theme(9+1 frames): misses=%llu hits=%llu\n",
                 static_cast<unsigned long long>(sSel.misses),
                 static_cast<unsigned long long>(sSel.hits));
    if (!Check(sSel.misses == 0 && sSel.hits > 0,
               L"STEP2 选词移动与切换主题帧应当零布局未命中")) return false;

    wnd.Hide();
    return true;
}

bool TestGeometryAndUiaConsistency()
{
    taishen::CCandidateWindow wnd;
    if (!Check(wnd.Initialize(), L"STEP3 Initialize 失败")) return false;
    const std::vector<std::string> cands = {
        "人民\trm", "人们", "人民币", "人民网", "人数", "人类"
    };
    const RECT caret{ 160, 160, 162, 180 };

    // 横排 + 行内预编辑展开光标
    int wH = 0, hH = 0;
    wnd.SetVerticalLayout(false);
    wnd.SetMultiRow(false);
    wnd.SetInlinePreedit(false);
    wnd.SetCursorPos(3);
    wnd.PrepareLayoutForTest("renmin", cands, 0, 3, wH, hH);

    // 竖排测量对比
    int wV = 0, hV = 0;
    wnd.SetVerticalLayout(true);
    wnd.PrepareLayoutForTest("renmin", cands, 0, 3, wV, hV);
    if (!Check(hV > hH && wV < wH, L"STEP3 竖排/横排宽高关系异常")) return false;

    // 实窗口 UIA 矩形与命中回环校验（确保 LayoutSnapshot 与绘制、HitTest 一致）
    wnd.SetVerticalLayout(false);
    wnd.UpdateState("renmin", cands, caret, 1, 3);
    PumpMessages();
    for (size_t i = 0; i < cands.size(); ++i) {
        const RECT rc = wnd.UiaItemScreenRect(i);
        const int cx = (rc.left + rc.right) / 2;
        const int cy = (rc.top + rc.bottom) / 2;
        const int hit = wnd.UiaHitTestScreenPoint(cx, cy);
        if (hit != static_cast<int>(i)) {
            std::wprintf(L"FAIL: STEP3 横排第 %zu 项 UIA 命中异常 got=%d rc=(%ld,%ld,%ld,%ld)\n",
                         i, hit, rc.left, rc.top, rc.right, rc.bottom);
            wnd.Hide();
            return false;
        }
    }

    wnd.SetVerticalLayout(true);
    wnd.UpdateState("renmin", cands, caret, 1, 3);
    PumpMessages();
    for (size_t i = 0; i < cands.size(); ++i) {
        const RECT rc = wnd.UiaItemScreenRect(i);
        const int cx = (rc.left + rc.right) / 2;
        const int cy = (rc.top + rc.bottom) / 2;
        const int hit = wnd.UiaHitTestScreenPoint(cx, cy);
        if (hit != static_cast<int>(i)) {
            std::wprintf(L"FAIL: STEP3 竖排第 %zu 项 UIA 命中异常 got=%d\n", i, hit);
            wnd.Hide();
            return false;
        }
    }
    wnd.Hide();
    std::wprintf(L"STEP3 Geometry & UIA hit-test OK horiz=(%d,%d) vert=(%d,%d)\n",
                 wH, hH, wV, hV);
    return true;
}

bool TestFontIsolationAndDeviceRecovery()
{
    // 字号隔离 & Bold 隔离 & 设备丢失恢复
    taishen::UIWindow uiWnd;
    if (!Check(uiWnd.Create(L"TaishenQ41RenderProbe", 240, 80, false, true, false),
               L"STEP4 UIWindow Create 失败")) return false;
    taishen::UIRenderer& r = uiWnd.Renderer();
    if (!Check(r.Ensure(uiWnd.Hwnd()), L"STEP4 Renderer Ensure 失败")) return false;
    r.ResetLayoutCacheStats();

    const D2D1_SIZE_F s16 = r.MeasureText(L"泰深输入法", 16.0f, false, L"Microsoft YaHei");
    const D2D1_SIZE_F s22 = r.MeasureText(L"泰深输入法", 22.0f, false, L"Microsoft YaHei");
    const D2D1_SIZE_F s16b = r.MeasureText(L"泰深输入法", 16.0f, true, L"Microsoft YaHei");
    if (!Check(s22.width > s16.width && s22.height > s16.height,
               L"STEP4 字号切换宽度未放大")) return false;
    // 复测字号 16 普通体，命中缓存且尺寸与 s16 完全一致
    const D2D1_SIZE_F s16Again = r.MeasureText(L"泰深输入法", 16.0f, false, L"Microsoft YaHei");
    if (!Check(s16Again.width == s16.width && s16b.height > 0.0f,
               L"STEP4 字号复测尺寸不一致")) return false;
    const auto fontStats = r.LayoutCacheStats();
    if (!Check(fontStats.entries == 3 && fontStats.hits == 1 && fontStats.misses == 3,
               L"STEP4 字号/粗体隔离统计异常")) return false;

    // 候选窗设备丢失清零与自愈恢复
    taishen::CCandidateWindow cw;
    if (!Check(cw.Initialize(), L"STEP4 CandidateWindow Initialize 失败")) return false;
    const RECT caret{ 120, 120, 122, 140 };
    cw.UpdateState("ceshi", { "测试", "策士" }, caret, 0, 1);
    PumpMessages();
    if (!Check(cw.LayoutCacheStatsForTest().entries > 0, L"STEP4 恢复前缓存应非空")) return false;

    cw.ReleaseRendererResourcesForTest();
    if (!Check(cw.LayoutCacheStatsForTest().entries == 0,
               L"STEP4 ReleaseDeviceResources 后缓存未清空")) return false;

    cw.UpdateState("ceshi", { "测试", "策士", "侧室" }, caret, 0, 1);
    PumpMessages();
    if (!Check(cw.LayoutCacheStatsForTest().entries > 0,
               L"STEP4 设备重建后缓存未恢复工作")) return false;
    cw.Hide();

    std::wprintf(L"STEP4 Font/Bold Isolation & Device Recovery OK (w16=%.1f w22=%.1f)\n",
                 s16.width, s22.width);
    return true;
}

bool TestLruEvictionAndFrameTiming()
{
    taishen::CCandidateWindow wnd;
    if (!Check(wnd.Initialize(), L"STEP5 Initialize 失败")) return false;
    wnd.SetInlinePreedit(false);
    wnd.SetLayoutCacheCapacityForTest(32);
    wnd.ResetLayoutCacheStatsForTest();
    const RECT caret{ 100, 100, 102, 120 };

    for (int i = 0; i < 80; ++i) {
        const std::string py = "ci" + std::to_string(i);
        const std::string w1 = "\xE8\xAF\x8D" + std::to_string(i);       // "词i"
        const std::string w2 = "\xE6\x9D\xA1" + std::to_string(i);       // "条i"
        wnd.UpdateState(py, { w1, w2 }, caret, 0, 1);
        PumpMessages();
    }
    const auto sEvict = wnd.LayoutCacheStatsForTest();
    std::wprintf(L"STEP5 LRU Eviction: entries=%zu capacity=%zu evictions=%llu\n",
                 sEvict.entries, sEvict.capacity,
                 static_cast<unsigned long long>(sEvict.evictions));
    if (!Check(sEvict.capacity == 32 && sEvict.entries <= 32 && sEvict.evictions > 0,
               L"STEP5 LRU 淘汰上限未生效")) return false;

    // 恢复默认容量并采集 50 帧暖态重绘参考耗时
    wnd.SetLayoutCacheCapacityForTest(256);
    const std::vector<std::string> warmCands = {
        "中国\tzg", "中华", "中间", "中心", "重要", "中央", "中文", "中学", "中旬"
    };
    wnd.UpdateState("zhongguo", warmCands, caret, 0, 3);
    PumpMessages();
    wnd.ResetLayoutCacheStatsForTest();

    LARGE_INTEGER freq{}, t0{}, t1{};
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&t0);
    for (int frame = 0; frame < 50; ++frame) {
        wnd.SetSelectedIndex(frame % 9);
        UpdateWindow(wnd.Hwnd());
    }
    QueryPerformanceCounter(&t1);
    const double totalMs =
        static_cast<double>(t1.QuadPart - t0.QuadPart) * 1000.0 /
        static_cast<double>(freq.QuadPart);
    const auto sWarm = wnd.LayoutCacheStatsForTest();
    std::wprintf(L"STEP5 Warm 50-frame redraw: total=%.2fms avg=%.3fms/frame hits=%llu misses=%llu\n",
                 totalMs, totalMs / 50.0,
                 static_cast<unsigned long long>(sWarm.hits),
                 static_cast<unsigned long long>(sWarm.misses));
    if (!Check(sWarm.misses == 0 && sWarm.hits > 0,
               L"STEP5 50 帧暖态重绘不应产生新增 misses")) return false;

    wnd.Hide();
    return true;
}

} // namespace

int wmain()
{
    std::wprintf(L"=== Q4.1 DWrite IDWriteTextLayout Cache Test ===\n");
    if (!TestKeySequenceCaching()) return 1;
    if (!TestGeometryAndUiaConsistency()) return 1;
    if (!TestFontIsolationAndDeviceRecovery()) return 1;
    if (!TestLruEvictionAndFrameTiming()) return 1;
    std::wprintf(L"ALL TESTS PASSED\n");
    return 0;
}
