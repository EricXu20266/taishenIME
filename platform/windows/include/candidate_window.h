/// Direct2D 候选窗口 — 声明（V0.3.2 迁移：基于自研窗体系统）
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md + docs/modules/ui-framework/SPEC.md §3.7
/// 内部 = UIWindow（置顶无边框不抢焦点）+ 候选内容面板（自绘：拼音/候选/高亮/翻页）。
/// 公开接口与 0.1.x 完全一致（tsf_module 调用方零改动）。

#pragma once

#include <functional>
#include <string>
#include <vector>
#include <windows.h>

#include "config_reader.h"
#include "ui_window.h"

namespace taishen {

/// Direct2D 候选窗口
class CCandidateWindow
{
public:
    CCandidateWindow();
    ~CCandidateWindow();

    /// 鼠标点击候选的回调（index 为 0 起的候选索引，由 TSF 层处理选词上屏）
    using ClickCallback = std::function<void(int index)>;

    /// 滚轮翻页回调（P0-1）：delta>0 上一页 / <0 下一页，由 TSF 层调 engine_page
    using PageCallback = std::function<void(int delta)>;

    /// 创建窗口（延迟创建，首次 UpdateState 时触发）
    /// @return true 成功
    bool Initialize();

    /// 更新内容并定位显示。
    /// @param pinyin     当前拼音串（UTF-8）
    /// @param candidates 当前页候选词列表（UTF-8）
    /// @param caretRect  光标屏幕坐标（用于定位窗口）
    /// @param page       当前页码（0 起，翻页指示用）
    /// @param totalPages 总页数（翻页指示用）
    /// 拼音为空或候选为空时自动隐藏。
    void UpdateState(const std::string& pinyin,
                     const std::vector<std::string>& candidates,
                     const RECT& caretRect,
                     int page = 0,
                     int totalPages = 0);

    /// 隐藏窗口
    void Hide();

    /// 设置选中候选索引（高亮显示用）
    void SetSelectedIndex(int index);

    /// 设置鼠标点击回调（选词上屏）
    void SetClickCallback(ClickCallback cb);

    /// 设置滚轮翻页回调（P0-1）
    void SetPageCallback(PageCallback cb);

    /// 设置候选标签格式（P0-1，%d=数字/%s=数字文本，如 "%d." "①"）
    void SetLabelFormat(const std::wstring& fmt);

    /// 设置布局参数（P0-1）：圆角/内边距/候选间距
    void SetLayout(float cornerRadius, float hiliteRadius, int padding, int spacing);

    /// 设置候选窗口主题（V0.2.4，渲染时应用）
    void SetTheme(const CandidateTheme& theme);

    /// 设置多行展开状态（V0.2.14）：↓ 展开 / ↑ 收起
    void SetMultiRow(bool enabled);

    /// 设置候选窗字体与字号（V0.2.21）
    void SetFont(const std::wstring& face, float size);

    /// 系统主题变化回调（V0.2.20）：未显式配置时切换默认主题并重绘
    void OnSystemThemeChanged();

    /// 设置是否跟随系统主题（V0.2.20）：false = 用户显式配置
    void SetFollowSystemTheme(bool follow);

    /// 设置行内预编辑（V0.2.18）：true=拼音写组合，候选窗不画拼音行
    void SetInlinePreedit(bool enable);

    /// 查询多行展开状态
    bool IsMultiRow() const { return m_multiRow; }

    /// 查询当前是否可见（冒烟测试用）
    bool IsVisible() const { return m_visible; }

private:
    /// 定位窗口（光标下方，屏幕超界回缩）
    void PositionWindow(const RECT& caretRect);

    /// 候选内容面板（自绘：背景/拼音/候选/高亮/悬停/翻页指示；命中/尺寸计算）
    class CandidatePanel;

    UIWindow m_window;
    CandidatePanel* m_panel = nullptr;   ///< 内容面板（根控件，本类拥有）

    // 状态（公开接口的语义载体）
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    int m_selectedIndex = 0;
    int m_page = 0;
    int m_totalPages = 0;
    bool m_visible = false;
    bool m_multiRow = false;
    bool m_inlinePreedit = true;
    bool m_followSystemTheme = true;
    std::wstring m_labelFormat = L"%d.";
    float m_cornerRadius = 4.0f;
    float m_hiliteRadius = 3.0f;
    int m_padding = 8;
    int m_spacing = 14;
    std::wstring m_fontFace = L"Microsoft YaHei";
    float m_fontSize = 16.0f;
    CandidateTheme m_theme;
    ClickCallback m_clickCb;
    PageCallback m_pageCb;
};

} // namespace taishen
