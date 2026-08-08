/// DPI 工具 — 坐标单位对齐（V0.4.3-P1B）
///
/// 背景：TSF ITfContextView::GetTextExt 返回的屏幕坐标单位取决于宿主进程
///       DPI 感知模式：
///         Per-Monitor V2 / System aware → 物理像素
///         DPI unaware（老游戏/老应用） → 96-DPI 逻辑像素
///       GetCursorPos / SetWindowPos 等 API 一律使用物理像素。
///       若 GetTextExt 坐标不换算直接使用，系统缩放 ≠ 100% 时候选窗会偏移。
///
/// 注意：TSF DLL in-proc，进程级 DPI 感知由宿主 manifest 决定，本 DLL 无法
///       改变——只能做坐标换算。DPI-unaware 宿主下候选窗自身的虚拟化问题
///       由调用方（候选窗创建路径）另行处理。

#pragma once

#include <windows.h>

namespace taishen {

/// 宿主进程是否 DPI unaware（老游戏/老应用）。
/// 此类进程的 TSF GetTextExt 返回 96-DPI 逻辑像素，需换算。
bool IsHostDpiUnaware(HWND hostWnd);

/// 把 TSF GetTextExt 返回的屏幕坐标换算为物理像素（原地修改）。
/// hostWnd: 目标应用窗口；pRect: GetTextExt 原始输出。
/// DPI aware 宿主直接返回（已是物理像素）。
void CaretToPhysicalPixel(HWND hostWnd, RECT* pRect);

} // namespace taishen
