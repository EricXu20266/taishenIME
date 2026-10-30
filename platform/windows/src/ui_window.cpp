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
    // 释放控件树（组合所有权：根负责整棵树销毁，V0.3.5 审查修复）
    if (m_root != nullptr) {
        DeleteControlTree(m_root);
        m_root = nullptr;
    }
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
                      bool topmost, bool noActivate, bool dpiScale)
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

    // DPI 换算：自绘坐标系是 DIP（D2D 按窗口 DPI 缩放），窗口物理尺寸必须同步放大，
    // 否则 125% 下客户区 DIP 尺寸缩小 20% → 内容溢出（设置界面显示不全/按键错位）。
    m_dpiScaled = dpiScale;
    float scale = 1.0f;
    if (dpiScale) {
        const UINT sysDpi = GetDpiForSystem();   // unaware 进程返回 96，无需再缩放
        scale = (sysDpi > 0) ? static_cast<float>(sysDpi) / 96.0f : 1.0f;
    }
    const int pw = static_cast<int>(static_cast<float>(w) * scale + 0.5f);
    const int ph = static_cast<int>(static_cast<float>(h) * scale + 0.5f);

    // 位置：默认屏幕居中（候选窗等由子类移动）
    const int sw = GetSystemMetrics(SM_CXSCREEN);
    const int sh = GetSystemMetrics(SM_CYSCREEN);
    const int x = (sw - pw) / 2;
    const int y = (sh - ph) / 3;

    m_hwnd = CreateWindowExW(exStyle, title.c_str(), title.c_str(),
                             WS_POPUP, x, y, pw, ph,
                             nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
    if (m_hwnd == nullptr) {
        return false;
    }
    SetWindowLongPtrW(m_hwnd, GWLP_USERDATA, reinterpret_cast<LONG_PTR>(this));
    m_theme = UIThemeCurrent();
    return true;
}

float UIWindow::DpiScale() const
{
    if (m_hwnd == nullptr) {
        return 1.0f;
    }
    // 宿主 DPI unaware 时系统会对整窗位图拉伸，自算会双重缩放
    const DPI_AWARENESS_CONTEXT ctx = GetWindowDpiAwarenessContext(m_hwnd);
    if (ctx != nullptr) {
        const DPI_AWARENESS aw = GetAwarenessFromDpiAwarenessContext(ctx);
        if (aw == DPI_AWARENESS_UNAWARE) {
            return 1.0f;
        }
    }
    const UINT dpi = GetDpiForWindow(m_hwnd);
    return dpi > 0 ? static_cast<float>(dpi) / 96.0f : 1.0f;
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
    // 渲染目标跟随窗口尺寸（WM_SIZE 路径；HwndRenderTarget 不自动 resize）
    m_renderer.Resize(m_hwnd);
    RECT rc{};
    GetClientRect(m_hwnd, &rc);
    // 自绘坐标系是 DIP（D2D 已按窗口 DPI 缩放），根控件矩形必须用逻辑尺寸。
    // 直接拿物理客户区会让布局在 125%/150% 下溢出。
    const float scale = m_dpiScaled ? DpiScale() : 1.0f;
    const int logicalW = static_cast<int>(static_cast<float>(rc.right) / scale + 0.5f);
    const int logicalH = static_cast<int>(static_cast<float>(rc.bottom) / scale + 0.5f);
    // 自绘标题栏存在时，内容区从标题栏下方开始（标题栏高度本身是 DIP）
    m_root->SetRect({ 0, m_titleBarHeight, logicalW, logicalH });
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
    case WM_RBUTTONDOWN:
    case WM_RBUTTONUP:
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

    case WM_DPICHANGED: {
        // 窗口被拖到不同缩放的显示器：按系统建议的物理矩形重设，再重建渲染目标
        const RECT* target = reinterpret_cast<const RECT*>(lp);
        if (target != nullptr) {
            SetWindowPos(m_hwnd, nullptr, target->left, target->top,
                         target->right - target->left,
                         target->bottom - target->top,
                         SWP_NOZORDER | SWP_NOACTIVATE);
        }
        m_renderer.ReleaseDeviceResources();
        Relayout();
        Invalidate();
        return 0;
    }

    case WM_NCHITTEST:
        // 自绘标题栏拖动
        if (m_titleBarHeight > 0) {
            const POINT pt{ GET_X_LPARAM(lp), GET_Y_LPARAM(lp) };
            RECT rc{};
            GetWindowRect(m_hwnd, &rc);
            const int titleH = static_cast<int>(
                static_cast<float>(m_titleBarHeight) *
                    (m_dpiScaled ? DpiScale() : 1.0f) + 0.5f);
            if (pt.y - rc.top < titleH && pt.x - rc.left < (rc.right - rc.left) - 60) {
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
    // V0.3.6：弹出层（下拉面板/色板）浮在控件树最上层，不被容器裁剪
    DrawPopups(r, m_theme);
}

void UIWindow::RegisterPopup(UIControl* popup)
{
    if (popup == nullptr) {
        return;
    }
    for (UIControl* p : m_popups) {
        if (p == popup) {
            return;
        }
    }
    popup->SetWindow(this);
    m_popups.push_back(popup);
    Invalidate();
}

void UIWindow::UnregisterPopup(UIControl* popup)
{
    for (auto it = m_popups.begin(); it != m_popups.end(); ++it) {
        if (*it == popup) {
            m_popups.erase(it);
            Invalidate();
            return;
        }
    }
}

void UIWindow::DrawPopups(UIRenderer& r, const UITheme& t)
{
    for (UIControl* p : m_popups) {
        if (p->IsVisible()) {
            p->Draw(r, t);
        }
    }
}

UIControl* UIWindow::HitTestPopups(int x, int y)
{
    // 后注册的在上层（后加入先命中）
    for (auto it = m_popups.rbegin(); it != m_popups.rend(); ++it) {
        if (UIControl* c = (*it)->HitTestTree(x, y)) {
            return c;
        }
    }
    return nullptr;
}

void UIWindow::DispatchMouse(UINT msg, WPARAM wp, LPARAM lp)
{
    if (m_root == nullptr && m_popups.empty()) {
        return;
    }
    // 控件树坐标是 DIP（dpiScale 模式下窗口按 DPI 放大、D2D 按 DPI 绘制），
    // 而鼠标消息带的是物理客户区像素 → 先换算，否则 >100% 缩放下命中整体右/下偏移。
    const float scale = m_dpiScaled ? DpiScale() : 1.0f;
    const int x = static_cast<int>(static_cast<float>(GET_X_LPARAM(lp)) / scale);
    const int y = static_cast<int>(static_cast<float>(GET_Y_LPARAM(lp)) / scale);
    const bool left = (wp & MK_LBUTTON) != 0;

    // 命中：弹出层优先于控件树（V0.3.6）
    const auto hit = [&]() -> UIControl* {
        UIControl* c = HitTestPopups(x, y);
        if (c == nullptr && m_root != nullptr) {
            c = m_root->HitTestTree(x, y);
        }
        return c;
    };

    switch (msg) {
    case WM_MOUSEMOVE: {
        m_mousePos = { x, y };
        UIControl* c = hit();
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
        UIControl* c = hit();
        m_pressedCtrl = c;
        SetFocusControl(c);
        if (c != nullptr) {
            c->OnMouseDown(x - c->X(), y - c->Y(), true);
        }
        break;
    }
    case WM_LBUTTONUP: {
        UIControl* c = hit();
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
    case WM_RBUTTONDOWN: {
        // V0.5.7 右键按下：记录按下控件（右键抬起时同控件才触发 OnRightClick）
        UIControl* c = hit();
        m_pressedCtrl = c;
        break;
    }
    case WM_RBUTTONUP: {
        // V0.5.7 右键抬起：按下与抬起在同一控件 → 右键点击
        UIControl* c = hit();
        if (c != nullptr && c == m_pressedCtrl) {
            c->OnRightClick(x - c->X(), y - c->Y());
        }
        m_pressedCtrl = nullptr;
        break;
    }
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
