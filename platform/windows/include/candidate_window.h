/// Direct2D 候选窗口 — 声明
///
/// 对应 SPEC: docs/modules/presentation/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.6 Direct2D 候选窗口渲染
///
/// 置顶无边框透明窗口，Direct2D + DirectWrite 渲染拼音串与候选词。
/// 数据由 TSF 层（CTextService::RefreshState）通过 FFI 拉取后传入。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>
#include <string>
#include <vector>

namespace taishen {

/// Direct2D 候选窗口
class CCandidateWindow
{
public:
    CCandidateWindow();
    ~CCandidateWindow();

    /// 创建窗口 + D2D 资源（延迟初始化，首次 UpdateState 时才创建）
    /// @return true 成功
    bool Initialize();

    /// 更新内容并定位显示。
    /// @param pinyin     当前拼音串（UTF-8）
    /// @param candidates 候选词列表（UTF-8）
    /// @param caretRect  光标屏幕坐标（用于定位窗口）
    /// 拼音为空或候选为空时自动隐藏。
    void UpdateState(const std::string& pinyin,
                     const std::vector<std::string>& candidates,
                     const RECT& caretRect);

    /// 隐藏窗口
    void Hide();

    /// 设置选中候选索引（高亮显示用）
    void SetSelectedIndex(int index);

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

    // 窗口
    HWND m_hwnd;
    bool m_initialized;

    // D2D 资源
    ID2D1Factory* m_pD2DFactory;
    ID2D1HwndRenderTarget* m_pRenderTarget;
    ID2D1SolidColorBrush* m_pBgBrush;
    ID2D1SolidColorBrush* m_pTextBrush;
    ID2D1SolidColorBrush* m_pHighlightBrush;
    IDWriteFactory* m_pDWriteFactory;
    IDWriteTextFormat* m_pTextFormat;

    // 状态
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    int m_selectedIndex;
    bool m_visible;

    // 布局常量
    static constexpr int kPadding = 8;        // 窗口内边距
    static constexpr int kItemGap = 14;       // 候选词间距
    static constexpr int kPinyinHeight = 18;  // 拼音行高
    static constexpr int kCandidateHeight = 22; // 候选行高
    static constexpr float kFontSize = 16.0f; // 正文字号
    static constexpr float kPinyinFontSize = 13.0f; // 拼音字号
};

} // namespace taishen
