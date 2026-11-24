/// 运行中可见窗口进程拾取器实现（Q2.2）

#include "process_picker.h"

#include <algorithm>
#include <cwctype>
#include <unordered_map>

namespace taishen {

namespace {

std::wstring ToLowerW(std::wstring s)
{
    for (wchar_t& ch : s) {
        ch = static_cast<wchar_t>(towlower(ch));
    }
    return s;
}

std::wstring TruncateTitle(const std::wstring& title)
{
    constexpr size_t kMaxChars = 40;
    if (title.size() <= kMaxChars) {
        return title;
    }
    return title.substr(0, kMaxChars) + L"…";
}

std::wstring QueryProcessExeLower(DWORD pid)
{
    if (pid == 0) {
        return std::wstring();
    }
    HANDLE hProc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (hProc == nullptr) {
        return std::wstring();
    }
    wchar_t buf[MAX_PATH * 2] = {};
    DWORD size = static_cast<DWORD>(sizeof(buf) / sizeof(buf[0]));
    std::wstring exe;
    if (QueryFullProcessImageNameW(hProc, 0, buf, &size) && size > 0) {
        exe = ExtractLowerExeName(std::wstring(buf, size));
    }
    CloseHandle(hProc);
    return exe;
}

BOOL CALLBACK EnumVisibleWindowsProc(HWND hwnd, LPARAM lParam)
{
    if (!IsWindowVisible(hwnd)) {
        return TRUE;
    }
    if (GetWindow(hwnd, GW_OWNER) != nullptr) {
        return TRUE;
    }
    const LONG_PTR exStyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    if ((exStyle & WS_EX_TOOLWINDOW) != 0 && (exStyle & WS_EX_APPWINDOW) == 0) {
        return TRUE;
    }

    const int titleLen = GetWindowTextLengthW(hwnd);
    if (titleLen <= 0) {
        return TRUE;
    }
    std::wstring title(static_cast<size_t>(titleLen) + 1, L'\0');
    const int copied = GetWindowTextW(hwnd, &title[0], titleLen + 1);
    if (copied <= 0) {
        return TRUE;
    }
    title.resize(static_cast<size_t>(copied));

    DWORD pid = 0;
    GetWindowThreadProcessId(hwnd, &pid);
    if (pid == 0 || pid == GetCurrentProcessId()) {
        return TRUE;
    }

    const std::wstring exe = QueryProcessExeLower(pid);
    if (exe.empty()) {
        return TRUE;
    }

    auto* out = reinterpret_cast<std::vector<RunningWindowApp>*>(lParam);
    out->push_back(RunningWindowApp{ exe, title, pid });
    return TRUE;
}

} // namespace

std::wstring ExtractLowerExeName(const std::wstring& fullPath)
{
    if (fullPath.empty()) {
        return std::wstring();
    }
    const size_t pos = fullPath.find_last_of(L"\\/");
    const std::wstring base =
        (pos == std::wstring::npos) ? fullPath : fullPath.substr(pos + 1);
    return ToLowerW(base);
}

std::vector<RunningWindowApp> DeduplicateAndSortApps(
    const std::vector<RunningWindowApp>& raw,
    const std::wstring& excludeExeLower)
{
    const std::wstring exclude = ToLowerW(excludeExeLower);
    std::unordered_map<std::wstring, RunningWindowApp> dedup;
    dedup.reserve(raw.size());

    for (const auto& entry : raw) {
        const std::wstring exe = ToLowerW(entry.exeName);
        if (exe.empty() || (!exclude.empty() && exe == exclude)) {
            continue;
        }
        const std::wstring cleanTitle = TruncateTitle(entry.windowTitle);
        auto it = dedup.find(exe);
        if (it == dedup.end()) {
            dedup.emplace(exe, RunningWindowApp{ exe, cleanTitle, entry.pid });
        } else if (it->second.windowTitle.empty() && !cleanTitle.empty()) {
            it->second.windowTitle = cleanTitle;
            it->second.pid = entry.pid;
        }
    }

    std::vector<RunningWindowApp> result;
    result.reserve(dedup.size());
    for (auto& kv : dedup) {
        result.push_back(std::move(kv.second));
    }
    std::sort(result.begin(), result.end(),
              [](const RunningWindowApp& a, const RunningWindowApp& b) {
                  return a.exeName < b.exeName;
              });
    return result;
}

std::vector<RunningWindowApp> EnumerateVisibleWindowApps()
{
    std::vector<RunningWindowApp> collected;
    EnumWindows(EnumVisibleWindowsProc, reinterpret_cast<LPARAM>(&collected));

    wchar_t selfPath[MAX_PATH] = {};
    std::wstring selfExe;
    const DWORD len = GetModuleFileNameW(nullptr, selfPath, MAX_PATH);
    if (len > 0 && len < MAX_PATH) {
        selfExe = ExtractLowerExeName(std::wstring(selfPath, len));
    }
    return DeduplicateAndSortApps(collected, selfExe);
}

} // namespace taishen
