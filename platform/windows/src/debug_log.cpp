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
    FILE* f = nullptr;
    if (_wfopen_s(&f, GetLogPath().c_str(), L"a, ccs=UTF-8") != 0 || f == nullptr) {
        return;
    }

    // 时间戳 [HH:MM:SS.mmm]
    SYSTEMTIME st = {};
    GetLocalTime(&st);
    char stamp[64] = {0};
    snprintf(stamp, sizeof(stamp), "[%02u:%02u:%02u.%03u][%lu:%lu] ",
             st.wHour, st.wMinute, st.wSecond, st.wMilliseconds,
             GetCurrentProcessId(), GetCurrentThreadId());

    fputs(stamp, f);
    fputs(msg.c_str(), f);
    fputs("\n", f);
    fclose(f);
}

void DebugLogHr(const std::string& msg, long hr)
{
    char hrBuf[64] = {0};
    snprintf(hrBuf, sizeof(hrBuf), " 0x%08lX", hr);
    DebugLog(msg + hrBuf);
}

} // namespace taishen
