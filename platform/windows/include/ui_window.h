/// 自研窗体系统 — 窗口基类（V0.3.0）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.3
/// 无边框窗口封装：窗口类注册、消息分发（鼠标命中树/键盘焦点）、
/// D2D 渲染循环（WM_PAINT → 清背景 → 控件树 Draw）、模态循环。
/// 三种用途：
///   - 候选窗/工具栏：topmost + noActivate（不抢焦点，不拦截鼠标）
///   - 设置窗体：模态（RunModal）+ 自绘标题栏（SetTitleBarHeight 后 WM_NCHITTEST 拖动）

#pragma once

#include <string>
#include <vector>
#include <windows.h>
#include "ui_control.h"
#include "ui_render.h"
#include "ui_theme.h"

namespace taishen {

/// 窗口基类
class UIWindow
{
public:
    UIWindow();
    virtual ~UIWindow();

    UIWindow(const UIWindow&) = delete;
    UIWindow& operator=(const UIWindow&) = delete;

    /// 创建窗口（无边框 WS_POPUP）。
    /// @param title      窗口标题（窗口类名，同进程唯一）
    /// @param w,h        客户区尺寸（逻辑像素，DPI 由系统缩放）
    /// @param topmost    是否置顶（候选窗/工具栏=true）
    /// @param noActivate 是否不抢焦点（候选窗/工具栏=true）
    bool Create(const std::wstring& title, int w, int h,
                bool topmost = true, bool noActivate = true);

    void Destroy();

    void Show();
    void Hide();
    bool IsVisible() const { return m_visible; }
    HWND Hwnd() const { return m_hwnd; }

    // ── 主题 ──
    void SetTheme(const UITheme& t);
    const UITheme& Theme() const { return m_theme; }
    /// 主题跟随系统（WM_SETTINGCHANGE 时自动重取并重绘）
    void SetFollowSystemTheme(bool follow) { m_followSystemTheme = follow; }

    // ── 控件树 ──
    void SetRoot(UIControl* root);
    UIControl* Root() const { return m_root; }
    void SetFocusControl(UIControl* c);
    UIControl* FocusControl() const { return m_focus; }

    /// 请求重绘（控件树调用）
    void Invalidate() { if (m_hwnd != nullptr) InvalidateRect(m_hwnd, nullptr, FALSE); }

    /// 清空鼠标指针跟踪（V0.3.5 审查修复）：
    /// 弹出面板收起（删除子控件）后调用，防止 m_hoverCtrl/m_pressedCtrl
    /// 指向已释放控件 → 后续鼠标移动 use-after-free。
    void ClearPointerTracking() {
        m_hoverCtrl = nullptr;
        m_pressedCtrl = nullptr;
        m_trackingLeave = false;
    }

    // ── 弹出层（V0.3.6）──
    /// 注册顶层弹出层（下拉面板/色板等）——绘制浮在控件树最上层、
    /// 不被 ScrollPanel 裁剪、命中优先于普通控件。展开时注册、收起时注销。
    void RegisterPopup(UIControl* popup);
    void UnregisterPopup(UIControl* popup);
    /// 绘制全部弹出层（OnRender 末尾调用）
    void DrawPopups(UIRenderer& r, const UITheme& t);
    /// 弹出层命中（优先于控件树）；无命中返回 nullptr
    UIControl* HitTestPopups(int x, int y);

    /// 布局重算（WM_SIZE/SetRoot 后）
    void Relayout();

    // ── 模态（设置窗体）──
    /// 运行模态循环，返回 EndModal 传入的结果
    int RunModal();
    /// 结束模态（result 由 RunModal 返回）
    void EndModal(int result);
    bool IsModal() const { return m_modal; }

    /// 自绘标题栏高度（0 = 无标题栏）。>0 时 WM_NCHITTEST 在标题区返回 HTCAPTION 支持拖动
    void SetTitleBarHeight(int h) { m_titleBarHeight = h; }

protected:
    /// 窗口消息处理（子类覆写；默认返回 DefWindowProc 语义 0）
    virtual LRESULT HandleMessage(UINT msg, WPARAM wp, LPARAM lp);

    /// 渲染帧：默认清背景 + 控件树绘制（子类可追加绘制）
    virtual void OnRender(UIRenderer& r);

    /// 窗口关闭请求（模态 Esc 等），默认关闭；子类可拦截
    virtual bool OnCloseRequest() { return true; }

    // ── 消息分发辅助 ──
    void DispatchMouse(UINT msg, WPARAM wp, LPARAM lp);
    void DispatchKey(UINT msg, WPARAM wp, LPARAM lp);
    /// 递归下发全局鼠标按下通知（弹出层收起检测）
    void NotifyGlobalMouseDown(UIControl* node, int x, int y);
    void OnPaint();

    HWND m_hwnd = nullptr;
    UIRenderer m_renderer;
    UITheme m_theme;
    UIControl* m_root = nullptr;
    UIControl* m_focus = nullptr;
    UIControl* m_hoverCtrl = nullptr;   // 当前悬停控件
    UIControl* m_pressedCtrl = nullptr; // 鼠标按下控件
    bool m_visible = false;
    bool m_modal = false;
    bool m_followSystemTheme = false;
    int m_modalResult = 0;
    int m_titleBarHeight = 0;

    /// 鼠标位置（窗口内坐标，WM_MOUSEMOVE 缓存）
    POINT m_mousePos{};
    bool m_trackingLeave = false;
    /// 顶层弹出层列表（V0.3.6：下拉面板/色板）
    std::vector<UIControl*> m_popups;

private:
    static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp);
    bool RegisterClassOnce(const std::wstring& clsName);
};

} // namespace taishen
