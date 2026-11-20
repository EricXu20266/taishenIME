/// V4 D2 候选窗进程外渲染 IPC 客户端 — 实现
///
/// 2026-11-20 审计修复：
///   P1-1 渲染路径不再使用「无超时阻塞写」。句柄以 FILE_FLAG_OVERLAPPED 打开，
///        单帧写超时 20ms；超时即 CancelIoEx 并进入硬失败退避（5s），
///        避免 server 卡死时每个按键都阻塞宿主 UI 线程。
///   P1-2 server 崩溃自愈。发送失败后，普通宿主进程会重新拉起 taishen_server.exe；
///        容器进程仍遵守 V1（只连不拉），只记录可见的错误。

#include "ipc_render_client.h"

#include "app_container.h"
#include "debug_log.h"

namespace taishen {

namespace {
constexpr ULONGLONG kReconnectBackoffMs = 500;
constexpr ULONGLONG kHardFailBackoffMs = 5000;
constexpr ULONGLONG kSpawnCooldownMs = 2000;
constexpr DWORD kPipeWaitMs = 15;      // 连接阶段：最多等 15ms，绝不在 UI 线程长等
constexpr DWORD kWriteTimeoutMs = 20;  // 单帧写：超过 20ms 视为 server 异常
constexpr DWORD kReadTimeoutMs = 200;  // Ping 等 pong 的上限
} // namespace

CRenderIpcClient::CRenderIpcClient() = default;

CRenderIpcClient::~CRenderIpcClient()
{
    Disconnect();
}

void CRenderIpcClient::Disconnect()
{
    if (m_pipe != INVALID_HANDLE_VALUE) {
        CloseHandle(m_pipe);
        m_pipe = INVALID_HANDLE_VALUE;
    }
}

void CRenderIpcClient::EnsureServerRunning(const std::wstring& dllDir)
{
    if (!dllDir.empty()) {
        m_dllDir = dllDir;
    }
    // V1 硬约束：AppContainer 进程禁止拉起 server（子进程会继承容器令牌）
    if (IsAppContainerProcess() || dllDir.empty()) {
        return;
    }
    // 管道已存在或互斥量已被持有 → server 已在运行
    HANDLE hMutex = OpenMutexW(SYNCHRONIZE, FALSE, ipc::kServerMutexName);
    if (hMutex != nullptr) {
        CloseHandle(hMutex);
        return;
    }
    const ULONGLONG now = GetTickCount64();
    if (m_lastSpawnTick != 0 && (now - m_lastSpawnTick) < kSpawnCooldownMs) {
        return;
    }
    m_lastSpawnTick = now;

    const std::wstring exePath = dllDir + L"taishen_server.exe";
    const DWORD attrs = GetFileAttributesW(exePath.c_str());
    if (attrs == INVALID_FILE_ATTRIBUTES || (attrs & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        return;
    }

    STARTUPINFOW si{};
    si.cb = sizeof(si);
    si.dwFlags = STARTF_USESHOWWINDOW;
    si.wShowWindow = SW_HIDE;
    PROCESS_INFORMATION pi{};
    std::wstring cmdLine = L"\"" + exePath + L"\"";
    const BOOL ok = CreateProcessW(
        exePath.c_str(),
        &cmdLine[0],
        nullptr,
        nullptr,
        FALSE,
        CREATE_NO_WINDOW | DETACHED_PROCESS,
        nullptr,
        dllDir.c_str(),
        &si,
        &pi);
    if (ok) {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        DebugLog("RenderIpc: launched taishen_server.exe pid=" + std::to_string(pi.dwProcessId));
    } else {
        DebugLog("RenderIpc: failed to launch taishen_server.exe err=" +
                 std::to_string(GetLastError()));
    }
}

bool CRenderIpcClient::EnsureConnected(bool ignoreBackoff)
{
    if (m_pipe != INVALID_HANDLE_VALUE) {
        return true;
    }
    const ULONGLONG now = GetTickCount64();
    if (!ignoreBackoff) {
        if (m_hardFailUntil != 0 && now < m_hardFailUntil) {
            return false;
        }
        if (m_lastFailTick != 0 && (now - m_lastFailTick) < kReconnectBackoffMs) {
            return false;
        }
    }

    for (int attempt = 0; attempt < 2; ++attempt) {
        // P1-1：重叠模式打开，才能给读写加超时
        HANDLE h = CreateFileW(
            ipc::kRenderPipeName,
            GENERIC_READ | GENERIC_WRITE,
            0,
            nullptr,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            nullptr);
        if (h != INVALID_HANDLE_VALUE) {
            m_pipe = h;
            m_lastFailTick = 0;
            m_hardFailUntil = 0;
            return true;
        }
        const DWORD err = GetLastError();
        if (err == ERROR_PIPE_BUSY && attempt == 0) {
            if (WaitNamedPipeW(ipc::kRenderPipeName, kPipeWaitMs)) {
                continue;
            }
        }
        if (IsAppContainerProcess()) {
            ForceLog("RenderIpc(AppContainer): connect failed err=" + std::to_string(err));
        } else {
            DebugLog("RenderIpc: connect failed err=" + std::to_string(err));
        }
        break;
    }

    m_lastFailTick = now;
    return false;
}

void CRenderIpcClient::HandleSendFailure()
{
    Disconnect();
    const ULONGLONG now = GetTickCount64();
    m_lastFailTick = now;
    // 硬退避：server 疑似卡死/崩溃时，短期内不再每键阻塞一次
    m_hardFailUntil = now + kHardFailBackoffMs;

    if (IsAppContainerProcess()) {
        // V1：容器内不能拉起 server，只能留下可见证据
        ForceLog("RenderIpc(AppContainer): send failed, server unreachable (spawn not allowed)");
        return;
    }
    DebugLog("RenderIpc: send failed, relaunching taishen_server.exe");
    m_lastSpawnTick = 0; // 本次失败不受上次拉起冷却限制
    EnsureServerRunning(m_dllDir);
}

bool CRenderIpcClient::SendPayload(const std::string& payload)
{
    if (!EnsureConnected()) {
        return false;
    }
    if (ipc::WritePipeFrameTimed(m_pipe, payload, kWriteTimeoutMs)) {
        return true;
    }
    // 管道断开（例如 server 重启）：清掉旧句柄并立即重连重试一次
    Disconnect();
    m_lastFailTick = 0;
    m_hardFailUntil = 0;
    if (EnsureConnected(true) && ipc::WritePipeFrameTimed(m_pipe, payload, kWriteTimeoutMs)) {
        return true;
    }
    HandleSendFailure();
    return false;
}

bool CRenderIpcClient::SendShow(const ipc::RenderCommand& cmd)
{
    return SendPayload(ipc::EncodeShowCommand(cmd));
}

bool CRenderIpcClient::SendMove(uint64_t sessionId, const RECT& caret)
{
    return SendPayload(ipc::EncodeMoveCommand(sessionId, caret));
}

bool CRenderIpcClient::SendHide(uint64_t sessionId)
{
    // 未连上 server 时 hide 无需触发连接重试
    if (m_pipe == INVALID_HANDLE_VALUE) {
        return false;
    }
    return SendPayload(ipc::EncodeHideCommand(sessionId));
}

bool CRenderIpcClient::SendReloadConfig()
{
    if (m_pipe == INVALID_HANDLE_VALUE) {
        return false;
    }
    return SendPayload(ipc::EncodeReloadCommand());
}

bool CRenderIpcClient::Ping()
{
    // 健康检查：忽略退避，供冒烟测试在 server 启动窗口期内重试
    if (!EnsureConnected(true)) {
        return false;
    }
    if (!ipc::WritePipeFrameTimed(m_pipe, ipc::EncodePingCommand(), kWriteTimeoutMs)) {
        Disconnect();
        m_lastFailTick = GetTickCount64();
        return false;
    }
    std::string reply;
    if (!ipc::ReadPipeFrameTimed(m_pipe, reply, kReadTimeoutMs)) {
        Disconnect();
        m_lastFailTick = GetTickCount64();
        return false;
    }
    return reply.find("\"pong\"") != std::string::npos;
}

} // namespace taishen
