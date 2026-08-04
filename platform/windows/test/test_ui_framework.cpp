/// 窗体系统底座冒烟测试（V0.3.0）
///
/// 验证：
///   1. 主题 token（深/浅/跟随系统）
///   2. 布局分配（VBox/HBox/Grid 的 Rect 计算）
///   3. 命中检测（HitTestTree 递归命中）
///   4. 窗口生命周期（创建/控件树/渲染一帧/显隐/销毁）
/// 返回 0 = 通过。

#include <cstdio>
#include <string>
#include <vector>
#include <windows.h>
#include <d2d1.h>

#include "ui_theme.h"
#include "ui_render.h"
#include "ui_control.h"
#include "ui_layout.h"
#include "ui_window.h"

using namespace taishen;

// ── 测试用最小控件：纯色块 + 标签 ──
class TestBox : public UIControl
{
public:
    explicit TestBox(const std::wstring& text, int minH = 28)
        : m_text(text), m_minH(minH)
    {
    }
    int PreferredHeight(int /*width*/) const override { return m_minH; }
    void Draw(UIRenderer& r, const UITheme& t) override
    {
        r.FillRoundedRect(D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                      static_cast<float>(X() + Width()),
                                      static_cast<float>(Y() + Height())),
                          t.cornerRadius, t.cardBg);
        r.DrawText(m_text, D2D1::RectF(static_cast<float>(X()), static_cast<float>(Y()),
                                       static_cast<float>(X() + Width()),
                                       static_cast<float>(Y() + Height())),
                   t.fontSize, t.text);
    }
    std::wstring Text() const { return m_text; }

private:
    std::wstring m_text;
    int m_minH;
};

// ── STEP 1：主题 ──
static bool TestTheme()
{
    const UITheme dark = UIThemeDark();
    const UITheme light = UIThemeLight();
    if (dark.text.a == 0.0f || light.text.a == 0.0f) {
        printf("STEP1 FAIL: theme color alpha zero\n");
        return false;
    }
    if (dark.bg.r > 0.5f || light.bg.r < 0.5f) {
        printf("STEP1 FAIL: dark/light bg mismatch (dark=%f light=%f)\n",
               dark.bg.r, light.bg.r);
        return false;
    }
    const UITheme cur = UIThemeCurrent();
    if (cur.text.a == 0.0f) {
        printf("STEP1 FAIL: current theme invalid\n");
        return false;
    }
    printf("STEP1 OK themes (system mode=%d)\n", UIThemeSystemMode());
    return true;
}

// ── STEP 2：布局 ──
static bool TestLayout()
{
    // VBox：3 个固定高控件，垂直堆叠
    UILayout vbox(UILayout::Dir::V);
    vbox.SetPadding(0);
    vbox.SetGap(0);
    TestBox a(L"A", 20), b(L"B", 30), c(L"C", 40);
    vbox.AddChild(&a);
    vbox.AddChild(&b);
    vbox.AddChild(&c);
    vbox.SetRect({ 0, 0, 100, 90 });
    vbox.Layout();
    if (a.Y() != 0 || a.Height() != 20 ||
        b.Y() != 20 || b.Height() != 30 ||
        c.Y() != 50 || c.Height() != 40) {
        printf("STEP2 FAIL: VBox layout wrong (%d,%d,%d / %d,%d,%d)\n",
               a.Y(), b.Y(), c.Y(), a.Height(), b.Height(), c.Height());
        return false;
    }
    // 弹性：flex 控件平分剩余
    UILayout flex(UILayout::Dir::V);
    flex.SetGap(0);
    flex.SetPadding(0);
    TestBox f1(L"F1", 20), f2(L"F2", -1);
    flex.AddChild(&f1);
    flex.AddChild(&f2);
    flex.SetRect({ 0, 0, 100, 80 });
    flex.Layout();
    if (f2.Height() != 60) {
        printf("STEP2 FAIL: flex layout wrong h=%d (expect 60)\n", f2.Height());
        return false;
    }
    // HBox：水平堆叠
    UILayout hbox(UILayout::Dir::H);
    TestBox h1(L"H1", 10), h2(L"H2", 20), h3(L"H3", 30);
    hbox.AddChild(&h1);
    hbox.AddChild(&h2);
    hbox.AddChild(&h3);
    hbox.SetRect({ 0, 0, 60, 50 });
    hbox.Layout();
    if (h1.X() != 0 || h2.X() != 20 || h3.X() != 50) {
        printf("STEP2 FAIL: HBox layout wrong (%d,%d,%d)\n",
               h1.X(), h2.X(), h3.X());
        return false;
    }
    // Grid：2 列
    UILayout grid(UILayout::Dir::Grid);
    grid.SetCols(2);
    TestBox g1(L"G1"), g2(L"G2"), g3(L"G3"), g4(L"G4");
    grid.AddChild(&g1);
    grid.AddChild(&g2);
    grid.AddChild(&g3);
    grid.AddChild(&g4);
    grid.SetRect({ 0, 0, 100, 60 });
    grid.Layout();
    if (g1.X() != 0 || g2.X() != 55 || g3.Y() != 35 || g4.X() != 55) {
        printf("STEP2 FAIL: Grid layout wrong (%d,%d,%d,%d)\n",
               g1.X(), g2.X(), g3.Y(), g4.X());
        return false;
    }
    // 嵌套布局递归（V0.3.5 审查修复验证）：外层 Layout() 必须递归展开内层，
    // 否则深层控件停留在 {0,0,0,0}（设置窗体 4 层嵌套同款场景）
    UILayout outer(UILayout::Dir::V);
    outer.SetGap(0);
    outer.SetPadding(0);
    UILayout inner(UILayout::Dir::V);
    inner.SetGap(0);
    inner.SetPadding(0);
    TestBox n1(L"N1", 20), n2(L"N2", 30);
    inner.AddChild(&n1);
    inner.AddChild(&n2);
    outer.AddChild(&inner);
    outer.SetRect({ 0, 0, 100, 50 });
    outer.Layout();
    if (n1.Y() != 0 || n1.Height() != 20 || n2.Y() != 20 || n2.Height() != 30) {
        printf("STEP2 FAIL: nested layout not recursive (%d,%d / %d,%d)\n",
               n1.Y(), n1.Height(), n2.Y(), n2.Height());
        return false;
    }
    printf("STEP2 OK layouts (VBox/flex/HBox/Grid/nested)\n");
    return true;
}

// ── STEP 3：命中检测 ──
static bool TestHitTest()
{
    UILayout vbox(UILayout::Dir::V);
    vbox.SetGap(0);
    vbox.SetPadding(0);
    TestBox a(L"A", 20), b(L"B", 20);
    vbox.AddChild(&a);
    vbox.AddChild(&b);
    vbox.SetRect({ 0, 0, 100, 40 });
    vbox.Layout();
    // 命中 b（下半区）
    UIControl* hit = vbox.HitTestTree(50, 30);
    if (hit != &b) {
        printf("STEP3 FAIL: hit b wrong (%p expect %p)\n", (void*)hit, (void*)&b);
        return false;
    }
    // 命中 a（上半区）
    hit = vbox.HitTestTree(50, 10);
    if (hit != &a) {
        printf("STEP3 FAIL: hit a wrong\n");
        return false;
    }
    // 未命中（越界）
    hit = vbox.HitTestTree(200, 200);
    if (hit != nullptr) {
        printf("STEP3 FAIL: out-of-range should miss\n");
        return false;
    }
    printf("STEP3 OK hit-test (tree recursion)\n");
    return true;
}

// ── STEP 4：窗口生命周期 + 渲染一帧 ──
static bool TestWindow()
{
    UIWindow wnd;
    if (!wnd.Create(L"TestUIFramework", 300, 200, true, true)) {
        printf("STEP4 FAIL: window create\n");
        return false;
    }
    UILayout* vbox = new UILayout(UILayout::Dir::V);
    vbox->SetPadding(10);
    vbox->SetGap(8);
    TestBox* box = new TestBox(L"Hello UI Framework", 30);
    vbox->AddChild(box);
    wnd.SetRoot(vbox);
    wnd.SetFollowSystemTheme(true);

    wnd.Show();
    if (!wnd.IsVisible()) {
        printf("STEP4 FAIL: window not visible\n");
        return false;
    }
    // 同步渲染一帧（WM_PAINT 不崩即可）
    UpdateWindow(wnd.Hwnd());
    // 泵几条消息（WM_MOUSEMOVE 等分发路径）
    MSG msg{};
    for (int i = 0; i < 16 && PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE); ++i) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    // 焦点/命中分发
    wnd.SetFocusControl(box);
    if (!box->IsFocused()) {
        printf("STEP4 FAIL: focus not set\n");
        return false;
    }
    wnd.Hide();
    if (wnd.IsVisible()) {
        printf("STEP4 FAIL: hide failed\n");
        return false;
    }
    wnd.Destroy();
    // 控件树由 UIWindow 析构统一递归释放（V0.3.5 组合所有权），不手动 delete
    printf("STEP4 OK window lifecycle + render\n");
    return true;
}

int wmain()
{
    printf("=== UI Framework smoke test (V0.3.0) ===\n");
    if (!TestTheme())  return 1;
    if (!TestLayout()) return 1;
    if (!TestHitTest()) return 1;
    if (!TestWindow()) return 1;
    printf("ALL TESTS PASSED\n");
    return 0;
}

