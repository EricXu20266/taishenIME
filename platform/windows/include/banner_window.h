/// 右下角状态横幅 — 声明
///
/// 对应 DEV-TRACKER: 0.2.15（0.1.26）
/// 参考 rime-ice/weasel.yaml 设计：紧凑卡片式，深灰底 + 浅字 + 圆角 + 阴影。
///
/// 关键设计（0.1.26 重构）：横幅是**进程级全局单例**，显示/隐藏由
/// 「前台窗口线程是否激活泰深」驱动（SetWinEventHook 监听前台切换 +
/// 进程内激活线程集合）。解决：切到游戏（英文输入法）横幅残留的问题——
/// TSF 输入法状态是每线程的，绑实例必然漏。

#pragma once

#include <windows.h>
#include <string>
#include <unordered_set>

namespace taishen {

/// 右下角常驻状态横幅（全局单例）
class CBannerWindow
{
public:
    /// 全局单例（横幅跨实例共享，跟随前台窗口）
    static CBannerWindow& Instance();

    /// 线程激活泰深（ActivateEx 调用）：注册到激活线程集合 + 评估前台
    void RegisterThread(DWORD tid);

    /// 线程停用泰深（Deactivate 调用）：注销 + 评估前台
    void UnregisterThread(DWORD tid);

    /// 更新状态文字（中英/简繁/双拼切换时调用）
    /// 当前前台线程是泰深时立即刷新显示；否则仅记录（下次前台评估用）
    void UpdateStatus(const std::wstring& text);

    /// 查询当前是否可见（冒烟测试用）
    bool IsVisible() const { return m_visible; }

    /// 查询当前状态文字（冒烟测试用）
    const std::wstring& StatusText() const { return m_text; }

private:
    CBannerWindow();
    ~CBannerWindow();

    /// 前台窗口切换回调（SetWinEventHook，OUTOFCONTEXT）
    static void CALLBACK OnForegroundChanged(HWINEVENTHOOK hook, DWORD event,
                                             HWND hwnd, LONG idObject,
                                             LONG idChild, DWORD idEventThread,
                                             DWORD dwmsEventTime);

    /// 评估当前前台窗口线程是否激活泰深 → 显示/隐藏横幅
    void EvaluateForeground();

    /// 显示横幅（右下角）
    void Show(const std::wstring& text);

    /// 隐藏横幅
    void Hide();

    /// 懒创建窗口
    bool EnsureWindow();
    /// 定位到工作区右下角（含边距）
    void PositionBottomRight();
    /// GDI 全量绘制（双缓冲，rime 深色风格）
    void OnPaint(HDC hdc, const RECT& rcPaint);

    // 窗口过程需要访问 OnPaint
    friend LRESULT CALLBACK BannerWndProc(HWND, UINT, WPARAM, LPARAM);

    HWND m_hwnd;
    bool m_initialized;
    bool m_visible;
    std::wstring m_text;  // 状态文字（如 "中文模式 · 简繁 · 双拼"）

    // 激活线程集合（进程内：哪些线程激活了泰深）
    // 前台线程 ∈ 集合 → 显示横幅；否则隐藏
    struct ThreadSet {
        std::unordered_set<DWORD> tids;
    } m_threads;

    HWINEVENTHOOK m_hook;  // 前台切换监听（每进程一个）
};

} // namespace taishen
