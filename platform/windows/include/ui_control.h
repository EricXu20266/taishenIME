/// 自研窗体系统 — 控件基类（V0.3.0）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.4
/// 控件 = 矩形 + 绘制 + 命中 + 交互状态机。
/// 基类持有子控件列表（布局容器/复杂控件复用），窗口递归分发命中与窗口指针。
/// 交互由 UIWindow 分发：鼠标命中 → 状态更新 + 事件回调；键盘 → 焦点控件。
/// 所有状态变更自动 Invalidate（请求所属窗口重绘）。

#pragma once

#include <vector>
#include <windows.h>
#include "ui_render.h"
#include "ui_theme.h"

namespace taishen {

class UIWindow;

/// 控件交互状态
enum class UIState {
    Normal,
    Hover,     // 鼠标悬停
    Pressed,   // 鼠标按下
    Disabled,  // 禁用
};

/// 控件基类
class UIControl
{
public:
    UIControl() = default;
    virtual ~UIControl() = default;

    UIControl(const UIControl&) = delete;
    UIControl& operator=(const UIControl&) = delete;

    // ── 几何 ──
    void SetRect(const RECT& rc) { m_rect = rc; }
    RECT Rect() const { return m_rect; }
    int X() const { return m_rect.left; }
    int Y() const { return m_rect.top; }
    int Width() const { return m_rect.right - m_rect.left; }
    int Height() const { return m_rect.bottom - m_rect.top; }

    /// 首选高度（布局用）：-1 = 弹性（占剩余空间），其他 = 固定高（像素）
    /// @param width 分配到的宽度（文本换行/高度自适应用）
    virtual int PreferredHeight(int /*width*/) const { return 28; }

    // ── 状态 ──
    void SetVisible(bool v) { m_visible = v; Invalidate(); }
    bool IsVisible() const { return m_visible; }
    void SetEnabled(bool e) { m_enabled = e; Invalidate(); }
    bool IsEnabled() const { return m_enabled; }
    void SetId(int id) { m_id = id; }
    int Id() const { return m_id; }

    UIState State() const;
    bool IsHovered() const { return m_hovered; }
    bool IsPressed() const { return m_pressed; }
    bool IsFocused() const { return m_focused; }

    // ── 层级 ──
    UIControl* Parent() const { return m_parent; }
    void SetParent(UIControl* p) { m_parent = p; }
    void AddChild(UIControl* c);
    /// 移除全部子控件（deleteChildren=true 时对子树递归释放——子布局含孙控件也必须释放）
    void RemoveAllChildren(bool deleteChildren);
    const std::vector<UIControl*>& Children() const { return m_children; }

    /// 设置所属窗口（递归下发到子控件树）
    virtual void SetWindow(UIWindow* w);
    UIWindow* Window() const { return m_window; }

    // ── 绘制 ──
    /// 绘制自身（坐标相对所属窗口客户区）
    virtual void Draw(UIRenderer& r, const UITheme& t) = 0;

    // ── 命中与交互（UIWindow 分发，默认空实现）──
    /// 命中检测：默认矩形内判定（子控件可覆写为形状命中）
    virtual bool HitTest(int x, int y) const;

    /// 递归命中：子控件优先（子可弹出超出父矩形），返回最深的可见命中控件
    virtual UIControl* HitTestTree(int x, int y);

    virtual void OnMouseMove(int x, int y);
    virtual void OnMouseLeave();
    virtual void OnMouseDown(int x, int y, bool left);
    virtual void OnMouseUp(int x, int y, bool left);
    virtual void OnClick(int x, int y);
    virtual void OnKeyDown(int vk, bool ctrl, bool shift, bool alt);
    virtual void OnChar(wchar_t ch);
    virtual void OnFocus(bool focused);
    /// 鼠标滚轮（delta = WM_MOUSEWHEEL 的 wheel delta，正=上滚）。
    /// V0.3.6：默认向父控件冒泡——滚动面板/可滚动容器在祖先节点处理，
    /// 深层的静态控件（toggle/标签）无需各自处理。
    virtual void OnMouseWheel(int delta)
    {
        if (m_parent != nullptr) {
            m_parent->OnMouseWheel(delta);
        }
    }

    /// 全局鼠标按下通知（UIWindow 每次按下时分发给整棵控件树）：
    /// 弹出层/下拉框用它检测"点击外部 → 收起"。
    virtual void OnGlobalMouseDown(int /*x*/, int /*y*/) {}

protected:
    /// 请求所属窗口重绘（控件状态变化时调用）
    void Invalidate();

    RECT m_rect{};
    UIControl* m_parent = nullptr;
    UIWindow* m_window = nullptr;
    std::vector<UIControl*> m_children;
    bool m_visible = true;
    bool m_enabled = true;
    bool m_hovered = false;
    bool m_pressed = false;
    bool m_focused = false;
    int m_id = 0;
};

/// 递归释放控件树（V0.3.5 审查修复）：
/// 控件树 = 组合所有权，根的拥有者负责整棵树销毁。
/// 供 RemoveAllChildren(true) / UIWindow 析构调用。
/// 注意：UIControl 析构本身不删 children（避免隐式所有权），树的回收显式走本函数。
void DeleteControlTree(UIControl* root);

} // namespace taishen
