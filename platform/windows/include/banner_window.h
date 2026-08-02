/// 右下角状态横幅 — 声明
///
/// 对应 DEV-TRACKER: 0.2.15（0.1.26）
/// 搜狗/QQ拼音同款：切到泰深输入法时右下角常驻显示状态横幅，
/// 切走隐藏；中英/简繁/双拼状态切换时即时更新文字。
/// GDI 绘制（无需 D2D 工厂），双缓冲防闪烁。

#pragma once

#include <windows.h>
#include <string>

namespace taishen {

/// 右下角常驻状态横幅
class CBannerWindow
{
public:
    CBannerWindow();
    ~CBannerWindow();

    /// 显示横幅并设置状态文字（ActivateEx 时调用）
    void Show(const std::wstring& text);

    /// 更新状态文字（状态切换时调用；未显示则仅记录）
    void UpdateStatus(const std::wstring& text);

    /// 隐藏（Deactivate 时调用）
    void Hide();

    /// 查询当前是否可见（冒烟测试用）
    bool IsVisible() const { return m_visible; }

    /// 查询当前状态文字（冒烟测试用）
    const std::wstring& StatusText() const { return m_text; }

private:
    // 窗口过程需要访问 OnPaint
    friend LRESULT CALLBACK BannerWndProc(HWND, UINT, WPARAM, LPARAM);

    /// 懒创建窗口（首次 Show 时）
    bool EnsureWindow();
    /// 定位到工作区右下角（含边距）
    void PositionBottomRight();
    /// GDI 全量绘制（双缓冲）
    void OnPaint(HDC hdc, const RECT& rcPaint);

    HWND m_hwnd;
    bool m_initialized;
    bool m_visible;
    std::wstring m_text;  // 状态文字（如 "中文模式 · 简繁 · 双拼"）
};

} // namespace taishen
