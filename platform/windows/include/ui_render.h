/// 自研窗体系统 — D2D 渲染层（V0.3.0）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.1
/// 从候选窗抽取的 Direct2D/DirectWrite 公共能力：
/// 工厂/渲染目标管理、画刷缓存（同色不重建）、字体格式缓存、
/// 常用绘制原语（圆角矩形/文本/测量）。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include <dwrite.h>
#include <map>
#include <string>
#include <tuple>

namespace taishen {

/// Direct2D 渲染器（每窗口一个实例）
class UIRenderer
{
public:
    UIRenderer();
    ~UIRenderer();

    // 不允许拷贝
    UIRenderer(const UIRenderer&) = delete;
    UIRenderer& operator=(const UIRenderer&) = delete;

    /// 延迟创建设备资源（首次绘制时才创建工厂/渲染目标）。
    /// @return true 成功
    bool Ensure(HWND hwnd);

    /// 窗口尺寸变化时同步渲染目标（HwndRenderTarget 不自动跟随窗口 resize，
    /// 否则绘制区域停留在旧尺寸——候选窗多行展开/收起后内容被裁剪/错位）
    void Resize(HWND hwnd);

    /// 渲染帧：BeginDraw → 各绘制原语 → EndDraw
    void BeginDraw();
    void EndDraw();

    /// 整窗清色
    void Clear(D2D1_COLOR_F color);

    /// 填充矩形
    void FillRect(const D2D1_RECT_F& rc, D2D1_COLOR_F color);

    /// 填充圆角矩形
    void FillRoundedRect(const D2D1_RECT_F& rc, float radius, D2D1_COLOR_F color);

    /// V0.3.6：绘制裁剪（滚动面板用）。Push 后所有绘制限制在 rc 内。
    void PushClip(const D2D1_RECT_F& rc);
    void PopClip();

    /// 画边框（空心圆角矩形，线宽 1）
    void DrawRoundedRect(const D2D1_RECT_F& rc, float radius, D2D1_COLOR_F color,
                         float strokeWidth = 1.0f);

    /// 画直线
    void DrawLine(float x1, float y1, float x2, float y2,
                  D2D1_COLOR_F color, float strokeWidth = 1.0f);

    /// 绘制文本（自动裁剪到 rc 内）。
    /// @param align 水平对齐（LEADING/CENTER/TRAILING）
    void DrawText(const std::wstring& text, const D2D1_RECT_F& rc, float size,
                  D2D1_COLOR_F color, bool bold = false,
                  DWRITE_TEXT_ALIGNMENT align = DWRITE_TEXT_ALIGNMENT_LEADING,
                  DWRITE_PARAGRAPH_ALIGNMENT valign = DWRITE_PARAGRAPH_ALIGNMENT_NEAR);

    /// 测量文本尺寸（不含裁剪）
    D2D1_SIZE_F MeasureText(const std::wstring& text, float size, bool bold = false);

    /// 取画刷（同色缓存，不重复创建）
    ID2D1SolidColorBrush* Brush(D2D1_COLOR_F color);

    /// 取字体格式（按字号缓存，不重复创建）
    IDWriteTextFormat* Format(float size, bool bold);

    /// 当前 DPI 缩放系数（96 基准）
    float DpiScale() const { return m_dpiScale; }

    /// 释放设备资源（WM_DISPLAYCHANGE/设备丢失时调用）
    void ReleaseDeviceResources();

private:
    bool CreateDeviceResources();

    ID2D1Factory* m_factory = nullptr;
    ID2D1HwndRenderTarget* m_rt = nullptr;
    IDWriteFactory* m_dwrite = nullptr;
    HWND m_hwnd = nullptr;
    float m_dpiScale = 1.0f;

    /// 画刷缓存 key: COLORREF（0x00BBGGRR）
    std::map<DWORD, ID2D1SolidColorBrush*> m_brushes;
    /// 字体格式缓存 key: (字体名, 字号, 粗体)（V0.3.5 审查修复：原键漏 bold，
    /// 同字号粗体/普通首次创建后互相复用 → 粗体被画成非粗体）
    std::map<std::tuple<std::wstring, float, bool>, IDWriteTextFormat*> m_formats;
};

} // namespace taishen
