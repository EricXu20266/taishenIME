/// 设置对话框 — 声明
///
/// 对应 SPEC: docs/modules/settings-ui/SPEC.md
/// 替代工具栏「设置」按钮直接打开 config.ini：弹出原生 Win32 对话框，
/// 可视化编辑全部配置项，确定后写回 config.ini（热加载自动生效）。

#pragma once

#include <windows.h>
#include <string>

namespace taishen {

/// 弹出设置对话框（模态，自带消息循环）。
/// 资源从 DLL 模块句柄（g_hModule）加载。
/// @param parent 父窗口（工具栏 HWND），可空
/// @param dllDir DLL 所在目录（带尾分隔符），用于定位 config.ini
void ShowSettingsDialog(HWND parent, const std::wstring& dllDir);

} // namespace taishen
