/// 自研窗体系统 — 窗口基类实现（V0.3.0）

#include "ui_window.h"
#include "ui_layout.h"
#include <mutex>
#include <unordered_set>
#include <windowsx.h>

namespace taishen {

namespace {
/// 已注册窗口类名集合（同进程多窗口实例共享类）
std::unordered_set<std::wstring>& RegisteredClasses()
{
    static std::unordered_set<std::wstring> s;
    return s;
}
std::mutex& ClassMutex()
{
    static std::mutex m;
    return m;
}
} // namespace

UIWindow::UIWindow() = default;

UIWindow::~UIWindow()
{
    Destroy();
}

bool UIWindow::RegisterClassOnce(const std::wstring& clsName)
{
    std::lock_guard<std::mutex> lock(ClassMutex());
    if (RegisteredClasses().count(clsName) != 0) {
        return true;
    }
    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = UIWindow::WndProc;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.hCursor = LoadCursorW(nullptr, reinterpret_cast<LPCWSTR>(IDC_ARROW));
    wc.lpszClassName = clsName.c_str();
    wc.hbrBackground = nullptr; // D2D 自绘
    if (RegisterClassExW(&wc) == 0) {
        return false;
    }
    RegisteredClasses().insert(clsName);
    return true;
}

bool UIWindow::Create(const std::wstring& title, int w, int h,
                      bool topmost, bool noActivate)
{
    if (m_hwnd != nullptr) {
        return true;
    }
    if (!RegisterClassOnce(title)) {
        return false;
    }
    DWORD exStyle = WS_EX_TOOLWINDOW;
    if (topmost) {
        exStyle |= WS_EX_TOPMOST;
    }
    if (noActivate) {
        exStyle |= WS_EX_NOACTIVATE;
    }

    // 位置：默认屏幕居中（候选窗等由子类移动）
    const int sw = GetSystemMetrics(SM_CXSCREEN);
    const int sh = GetSystemMetrics(SM_CYSCREEN);
    const int x = (sw - w) / 2;
    const int y = (sh - h) / 3;

    m_hwnd = CreateWindowExW(exStyle, title.c_str(), title.c_str(),
                             WS_POPUP, x, y, w, h,
                             nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (m_hwnd == nullptr) {
        return false;
    }
    SetWindowLongPtrW(m_hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(this));
    m_theme = UIThemeCurrent();
    return true;
}

void UIWindow::Destroy()
{
    if (m_hwnd != nullptr) {
        SetWindowLongPtrW(m_hwnd, GWLP_USERDATA, 0);
        DestroyWindow(m_hwnd);
        m_hwnd = nullptr;
    }
    m_visible = false;
}

void UIWindow::Show()
{
    if (m_hwnd == nullptr) {
        return;
    }
    ShowWindow(m_hwnd, SW_SHOWNOACTIVATE);
    m_visible = true;
    Relayout();
    Invalidate();
}

void UIWindow::Hide()
{
    if (m_hwnd != nullptr) {
        ShowWindow(m_hwnd, SW_HIDE);
    }
    m_visible = false;
}

void UIWindow::SetTheme(const UITheme& t)
{
    m_theme = t;
    Invalidate();
}

void UIWindow::SetRoot(UIControl* root)
{
    m_root = root;
    if (m_root != nullptr) {
        m_root->SetWindow(this);
    }
    Relayout();
    Invalidate();
}

void UIWindow::Relayout()
{
    if (m_hwnd == nullptr || m_root == nullptr) {
        return;
    }
    RECT rc{};
    GetClientRect(m_hwnd, &rc);
    m_root->SetRect({ 0, 0, rc.right, rc.bottom });
    if (auto* layout = dynamic_cast<UILayout*>(m_root)) {
        layout->Layout();
    }
}

void UIWindow::SetFocusControl(UIControl* c)
{
    if (m_focus == c) {
        return;
    }
    if (m_focus != nullptr) {
        m_focus->OnFocus(false);
    }
    m_focus = c;
    if (m_focus != nullptr) {
        m_focus->OnFocus(true);
    }
}

int UIWindow::RunModal()
{
    m_modal = true;
    m_modalResult = 0;
    Show();
    MSG msg{};
    while (m_modal && GetMessageW(&msg, nullptr, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    Hide();
    return m_modalResult;
}

void UIWindow::EndModal(int result)
{
    m_modalResult = result;
    m_modal = false;
    if (m_hwnd != nullptr) {
        PostMessageW(m_hwnd, WM_APP, 0, 0);
    }
}

LRESULT CALLBACK UIWindow::WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    UIWindow* self = reinterpret_cast<UIWindow*>(
        GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    if (self == nullptr) {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    if (msg == WM_NCDESTROY) {
        self->m_hwnd = nullptr;
        self->m_visible = false;
        return 0;
    }
    return self->HandleMessage(msg, wp, lp);
}

LRESULT UIWindow::HandleMessage(UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_PAINT:
        OnPaint();
        return 0;

    case WM_SIZE:
        Relayout();
        return 0;

    case WM_MOUSEMOVE:
    case WM_LBUTTONDOWN:
    case WM_LBUTTONUP:
    case WM_LBUTTONDBLCLK:
        DispatchMouse(msg, wp, lp);
        return 0;

    case WM_MOUSELEAVE:
        m_trackingLeave = false;
        if (m_hoverCtrl != nullptr) {
            m_hoverCtrl->OnMouseLeave();
            m_hoverCtrl = nullptr;
        }
        return 0;

    case WM_KEYDOWN:
    case WM_SYSKEYDOWN:
        DispatchKey(msg, wp, lp);
        return 0;

    case WM_MOUSEWHEEL: {
        // 滚轮 → 当前悬停控件（滚动条/滚动面板）
        const int delta = GET_WHEEL_DELTA_WPARAM(wp);
        if (m_hoverCtrl != nullptr) {
            m_hoverCtrl->OnMouseWheel(delta);
        }
        return 0;
    }

    case WM_CHAR:
        if (m_focus != nullptr && wp >= 32) {
            m_focus->OnChar(static_cast<wchar_t>(wp));
        }
        return 0;

    case WM_SETTINGCHANGE:
        // 主题跟随系统：注册表 AppsUseLightTheme 变化（设置 → 个性化 → 颜色）
        if (m_followSystemTheme) {
            const UITheme t = UIThemeCurrent();
            if (memcmp(&t, &m_theme, sizeof(UITheme)) != 0) {
                m_theme = t;
                Invalidate();
            }
        }
        return 0;

    case WM_DISPLAYCHANGE:
        m_renderer.ReleaseDeviceResources();
        Invalidate();
        return 0;

    case WM_NCHITTEST:
        // 自绘标题栏拖动
        if (m_titleBarHeight > 0) {
            const POINT pt{ GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            RECT rc{};
            GetWindowRect(m_hwnd, &rc);
            if (pt.y - rc.top < m_titleBarHeight && pt.x - rc.left < (rc.right - rc.left) - 60) {
                return HTCAPTION;
            }
        }
        return HTCLIENT;

    case WM_APP:
        // 模态退出信号
        return 0;

    case WM_CLOSE:
        if (OnCloseRequest()) {
            if (m_modal) {
                EndModal(IDCANCEL);
            } else {
                Destroy();
            }
        }
        return 0;

    default:
        return DefWindowProcW(m_hwnd, msg, wp, lp);
    }
}

void UIWindow::OnPaint()
{
    PAINTSTRUCT ps{};
    HDC hdc = BeginPaint(m_hwnd, &ps);
    (void)hdc;
    if (!m_renderer.Ensure(m_hwnd)) {
        EndPaint(m_hwnd, &ps);
        return;
    }
    m_renderer.BeginDraw();
    OnRender(m_renderer);
    m_renderer.EndDraw();
    EndPaint(m_hwnd, &ps);
}

void UIWindow::OnRender(UIRenderer& r)
{
    r.Clear(m_theme.bg);
    if (m_root != nullptr) {
        m_root->Draw(r, m_theme);
    }
}

void UIWindow::DispatchMouse(UINT msg, WPARAM wp, LPARAM lp)
{
    if (m_root == nullptr) {
        return;
    }
    const int x = GET_X_LPARAM(lp);
    const int y = GET_Y_LPARAM(lp);
    const bool left = (wp & MK_LBUTTON) != 0;

    switch (msg) {
    case WM_MOUSEMOVE: {
        m_mousePos = { x, y };
        UIControl* c = m_root->HitTestTree(x, y);
        if (c != m_hoverCtrl) {
            if (m_hoverCtrl != nullptr) {
                m_hoverCtrl->OnMouseLeave();
            }
            m_hoverCtrl = c;
            if (c != nullptr && !m_trackingLeave) {
                TRACKMOUSEEVENT tme{ sizeof(tme), TME_LEAVE, m_hwnd, 0 };
                TrackMouseEvent(&tme);
                m_trackingLeave = true;
            }
        }
        if (c != nullptr) {
            c->OnMouseMove(x - c->X(), y - c->Y());
        }
        break;
    }
    case WM_LBUTTONDOWN: {
        // 全局按下通知（弹出层/下拉框检测外部点击收起）
        if (m_root != nullptr) {
            NotifyGlobalMouseDown(m_root, x, y);
        }
        UIControl* c = m_root->HitTestTree(x, y);
        m_pressedCtrl = c;
        SetFocusControl(c);
        if (c != nullptr) {
            c->OnMouseDown(x - c->X(), y - c->Y(), true);
        }
        break;
    }
    case WM_LBUTTONUP: {
        UIControl* c = m_root->HitTestTree(x, y);
        if (m_pressedCtrl != nullptr) {
            m_pressedCtrl->OnMouseUp(x - m_pressedCtrl->X(),
                                     y - m_pressedCtrl->Y(), true);
        }
        // 按下与抬起在同一控件 → 点击
        if (c != nullptr && c == m_pressedCtrl) {
            c->OnClick(x - c->X(), y - c->Y());
        }
        m_pressedCtrl = nullptr;
        break;
    }
    case WM_LBUTTONDBLCLK:
        // 双发单击语义（默认等同单击处理）
        break;
    default:
        break;
    }
}

void UIWindow::NotifyGlobalMouseDown(UIControl* node, int x, int y)
{
    if (node == nullptr) {
        return;
    }
    node->OnGlobalMouseDown(x, y);
    for (UIControl* c : node->Children()) {
        NotifyGlobalMouseDown(c, x, y);
    }
}

void UIWindow::DispatchKey(UINT msg, WPARAM wp, LPARAM lp)
{
    const int vk = static_cast<int>(wp);
    const bool ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
    const bool alt = (GetKeyState(VK_MENU) & 0x8000) != 0;

    if (m_focus != nullptr && m_focus->IsEnabled()) {
        m_focus->OnKeyDown(vk, ctrl, shift, alt);
    }
    // Esc：模态窗口关闭（子类可在 OnCloseRequest 拦截）
    if (vk == VK_ESCAPE && m_modal) {
        if (OnCloseRequest()) {
            EndModal(IDCANCEL);
        }
    }
}

} // namespace taishen
