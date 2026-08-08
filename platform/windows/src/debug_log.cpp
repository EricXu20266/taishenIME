/// 诊断日志实现 — 见 debug_log.h
/// V0.4.x 性能优化：文件句柄保持打开（不再每次 CreateFile/CloseHandle），
/// 减少按键延迟。可通过环境变量 TAISHEN_DEBUG_LOG=0 关闭日志。
#include "debug_log.h"

#include <windows.h>

#include <cstdio>
#include <ctime>
#include <string>

namespace taishen {

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

static bool IsLogEnabled()
{
    // 默认开启；设置 TAISHEN_DEBUG_LOG=0 关闭
    static int s_enabled = -1;
    if (s_enabled < 0) {
        wchar_t buf[8] = {0};
        const DWORD len = GetEnvironmentVariableW(L"TAISHEN_DEBUG_LOG", buf, 7);
        s_enabled = (len == 1 && buf[0] == L'0') ? 0 : 1;
    }
    return s_enabled == 1;
}

static HANDLE EnsureLogHandle()
{
    static HANDLE s_hLog = INVALID_HANDLE_VALUE;
    if (s_hLog != INVALID_HANDLE_VALUE) {
        return s_hLog;
    }
    s_hLog = CreateFileW(GetLogPath().c_str(), FILE_APPEND_DATA,
                         FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                         OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (s_hLog == INVALID_HANDLE_VALUE) {
        return s_hLog;
    }
    // 空文件写 UTF-8 BOM
    LARGE_INTEGER sz = {};
    if (GetFileSizeEx(s_hLog, &sz) && sz.QuadPart == 0) {
        DWORD written = 0;
        const BYTE bom[3] = {0xEF, 0xBB, 0xBF};
        WriteFile(s_hLog, bom, 3, &written, nullptr);
    }
    return s_hLog;
}

void DebugLog(const std::string& msg)
{
    if (!IsLogEnabled()) {
        return;
    }
    HANDLE h = EnsureLogHandle();
    if (h == INVALID_HANDLE_VALUE) {
        return;
    }

    SYSTEMTIME st = {};
    GetLocalTime(&st);
    char stamp[96] = {0};
    snprintf(stamp, sizeof(stamp), "[%02u:%02u:%02u.%03u][%lu:%lu] ",
             st.wHour, st.wMinute, st.wSecond, st.wMilliseconds,
             GetCurrentProcessId(), GetCurrentThreadId());

    const std::string line = std::string(stamp) + msg + "\n";
    DWORD written = 0;
    WriteFile(h, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
    // 不 CloseHandle——保持打开，避免每次 CreateFile 开销
}

void DebugLogHr(const std::string& msg, long hr)
{
    char hrBuf[64] = {0};
    snprintf(hrBuf, sizeof(hrBuf), " 0x%08lX", hr);
    DebugLog(msg + hrBuf);
}

} // namespace taishen
