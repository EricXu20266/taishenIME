/// V4 D2/D3 进程外渲染与引擎 IPC 客户端 — 实现
///
/// 2026-11-20 审计修复：
///   P1-1 渲染路径不再使用「无超时阻塞写」。句柄以 FILE_FLAG_OVERLAPPED 打开，
///        单帧写超时 20ms；超时即 CancelIoEx 并进入硬失败退避（5s），
///        避免 server 卡死时每个按键都阻塞宿主 UI 线程。
///   P1-2 server 崩溃自愈。发送失败后，普通宿主进程会重新拉起 taishen_server.exe；
///        容器进程仍遵守 V1（只连不拉），只记录可见的错误。
///   P1-2b 拉起熔断。连续 5 次拉起后仍连不上，说明 server 起不来而非偶发崩溃，
///        停止重拉并熔断 5 分钟，避免「打字→崩→重拉→再崩」的热循环。
/// 2026-11-23 V4 D3 扩展：
///   - 建连即执行 V2 Ping/Pong 握手，版本号不匹配立刻断开并拒绝静默退化；
///   - SendKeyInput 全程受单一 deadline（<= 50ms）约束，超时立即断管并透传；
///   - SendCommit 单向异步写，用于用户学习与上下文更新。

#include "ipc_render_client.h"

#include <shellapi.h>

#include <algorithm>

#include "app_container.h"
#include "debug_log.h"

namespace taishen {

namespace {
constexpr ULONGLONG kReconnectBackoffMs = 500;
constexpr ULONGLONG kHardFailBackoffMs = 5000;
constexpr ULONGLONG kSpawnCooldownMs = 2000;
constexpr ULONGLONG kCircuitBackoffMs = 5 * 60 * 1000;
constexpr unsigned kMaxSpawnStreak = 5;
constexpr DWORD kPipeWaitMs = 15;      // 连接阶段：最多等 15ms，绝不在 UI 线程长等
constexpr DWORD kWriteTimeoutMs = 20;  // 单帧写：超过 20ms 视为 server 异常
constexpr DWORD kReadTimeoutMs = 200;  // Ping 烟测等 pong 的上限
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

bool CRenderIpcClient::IsInHardBackoff() const
{
    if (m_hardFailUntil == 0) {
        return false;
    }
    return GetTickCount64() < m_hardFailUntil;
}

void CRenderIpcClient::EnsureServerRunning(const std::wstring& dllDir)
{
    if (!dllDir.empty()) {
        m_dllDir = dllDir;
    }
    // V1 硬约束：AppContainer 进程禁止拉起 server（子进程会继承容器令牌）
    if (IsAppContainerProcess() || m_dllDir.empty()) {
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

    // 熔断（P1-2b）：连续拉起后仍连不上，停止重拉
    if (m_spawnStreak >= kMaxSpawnStreak) {
        if (now >= m_circuitOpenUntil) {
            m_circuitOpenUntil = now + kCircuitBackoffMs;
            ForceLog("RenderIpc: server 连续 " + std::to_string(m_spawnStreak) +
                     " 次拉起后仍不可用，熔断 5 分钟（不再重拉）");
        }
        return;
    }

    m_lastSpawnTick = now;

    // 若安装目录 %ProgramFiles%\TaishenIME\taishen_server.exe 存在（V3 uiAccess 版本），
    // 必须走 ShellExecuteW("open")，CreateProcessW 对 uiAccess=true 会返回 740。
    wchar_t pfBuf[MAX_PATH] = {0};
    const DWORD pfLen = GetEnvironmentVariableW(L"ProgramFiles", pfBuf, MAX_PATH);
    if (pfLen > 0 && pfLen < MAX_PATH) {
        const std::wstring pfDir = std::wstring(pfBuf, pfLen) + L"\\TaishenIME\\";
        const std::wstring pfExe = pfDir + L"taishen_server.exe";
        const DWORD pfAttrs = GetFileAttributesW(pfExe.c_str());
        if (pfAttrs != INVALID_FILE_ATTRIBUTES &&
            (pfAttrs & FILE_ATTRIBUTE_DIRECTORY) == 0) {
            const HINSTANCE hInst = ShellExecuteW(
                nullptr, L"open", pfExe.c_str(), nullptr, pfDir.c_str(), SW_HIDE);
            const INT_PTR shellRet = reinterpret_cast<INT_PTR>(hInst);
            if (shellRet > 32) {
                ++m_spawnStreak;
                DebugLog("RenderIpc: ShellExecute launched uiAccess server ret=" +
                         std::to_string(shellRet) + " streak=" + std::to_string(m_spawnStreak));
            } else {
                DebugLog("RenderIpc: ShellExecute failed ret=" +
                         std::to_string(shellRet));
            }
            return;
        }
    }

    const std::wstring exePath = m_dllDir + L"taishen_server.exe";
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
        m_dllDir.c_str(),
        &si,
        &pi);
    if (ok) {
        ++m_spawnStreak;
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        DebugLog("RenderIpc: launched taishen_server.exe pid=" + std::to_string(pi.dwProcessId) +
                 " streak=" + std::to_string(m_spawnStreak));
    } else {
        DebugLog("RenderIpc: failed to launch taishen_server.exe err=" +
                 std::to_string(GetLastError()));
    }
}

bool CRenderIpcClient::PerformHandshake(HANDLE hPipe, DWORD timeoutMs, ipc::PongInfo& outPong)
{
    if (hPipe == INVALID_HANDLE_VALUE || timeoutMs == 0) {
        return false;
    }
    const ULONGLONG deadline = GetTickCount64() + timeoutMs;
    const DWORD writeMs = (std::min)(kWriteTimeoutMs, ipc::RemainingBudgetMs(deadline));
    if (writeMs == 0 ||
        !ipc::WritePipeFrameTimed(hPipe, ipc::EncodePingCommand(0, GetCurrentProcessId()), writeMs)) {
        return false;
    }
    const DWORD readMs = ipc::RemainingBudgetMs(deadline);
    if (readMs == 0) {
        return false;
    }
    std::string reply;
    if (!ipc::ReadPipeFrameTimed(hPipe, reply, readMs)) {
        return false;
    }
    if (!ipc::DecodePong(reply, outPong)) {
        ForceLog("RenderIpc: invalid handshake reply");
        return false;
    }
    if (outPong.version != ipc::kRenderIpcVersion || !outPong.ok) {
        ForceLog("RenderIpc: handshake rejected v=" + std::to_string(outPong.version) +
                 " expected=" + std::to_string(ipc::kRenderIpcVersion) +
                 " code=" + outPong.errorCode);
        return false;
    }
    return true;
}

bool CRenderIpcClient::EnsureConnected(bool ignoreBackoff, DWORD budgetMs)
{
    if (m_pipe != INVALID_HANDLE_VALUE) {
        return true;
    }
    if (budgetMs == 0) {
        return false;
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

    const ULONGLONG deadline = now + budgetMs;
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
            ipc::PongInfo pong;
            const DWORD hsBudget = ipc::RemainingBudgetMs(deadline);
            if (!PerformHandshake(h, hsBudget, pong)) {
                CloseHandle(h);
                m_lastFailTick = GetTickCount64();
                m_hardFailUntil = m_lastFailTick + kHardFailBackoffMs;
                return false;
            }
            m_pipe = h;
            m_lastPong = pong;
            m_lastFailTick = 0;
            m_hardFailUntil = 0;
            // 连接并握手成功说明 server 可用，重置熔断计数
            m_spawnStreak = 0;
            m_circuitOpenUntil = 0;
            const std::string msg = "RenderIpc: connected v" +
                                    std::to_string(ipc::kRenderIpcVersion) +
                                    " dictReady=" + (pong.dictReady ? "1" : "0") +
                                    " entries=" + std::to_string(pong.entries) +
                                    " build=" + pong.build;
            if (IsAppContainerProcess()) {
                ForceLog(msg);
            } else {
                DebugLog(msg);
            }
            return true;
        }
        const DWORD err = GetLastError();
        if (err == ERROR_PIPE_BUSY && attempt == 0) {
            const DWORD rem = ipc::RemainingBudgetMs(deadline);
            const DWORD waitMs = (std::min)(kPipeWaitMs, rem);
            if (waitMs > 0 && WaitNamedPipeW(ipc::kRenderPipeName, waitMs)) {
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

    m_lastFailTick = GetTickCount64();
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

bool CRenderIpcClient::SendKeyInput(const ipc::KeyInputRequest& req,
                                    ipc::CandidateListResponse& outResp,
                                    DWORD budgetMs)
{
    const ULONGLONG deadline = GetTickCount64() + budgetMs;
    const std::string payload = ipc::EncodeKeyInput(req);

    for (int attempt = 0; attempt < 2; ++attempt) {
        const bool wasConnected = (m_pipe != INVALID_HANDLE_VALUE);
        const DWORD connBudget = ipc::RemainingBudgetMs(deadline);
        if (connBudget == 0 || !EnsureConnected(attempt > 0, connBudget)) {
            return false;
        }
        const DWORD writeBudget = (std::min)(kWriteTimeoutMs, ipc::RemainingBudgetMs(deadline));
        if (writeBudget == 0 || !ipc::WritePipeFrameTimed(m_pipe, payload, writeBudget)) {
            if (attempt == 0 && wasConnected && ipc::RemainingBudgetMs(deadline) >= 15) {
                Disconnect();
                m_lastFailTick = 0;
                m_hardFailUntil = 0;
                continue;
            }
            HandleSendFailure();
            return false;
        }

        const DWORD readBudget = ipc::RemainingBudgetMs(deadline);
        std::string reply;
        if (readBudget == 0 || !ipc::ReadPipeFrameTimed(m_pipe, reply, readBudget)) {
            ForceLog("RenderIpc: KeyInput read timeout/fail seq=" + std::to_string(req.seq));
            HandleSendFailure();
            return false;
        }

        if (!ipc::DecodeCandidateList(reply, outResp)) {
            uint32_t ver = 0;
            std::string op;
            if (ipc::PeekEnvelope(reply, ver, op)) {
                ForceLog("RenderIpc: unexpected KeyInput reply v=" + std::to_string(ver) +
                         " op=" + op);
            } else {
                ForceLog("RenderIpc: malformed KeyInput reply");
            }
            HandleSendFailure();
            return false;
        }
        if (outResp.seq != req.seq || outResp.sessionId != req.sessionId) {
            ForceLog("RenderIpc: seq/sid mismatch expected=(" + std::to_string(req.seq) + "," +
                     std::to_string(req.sessionId) + ") got=(" + std::to_string(outResp.seq) +
                     "," + std::to_string(outResp.sessionId) + ")");
            HandleSendFailure();
            return false;
        }
        return true;
    }

    return false;
}

bool CRenderIpcClient::SendCommit(const ipc::CommitRequest& req)
{
    return SendPayload(ipc::EncodeCommit(req));
}

bool CRenderIpcClient::Ping()
{
    // 健康检查：忽略退避，供冒烟测试在 server 启动窗口期内重试
    if (!EnsureConnected(true, kReadTimeoutMs)) {
        return false;
    }
    if (!ipc::WritePipeFrameTimed(
            m_pipe, ipc::EncodePingCommand(0, GetCurrentProcessId()), kWriteTimeoutMs)) {
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
    ipc::PongInfo pong;
    if (!ipc::DecodePong(reply, pong) || pong.version != ipc::kRenderIpcVersion || !pong.ok) {
        Disconnect();
        m_lastFailTick = GetTickCount64();
        return false;
    }
    m_lastPong = pong;
    return true;
}

} // namespace taishen
