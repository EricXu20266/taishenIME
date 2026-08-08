/// DPI 工具 — 坐标单位对齐（V0.4.3-P1B）
///
/// 详见 dpi_util.h。实现要点：
///   - GetWindowDpiAwarenessContext 检测宿主感知模式（Win10 1607+）
///   - DPI unaware 时按 GetDpiForSystem()/96 把逻辑像素换算为物理像素
///   - 不能用 GetDpiForWindow(宿主)：unaware 进程该 API 恒返回 96

#include "dpi_util.h"

namespace taishen {

bool IsHostDpiUnaware(HWND hostWnd)
{
    // GetWindowDpiAwarenessContext 失败（Win10 1607 之前）→ 保守按 unaware 处理
    const DPI_AWARENESS_CONTEXT ctx = GetWindowDpiAwarenessContext(hostWnd);
    if (ctx == nullptr) {
        return true;
    }
    const DPI_AWARENESS awareness = GetAwarenessFromDpiAwarenessContext(ctx);
    // 0 = DPI_AWARENESS_UNAWARE；-4 = DPI_AWARENESS_UNAWARE_GDISCALED
    // （后者为兼容旧 SDK 不用宏，坐标同为 96-DPI 逻辑像素）
    return awareness == DPI_AWARENESS_UNAWARE || awareness == static_cast<DPI_AWARENESS>(-4);
}

void CaretToPhysicalPixel(HWND hostWnd, RECT* pRect)
{
    if (pRect == nullptr || !IsHostDpiUnaware(hostWnd)) {
        return; // DPI aware：已是物理像素
    }
    const UINT sysDpi = GetDpiForSystem(); // 系统 DPI（缩放 125% → 120）
    if (sysDpi == 0 || sysDpi == 96) {
        return; // 100% 缩放无需换算
    }
    const float k = static_cast<float>(sysDpi) / 96.0f;
    pRect->left = static_cast<LONG>(static_cast<float>(pRect->left) * k);
    pRect->top = static_cast<LONG>(static_cast<float>(pRect->top) * k);
    pRect->right = static_cast<LONG>(static_cast<float>(pRect->right) * k);
    pRect->bottom = static_cast<LONG>(static_cast<float>(pRect->bottom) * k);
}

} // namespace taishen
