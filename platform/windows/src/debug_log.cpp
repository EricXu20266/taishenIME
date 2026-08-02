/// 诊断日志实现 — 见 debug_log.h
#include "debug_log.h"

#include <windows.h>

#include <cstdio>
#include <ctime>
#include <string>

namespace taishen {

// 日志文件路径（进程内缓存，避免反复查环境变量）
static std::wstring GetLogPath()
{
    static std::wstring s_path;
    if (!s_path.empty()) {
        return s_path;
    }
    wchar_t buf[MAX_PATH] = {0};
    const DWORD len = GetEnvironmentVariableW(L"LOCALAPPDATA", buf, MAX_PATH);
    if (len > 0 && len < MAX_PATH) {
        s_path = std::wstring(buf) + L"\\TaishenIME\\ime_debug.log";
    } else {
        s_path = L"ime_debug.log";
    }
    return s_path;
}

void DebugLog(const std::string& msg)
{
    // 用 Win32 API 写文件——之前 _wfopen_s("a, ccs=UTF-8") 只写 BOM 不写内容
    HANDLE h = CreateFileW(GetLogPath().c_str(), FILE_APPEND_DATA,
                           FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (h == INVALID_HANDLE_VALUE) {
        return;
    }

    // 空文件先写 UTF-8 BOM
    LARGE_INTEGER sz = {};
    if (GetFileSizeEx(h, &sz) && sz.QuadPart == 0) {
        DWORD written = 0;
        const BYTE bom[3] = {0xEF, 0xBB, 0xBF};
        WriteFile(h, bom, 3, &written, nullptr);
    }

    // 时间戳 [HH:MM:SS.mmm][pid:tid]
    SYSTEMTIME st = {};
    GetLocalTime(&st);
    char stamp[96] = {0};
    snprintf(stamp, sizeof(stamp), "[%02u:%02u:%02u.%03u][%lu:%lu] ",
             st.wHour, st.wMinute, st.wSecond, st.wMilliseconds,
             GetCurrentProcessId(), GetCurrentThreadId());

    const std::string line = std::string(stamp) + msg + "\n";
    DWORD written = 0;
    WriteFile(h, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
    CloseHandle(h);
}

void DebugLogHr(const std::string& msg, long hr)
{
    char hrBuf[64] = {0};
    snprintf(hrBuf, sizeof(hrBuf), " 0x%08lX", hr);
    DebugLog(msg + hrBuf);
}

} // namespace taishen
