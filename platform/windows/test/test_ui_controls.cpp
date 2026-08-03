/// 控件库冒烟测试（V0.3.1）
///
/// 验证 8 控件核心行为：
///   Label 文本/Button 点击/CheckBox 切换/Edit 编辑+数字过滤/
///   ComboBox 展开收起+外部点击/Tab 切页/ColorSwatch 选色/ScrollBar 范围钳制
/// 返回 0 = 通过。

#include <cstdio>
#include <string>
#include <d2d1.h>

#include "ui_label.h"
#include "ui_button.h"
#include "ui_checkbox.h"
#include "ui_edit.h"
#include "ui_combobox.h"
#include "ui_tab.h"
#include "ui_colorpicker.h"
#include "ui_scrollbar.h"

using namespace taishen;

// ── STEP 1：Label / Button / CheckBox ──
static bool TestBasic()
{
    UILabel label(L"你好");
    if (label.Text() != L"你好") {
        printf("STEP1 FAIL: label text\n");
        return false;
    }
    label.SetText(L"World");
    if (label.Text() != L"World") {
        printf("STEP1 FAIL: label set text\n");
        return false;
    }

    int clicks = 0;
    UIButton btn(L"确定");
    btn.SetOnClick([&clicks]() { ++clicks; });
    btn.OnClick(0, 0);
    if (clicks != 1) {
        printf("STEP1 FAIL: button click cb\n");
        return false;
    }
    btn.SetEnabled(false);
    btn.OnClick(0, 0);
    if (clicks != 1) {
        printf("STEP1 FAIL: disabled button should not fire\n");
        return false;
    }
    btn.SetEnabled(true);

    bool checked = false;
    UICheckBox cb(L"模糊音");
    cb.SetOnChanged([&checked](bool c) { checked = c; });
    cb.OnClick(0, 0);
    if (!cb.IsChecked() || !checked) {
        printf("STEP1 FAIL: checkbox check\n");
        return false;
    }
    cb.OnClick(0, 0);
    if (cb.IsChecked() || checked) {
        printf("STEP1 FAIL: checkbox uncheck\n");
        return false;
    }
    printf("STEP1 OK label/button/checkbox\n");
    return true;
}

// ── STEP 2：Edit ──
static bool TestEdit()
{
    UIEdit edit;
    edit.SetText(L"abc");
    if (edit.Text() != L"abc") {
        printf("STEP2 FAIL: set text\n");
        return false;
    }
    // OnChar 插入到 caret 尾
    edit.OnChar(L'1');
    if (edit.Text() != L"abc1") {
        printf("STEP2 FAIL: char insert (%ls)\n", edit.Text().c_str());
        return false;
    }
    // 退格（光标在末尾）
    edit.OnKeyDown(VK_BACK, false, false, false);
    if (edit.Text() != L"abc") {
        printf("STEP2 FAIL: backspace\n");
        return false;
    }
    // 方向键 + 插入
    edit.OnKeyDown(VK_LEFT, false, false, false);
    edit.OnKeyDown(VK_LEFT, false, false, false);
    edit.OnChar(L'X');
    if (edit.Text() != L"aXbc") {
        printf("STEP2 FAIL: caret insert (%ls)\n", edit.Text().c_str());
        return false;
    }
    // 数字模式：过滤字母
    UIEdit num;
    num.SetNumeric(1, 20);
    num.OnChar(L'5');
    num.OnChar(L'x');
    if (num.Text() != L"5") {
        printf("STEP2 FAIL: numeric filter (%ls)\n", num.Text().c_str());
        return false;
    }
    printf("STEP2 OK edit (insert/backspace/caret/numeric)\n");
    return true;
}

// ── STEP 3：ComboBox ──
static bool TestComboBox()
{
    UIComboBox combo;
    combo.SetRect({ 0, 0, 100, 28 });
    combo.SetItems({ L"深色", L"浅色", L"跟随系统" });
    combo.SetSelectedIndex(1);
    if (combo.SelectedText() != L"浅色") {
        printf("STEP3 FAIL: selected text\n");
        return false;
    }
    int sel = -1;
    combo.SetOnSelected([&sel](int i) { sel = i; });

    // 展开 → 有子控件（面板行）
    combo.OnClick(0, 0); // 展开
    if (!combo.IsExpanded() || combo.Children().size() != 3) {
        printf("STEP3 FAIL: expand (children=%zu)\n", combo.Children().size());
        return false;
    }
    // 点击面板行（第 0 行）→ 选中 0 并收起 —— 直接走行点击
    UIControl* row0 = combo.Children()[0];
    row0->OnClick(0, 0);
    if (combo.IsExpanded() || sel != 0 || combo.SelectedIndex() != 0) {
        printf("STEP3 FAIL: row select (expanded=%d sel=%d idx=%d)\n",
               combo.IsExpanded(), sel, combo.SelectedIndex());
        return false;
    }
    // 展开后点击外部 → 收起（不改变选中）
    combo.OnClick(0, 0); // 展开
    combo.OnGlobalMouseDown(500, 500);
    if (combo.IsExpanded()) {
        printf("STEP3 FAIL: external click should collapse\n");
        return false;
    }
    printf("STEP3 OK combobox (expand/collapse/external)\n");
    return true;
}

// ── STEP 4：Tab ──
static bool TestTab()
{
    UITab tab;
    tab.SetRect({ 0, 0, 300, 200 });
    auto* p0 = new UILabel(L"页0");
    auto* p1 = new UILabel(L"页1");
    tab.AddPage(L"基础", p0);
    tab.AddPage(L"输入", p1);
    if (tab.PageCount() != 2 || tab.Current() != 0) {
        printf("STEP4 FAIL: add pages\n");
        return false;
    }
    if (!p0->IsVisible() || p1->IsVisible()) {
        printf("STEP4 FAIL: initial page visibility\n");
        return false;
    }
    tab.SetCurrent(1);
    if (tab.Current() != 1 || p0->IsVisible() || !p1->IsVisible()) {
        printf("STEP4 FAIL: switch page visibility\n");
        return false;
    }
    delete p0;
    delete p1;
    printf("STEP4 OK tab (pages/switch)\n");
    return true;
}

// ── STEP 5：ColorSwatch ──
static bool TestColorSwatch()
{
    UIColorSwatch sw;
    sw.SetRect({ 0, 0, 120, 24 });
    D2D1_COLOR_F got{};
    bool fired = false;
    sw.SetOnColorChanged([&](D2D1_COLOR_F c) { got = c; fired = true; });

    sw.OnClick(0, 0); // 点击自身 → 展开色板
    if (!sw.IsExpanded() || sw.Children().size() != 16 * 8) {
        printf("STEP5 FAIL: expand grid (%zu)\n", sw.Children().size());
        return false;
    }
    // 点击一个色格
    sw.Children()[0]->OnClick(0, 0);
    if (!fired || sw.IsExpanded()) {
        printf("STEP5 FAIL: color pick (fired=%d expanded=%d)\n", fired, sw.IsExpanded());
        return false;
    }
    printf("STEP5 OK color swatch (grid/pick)\n");
    return true;
}

// ── STEP 6：ScrollBar ──
static bool TestScrollBar()
{
    UIScrollBar sb;
    sb.SetRect({ 0, 0, 12, 200 });
    int last = -1;
    sb.SetOnScroll([&last](int p) { last = p; });

    sb.SetRange(1000, 200); // maxPos = 800
    if (sb.MaxPos() != 800) {
        printf("STEP6 FAIL: range (%d)\n", sb.MaxPos());
        return false;
    }
    sb.SetPos(900); // 钳制到 800
    if (sb.Pos() != 800) {
        printf("STEP6 FAIL: clamp (%d)\n", sb.Pos());
        return false;
    }
    sb.SetPos(100);
    if (sb.Pos() != 100 || last != 100) {
        printf("STEP6 FAIL: set pos cb (%d,%d)\n", sb.Pos(), last);
        return false;
    }
    sb.SetRange(100, 200); // view > total → maxPos = 0
    if (sb.MaxPos() != 0) {
        printf("STEP6 FAIL: no-scroll range\n");
        return false;
    }
    printf("STEP6 OK scrollbar (range/clamp/cb)\n");
    return true;
}

int wmain()
{
    printf("=== UI Controls smoke test (V0.3.1) ===\n");
    if (!TestBasic())     return 1;
    if (!TestEdit())      return 2;
    if (!TestComboBox())  return 3;
    if (!TestTab())       return 4;
    if (!TestColorSwatch()) return 5;
    if (!TestScrollBar()) return 6;
    printf("ALL TESTS PASSED\n");
    return 0;
}

