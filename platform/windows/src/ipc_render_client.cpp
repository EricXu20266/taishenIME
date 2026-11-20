/// V4 D2 候选窗进程外渲染 IPC 客户端 — 实现

#include "ipc_render_client.h"

#include "app_container.h"
#include "debug_log.h"

namespace taishen {

namespace {
constexpr ULONGLONG kReconnectBackoffMs = 500;
constexpr ULONGLONG kSpawnCooldownMs = 5000;
constexpr DWORD kPipeWaitMs = 15; // V4 约束：绝不在 UI 线程长等
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

bool CRenderIpcClient::EnsureConnected()
{
    if (m_pipe != INVALID_HANDLE_VALUE) {
        return true;
    }
    const ULONGLONG now = GetTickCount64();
    if (m_lastFailTick != 0 && (now - m_lastFailTick) < kReconnectBackoffMs) {
        return false;
    }

    for (int attempt = 0; attempt < 2; ++attempt) {
        HANDLE h = CreateFileW(
            ipc::kRenderPipeName,
            GENERIC_READ | GENERIC_WRITE,
            0,
            nullptr,
            OPEN_EXISTING,
            0,
            nullptr);
        if (h != INVALID_HANDLE_VALUE) {
            m_pipe = h;
            m_lastFailTick = 0;
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

bool CRenderIpcClient::SendPayload(const std::string& payload)
{
    if (!EnsureConnected()) {
        return false;
    }
    if (ipc::WritePipeFrame(m_pipe, payload)) {
        return true;
    }
    // 管道断开（例如 server 重启）：清掉旧句柄并立即重连重试一次
    Disconnect();
    m_lastFailTick = 0;
    if (!EnsureConnected()) {
        return false;
    }
    if (ipc::WritePipeFrame(m_pipe, payload)) {
        return true;
    }
    Disconnect();
    m_lastFailTick = GetTickCount64();
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
    if (!SendPayload(ipc::EncodePingCommand())) {
        return false;
    }
    std::string reply;
    if (!ipc::ReadPipeFrame(m_pipe, reply)) {
        Disconnect();
        return false;
    }
    return reply.find("\"pong\"") != std::string::npos;
}

} // namespace taishen
