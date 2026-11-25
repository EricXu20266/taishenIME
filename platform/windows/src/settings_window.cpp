/// 现代化设置窗体 — 实现（V0.3.4）
///
/// 结构：UIWindow 模态（自绘标题栏 36px 可拖动）+ 左侧导航（4 项）+ 右侧内容面板。
/// 配置：LoadConfig → 控件；控件 → SaveConfig（热加载自动生效）。
/// 替代 settings.rc / resource.h 的 Win32 对话框资源。

#include "settings_window.h"
#include "app_container.h"
#include "config_apply.h"
#include "debug_log.h"
#include "settings_ui_helpers.h"
#include "theme.h"
#include "ui_button.h"
#include "ui_label.h"
#include "ui_render.h"
#include "voice_manager.h"

#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstdlib>
#include <shellapi.h>
#include <windowsx.h>

namespace taishen {

namespace {

using settings_ui::ButtonWidth;
using settings_ui::CardLayout;
using settings_ui::CheckRow;
using settings_ui::FormRow;
using settings_ui::HintText;

// 布局常量
constexpr int kTitleBarH = 36;    // 标题栏高
constexpr int kNavW = 120;        // 左侧导航宽
constexpr int kFooterH = 60;      // 底部按钮栏高（60 - padding 24 = 按钮 36px）
// 应用级配置：延迟删除行（0.2.34 fix——避免在按钮 OnClick 栈内自删除崩溃）
constexpr UINT kMsgRemoveAppRow = WM_APP + 1;

// 双拼方案名（与 config_reader shuangpin_scheme 对应）
const wchar_t* const kSchemes[] = { L"微软双拼", L"小鹤", L"搜狗", L"自然码",
                                    L"紫光(兼容)", L"加加(兼容)" };
const char* const kSchemeKeys[] = { "mspy", "flypy", "sogou", "zrm",
                                    "ziguang", "jiajia" };

/// 浮点显示：固定小数位（VAD 参数用，避免 1.8 显示成 18）
std::wstring FormatFloat(float value, int digits)
{
    wchar_t buf[32] = {};
    swprintf_s(buf, L"%.*f", digits, static_cast<double>(value));
    return std::wstring(buf);
}

/// 浮点解析：非法输入回落到 fallback（用户可能清空或键错）
float ParseFloat(const std::wstring& text, float fallback)
{
    const wchar_t* s = text.c_str();
    wchar_t* end = nullptr;
    const double v = wcstod(s, &end);
    if (end == s || !std::isfinite(v)) {
        return fallback;
    }
    return static_cast<float>(v);
}

/// 双列配色行：每行 2 组「标签+色块」（10 色配色器 5 行排完，省一半高度）。
/// 修复原裸 HBox 把标签/色块压成 28px 宽的错乱。
class ColorRowLayout : public UILayout
{
public:
    ColorRowLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(10);
    }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 4) {
            return;
        }
        constexpr int kLabelW = 70;
        constexpr int kSwatchW = 120;
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + kLabelW, y + h });
        cx += kLabelW + m_gap;
        m_children[1]->SetRect({ cx, y, cx + kSwatchW, y + h });
        cx += kSwatchW + m_gap;
        m_children[2]->SetRect({ cx, y, cx + kLabelW, y + h });
        cx += kLabelW + m_gap;
        m_children[3]->SetRect({ cx, y, cx + kSwatchW, y + h });
    }
};

/// 双列表单行：2 组「标签+控件」（窗口配置 4 项 2×2，省高度）
class FormRow2Layout : public UILayout
{
public:
    explicit FormRow2Layout(int labelW = 90)
        : UILayout(UILayout::Dir::H)
        , m_labelW(labelW)
    {
        SetGap(12);
    }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 4) {
            return;
        }
        const int groupW = (w - m_gap) / 2;
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + m_labelW, y + h });
        m_children[1]->SetRect({ cx + m_labelW + m_gap, y, cx + groupW, y + h });
        cx += groupW + m_gap;
        m_children[2]->SetRect({ cx, y, cx + m_labelW, y + h });
        m_children[3]->SetRect({ cx + m_labelW + m_gap, y, cx + groupW, y + h });
    }

private:
    int m_labelW;
};

/// 应用级配置行：Edit 弹性 + 模式下拉/行内/vim/删除固定宽。
/// （审计 P2-4：原 HBox 把子控件都按 28px 宽布局——edit 极窄无法输入）
class AppRowLayout : public UILayout
{
public:
    AppRowLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(4);
    }

    // 行固定高 32——父列表按行数×行高结算高度，行不参与弹性挤压
    int PreferredHeight(int /*width*/) const override { return 32; }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 6) {
            return;
        }
        // 顺序：0=edit 弹性 / 1=中英模式 / 2=行内策略 / 3=自绘策略 / 4=vim / 5=×
        constexpr int kModeW = 82;
        constexpr int kInlineW = 86;
        constexpr int kSelfDrawW = 86;
        constexpr int kVimW = 54;
        constexpr int kDelW = 24;
        const int fixed = kModeW + kInlineW + kSelfDrawW + kVimW + kDelW + 5 * m_gap;
        int editW = w - fixed;
        if (editW < 60) {
            editW = 60;
        }
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + editW, y + h });
        cx += editW + m_gap;
        m_children[1]->SetRect({ cx, y, cx + kModeW, y + h });
        cx += kModeW + m_gap;
        m_children[2]->SetRect({ cx, y, cx + kInlineW, y + h });
        cx += kInlineW + m_gap;
        m_children[3]->SetRect({ cx, y, cx + kSelfDrawW, y + h });
        cx += kSelfDrawW + m_gap;
        m_children[4]->SetRect({ cx, y, cx + kVimW, y + h });
        cx += kVimW + m_gap;
        m_children[5]->SetRect({ cx, y, cx + kDelW, y + h });
    }
};

/// 左侧导航容器：固定宽 kNavW——修复 content(H) 里 nav/panel 双弹性
/// 平分宽度导致导航占半屏、右侧配置页被压缩。
class NavLayout : public UILayout
{
public:
    NavLayout()
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(8);
        SetGap(6);
    }

    // content(H) 的 LayoutH 把 PreferredHeight 当宽度用——返回固定导航宽
    int PreferredHeight(int /*width*/) const override { return kNavW; }
};

/// 底部按钮栏：左组（打开配置文件/恢复默认）靠左，右组（取消/确定）靠右，
/// 全部按内容宽——修复 LayoutH 把按钮压成 28px 导致文字换行挤扁。
class FooterLayout : public UILayout
{
public:
    FooterLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetPadding(12);
        SetGap(8);
    }

    // 底部按钮栏固定高（root 里 content 弹性占剩余）——
    // 缺省 UILayout 弹性会让 footer 与 content 平分窗口高度（按钮被拉成巨块）
    int PreferredHeight(int /*width*/) const override { return kFooterH; }

    void Layout() override
    {
        const int x = m_rect.left + m_padding;
        const int y = m_rect.top + m_padding;
        const int w = m_rect.right - m_rect.left - 2 * m_padding;
        const int h = m_rect.bottom - m_rect.top - 2 * m_padding;
        if (w <= 0 || m_children.size() < 4) {
            return;
        }
        // 子控件：0=打开配置文件 1=恢复默认 2=确定(主) 3=取消
        const int w0 = ButtonWidth(L"打开配置文件");
        const int w1 = ButtonWidth(L"恢复默认");
        const int w2 = ButtonWidth(L"确定");
        const int w3 = ButtonWidth(L"取消");
        int cx = x;
        m_children[0]->SetRect({ cx, y, cx + w0, y + h });
        cx += w0 + m_gap;
        m_children[1]->SetRect({ cx, y, cx + w1, y + h });
        // 右组：确定最右，取消在左
        int rx = x + w;
        m_children[2]->SetRect({ rx - w2, y, rx, y + h });
        rx -= w2 + m_gap;
        m_children[3]->SetRect({ rx - w3, y, rx, y + h });
    }
};

/// 应用级配置列表容器：高度随行数自适应（行数×行高+gaps）。
/// 修复：原弹性容器与头部平分卡片高度 → 多行挤压在同一固定区域。
class AppListLayout : public UILayout
{
public:
    AppListLayout()
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(0);
        SetGap(4);
    }

    // 高度 = 可见行数 × 行高 + gaps（动态列表自适应）
    int PreferredHeight(int /*width*/) const override
    {
        constexpr int kRowH = 32;
        int n = 0;
        for (const UIControl* c : m_children) {
            if (c->IsVisible()) {
                ++n;
            }
        }
        return n > 0 ? n * kRowH + (n - 1) * m_gap : kRowH;
    }
};

/// 卡片头部：标题弹性 + 右侧按钮按内容宽（修复 "+ 添加程序" 挤扁）
class HeaderLayout : public UILayout
{
public:
    HeaderLayout()
        : UILayout(UILayout::Dir::H)
    {
        SetGap(8);
    }

    // 头部固定行高——不与下方列表平分卡片高度（列表需按行数自适应）
    int PreferredHeight(int /*width*/) const override { return 32; }

    void Layout() override
    {
        const int x = m_rect.left;
        const int y = m_rect.top;
        const int w = m_rect.right - m_rect.left;
        const int h = m_rect.bottom - m_rect.top;
        if (w <= 0 || m_children.size() < 2) {
            return;
        }
        if (m_children.size() >= 3) {
            const int w1 = ButtonWidth(L"拾取进程…");
            const int w2 = ButtonWidth(L"+ 添加");
            const int titleW = (std::max)(80, w - w1 - w2 - m_gap * 2);
            m_children[0]->SetRect({ x, y, x + titleW, y + h });
            const int x1 = x + titleW + m_gap;
            m_children[1]->SetRect({ x1, y, x1 + w1, y + h });
            m_children[2]->SetRect({ x1 + w1 + m_gap, y, x + w, y + h });
            return;
        }
        const int btnW = ButtonWidth(L"+ 添加程序");
        m_children[0]->SetRect({ x, y, x + w - btnW - m_gap, y + h });
        m_children[1]->SetRect({ x + w - btnW, y, x + w, y + h });
    }
};

// ===========================================================================
// V0.3.6：ScrollPanel（可滚动内容面板）
// ===========================================================================

/// 可滚动内容面板：垂直排列子控件，内容超高时滚轮滚动 + 裁剪。
/// 子控件 Y 坐标随滚动整体偏移（重 Layout），绘制时 PushClip 到面板矩形。
class ScrollPanel : public UILayout
{
public:
    explicit ScrollPanel()
        : UILayout(UILayout::Dir::V)
    {
        SetPadding(16);
        SetGap(14);
    }

    void Layout() override
    {
        // P2-2：面板宽度无效时不布局（防御——避免子控件保留旧滚动偏移污染 m_maxScroll）
        if (Width() <= 0) {
            return;
        }
        UILayout::Layout();
        // 内容总高 = 子控件底部 - 面板顶（弹性子控件占满时按实际内容算）
        int maxBottom = m_rect.top;
        for (const UIControl* c : m_children) {
            if (c->IsVisible()) {
                maxBottom = (std::max)(maxBottom, static_cast<int>(c->Rect().bottom));
            }
        }
        m_contentHeight = maxBottom - m_rect.top;
        m_maxScroll = (std::max)(0, m_contentHeight - Height());
        if (m_scrollY > m_maxScroll) {
            m_scrollY = m_maxScroll;
        }
        // 每次从原始布局出发做偏移——严禁基于当前 rect 相对累加
        // （旧实现多次滚动后内容漂移失控，回滚时坐标更乱 →「不能往上滚」）
        ApplyOffset();
    }

    void OnMouseWheel(int delta) override
    {
        if (m_maxScroll <= 0) {
            // P2-5：无滚动内容时向上冒泡（父级可滚动容器仍能响应）
            UILayout::OnMouseWheel(delta);
            return;
        }
        const int target = m_scrollY - delta / 2; // 120/格 → 60px
        const int clamped = (std::clamp)(target, 0, m_maxScroll);
        if (clamped == m_scrollY) {
            return;
        }
        m_scrollY = clamped;
        // 重做原始布局 + 偏移（保证坐标从基线出发，无累积漂移）
        Layout();
        Invalidate();
    }

    void Draw(UIRenderer& r, const UITheme& t) override
    {
        // 裁剪前验证矩形有效（Rect 未分配/零尺寸时跳过，避免 D2D clip 异常）
        const float x = static_cast<float>(X());
        const float y = static_cast<float>(Y());
        const float w = static_cast<float>(Width());
        const float h = static_cast<float>(Height());
        if (w > 0.0f && h > 0.0f) {
            r.PushClip(D2D1::RectF(x, y, x + w, y + h));
            UILayout::Draw(r, t);
            r.PopClip();
        } else {
            UILayout::Draw(r, t);
        }
    }

    UIControl* HitTestTree(int x, int y) override
    {
        if (!IsVisible() || !HitTest(x, y)) {
            return nullptr;
        }
        return UILayout::HitTestTree(x, y);
    }

private:
    /// 基于当前（已由 UILayout::Layout 重置的）原始布局整体偏移。
    /// 每次滚动都从原始布局重建——无累积漂移。
    void ApplyOffset()
    {
        const int dy = -m_scrollY;
        if (dy == 0) {
            return;
        }
        for (UIControl* c : m_children) {
            if (!c->IsVisible()) {
                continue;
            }
            const RECT r = c->Rect();
            c->SetRect({ r.left, r.top + dy, r.right, r.bottom + dy });
            if (auto* sub = dynamic_cast<UILayout*>(c)) {
                sub->Layout();
            }
        }
    }

    int m_scrollY = 0;
    int m_maxScroll = 0;
    int m_contentHeight = 0;
};

} // namespace

// ===========================================================================
// 左侧导航项（V0.3.6：accent 左边界条 + 浅色底 + accent 文字）
// 定义在 taishen 命名空间（settings_window.h forward-declare 成员指针）
// ===========================================================================
class CSettingsWindow::NavItem : public UIControl
{
public:
    explicit NavItem(std::wstring text)
        : m_text(std::move(text))
    {
    }
    void SetSelected(bool s) { m_selected = s; Invalidate(); }
    bool IsSelected() const { return m_selected; }
    int PreferredHeight(int /*width*/) const override { return 34; }
    void Draw(UIRenderer& r, const UITheme& t) override
    {
        const float x = static_cast<float>(X());
        const float y = static_cast<float>(Y());
        const D2D1_RECT_F rc = D2D1::RectF(x, y, x + static_cast<float>(Width()),
                                           y + static_cast<float>(Height()));
        D2D1_COLOR_F bg = t.cardBg;
        D2D1_COLOR_F fg = t.text;
        if (m_selected) {
            bg = t.hoverBg;      // 选中：浅色底
            fg = t.accent;       // accent 文字
        } else if (IsHovered()) {
            bg = t.hoverBg;
        }
        r.FillRoundedRect(rc, 6.0f, bg);
        if (m_selected) {
            // accent 左边界条（4px 竖条，垂直居中）
            r.FillRoundedRect(
                D2D1::RectF(x, y + 6.0f, x + 4.0f, y + static_cast<float>(Height()) - 6.0f),
                2.0f, t.accent);
        }
        r.DrawText(m_text, rc, t.fontSize, fg, m_selected,
                   DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    }
    void OnClick(int /*x*/, int /*y*/) override
    {
        if (m_onClick) {
            m_onClick();
        }
    }

    void SetOnClick(std::function<void()> cb) { m_onClick = std::move(cb); }

private:
    std::wstring m_text;
    bool m_selected = false;
    std::function<void()> m_onClick;
};

// ===========================================================================
// 构造 / 运行
// ===========================================================================
CSettingsWindow::CSettingsWindow(const std::wstring& dllDir, int initialPage)
    : m_dllDir(dllDir)
    , m_currentPage((initialPage >= 0 && initialPage < kSettingsPageCount) ? initialPage : 0)
{
    m_cfg = LoadConfig(dllDir);
    SetTitleBarHeight(kTitleBarH);
}

int CSettingsWindow::Run()
{
    // 模态窗口：居中显示，标题栏可拖动（V0.3.6：560×440 → 640×520）
    // 末参 true：按 DPI 把 640×520 DIP 换算成物理尺寸创建（125%/150% 下内容不溢出）
    if (!Create(L"TaishenSettingsWindow", 640, 520, false, false, true)) {
        return IDCANCEL;
    }
    SetFollowSystemTheme(true);
    BuildUI();
    // V0.3.6 fix：原代码漏调页面构建函数——控件全是 nullptr，ApplyToUI 空指针崩溃。
    // 页面内容必须在 ApplyToUI 前构建。
    BuildBasicPage();
    BuildInputPage();
    BuildAppearancePage();
    BuildAdvancedPage();
    BuildSymbolPage();
    BuildVoicePage();
    BuildKeyPage();
    BuildCertPage();
    BuildDictPage();
    ApplyToUI();
    return RunModal();
}

// ===========================================================================
// 标题栏 + 关闭按钮
// ===========================================================================
void CSettingsWindow::OnRender(UIRenderer& r)
{
    r.Clear(m_theme.bg);
    RECT rc{};
    GetClientRect(m_hwnd, &rc);
    // D2D 渲染目标已设置窗口 DPI，绘图坐标为 DIP → 物理宽度需除以 DpiScale()
    const float scale = DpiScale();
    const float w = static_cast<float>(rc.right - rc.left) / scale;
    // 标题栏
    r.FillRect(D2D1::RectF(0, 0, w, static_cast<float>(kTitleBarH)),
               m_theme.cardBg);
    r.DrawText(L"泰深输入法设置",
               D2D1::RectF(12, 0, 300, static_cast<float>(kTitleBarH)),
               m_theme.fontSize, m_theme.text, true,
               DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    // 关闭按钮 ✕（右侧）
    r.DrawText(L"✕",
               D2D1::RectF(w - 46, 0, w - 8, static_cast<float>(kTitleBarH)),
               m_theme.fontSize, m_theme.textDim, false,
               DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    r.DrawLine(0, static_cast<float>(kTitleBarH), w, static_cast<float>(kTitleBarH),
               m_theme.border, 1.0f);
    // 内容区
    if (m_root != nullptr) {
        m_root->Draw(r, m_theme);
    }
    // V0.3.6：弹出层（下拉面板/色板）浮最上层，不被 ScrollPanel 裁剪
    DrawPopups(r, m_theme);
}

LRESULT CSettingsWindow::HandleMessage(UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case kMsgRemoveAppRow:
        // 延迟删除（0.2.34）：点击回调返回后再删行——避免在按钮自身
        // 的 OnClick 栈内 RebuildAppList 删除该按钮导致悬垂访问崩溃
        RemoveAppRow(static_cast<size_t>(wp));
        return 0;
    case WM_LBUTTONUP: {
        // 标题栏右侧 ✕ 点击 → 取消关闭（统一换算为 DIP 逻辑坐标）
        const float scale = DpiScale();
        const float x = static_cast<float>(GET_X_LPARAM(lp)) / scale;
        const float y = static_cast<float>(GET_Y_LPARAM(lp)) / scale;
        RECT rc{};
        GetClientRect(m_hwnd, &rc);
        const float w = static_cast<float>(rc.right - rc.left) / scale;
        if (y < static_cast<float>(kTitleBarH) && x > w - 50.0f) {
            EndModal(IDCANCEL);
            return 0;
        }
        break;
    }
    default:
        break;
    }
    return UIWindow::HandleMessage(msg, wp, lp);
}

// ===========================================================================
// 整体骨架
// ===========================================================================
void CSettingsWindow::BuildUI()
{
    auto* root = new UILayout(UILayout::Dir::V);
    root->SetPadding(0);
    root->SetGap(0);

    // 内容区（标题栏下方）：左侧导航 + 右侧面板
    auto* content = new UILayout(UILayout::Dir::H);
    content->SetPadding(0);
    content->SetGap(0);

    // 左侧导航（V0.3.6：NavLayout 固定宽 120px，右侧面板弹性占剩余）
    auto* nav = new NavLayout();
    const wchar_t* kNavNames[kSettingsPageCount] = {
        L"基础", L"输入", L"外观", L"高级", L"符号", L"语音", L"按键", L"证书", L"词库"
    };
    for (int i = 0; i < kSettingsPageCount; ++i) {
        auto* item = new NavItem(kNavNames[i]);
        item->SetOnClick([this, i]() { SwitchPage(i); });
        item->SetSelected(i == m_currentPage);
        m_navItems[i] = item;
        nav->AddChild(item);
    }
    content->AddChild(nav);

    // 右侧面板（V0.3.6：页面根 = ScrollPanel，内容超高可滚动）
    auto* panel = new UILayout(UILayout::Dir::V);
    panel->SetPadding(0);
    panel->SetGap(0);
    for (int i = 0; i < kSettingsPageCount; ++i) {
        m_pageRoots[i] = new ScrollPanel();
        m_pageRoots[i]->SetVisible(i == m_currentPage);
        panel->AddChild(m_pageRoots[i]);
    }
    content->AddChild(panel);

    root->AddChild(content);

    // 底部按钮栏（V0.3.6：FooterLayout——按钮按内容宽，确定/取消靠右）
    auto* footer = new FooterLayout();
    auto* btnOpen = new UIButton(L"打开配置文件");
    btnOpen->SetOnClick([this]() {
        const std::wstring ini = ResolveConfigWritePath(m_dllDir);
        const std::wstring target = ini.empty() ? ResolveConfigPath(m_dllDir) : ini;
        if (!ini.empty() && GetFileAttributesW(ini.c_str()) == INVALID_FILE_ATTRIBUTES) {
            SaveConfig(m_dllDir, m_cfg);
        }
        if (!target.empty()) {
            ShellExecuteW(nullptr, L"open", target.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        }
    });
    auto* btnDefaults = new UIButton(L"恢复默认");
    btnDefaults->SetOnClick([this]() {
        CollectFromUI();
        m_cfg = ResetConfigToDefaultsPreservingApps(m_cfg);
        ApplyToUI();
        OnThemeModeChanged(m_cfg.theme_mode);
    });
    auto* btnOk = new UIButton(L"确定");
    btnOk->SetPrimary(true);
    btnOk->SetOnClick([this]() { SaveAndClose(); });
    auto* btnCancel = new UIButton(L"取消");
    btnCancel->SetOnClick([this]() { EndModal(IDCANCEL); });
    footer->AddChild(btnOpen);
    footer->AddChild(btnDefaults);
    footer->AddChild(btnOk);
    footer->AddChild(btnCancel);
    root->AddChild(footer);

    SetRoot(root);
}

void CSettingsWindow::SwitchPage(int idx)
{
    if (idx < 0 || idx >= kSettingsPageCount || idx == m_currentPage) {
        return;
    }
    m_pageRoots[m_currentPage]->SetVisible(false);
    if (m_navItems[m_currentPage] != nullptr) {
        m_navItems[m_currentPage]->SetSelected(false);
    }
    m_currentPage = idx;
    m_pageRoots[m_currentPage]->SetVisible(true);
    if (m_navItems[m_currentPage] != nullptr) {
        m_navItems[m_currentPage]->SetSelected(true);
    }
    // 新页此前未布局（LayoutV 跳过不可见子控件）——整树重排
    Relayout();
    Invalidate();
}

/// V0.3.6：重排当前页（卡片高度变化后更新滚动范围）
void CSettingsWindow::ReflowPage()
{
    if (m_currentPage >= 0 && m_currentPage < kSettingsPageCount &&
        m_pageRoots[m_currentPage] != nullptr) {
        m_pageRoots[m_currentPage]->Layout();
    }
    Invalidate();
}

// ===========================================================================
// 页 0 基础
// ===========================================================================
void CSettingsWindow::BuildBasicPage()
{
    UILayout* page = m_pageRoots[0];

    // Q2.2 卡片 0：迁移预设
    BuildPresetCard(page);

    // 卡片 1：候选设置
    auto* card1 = new CardLayout(L"候选设置");
    m_editCandidate = new UIEdit();
    m_editCandidate->SetNumeric(1, 20);
    m_editCandidate->SetPlaceholder(L"1-20");
    card1->AddChild(FormRow(L"候选词数量", m_editCandidate));

    m_editFontFace = new UIEdit();
    card1->AddChild(FormRow(L"候选窗字体", m_editFontFace));

    m_editFontSize = new UIEdit();
    m_editFontSize->SetNumeric(12, 32);
    m_editFontSize->SetPlaceholder(L"12-32");
    card1->AddChild(FormRow(L"正文字号", m_editFontSize));

    m_editLabelFormat = new UIEdit();
    m_editLabelFormat->SetPlaceholder(L"%d. 或 ①");
    card1->AddChild(FormRow(L"候选标签格式", m_editLabelFormat));
    m_chkVerticalCandidates = CheckRow(card1, L"竖排候选窗（每行显示一个候选词）");
    page->AddChild(card1);

    // 卡片 2：输入行为
    auto* card2 = new CardLayout(L"输入行为");
    m_chkInline = CheckRow(card2, L"行内预编辑（拼音写组合，候选窗不重复）");
    m_chkDebugLog = CheckRow(card2, L"诊断日志（默认关，开发调试用）");
    page->AddChild(card2);
}

// ===========================================================================
// 页 1 输入
// ===========================================================================
void CSettingsWindow::BuildInputPage()
{
    UILayout* page = m_pageRoots[1];

    // 卡片 1：输入模式
    auto* card1 = new CardLayout(L"输入模式");
    BuildFuzzyGroupRows(card1);
    m_chkCorrection = CheckRow(card1, L"智能纠错（相邻键容错）");
    m_chkMix = CheckRow(card1, L"中英混输");
    m_chkTraditional = CheckRow(card1, L"简繁转换");
    m_chkShuangpin = CheckRow(card1, L"双拼模式");
    m_comboScheme = new UIComboBox();
    m_comboScheme->SetItems({ kSchemes[0], kSchemes[1], kSchemes[2],
                              kSchemes[3], kSchemes[4], kSchemes[5] });
    card1->AddChild(FormRow(L"双拼方案", m_comboScheme));
    m_chkSpFullPreedit = CheckRow(card1, L"双拼组合区显示全拼音节（关=显示按键）");
    card1->AddChild(FormRow(
        L"键位",
        new UILabel(L"微软与搜狗完全相同；紫光/加加按微软键位兼容，不建议选用"), 70));
    m_chkPhrase = CheckRow(card1, L"快捷短语（简码→短语）");
    m_chkAsciiPunct = CheckRow(card1, L"英文标点透传");
    m_chkPairPunct = CheckRow(card1, L"配对符号成对上屏（《》（）等光标居中）");
    m_chkEmoji = CheckRow(card1, L"Emoji 候选");
    page->AddChild(card1);

    // 卡片 2：智能候选（P0-2 排序 / P1-1 联想；专业词库 v2 自动加载，无需配置）
    auto* card2 = new CardLayout(L"智能候选");
    m_comboSortMode = new UIComboBox();
    m_comboSortMode->SetItems({ L"默认（词频+长词过滤）", L"单字优先", L"长词优先" });
    card2->AddChild(FormRow(L"候选排序", m_comboSortMode));
    m_chkContextAssoc = CheckRow(card2, L"智能联想（学习你的用词习惯）");
    m_chkCompletion = CheckRow(card2, L"前缀补全（单字后跟更长词，如 ji→继续）");
    page->AddChild(card2);
}

// ===========================================================================
// 页 2 外观
// ===========================================================================
void CSettingsWindow::BuildAppearancePage()
{
    UILayout* page = m_pageRoots[2];

    // 卡片 1：主题
    auto* card1 = new CardLayout(L"主题");
    m_comboThemeMode = new UIComboBox();
    m_comboThemeMode->SetItems({ L"跟随系统", L"深色", L"浅色" });
    m_comboThemeMode->SetOnSelected([this](int idx) { OnThemeModeChanged(idx); });
    card1->AddChild(FormRow(L"主题模式", m_comboThemeMode));

    // 主题色 10 项（候选窗配色）——V0.3.6：2 列网格，5 行排完（省一半高度）
    const wchar_t* const kColorNames[10] = {
        L"背景", L"正文", L"序号", L"注释", L"边框",
        L"选中背景", L"选中文字", L"选中序号", L"页码/次要", L"选中标记",
    };
    for (int i = 0; i < 10; i += 2) {
        auto* row = new ColorRowLayout();
        auto* lbl0 = new UILabel(kColorNames[i]);
        m_swatches[i] = new UIColorSwatch();
        row->AddChild(lbl0);
        row->AddChild(m_swatches[i]);
        if (i + 1 < 10) {
            auto* lbl1 = new UILabel(kColorNames[i + 1]);
            m_swatches[i + 1] = new UIColorSwatch();
            row->AddChild(lbl1);
            row->AddChild(m_swatches[i + 1]);
        }
        card1->AddChild(row);
    }
    page->AddChild(card1);

    // 卡片 2：窗口（V0.3.6：4 项 2×2 两行排完）
    auto* card2 = new CardLayout(L"窗口");
    m_editCorner = new UIEdit();
    m_editCorner->SetNumeric(1, 16);
    m_editCorner->SetPlaceholder(L"1-16");
    m_editHilite = new UIEdit();
    m_editHilite->SetNumeric(1, 16);
    m_editHilite->SetPlaceholder(L"1-16");
    {
        auto* row = new FormRow2Layout();
        auto* lbl1 = new UILabel(L"窗口圆角");
        auto* lbl2 = new UILabel(L"高亮圆角");
        row->AddChild(lbl1);
        row->AddChild(m_editCorner);
        row->AddChild(lbl2);
        row->AddChild(m_editHilite);
        card2->AddChild(row);
    }
    m_editPadding = new UIEdit();
    m_editPadding->SetNumeric(0, 20);
    m_editPadding->SetPlaceholder(L"0-20");
    m_editSpacing = new UIEdit();
    m_editSpacing->SetNumeric(0, 40);
    m_editSpacing->SetPlaceholder(L"0-40");
    {
        auto* row = new FormRow2Layout();
        auto* lbl1 = new UILabel(L"内边距");
        auto* lbl2 = new UILabel(L"候选间距");
        row->AddChild(lbl1);
        row->AddChild(m_editPadding);
        row->AddChild(lbl2);
        row->AddChild(m_editSpacing);
        card2->AddChild(row);
    }
    page->AddChild(card2);
}

void CSettingsWindow::OnThemeModeChanged(int mode)
{
    // 切换深/浅预设时同步 10 个色板（跟随系统不动色板）。
    // 显式成员数组避免依赖 CandidateTheme 字段内存顺序（V0.3.5 审查加固）
    const auto fill = [this](const CandidateTheme& t) {
        const D2D1_COLOR_F colors[10] = {
            t.bg, t.text, t.label, t.comment, t.border,
            t.highlight_bg, t.highlight_text, t.highlight_label, t.dim, t.mark,
        };
        for (int i = 0; i < 10; ++i) {
            m_swatches[i]->SetColor(colors[i]);
        }
    };
    // V0.3.6 修复：选深色/浅色实际切换窗口主题（此前只填色板、界面无变化）
    if (mode == 1) {
        SetTheme(UIThemeDark());
        SetFollowSystemTheme(false);
        fill(CandidateTheme::Default());
    } else if (mode == 2) {
        SetTheme(UIThemeLight());
        SetFollowSystemTheme(false);
        fill(LightTheme());
    } else {
        // 跟随系统
        SetTheme(UIThemeCurrent());
        SetFollowSystemTheme(true);
    }
    Invalidate();
}

// ===========================================================================
// 页 3 高级
// ===========================================================================
void CSettingsWindow::BuildAdvancedPage()
{
    UILayout* page = m_pageRoots[3];

    // 卡片 1：应用级配置
    auto* card1 = new CardLayout(L"应用级配置");
    auto* appHeader = new HeaderLayout();
    auto* appTitle = new UILabel(L"进程名（如 cmd.exe），按应用覆盖规则");
    auto* btnPick = new UIButton(L"拾取进程…");
    btnPick->SetOnClick([this]() { ShowAppPickerMenu(); });
    auto* btnAdd = new UIButton(L"+ 添加");
    btnAdd->SetOnClick([this]() { AddAppRow(); });
    appHeader->AddChild(appTitle);
    appHeader->AddChild(btnPick);
    appHeader->AddChild(btnAdd);
    card1->AddChild(appHeader);

    // V0.3.6：AppListLayout——列表高度随行数自适应（多行不挤压）
    m_appList = new AppListLayout();
    card1->AddChild(m_appList);
    page->AddChild(card1);
}

void CSettingsWindow::AddAppRow()
{
    m_appData.push_back({});
    RebuildAppList();
}

void CSettingsWindow::RemoveAppRow(size_t idx)
{
    if (idx < m_appData.size()) {
        m_appData.erase(m_appData.begin() + static_cast<ptrdiff_t>(idx));
        RebuildAppList();
    }
}

void CSettingsWindow::RebuildAppList()
{
    if (m_appList == nullptr) {
        return;
    }
    // 清理旧行控件（行控件由本函数 new，安全 delete）
    m_appList->RemoveAllChildren(true);

    const wchar_t* const kModes[] = { L"跟随全局", L"默认英文", L"默认中文" };
    const wchar_t* const kInlineModes[] = { L"行内:默认", L"行内:开启", L"行内:禁用" };
    const wchar_t* const kSelfDrawModes[] = { L"自绘:默认", L"强制自绘", L"禁止自绘" };
    for (size_t i = 0; i < m_appData.size(); ++i) {
        // V0.3.6：AppRowLayout——edit 弹性撑满，固定项定宽（P2-4）
        auto* row = new AppRowLayout();

        auto* edit = new UIEdit();
        edit->SetText(m_appData[i].name);
        edit->SetPlaceholder(L"进程名.exe");
        edit->SetOnChanged([this, i](const std::wstring& v) { m_appData[i].name = v; });

        auto* combo = new UIComboBox();
        combo->SetItems({ kModes[0], kModes[1], kModes[2] });
        combo->SetSelectedIndex(m_appData[i].mode);
        combo->SetOnSelected([this, i](int m) { m_appData[i].mode = m; });

        auto* comboInline = new UIComboBox();
        comboInline->SetItems({ kInlineModes[0], kInlineModes[1], kInlineModes[2] });
        comboInline->SetSelectedIndex(m_appData[i].inlineMode);
        comboInline->SetOnSelected([this, i](int m) { m_appData[i].inlineMode = m; });

        auto* comboSelfDraw = new UIComboBox();
        comboSelfDraw->SetItems({ kSelfDrawModes[0], kSelfDrawModes[1], kSelfDrawModes[2] });
        comboSelfDraw->SetSelectedIndex(m_appData[i].selfDraw);
        comboSelfDraw->SetOnSelected([this, i](int m) { m_appData[i].selfDraw = m; });

        auto* chkVim = new UICheckBox(L"vim");
        chkVim->SetChecked(m_appData[i].vimOn);
        chkVim->SetOnChanged([this, i](bool on) { m_appData[i].vimOn = on; });

        auto* btnDel = new UIButton(L"×");
        // 0.2.34 fix：PostMessage 延迟删除——RebuildAppList 会删除本按钮自身，
        // 若在 OnClick 栈内同步执行，回调返回后访问悬垂 this → 崩溃
        btnDel->SetOnClick([this, i]() {
            PostMessageW(m_hwnd, kMsgRemoveAppRow, static_cast<WPARAM>(i), 0);
        });

        row->AddChild(edit);
        row->AddChild(combo);
        row->AddChild(comboInline);
        row->AddChild(comboSelfDraw);
        row->AddChild(chkVim);
        row->AddChild(btnDel);
        m_appList->AddChild(row);
    }
    // 空列表提示（V0.3.6：文本缩短单行——原 50+ 字 wrap 换行但行高不足导致截断）
    if (m_appData.empty()) {
        auto* hint = new UILabel(L"未配置。点「添加程序」配置 cmd.exe/nvim-qt.exe 等规则");
        hint->SetDim(true);
        hint->SetWrap(false);
        m_appList->AddChild(hint);
    }
    // 重新布局列表 + 整页重排（卡片高度变化 → 滚动范围更新）
    m_appList->Layout();
    ReflowPage();
}

// ===========================================================================
// 页 4 符号速查（0.2.33）
// ===========================================================================
void CSettingsWindow::BuildSymbolPage()
{
    UILayout* page = m_pageRoots[4];

    // 说明（收进卡片，避免标题与列表间距过大）
    auto* hintCard = new CardLayout(L"符号输入");
    hintCard->AddChild(FormRow(L"入口", new UILabel(L"按 v 立即显示常用符号；v+数字/码 直达更多"), 70));
    hintCard->AddChild(FormRow(L"选词", new UILabel(L"数字 1-9 选词，空格选首个，+/- 翻页"), 70));
    hintCard->AddChild(FormRow(L"双拼", new UILabel(L"v 为 zh 声母，符号模式自动排除"), 70));
    page->AddChild(hintCard);

    // 数字模式
    auto* card1 = new CardLayout(L"数字模式 v1-v9（QQ 拼音同款）");
    const wchar_t* kDigits[][2] = {
        { L"v1", L"序号 ① ⑪ ㉑ · ⑴ ⒈ · ⅰ Ⅰ" },
        { L"v2", L"数学 ± √ ∞ ∑ · ≈ ≠ ≤ ≥" },
        { L"v3", L"标点 、 。 「 」 《 》 〖 〗" },
        { L"v4", L"箭头 → ← ↑ ↓ · ↔ ⇒ ➜" },
        { L"v5", L"单位货币 ℃ ㎞ ㎡ · ¥ € £" },
        { L"v6", L"希腊 α β γ · Ω（大小写）" },
        { L"v7", L"特殊符号 © ® ™ · ☯ ⚡ ⚠" },
        { L"v8", L"拼音注音 ā á ǎ · ㄅ ㄆ ˉ" },
        { L"v9", L"部首笔画 一 丨 ㇀ · ⿰" },
    };
    for (const auto& row : kDigits) {
        card1->AddChild(FormRow(row[0], new UILabel(row[1]), 60));
    }
    page->AddChild(card1);

    // 分类码模式（label 控制在 9 字符内避免裁切）
    auto* card2 = new CardLayout(L"分类码模式（rime/雾凇同款，183 分类全量）");
    const wchar_t* kCodes[][2] = {
        { L"vbd", L"标点 、 。 「 」 《 》" },
        { L"vjt", L"箭头 → ← ↑ ↓（160+）" },
        { L"vsx", L"数学 ≈ ≠ ≤ · ± × ÷" },
        { L"vdw", L"单位 ℃ ㎞ ㎡ Å" },
        { L"vhb", L"货币 ¥ € £ $ ￥" },
        { L"vxl/xld", L"希腊 α … Ω 大小写" },
        { L"vszq", L"圆序号 ① ⑪ ⓪" },
        { L"vszh/szd", L"括号·点 ⑴ ⒈" },
        { L"vzmq", L"圈字母 ⓐ ⓑ ⓒ" },
        { L"vyf", L"月份 一月 · ㋀ ㋁" },
        { L"vtg/dz/gz", L"干支 甲 子 · 甲子" },
        { L"vjq", L"节气 立春 … 冬至" },
        { L"vkx/bh", L"部首笔画 一 丨 ㇀" },
        { L"vjm/vpjm", L"假名 の ノ" },
        { L"vbg/lssg", L"八卦/六十四卦 ☰ ䷀" },
        { L"vxz/xzm", L"星座 ♈ 白羊座" },
    };
    for (const auto& row : kCodes) {
        card2->AddChild(FormRow(row[0], new UILabel(row[1]), 105));
    }
    page->AddChild(card2);

    // 快捷短码（长 label 拆行）
    auto* card3 = new CardLayout(L"快捷短码（高频直达）");
    const wchar_t* kShortcuts[][2] = {
        { L"vdui", L"✓ ✔ ✅" },
        { L"vcuo", L"✗ ✘ ❎ ❌" },
        { L"vpi", L"π" },
        { L"vgenhao", L"√" },
        { L"vno", L"の（之的日文版）" },
        { L"vshang/vxia", L"↑ ↓" },
        { L"vzuo/vyou", L"← →" },
        { L"vdu", L"° °C °F" },
        { L"vyinwei/suoyi", L"∵ ∴" },
    };
    for (const auto& row : kShortcuts) {
        card3->AddChild(FormRow(row[0], new UILabel(row[1]), 140));
    }
    page->AddChild(card3);
}

// ===========================================================================
// 页 5 语音（V0.5.9，SPEC 5.4）
// ===========================================================================
void CSettingsWindow::BuildVoicePage()
{
    UILayout* page = m_pageRoots[5];

    // 卡片 1：总开关与状态
    auto* card1 = new CardLayout(L"语音输入");
    m_chkVoiceEnabled = CheckRow(card1, L"启用语音输入（工具栏麦克风按钮）");
    m_lblVoiceStatus = new UILabel();
    m_lblVoiceStatus->SetColor(D2D1::ColorF(0x6AA84F, 1.0f));
    card1->AddChild(FormRow(L"引擎状态", m_lblVoiceStatus, 70));
    page->AddChild(card1);

    // 卡片 2：模型管理
    auto* card2 = new CardLayout(L"模型管理");
    m_comboVoiceModel = new UIComboBox();
    m_comboVoiceModel->SetItems({
        L"tiny（~78MB，最快）",
        L"base（~148MB）",
        L"small（~488MB）",
        L"medium（~1.5GB）",
        L"large-v3-turbo（~1.5GB，推荐）",
    });
    card2->AddChild(FormRow(L"模型大小", m_comboVoiceModel, 70));
    m_comboVoiceFlavor = new UIComboBox();
    m_comboVoiceFlavor->SetItems({ L"CPU", L"CUDA（NVIDIA GPU）", L"BLAS" });
    card2->AddChild(FormRow(L"引擎风格", m_comboVoiceFlavor, 70));
    page->AddChild(card2);

    // 卡片 3：识别参数
    auto* card3 = new CardLayout(L"识别参数");
    m_comboVoiceLang = new UIComboBox();
    m_comboVoiceLang->SetItems({ L"中文", L"自动检测" });
    card3->AddChild(FormRow(L"识别语言", m_comboVoiceLang, 110));
    m_editVoiceThreshold = new UIEdit();
    m_editVoiceThreshold->SetPlaceholder(L"0.005-0.20");
    card3->AddChild(FormRow(L"VAD 能量阈值", m_editVoiceThreshold, 110));
    m_editVoiceSilence = new UIEdit();
    m_editVoiceSilence->SetPlaceholder(L"0.5-5.0");
    card3->AddChild(FormRow(L"静音超时（秒）", m_editVoiceSilence, 110));
    m_editVoiceMinSpeech = new UIEdit();
    m_editVoiceMinSpeech->SetPlaceholder(L"0.3-2.0");
    card3->AddChild(FormRow(L"最短语音（秒）", m_editVoiceMinSpeech, 110));
    page->AddChild(card3);

    // 说明（收进卡片）：整宽多行文本，不再用 FormRow 的固定标签列
    auto* hintCard = new CardLayout(L"说明");
    hintCard->AddChild(new HintText(
        L"双路径：检测到泰深 whisper-server 时直连复用；否则需自行下载引擎与模型", 2));
    hintCard->AddChild(new HintText(
        L"模型：下载管理（v0.5.5）与 server 自启（v0.5.6）即将上线，当前需手动准备", 2));
    page->AddChild(hintCard);
}

// ===========================================================================
// 页 6 按键（0.1.49，对齐微软拼音「按键」页）
// ===========================================================================
void CSettingsWindow::BuildKeyPage()
{
    UILayout* page = m_pageRoots[6];

    // 卡片 1：模式切换
    auto* card1 = new CardLayout(L"模式切换");
    m_chkKeyCtrlSpace = CheckRow(card1, L"Ctrl + 空格：中英文模式切换");
    m_chkKeyShift = CheckRow(card1, L"Shift：中英文模式切换（快速点按）");

    m_comboKeyFullwidth = new UIComboBox();
    m_comboKeyFullwidth->SetItems({ L"无", L"Shift + 空格" });
    card1->AddChild(FormRow(L"全/半角切换", m_comboKeyFullwidth, 110));

    m_comboKeyPunct = new UIComboBox();
    m_comboKeyPunct->SetItems({ L"无", L"Ctrl + 句点" });
    card1->AddChild(FormRow(L"中/英文标点切换", m_comboKeyPunct, 110));

    m_comboKeyTrad = new UIComboBox();
    {
        std::vector<std::wstring> items;
        items.push_back(L"无");
        for (wchar_t c = L'A'; c <= L'Z'; ++c) {
            items.push_back(std::wstring(L"Ctrl + Shift + ") + c);
        }
        m_comboKeyTrad->SetItems(items);
    }
    card1->AddChild(FormRow(L"简/繁切换", m_comboKeyTrad, 110));
    page->AddChild(card1);

    // 卡片 2：候选词导航
    auto* card2 = new CardLayout(L"候选词导航");
    card2->AddChild(new HintText(
        L"翻页：PgUp / PgDn 固定生效。以下按键可同时勾选作为额外翻页键：", 2));
    m_chkPageMinus = CheckRow(card2, L"主键盘 + / −");
    m_chkPageNumpad = CheckRow(card2, L"小键盘 + / −");
    m_chkPageComma = CheckRow(card2, L"逗号 / 句号");
    m_chkPageBrackets = CheckRow(card2, L"左 / 右方括号：取候选首 / 末字（非翻页）");
    page->AddChild(card2);

    // 卡片 3：恢复默认（只重置按键，不动其他设置）
    auto* card3 = new CardLayout(L"默认值");
    card3->AddChild(new HintText(
        L"恢复后需点右下角「确定」才会写入 config.ini", 2));
    auto* btnReset = new UIButton(L"恢复按键默认设置");
    btnReset->SetOnClick([this]() {
        m_cfg.keys = KeyBindings();
        ApplyToUI();
    });
    card3->AddChild(btnReset);
    page->AddChild(card3);
}

// ===========================================================================
// 配置读写
// ===========================================================================
void CSettingsWindow::ApplyToUI()
{
    // 基础
    m_editCandidate->SetText(std::to_wstring(m_cfg.candidate_count));
    m_editFontFace->SetText(m_cfg.font_face);
    m_editFontSize->SetText(std::to_wstring(static_cast<int>(m_cfg.font_size)));
    m_chkVerticalCandidates->SetChecked(m_cfg.vertical_candidates);
    m_chkInline->SetChecked(m_cfg.inline_preedit);
    m_editLabelFormat->SetText(m_cfg.label_format);

    // 输入
    ApplyFuzzyGroupsToUI();
    m_chkCorrection->SetChecked(m_cfg.correction_enabled);
    m_chkMix->SetChecked(m_cfg.mix_mode_enabled);
    m_chkTraditional->SetChecked(m_cfg.traditional_enabled);
    m_chkShuangpin->SetChecked(m_cfg.shuangpin_mode);
    m_chkSpFullPreedit->SetChecked(m_cfg.shuangpin_full_preedit);
    int schemeIdx = 0;
    for (int i = 0; i < 6; ++i) {
        if (m_cfg.shuangpin_scheme == kSchemeKeys[i]) {
            schemeIdx = i;
            break;
        }
    }
    m_comboScheme->SetSelectedIndex(schemeIdx);
    m_chkPhrase->SetChecked(m_cfg.phrase_enabled);
    m_chkAsciiPunct->SetChecked(m_cfg.ascii_punct);
    m_chkPairPunct->SetChecked(m_cfg.pair_punct);
    m_chkEmoji->SetChecked(m_cfg.emoji_enabled);
    m_chkDebugLog->SetChecked(m_cfg.debug_log);
    // P0-2：候选排序模式
    m_comboSortMode->SetSelectedIndex(m_cfg.sort_mode >= 0 && m_cfg.sort_mode <= 2
                                           ? m_cfg.sort_mode
                                           : 0);
    // P1-1：上下文联想
    m_chkContextAssoc->SetChecked(m_cfg.context_assoc);
    // V0.6.x：前缀补全
    m_chkCompletion->SetChecked(m_cfg.completion_enabled);

    // 外观
    // 0.2.34：直接用持久化的 theme_mode（0=跟随系统 1=深色 2=浅色），
    // 不再按背景明暗推断（跟随系统保存后背景仍是旧颜色 → 误判）
    m_comboThemeMode->SetSelectedIndex(m_cfg.theme_mode);
    m_swatches[0]->SetColor(m_cfg.theme.bg);
    m_swatches[1]->SetColor(m_cfg.theme.text);
    m_swatches[2]->SetColor(m_cfg.theme.label);
    m_swatches[3]->SetColor(m_cfg.theme.comment);
    m_swatches[4]->SetColor(m_cfg.theme.border);
    m_swatches[5]->SetColor(m_cfg.theme.highlight_bg);
    m_swatches[6]->SetColor(m_cfg.theme.highlight_text);
    m_swatches[7]->SetColor(m_cfg.theme.highlight_label);
    m_swatches[8]->SetColor(m_cfg.theme.dim);
    m_swatches[9]->SetColor(m_cfg.theme.mark);
    m_editCorner->SetText(std::to_wstring(static_cast<int>(m_cfg.corner_radius)));
    m_editHilite->SetText(std::to_wstring(static_cast<int>(m_cfg.hilite_corner_radius)));
    m_editPadding->SetText(std::to_wstring(m_cfg.padding));
    m_editSpacing->SetText(std::to_wstring(m_cfg.candidate_spacing));

    // 高级：4 列表 → 行模型（并集，mode 冲突时 ascii 优先）
    m_appData.clear();
    for (const auto& p : m_cfg.app_ascii_list) {
        m_appData.push_back(AppRowData{ p, 1, 0, 0, false });
    }
    for (const auto& p : m_cfg.app_cn_list) {
        const bool dup = std::any_of(m_appData.begin(), m_appData.end(),
                                     [&](const AppRowData& d) { return d.name == p; });
        if (!dup) {
            m_appData.push_back(AppRowData{ p, 2, 0, 0, false });
        }
    }
    auto upsertApp = [&](const std::wstring& p) -> AppRowData& {
        auto it = std::find_if(m_appData.begin(), m_appData.end(),
                               [&](const AppRowData& d) { return d.name == p; });
        if (it != m_appData.end()) {
            return *it;
        }
        m_appData.push_back(AppRowData{ p, 0, 0, 0, false });
        return m_appData.back();
    };
    for (const auto& p : m_cfg.app_inline_list) {
        upsertApp(p).inlineMode = 1;
    }
    for (const auto& p : m_cfg.app_no_inline_list) {
        auto& row = upsertApp(p);
        if (row.inlineMode == 0) {
            row.inlineMode = 2;
        }
    }
    for (const auto& p : m_cfg.app_self_draw_list) {
        upsertApp(p).selfDraw = 1;
    }
    for (const auto& p : m_cfg.app_no_self_draw_list) {
        auto& row = upsertApp(p);
        if (row.selfDraw == 0) {
            row.selfDraw = 2;
        }
    }
    for (const auto& p : m_cfg.app_vim_list) {
        upsertApp(p).vimOn = true;
    }
    RebuildAppList();

    // 语音（V0.5.9）
    m_chkVoiceEnabled->SetChecked(m_cfg.voice.enabled);
    // 引擎状态：泰深检测（优先路径）
    const taishen::TaishenDetection det =
        taishen::VoiceManager::DetectTaishen(m_cfg.voice.server_port);
    if (det.server_running) {
        m_lblVoiceStatus->SetText(L"泰深已连接（直连模式）");
        m_lblVoiceStatus->SetColor(D2D1::ColorF(0x6AA84F, 1.0f));
    } else if (det.installed) {
        m_lblVoiceStatus->SetText(L"泰深已安装（server 未运行）");
        m_lblVoiceStatus->SetColor(D2D1::ColorF(0xB8860B, 1.0f));
    } else {
        m_lblVoiceStatus->SetText(L"本机独立运行（需下载引擎与模型）");
        m_lblVoiceStatus->SetColor(D2D1::ColorF(0x9A9A9A, 1.0f));
    }
    // 模型大小
    static const wchar_t* const kModels[] = {
        L"tiny", L"base", L"small", L"medium", L"large-v3-turbo",
    };
    int modelIdx = 4;
    for (int i = 0; i < 5; ++i) {
        if (m_cfg.voice.model_size == kModels[i]) {
            modelIdx = i;
            break;
        }
    }
    m_comboVoiceModel->SetSelectedIndex(modelIdx);
    // 引擎风格
    static const wchar_t* const kFlavors[] = { L"cpu", L"cuda", L"blas" };
    int flavorIdx = 0;
    for (int i = 0; i < 3; ++i) {
        if (m_cfg.voice.engine_flavor == kFlavors[i]) {
            flavorIdx = i;
            break;
        }
    }
    m_comboVoiceFlavor->SetSelectedIndex(flavorIdx);
    // 语言
    m_comboVoiceLang->SetSelectedIndex(m_cfg.voice.language == L"auto" ? 1 : 0);
    // VAD 参数：输入框直接显示真实数值（阈值 0.01-0.20；时间为秒，可带一位小数）
    m_editVoiceThreshold->SetText(FormatFloat(m_cfg.voice.vad_threshold, 2));
    m_editVoiceSilence->SetText(FormatFloat(m_cfg.voice.vad_silence_timeout_sec, 1));
    m_editVoiceMinSpeech->SetText(FormatFloat(m_cfg.voice.vad_min_speech_sec, 1));

    // 按键（0.1.49）
    m_chkKeyCtrlSpace->SetChecked(m_cfg.keys.toggle_ascii_ctrl_space);
    m_chkKeyShift->SetChecked(m_cfg.keys.toggle_ascii_shift);
    m_comboKeyFullwidth->SetSelectedIndex(m_cfg.keys.toggle_fullwidth == 1 ? 1 : 0);
    m_comboKeyPunct->SetSelectedIndex(m_cfg.keys.toggle_punct == 1 ? 1 : 0);
    {
        const int k = m_cfg.keys.toggle_traditional_key;
        int idx = 0;
        if (k >= 'A' && k <= 'Z') {
            idx = 1 + (k - 'A');
        }
        m_comboKeyTrad->SetSelectedIndex(idx);
    }
    m_chkPageComma->SetChecked(m_cfg.keys.page_comma_period);
    m_chkPageMinus->SetChecked(m_cfg.keys.page_minus_equal);
    m_chkPageBrackets->SetChecked(m_cfg.keys.page_brackets);
    m_chkPageNumpad->SetChecked(m_cfg.keys.page_numpad);

    // 词库与数据（Q2.2）
    ApplyDictPageToUI();
}

void CSettingsWindow::CollectFromUI()
{
    // 基础
    m_cfg.candidate_count = std::clamp(_wtoi(m_editCandidate->Text().c_str()), 1, 20);
    m_cfg.font_face = m_editFontFace->Text();
    m_cfg.font_size = static_cast<float>(std::clamp(_wtoi(m_editFontSize->Text().c_str()), 12, 32));
    m_cfg.vertical_candidates = m_chkVerticalCandidates->IsChecked();
    m_cfg.inline_preedit = m_chkInline->IsChecked();
    m_cfg.label_format = m_editLabelFormat->Text();
    if (m_cfg.label_format.empty()) {
        m_cfg.label_format = L"%d.";
    }

    // 输入
    CollectFuzzyGroupsFromUI();
    m_cfg.correction_enabled = m_chkCorrection->IsChecked();
    m_cfg.mix_mode_enabled = m_chkMix->IsChecked();
    m_cfg.traditional_enabled = m_chkTraditional->IsChecked();
    m_cfg.shuangpin_mode = m_chkShuangpin->IsChecked();
    m_cfg.shuangpin_full_preedit = m_chkSpFullPreedit->IsChecked();
    const int si = m_comboScheme->SelectedIndex();
    m_cfg.shuangpin_scheme = (si >= 0 && si < 6) ? kSchemeKeys[si] : "mspy";
    m_cfg.phrase_enabled = m_chkPhrase->IsChecked();
    m_cfg.ascii_punct = m_chkAsciiPunct->IsChecked();
    m_cfg.pair_punct = m_chkPairPunct->IsChecked();
    m_cfg.emoji_enabled = m_chkEmoji->IsChecked();
    m_cfg.debug_log = m_chkDebugLog->IsChecked();
    // P0-2：候选排序模式
    const int sm = m_comboSortMode->SelectedIndex();
    m_cfg.sort_mode = (sm >= 0 && sm <= 2) ? sm : 0;
    // P1-1：上下文联想
    m_cfg.context_assoc = m_chkContextAssoc->IsChecked();
    // V0.6.x：前缀补全
    m_cfg.completion_enabled = m_chkCompletion->IsChecked();

    // 外观
    const int tm = m_comboThemeMode->SelectedIndex();
    m_cfg.theme_mode = tm;
    m_cfg.userThemeExplicit = (tm != 0);
    m_cfg.theme.bg = m_swatches[0]->Color();
    m_cfg.theme.text = m_swatches[1]->Color();
    m_cfg.theme.label = m_swatches[2]->Color();
    m_cfg.theme.comment = m_swatches[3]->Color();
    m_cfg.theme.border = m_swatches[4]->Color();
    m_cfg.theme.highlight_bg = m_swatches[5]->Color();
    m_cfg.theme.highlight_text = m_swatches[6]->Color();
    m_cfg.theme.highlight_label = m_swatches[7]->Color();
    m_cfg.theme.dim = m_swatches[8]->Color();
    m_cfg.theme.mark = m_swatches[9]->Color();
    m_cfg.corner_radius = static_cast<float>(std::clamp(_wtoi(m_editCorner->Text().c_str()), 1, 16));
    m_cfg.hilite_corner_radius = static_cast<float>(std::clamp(_wtoi(m_editHilite->Text().c_str()), 1, 16));
    m_cfg.padding = std::clamp(_wtoi(m_editPadding->Text().c_str()), 0, 20);
    m_cfg.candidate_spacing = std::clamp(_wtoi(m_editSpacing->Text().c_str()), 0, 40);

    // 高级：行模型 → 7 列表
    m_cfg.app_ascii_list.clear();
    m_cfg.app_cn_list.clear();
    m_cfg.app_inline_list.clear();
    m_cfg.app_no_inline_list.clear();
    m_cfg.app_self_draw_list.clear();
    m_cfg.app_no_self_draw_list.clear();
    m_cfg.app_vim_list.clear();
    for (const auto& d : m_appData) {
        std::wstring name = d.name;
        for (auto& ch : name) {
            ch = static_cast<wchar_t>(towlower(ch));
        }
        if (name.empty()) {
            continue;
        }
        if (d.mode == 1) {
            m_cfg.app_ascii_list.push_back(name);
        } else if (d.mode == 2) {
            m_cfg.app_cn_list.push_back(name);
        }
        if (d.inlineMode == 1) {
            m_cfg.app_inline_list.push_back(name);
        } else if (d.inlineMode == 2) {
            m_cfg.app_no_inline_list.push_back(name);
        }
        if (d.selfDraw == 1) {
            m_cfg.app_self_draw_list.push_back(name);
        } else if (d.selfDraw == 2) {
            m_cfg.app_no_self_draw_list.push_back(name);
        }
        if (d.vimOn) {
            m_cfg.app_vim_list.push_back(name);
        }
    }

    // 语音（V0.5.9）
    m_cfg.voice.enabled = m_chkVoiceEnabled->IsChecked();
    static const wchar_t* const kModels[] = {
        L"tiny", L"base", L"small", L"medium", L"large-v3-turbo",
    };
    const int mi = m_comboVoiceModel->SelectedIndex();
    m_cfg.voice.model_size = (mi >= 0 && mi < 5) ? kModels[mi] : L"large-v3-turbo";
    static const wchar_t* const kFlavors[] = { L"cpu", L"cuda", L"blas" };
    const int fi = m_comboVoiceFlavor->SelectedIndex();
    m_cfg.voice.engine_flavor = (fi >= 0 && fi < 3) ? kFlavors[fi] : L"cpu";
    m_cfg.voice.language = (m_comboVoiceLang->SelectedIndex() == 1) ? L"auto" : L"zh";
    // VAD 参数：按真实数值解析，夹到与旧版等价的范围（阈值 0.01-0.20；时间 0.5-5.0 / 0.3-2.0 秒）
    m_cfg.voice.vad_threshold = std::clamp(
        ParseFloat(m_editVoiceThreshold->Text(), m_cfg.voice.vad_threshold), 0.01f, 0.20f);
    m_cfg.voice.vad_silence_timeout_sec = std::clamp(
        ParseFloat(m_editVoiceSilence->Text(), m_cfg.voice.vad_silence_timeout_sec), 0.5f, 5.0f);
    m_cfg.voice.vad_min_speech_sec = std::clamp(
        ParseFloat(m_editVoiceMinSpeech->Text(), m_cfg.voice.vad_min_speech_sec), 0.3f, 2.0f);

    // 按键（0.1.49）：两项都取消时回落到 Ctrl+空格，避免完全无法切中英
    m_cfg.keys.toggle_ascii_ctrl_space = m_chkKeyCtrlSpace->IsChecked();
    m_cfg.keys.toggle_ascii_shift = m_chkKeyShift->IsChecked();
    if (!m_cfg.keys.toggle_ascii_ctrl_space && !m_cfg.keys.toggle_ascii_shift) {
        m_cfg.keys.toggle_ascii_ctrl_space = true;
    }
    m_cfg.keys.toggle_fullwidth =
        (m_comboKeyFullwidth->SelectedIndex() == 1) ? 1 : 0;
    m_cfg.keys.toggle_punct = (m_comboKeyPunct->SelectedIndex() == 1) ? 1 : 0;
    {
        const int idx = m_comboKeyTrad->SelectedIndex();
        m_cfg.keys.toggle_traditional_key =
            (idx >= 1 && idx <= 26) ? ('A' + idx - 1) : 0;
    }
    m_cfg.keys.page_comma_period = m_chkPageComma->IsChecked();
    m_cfg.keys.page_minus_equal = m_chkPageMinus->IsChecked();
    m_cfg.keys.page_brackets = m_chkPageBrackets->IsChecked();
    m_cfg.keys.page_numpad = m_chkPageNumpad->IsChecked();

    // 词库与数据（Q2.2）
    CollectDictPageFromUI();
}

void CSettingsWindow::SaveAndClose()
{
    CollectFromUI();
    if (SaveConfig(m_dllDir, m_cfg)) {
        // 0.1.68：保存后立即应用到本进程引擎并刷新工具栏。
        // 原来只写文件——热加载靠 2s 轮询（无托盘窗口时轮询根本没启动），
        // 用户关掉双拼后工具栏仍显示「双拼」，要再按一次键才跟上。
        taishen::ApplyConfigToEngine(m_cfg);
        taishen::NotifyConfigChanged();
        taishen::DebugLog("Settings: config saved (engine applied, toolbar refreshed)");
    } else {
        taishen::DebugLog("Settings: SaveConfig FAILED");
    }
    EndModal(IDOK);
}

} // namespace taishen

// ===========================================================================
// 入口（settings_dialog.h 兼容：工具栏「设置」按钮调用）
// ===========================================================================
#include "settings_dialog.h"

namespace taishen {

void ShowSettingsDialog(HWND /*parent*/, const std::wstring& dllDir, int initialPage)
{
    // AppContainer：设置窗口的保存动作写 config.ini（容器外不可写），
    // 打开只会让用户改完发现没生效。直接拒绝并记 WARN。
    if (taishen::IsAppContainerProcess()) {
        taishen::ForceLog(
            "AppContainer=1: settings dialog disabled (config.ini is read-only here)");
        return;
    }
    // P2-11：单实例互斥——跨进程已存在设置窗口时直接前置激活，防止并发写 config.ini
    if (HWND existing = FindWindowW(L"TaishenSettingsWindow", nullptr)) {
        ShowWindow(existing, SW_RESTORE);
        SetForegroundWindow(existing);
        return;
    }
    static std::atomic<bool> s_opening{ false };
    bool expected = false;
    if (!s_opening.compare_exchange_strong(expected, true)) {
        if (HWND existing = FindWindowW(L"TaishenSettingsWindow", nullptr)) {
            ShowWindow(existing, SW_RESTORE);
            SetForegroundWindow(existing);
        }
        return;
    }
    struct Guard {
        std::atomic<bool>& flag;
        ~Guard() { flag.store(false); }
    } guard{ s_opening };

    // 生命周期埋点（ForceLog 绕过诊断开关）——崩溃定位：
    // ① Run 返回后 ② 析构后 都不出现 → 崩在 Run() 内；
    // "Run returned" 出现但 "wnd destroyed" 不出现 → 崩在控件树析构。
    taishen::ForceLog("Settings dialog: open");
    {
        CSettingsWindow wnd(dllDir, initialPage);
        wnd.Run();
        taishen::ForceLog("Settings dialog: Run returned");
    }
    taishen::ForceLog("Settings dialog: wnd destroyed");
}

// ===========================================================================
// 证书页（开发自签证书可选导入）
// ===========================================================================
namespace {

// 开发自签证书 SHA-1 指纹，与 certs/TaishenIME-Dev-Root.cer 保持一致
constexpr const char* kDevCertSha1Hex = "E940406690BC7898EEAE6889C5486C8AC9B053A6";

int HexNibble(char c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/// 查询 LocalMachine\\Root 是否已存在开发自签证书
bool DevCertInstalledInRoot()
{
    BYTE hash[20] = {};
    for (size_t i = 0; i < sizeof(hash); ++i) {
        const int hi = HexNibble(kDevCertSha1Hex[i * 2]);
        const int lo = HexNibble(kDevCertSha1Hex[i * 2 + 1]);
        if (hi < 0 || lo < 0) {
            return false;
        }
        hash[i] = static_cast<BYTE>((hi << 4) | lo);
    }
    HCERTSTORE store = CertOpenSystemStoreW(0, L"ROOT");
    if (store == nullptr) {
        return false;
    }
    PCCERT_CONTEXT found = CertFindCertificateInStore(
        store, X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, 0, CERT_FIND_SHA1_HASH, hash, nullptr);
    const bool installed = (found != nullptr);
    if (found != nullptr) {
        CertFreeCertificateContext(found);
    }
    CertCloseStore(store, 0);
    return installed;
}

} // namespace

/// 证书文件位置：优先 exe 所在目录（安装目录），回退 %ProgramFiles%\TaishenIME 与传入目录。
/// 注：设置窗运行在 taishen_server.exe 中，传入的 dllDir 实为配置目录（%APPDATA%\taishen-ime），
/// 证书则随安装包释放到 $INSTDIR，不能直接用 dllDir 拼接。
std::wstring ResolveDevCertPath(const std::wstring& dllDir)
{
    wchar_t exe[MAX_PATH] = {};
    if (GetModuleFileNameW(nullptr, exe, MAX_PATH) > 0) {
        std::wstring dir(exe);
        const size_t pos = dir.find_last_of(L"\\/");
        if (pos != std::wstring::npos) {
            dir.resize(pos + 1);
            const std::wstring p = dir + L"TaishenIME-Dev-Root.cer";
            if (GetFileAttributesW(p.c_str()) != INVALID_FILE_ATTRIBUTES) {
                return p;
            }
        }
    }
    wchar_t pf[MAX_PATH] = {};
    if (GetEnvironmentVariableW(L"ProgramFiles", pf, MAX_PATH) > 0) {
        const std::wstring p = std::wstring(pf) + L"\\TaishenIME\\TaishenIME-Dev-Root.cer";
        if (GetFileAttributesW(p.c_str()) != INVALID_FILE_ATTRIBUTES) {
            return p;
        }
    }
    return dllDir + L"TaishenIME-Dev-Root.cer";
}

void CSettingsWindow::BuildCertPage()
{
    UILayout* page = m_pageRoots[7];

    auto* card = new CardLayout(L"开发版签名证书");

    m_lblCertStatus =
        new UILabel(DevCertInstalledInRoot() ? L"状态：已导入" : L"状态：未导入");
    m_lblCertStatus->SetBold(true);
    card->AddChild(m_lblCertStatus);

    card->AddChild(new HintText(
        L"泰深输入法目前使用开发者自签证书。导入后，本机会信任由该证书签名的任何程序，"
        L"不仅限于泰深输入法；导入后才能在任务栏搜索框、管理员权限窗口中显示候选窗。"
        L"不导入不影响常规应用内使用，提权窗口会退化为系统其他输入法。",
        3));

    auto* thumb = new UILabel(
        L"SHA-256 指纹：F8D7 4FAE F3C4 A510 A89E F950 9A79 84D5\n"
        L"81AA 6D42 8F2B E0E1 8190 3B5B 3FB7 01C0");
    thumb->SetDim(true);
    thumb->SetWrap(true);
    card->AddChild(thumb);

    auto* expire = new UILabel(L"有效期：2026-11-23 至 2031-11-23");
    expire->SetDim(true);
    card->AddChild(expire);

    m_btnImportCert =
        new UIButton(DevCertInstalledInRoot() ? L"已导入" : L"导入证书");
    m_btnImportCert->SetPrimary(true);
    m_btnImportCert->SetOnClick([this]() { OnImportDevCert(); });
    card->AddChild(m_btnImportCert);

    card->AddChild(new HintText(
        L"开发者正在申请公开代码签名证书，取得后后续版本不再需要这一步。"
        L"若在安装时跳过了导入，可随时在这里补导入。",
        2));

    page->AddChild(card);
}

void CSettingsWindow::OnImportDevCert()
{
    if (DevCertInstalledInRoot()) {
        MessageBoxW(Hwnd(), L"证书已导入，无需重复操作。", L"泰深输入法",
                    MB_OK | MB_ICONINFORMATION);
        return;
    }

    const std::wstring cer = ResolveDevCertPath(m_dllDir);
    if (GetFileAttributesW(cer.c_str()) == INVALID_FILE_ATTRIBUTES) {
        const std::wstring msg = L"未找到证书文件：\n" + cer +
                                 L"\n\n请重新运行安装程序以释放证书文件。";
        MessageBoxW(Hwnd(), msg.c_str(), L"泰深输入法", MB_OK | MB_ICONWARNING);
        return;
    }

    // 写 LocalMachine\Root 需要管理员权限 → 提权执行 certutil
    const std::wstring args = L"-addstore -f \"Root\" \"" + cer + L"\"";
    const HINSTANCE hInst =
        ShellExecuteW(Hwnd(), L"runas", L"certutil.exe", args.c_str(), nullptr, SW_HIDE);
    const INT_PTR ret = reinterpret_cast<INT_PTR>(hInst);
    if (ret <= 32) {
        // 1223 = 用户在 UAC 点取消，属正常选择，不弹错误
        if (ret != 1223) {
            MessageBoxW(Hwnd(), L"导入未完成（权限不足或执行失败）。", L"泰深输入法",
                        MB_OK | MB_ICONWARNING);
        }
        return;
    }

    // 提权进程异步写入，这里短暂等待后刷新一次状态，不长时间阻塞界面
    Sleep(300);
    const bool ok = DevCertInstalledInRoot();
    if (m_lblCertStatus != nullptr) {
        m_lblCertStatus->SetText(ok ? L"状态：已导入" : L"状态：导入中，请稍后重开此页查看");
    }
    if (m_btnImportCert != nullptr) {
        m_btnImportCert->SetText(ok ? L"已导入" : L"导入证书");
    }
    if (ok) {
        MessageBoxW(Hwnd(),
                    L"证书已导入。\n\n重启正在使用输入法的程序后生效；"
                    L"任务栏搜索框需要重新打开一次。",
                    L"泰深输入法", MB_OK | MB_ICONINFORMATION);
    } else {
        MessageBoxW(Hwnd(), L"导入请求已发出，请稍后重新打开此页面确认状态。",
                    L"泰深输入法", MB_OK | MB_ICONINFORMATION);
    }
}

} // namespace taishen
