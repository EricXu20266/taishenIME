/// 右下角输入法工具栏 — 声明
///
/// 对应 DEV-TRACKER: 0.2.15（0.1.26）
/// 搜狗/QQ拼音同款工具栏：按钮 = 中/英切换、简/繁、双拼、设置。
/// 视觉参考 rime-ice/weasel.yaml（purity_of_form_custom）：深灰底+圆角+阴影。
///
/// 关键设计：
/// 1. 进程级全局单例（跨 CTextService 实例共享）
/// 2. 显示 = 托盘开关(enabled) && 前台窗口线程激活泰深
///    （SetWinEventHook 监听前台切换 + 激活线程集合）
///    → 切到游戏（英文输入法）工具栏自动隐藏
/// 3. 托盘右键菜单可显示/隐藏工具栏（SetEnabled）

#pragma once

#include <windows.h>
#include <string>
#include <unordered_set>

#include "ui_window.h"

namespace taishen {

/// 工具栏按钮命令
enum class ToolbarCmd {
    Ascii,      // 中/英切换
    Trad,       // 简/繁切换
    Shuangpin,  // 双拼/全拼切换
    Settings,   // 打开输入法设置（config.ini）
    Count,
};

/// 右下角输入法工具栏（全局单例）
class CBannerWindow
{
public:
    /// 全局单例
    static CBannerWindow& Instance();

    /// 线程激活泰深（ActivateEx 调用）
    void RegisterThread(DWORD tid);

    /// 线程停用泰深（Deactivate 调用）
    void UnregisterThread(DWORD tid);

    /// 托盘开关：显示/隐藏工具栏（右键输入法图标菜单）
    /// 显示 = enabled && 前台线程激活泰深
    void SetEnabled(bool enabled);

    /// 当前是否开启（托盘开关状态）
    bool IsEnabled() const { return m_enabled; }

    /// 查询当前是否可见（冒烟测试用）
    bool IsVisible() const { return m_window.IsVisible(); }

    /// 设置主题模式（V0.2.20）：true=浅色，false=深色
    void SetLightTheme(bool light);

    /// 查询当前是否为浅色主题（V0.2.20）
    bool IsLightTheme() const { return m_lightTheme; }

    /// 强制重绘（0.2.26：Shift 切换中英后刷新按钮高亮状态）
    /// V0.3.3：同步引擎状态到按钮并重绘
    void Refresh();

    /// 评估显示条件：enabled && 前台线程激活泰深 → 显示/隐藏
    /// 公开供 OnKeyDown 兜底调用（0.3.x：SetWinEventHook 回调失效时
    /// 前台切换不再驱动工具栏，用户打字时主动重新评估恢复显示）
    void EvaluateForeground();

private:
    CBannerWindow();
    ~CBannerWindow();

    /// 前台窗口切换回调（SetWinEventHook，OUTOFCONTEXT）
    static void CALLBACK OnForegroundChanged(HWINEVENTHOOK hook, DWORD event,
                                             HWND hwnd, LONG idObject,
                                             LONG idChild, DWORD idEventThread,
                                             DWORD dwmsEventTime);

    /// 执行按钮命令（点击中/英/简繁/双拼/设置）
    void HandleCommand(ToolbarCmd cmd);

    /// 懒创建窗口
    bool EnsureWindow();
    /// 定位到工作区右下角（含边距）
    void PositionBottomRight();
    /// 同步按钮文字/激活态到面板（引擎状态 → UI）
    void RefreshButtons();
    /// 当前状态文字（中/英·简繁·双拼，tooltip 用）
    std::wstring StatusText() const;

    /// 工具栏内容面板（自绘：阴影/卡片/4 按钮，V0.3.3 基于窗体系统）
    class ToolbarPanel;

    UIWindow m_window;
    ToolbarPanel* m_panel = nullptr;  ///< 内容面板（本类拥有）
    bool m_enabled = true;    // 托盘开关（默认开）

    // 激活线程集合（前台线程 ∈ 集合 → 显示工具栏）
    std::unordered_set<DWORD> m_tids;

    bool m_lightTheme = false;  // 浅色主题（V0.2.20）

    HWINEVENTHOOK m_hook = nullptr;  // 前台切换监听
};

} // namespace taishen
