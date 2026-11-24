/// 运行中可见窗口进程拾取器（Q2.2）
///
/// 使用 EnumWindows 枚举桌面可见顶层窗口对应的进程名，去重排序后供高级页配置复用。

#pragma once

#include <windows.h>
#include <string>
#include <vector>

namespace taishen {

struct RunningWindowApp {
    std::wstring exeName;     // 小写文件名，如 L"code.exe"
    std::wstring windowTitle; // 窗口标题（过长截断至 40 字符）
    DWORD pid = 0;
};

/// 从完整路径提取小写可执行文件名（兼容 `\` 与 `/` 分隔符）。
std::wstring ExtractLowerExeName(const std::wstring& fullPath);

/// 按小写 `exeName` 去重（优先保留首个非空标题）、排除指定进程名并按字典序升序排列。
std::vector<RunningWindowApp> DeduplicateAndSortApps(
    const std::vector<RunningWindowApp>& raw,
    const std::wstring& excludeExeLower = L"");

/// 枚举当前桌面可见顶层窗口对应进程（排除自身及工具浮窗）。
std::vector<RunningWindowApp> EnumerateVisibleWindowApps();

} // namespace taishen
