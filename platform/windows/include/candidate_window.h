/// Direct2D 候选窗口 — 声明
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.6 Direct2D 候选窗口渲染
///
/// 置顶无边框透明窗口，Direct2D + DirectWrite 渲染拼音串与候选词。
/// 数据由 TSF 层（CTextService::RefreshState）通过 FFI 拉取后传入。
/// 支持鼠标点击选词（0.1.13）与翻页指示（0.1.13）。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>
#include <functional>
#include <string>
#include <vector>

#include "config_reader.h"

namespace taishen {

/// Direct2D 候选窗口
class CCandidateWindow
{
public:
    CCandidateWindow();
    ~CCandidateWindow();

    /// 鼠标点击候选的回调（index 为 0 起的候选索引，由 TSF 层处理选词上屏）
    using ClickCallback = std::function<void(int index)>;

    /// 创建窗口 + D2D 资源（延迟初始化，首次 UpdateState 时才创建）
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
    // D2D 设备资源（工厂、渲染目标、画刷、字体）
    bool CreateDeviceResources();
    void ReleaseDeviceResources();

    // 窗口过程需要访问 Render
    friend LRESULT CALLBACK CandidateWndProc(HWND, UINT, WPARAM, LPARAM);

    // 绘制背景、拼音、候选词
    void Render();

    // 计算窗口位置（光标下方，屏幕超界回缩）
    void PositionWindow(const RECT& caretRect);

    // 计算窗口期望宽度/高度（按内容自适应）
    void CalculateSize(int& width, int& height);

    // 命中检测：将窗口内 x/y 坐标映射为候选索引（-1 表示未命中）
    // V0.2.14：多行模式按行列定位
    int HitTest(int x, int y) const;

    // 窗口
    HWND m_hwnd;
    bool m_initialized;

    // D2D 资源
    ID2D1Factory* m_pD2DFactory;
    ID2D1HwndRenderTarget* m_pRenderTarget;
    ID2D1SolidColorBrush* m_pBgBrush;
    ID2D1SolidColorBrush* m_pTextBrush;
    ID2D1SolidColorBrush* m_pHighlightBrush;
    ID2D1SolidColorBrush* m_pDimBrush;
    IDWriteFactory* m_pDWriteFactory;
    IDWriteTextFormat* m_pTextFormat;

    // 状态
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    int m_selectedIndex;
    bool m_visible;
    int m_page;        // 当前页码（0 起）
    int m_totalPages;  // 总页数
    float m_dpiScale;  // DPI 缩放系数（96 基准）
    ClickCallback m_clickCb;
    CandidateTheme m_theme;  // 候选窗口主题（V0.2.4）
    bool m_multiRow;         // 多行展开状态（V0.2.14）
    std::wstring m_fontFace = L"Microsoft YaHei"; // 字体名（V0.2.21）
    float m_fontSize = 16.0f;                     // 正文字号（V0.2.21）
    bool m_followSystemTheme = true;              // 跟随系统主题（V0.2.20）
    bool m_inlinePreedit = true;                  // 行内预编辑（V0.2.18）

    // 布局常量
    static constexpr int kPadding = 8;        // 窗口内边距
    static constexpr int kItemGap = 14;       // 候选词间距
    static constexpr float kPinyinFontSize = 13.0f; // 拼音字号基准
    static constexpr int kPerRow = 5;        // 多行模式每行候选数（V0.2.14）
};

} // namespace taishen
