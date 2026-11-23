/// 诊断日志实现 — 见 debug_log.h
/// V0.4.x 性能优化：文件句柄保持打开（不再每次 CreateFile/CloseHandle），
/// 减少按键延迟。可通过环境变量 TAISHEN_DEBUG_LOG=0 关闭日志。
#include "debug_log.h"

#include <windows.h>

#include <atomic>
#include <cstdio>
#include <ctime>
#include <mutex>
#include <string>

namespace taishen {

namespace {

constexpr LONGLONG kMaxLogBytes = 5LL * 1024 * 1024; // 5 MB 轮转阈值

std::atomic<bool> s_logEnabled{false}; // V0.4.x: default off, set by ApplyConfig
std::mutex s_logMutex;
std::wstring s_path;
HANDLE s_hLog = INVALID_HANDLE_VALUE;

const std::wstring& GetLogPathLocked()
{
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

void RotateIfNeededLocked(const std::wstring& logPath)
{
    if (s_hLog == INVALID_HANDLE_VALUE) {
        return;
    }
    LARGE_INTEGER sz = {};
    if (!GetFileSizeEx(s_hLog, &sz) || sz.QuadPart < kMaxLogBytes) {
        return;
    }
    CloseHandle(s_hLog);
    s_hLog = INVALID_HANDLE_VALUE;
    const std::wstring backupPath = logPath + L".1";
    MoveFileExW(logPath.c_str(), backupPath.c_str(), MOVEFILE_REPLACE_EXISTING);
}

HANDLE EnsureLogHandleLocked()
{
    const std::wstring& logPath = GetLogPathLocked();
    RotateIfNeededLocked(logPath);
    if (s_hLog != INVALID_HANDLE_VALUE) {
        return s_hLog;
    }
    // 容器内 %LOCALAPPDATA% 被重定向到包内 `AC` 目录，`TaishenIME` 子目录通常
    // 不存在——CreateFile 会静默失败，AppContainer 诊断日志就永远看不到。
    // 先创建父目录（幂等；已存在时 CreateDirectory 返回 ERROR_ALREADY_EXISTS）。
    const size_t slash = logPath.find_last_of(L'\\');
    if (slash != std::wstring::npos) {
        CreateDirectoryW(logPath.substr(0, slash).c_str(), nullptr);
    }
    s_hLog = CreateFileW(logPath.c_str(), FILE_APPEND_DATA,
                         FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                         nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
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

void WriteLogLine(const char* prefix, const std::string& msg)
{
    SYSTEMTIME st = {};
    GetLocalTime(&st);
    char stamp[96] = {0};
    snprintf(stamp, sizeof(stamp), "[%02u:%02u:%02u.%03u][%lu:%lu] ",
             st.wHour, st.wMinute, st.wSecond, st.wMilliseconds,
             GetCurrentProcessId(), GetCurrentThreadId());
    const std::string line = std::string(stamp) + prefix + msg + "\n";

    std::lock_guard<std::mutex> lk(s_logMutex);
    HANDLE h = EnsureLogHandleLocked();
    if (h == INVALID_HANDLE_VALUE) {
        return;
    }
    DWORD written = 0;
    WriteFile(h, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
    // 不 CloseHandle——保持打开，避免每次 CreateFile 开销
}

} // namespace

void SetDebugLogEnabled(bool enabled)
{
    s_logEnabled.store(enabled, std::memory_order_relaxed);
}

void DebugLog(const std::string& msg)
{
    if (!s_logEnabled.load(std::memory_order_relaxed)) {
        return;
    }
    WriteLogLine("", msg);
}

/// 强制写日志：绕过 s_logEnabled 开关（崩溃/保护路径必须落盘）
void ForceLog(const std::string& msg)
{
    WriteLogLine("[FORCE] ", msg);
}

void DebugLogHr(const std::string& msg, long hr)
{
    char hrBuf[64] = {0};
    snprintf(hrBuf, sizeof(hrBuf), " 0x%08lX", hr);
    DebugLog(msg + hrBuf);
}

} // namespace taishen
