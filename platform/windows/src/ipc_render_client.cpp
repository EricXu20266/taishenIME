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
#include <cwctype>

#include "app_container.h"
#include "debug_log.h"

namespace taishen {

namespace {
constexpr ULONGLONG kHardFailBackoffMs = 5000;
constexpr ULONGLONG kReconnectProbeBackoffMs[] = {50, 150, 500, 1500, 5000};
constexpr ULONGLONG kDictNotReadyBackoffMs = 1000;
constexpr ULONGLONG kMismatchHealCooldownMs = 10000;
constexpr ULONGLONG kMismatchReconnectIntervalMs = 30000;
constexpr ULONGLONG kMismatchLogIntervalMs = 60000;
constexpr ULONGLONG kSpawnCooldownMs = 2000;
constexpr ULONGLONG kCircuitBackoffMs = 5 * 60 * 1000;
constexpr unsigned kMaxSpawnStreak = 5;
constexpr DWORD kPipeWaitMs = 15;      // 连接阶段：最多等 15ms，绝不在 UI 线程长等
constexpr DWORD kMismatchProbeBudgetMs = 30;
constexpr DWORD kWriteTimeoutMs = 20;  // 单帧写：超过 20ms 视为 server 异常
constexpr DWORD kReadTimeoutMs = 200;  // Ping 烟测等 pong 的上限

/// V2.1 指数短探测退避：50 → 150 → 500 → 1500 → 5000 ms，兼顾自拉起秒连与失败兜底
ULONGLONG ComputeReconnectBackoffMs(unsigned failCount)
{
    constexpr size_t kCount =
        sizeof(kReconnectProbeBackoffMs) / sizeof(kReconnectProbeBackoffMs[0]);
    if (failCount == 0) {
        return kReconnectProbeBackoffMs[0];
    }
    const size_t idx = (std::min)(static_cast<size_t>(failCount - 1), kCount - 1);
    return kReconnectProbeBackoffMs[idx];
}

std::wstring EnsureTrailingSlash(const std::wstring& dir)
{
    if (dir.empty()) {
        return L"";
    }
    const wchar_t back = dir.back();
    if (back == L'\\' || back == L'/') {
        return dir;
    }
    return dir + L"\\";
}

bool EqualsInsensitiveW(const std::wstring& a, const std::wstring& b)
{
    if (a.size() != b.size()) {
        return false;
    }
    for (size_t i = 0; i < a.size(); ++i) {
        if (std::towlower(a[i]) != std::towlower(b[i])) {
            return false;
        }
    }
    return true;
}

bool EndsWithInsensitiveW(const std::wstring& s, const std::wstring& suffix)
{
    if (s.size() < suffix.size()) {
        return false;
    }
    const size_t off = s.size() - suffix.size();
    for (size_t i = 0; i < suffix.size(); ++i) {
        if (std::towlower(s[off + i]) != std::towlower(suffix[i])) {
            return false;
        }
    }
    return true;
}

bool IsTrustedServerImagePath(const std::wstring& exePath, const std::wstring& dllDir)
{
    if (!EndsWithInsensitiveW(exePath, L"\\taishen_server.exe")) {
        return false;
    }
    const size_t slash = exePath.find_last_of(L"\\/");
    if (slash == std::wstring::npos) {
        return false;
    }
    const std::wstring parentDir = EnsureTrailingSlash(exePath.substr(0, slash + 1));

    if (!dllDir.empty() && EqualsInsensitiveW(parentDir, EnsureTrailingSlash(dllDir))) {
        return true;
    }
    wchar_t buf[MAX_PATH] = {0};
    DWORD len = GetEnvironmentVariableW(L"ProgramFiles", buf, MAX_PATH);
    if (len > 0 && len < MAX_PATH) {
        if (EqualsInsensitiveW(parentDir, std::wstring(buf, len) + L"\\TaishenIME\\")) {
            return true;
        }
    }
    len = GetEnvironmentVariableW(L"LOCALAPPDATA", buf, MAX_PATH);
    if (len > 0 && len < MAX_PATH) {
        if (EqualsInsensitiveW(parentDir, std::wstring(buf, len) + L"\\TaishenIME\\")) {
            return true;
        }
    }
    return false;
}
} // namespace

CRenderIpcClient::CRenderIpcClient(std::wstring pipeName)
    : m_pipeName(pipeName.empty() ? std::wstring(ipc::kRenderPipeName)
                                  : std::move(pipeName))
{
}

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
    m_dictNotReadyUntil = 0;
}

bool CRenderIpcClient::IsInHardBackoff() const
{
    if (m_hardFailUntil == 0) {
        return false;
    }
    return GetTickCount64() < m_hardFailUntil;
}

bool CRenderIpcClient::IsServerDictReady() const
{
    if (m_dictNotReadyUntil == 0) {
        return true;
    }
    return GetTickCount64() >= m_dictNotReadyUntil;
}

bool CRenderIpcClient::TryReconnectIfMismatchDue()
{
    if (!m_protocolMismatch.load()) {
        return true;
    }
    const ULONGLONG now = GetTickCount64();
    if (m_mismatchRetryAfter != 0 && now < m_mismatchRetryAfter) {
        return false;
    }
    return EnsureConnected(false, kMismatchProbeBudgetMs);
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
    const bool versionMismatch =
        (outPong.version != ipc::kRenderIpcVersion) ||
        (outPong.errorCode == "version_mismatch");
    if (versionMismatch) {
        m_protocolMismatch.store(true);
        const ULONGLONG now = GetTickCount64();
        if (m_lastMismatchLogTick == 0 ||
            (now - m_lastMismatchLogTick) >= kMismatchLogIntervalMs) {
            m_lastMismatchLogTick = now;
            ForceLog("RenderIpc: 协议不兼容：server 与 DLL 版本不一致，重启宿主应用或更新部署 "
                     "(server_v=" + std::to_string(outPong.version) +
                     " dll_v=" + std::to_string(ipc::kRenderIpcVersion) +
                     " code=" + outPong.errorCode + ")");
        }
        return false;
    }
    if (!outPong.ok) {
        ForceLog("RenderIpc: handshake rejected v=" + std::to_string(outPong.version) +
                 " expected=" + std::to_string(ipc::kRenderIpcVersion) +
                 " code=" + outPong.errorCode);
        return false;
    }
    m_protocolMismatch.store(false);
    m_mismatchRetryAfter = 0;
    m_lastMismatchLogTick = 0;
    return true;
}

bool CRenderIpcClient::TryHealVersionMismatch(ULONG serverPid, uint32_t serverVersion)
{
    // 只允许非容器宿主自愈，且严禁杀死高于自身协议版本的新服务端（防止降级攻击/残留 DLL 破坏）
    if (IsAppContainerProcess() || serverPid == 0 || serverPid == GetCurrentProcessId()) {
        return false;
    }
    if (serverVersion == 0 || serverVersion >= ipc::kRenderIpcVersion) {
        return false;
    }
    const ULONGLONG now = GetTickCount64();
    if (m_lastMismatchHealTick != 0 &&
        (now - m_lastMismatchHealTick) < kMismatchHealCooldownMs) {
        return false;
    }
    m_lastMismatchHealTick = now;

    HANDLE hProc = OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE,
        FALSE, serverPid);
    if (hProc == nullptr) {
        ForceLog("RenderIpc: mismatch heal OpenProcess failed pid=" +
                 std::to_string(serverPid) + " err=" + std::to_string(GetLastError()));
        return false;
    }

    wchar_t imgBuf[MAX_PATH * 2] = {0};
    DWORD imgLen = static_cast<DWORD>(sizeof(imgBuf) / sizeof(imgBuf[0]));
    const BOOL gotPath = QueryFullProcessImageNameW(hProc, 0, imgBuf, &imgLen);
    const std::wstring exePath = gotPath ? std::wstring(imgBuf, imgLen) : L"";
    if (!gotPath || !IsTrustedServerImagePath(exePath, m_dllDir)) {
        ForceLog("RenderIpc: mismatch heal skipped untrusted pid=" +
                 std::to_string(serverPid));
        CloseHandle(hProc);
        return false;
    }

    ForceLog("RenderIpc: terminating stale taishen_server.exe pid=" +
             std::to_string(serverPid) + " server_v=" + std::to_string(serverVersion) +
             " client_v=" + std::to_string(ipc::kRenderIpcVersion));
    const BOOL termOk = TerminateProcess(hProc, 0);
    if (termOk) {
        WaitForSingleObject(hProc, 500);
    } else {
        ForceLog("RenderIpc: TerminateProcess failed err=" + std::to_string(GetLastError()));
    }
    CloseHandle(hProc);
    if (!termOk) {
        return false;
    }

    m_lastSpawnTick = 0;
    m_lastFailTick = 0;
    m_hardFailUntil = 0;
    m_consecutiveFails = 0;
    EnsureServerRunning(m_dllDir);
    return true;
}

bool CRenderIpcClient::EnsureConnected(bool ignoreBackoff, DWORD budgetMs, bool allowAutoSpawn)
{
    if (m_pipe != INVALID_HANDLE_VALUE) {
        return true;
    }
    if (budgetMs == 0) {
        return false;
    }
    const ULONGLONG now = GetTickCount64();
    if (!ignoreBackoff) {
        if (m_protocolMismatch.load() &&
            m_mismatchRetryAfter != 0 && now < m_mismatchRetryAfter) {
            return false;
        }
        if (m_hardFailUntil != 0 && now < m_hardFailUntil) {
            return false;
        }
    }

    const ULONGLONG deadline = now + budgetMs;
    DWORD lastErr = 0;
    for (int attempt = 0; attempt < 2; ++attempt) {
        // P1-1：重叠模式打开，才能给读写加超时
        HANDLE h = CreateFileW(
            m_pipeName.c_str(),
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
                ULONG serverPid = 0;
                if (pong.errorCode == "version_mismatch" && pong.version > 0 &&
                    pong.version < ipc::kRenderIpcVersion) {
                    GetNamedPipeServerProcessId(h, &serverPid);
                }
                CloseHandle(h);
                const bool healed = TryHealVersionMismatch(serverPid, pong.version);
                const ULONGLONG failNow = GetTickCount64();
                m_lastFailTick = failNow;
                if (healed) {
                    // 自愈重启旧 server 期间按 50ms 起步快速探活
                    m_consecutiveFails = 1;
                    m_mismatchRetryAfter = 0;
                    m_hardFailUntil = failNow + ComputeReconnectBackoffMs(m_consecutiveFails);
                } else if (m_protocolMismatch.load()) {
                    m_mismatchRetryAfter = failNow + kMismatchReconnectIntervalMs;
                    m_hardFailUntil = failNow + kHardFailBackoffMs;
                } else {
                    ++m_consecutiveFails;
                    m_hardFailUntil = failNow + ComputeReconnectBackoffMs(m_consecutiveFails);
                }
                return false;
            }
            m_pipe = h;
            m_lastPong = pong;
            m_lastFailTick = 0;
            m_hardFailUntil = 0;
            m_consecutiveFails = 0;
            m_dictNotReadyUntil =
                pong.dictReady ? 0 : (GetTickCount64() + kDictNotReadyBackoffMs);
            // 连接并握手成功说明 server 可用，重置熔断计数
            m_spawnStreak = 0;
            m_circuitOpenUntil = 0;
            const std::string msg = "RenderIpc: connected v" +
                                    std::to_string(ipc::kRenderIpcVersion) +
                                    " dictReady=" + (pong.dictReady ? "1" : "0") +
                                    " entries=" + std::to_string(pong.entries) +
                                    " uiAccess=" + (pong.uiAccess ? "1" : "0") +
                                    " band=" + std::to_string(pong.windowBand) +
                                    " build=" + pong.build;
            if (IsAppContainerProcess()) {
                ForceLog(msg);
                if (!pong.uiAccess && pong.windowBand < 2) {
                    ForceLog("RenderIpc(AppContainer): WARN uiAccess server 未生效 (uiAccess=0 band=" +
                             std::to_string(pong.windowBand) + ")，容器内候选窗可能不可见");
                }
            } else {
                DebugLog(msg);
            }
            if (!pong.dictReady) {
                ForceLog("RenderIpc: server dict not ready (dictReady=0 -> 未就绪放行)");
            }
            return true;
        }
        const DWORD err = GetLastError();
        lastErr = err;
        if (err == ERROR_PIPE_BUSY && attempt == 0) {
            const DWORD rem = ipc::RemainingBudgetMs(deadline);
            const DWORD waitMs = (std::min)(kPipeWaitMs, rem);
            if (waitMs > 0 && WaitNamedPipeW(m_pipeName.c_str(), waitMs)) {
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

    const ULONGLONG failNow = GetTickCount64();
    m_lastFailTick = failNow;
    ++m_consecutiveFails;
    const ULONGLONG backoffMs = ComputeReconnectBackoffMs(m_consecutiveFails);
    m_hardFailUntil = failNow + backoffMs;
    if (m_protocolMismatch.load()) {
        const bool inHealGrace =
            (m_lastMismatchHealTick != 0) &&
            (failNow - m_lastMismatchHealTick) < kHardFailBackoffMs;
        if (!inHealGrace) {
            m_mismatchRetryAfter = failNow + kMismatchReconnectIntervalMs;
            m_hardFailUntil = failNow + kHardFailBackoffMs;
        }
    } else if (allowAutoSpawn && !IsAppContainerProcess() && !m_dllDir.empty() &&
               lastErr == ERROR_FILE_NOT_FOUND) {
        EnsureServerRunning(m_dllDir);
    }
    return false;
}

void CRenderIpcClient::HandleSendFailure(bool alreadyCountedFail)
{
    Disconnect();
    const ULONGLONG now = GetTickCount64();
    m_lastFailTick = now;
    if (m_protocolMismatch.load()) {
        m_hardFailUntil = now + kHardFailBackoffMs;
        if (m_mismatchRetryAfter == 0 || now >= m_mismatchRetryAfter) {
            m_mismatchRetryAfter = now + kMismatchReconnectIntervalMs;
        }
        return;
    }

    if (!alreadyCountedFail) {
        ++m_consecutiveFails;
    }
    const ULONGLONG backoffMs = ComputeReconnectBackoffMs(m_consecutiveFails);
    // V2.1：按 50/150/500/1500/5000 ms 指数短探测退避，避免 5s 黑洞且不阻塞宿主 UI
    m_hardFailUntil = now + backoffMs;

    if (IsAppContainerProcess()) {
        // V1：容器内不能拉起 server，只能留下可见证据
        ForceLog("RenderIpc(AppContainer): send failed, server unreachable (spawn not allowed)");
        return;
    }
    DebugLog("RenderIpc: send failed (fails=" + std::to_string(m_consecutiveFails) +
             " backoffMs=" + std::to_string(backoffMs) + "), relaunching taishen_server.exe");
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
    bool retryConnected = true;
    if (!m_protocolMismatch.load()) {
        m_lastFailTick = 0;
        m_hardFailUntil = 0;
        retryConnected = EnsureConnected(true);
        if (retryConnected && ipc::WritePipeFrameTimed(m_pipe, payload, kWriteTimeoutMs)) {
            return true;
        }
    }
    HandleSendFailure(!retryConnected && !m_protocolMismatch.load());
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

bool CRenderIpcClient::SendControlCommand(const std::string& payload,
                                          ipc::AckReply& outAck,
                                          DWORD budgetMs)
{
    if (budgetMs == 0) {
        return false;
    }
    const ULONGLONG deadline = GetTickCount64() + budgetMs;
    for (int attempt = 0; attempt < 2; ++attempt) {
        const bool wasConnected = (m_pipe != INVALID_HANDLE_VALUE);
        const DWORD connBudget = ipc::RemainingBudgetMs(deadline);
        if (connBudget == 0 || !EnsureConnected(true, connBudget, false)) {
            return false;
        }
        const DWORD writeBudget = (std::min)(kWriteTimeoutMs, ipc::RemainingBudgetMs(deadline));
        if (writeBudget == 0 || !ipc::WritePipeFrameTimed(m_pipe, payload, writeBudget)) {
            Disconnect();
            if (attempt == 0 && wasConnected && !m_protocolMismatch.load() &&
                ipc::RemainingBudgetMs(deadline) >= 15) {
                continue;
            }
            return false;
        }
        const DWORD readBudget = ipc::RemainingBudgetMs(deadline);
        std::string reply;
        if (readBudget == 0 || !ipc::ReadPipeFrameTimed(m_pipe, reply, readBudget)) {
            Disconnect();
            return false;
        }
        if (!ipc::DecodeAck(reply, outAck) || !outAck.ok) {
            Disconnect();
            return false;
        }
        return true;
    }
    return false;
}

bool CRenderIpcClient::SendOpenSettings(int tabIndex, DWORD budgetMs)
{
    ipc::AckReply ack;
    return SendControlCommand(
        ipc::EncodeOpenSettingsCommand(tabIndex, GetCurrentProcessId()),
        ack,
        budgetMs);
}

bool CRenderIpcClient::SendToggleVoice(bool* outListening, DWORD budgetMs)
{
    ipc::AckReply ack;
    if (!SendControlCommand(
            ipc::EncodeToggleVoiceCommand(GetCurrentProcessId()),
            ack,
            budgetMs)) {
        return false;
    }
    if (outListening != nullptr) {
        *outListening = ack.listening;
    }
    return true;
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
        // F1：之前连上时若 dictReady=0，窗口到期重探一次，确保词库就绪后可自动恢复
        if (!m_lastPong.dictReady && m_dictNotReadyUntil != 0 &&
            GetTickCount64() >= m_dictNotReadyUntil) {
            ipc::PongInfo refreshed;
            const DWORD hsBudget = ipc::RemainingBudgetMs(deadline);
            if (hsBudget == 0 || !PerformHandshake(m_pipe, hsBudget, refreshed)) {
                if (attempt == 0 && wasConnected && !m_protocolMismatch.load() &&
                    ipc::RemainingBudgetMs(deadline) >= 15) {
                    Disconnect();
                    m_lastFailTick = 0;
                    m_hardFailUntil = 0;
                    continue;
                }
                HandleSendFailure();
                return false;
            }
            m_lastPong = refreshed;
            m_dictNotReadyUntil =
                refreshed.dictReady ? 0 : (GetTickCount64() + kDictNotReadyBackoffMs);
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
        if (outResp.eaten && !m_lastPong.dictReady) {
            m_lastPong.dictReady = true;
            m_dictNotReadyUntil = 0;
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
    // 健康检查：忽略退避，供冒烟测试在 server 启动窗口期内重试（不触发自拉起）
    if (!EnsureConnected(true, kReadTimeoutMs, false)) {
        return false;
    }
    ipc::PongInfo pong;
    if (!PerformHandshake(m_pipe, kReadTimeoutMs, pong)) {
        Disconnect();
        const ULONGLONG now = GetTickCount64();
        m_lastFailTick = now;
        if (m_protocolMismatch.load()) {
            m_mismatchRetryAfter = now + kMismatchReconnectIntervalMs;
            m_hardFailUntil = now + kHardFailBackoffMs;
        } else {
            ++m_consecutiveFails;
            m_hardFailUntil = now + ComputeReconnectBackoffMs(m_consecutiveFails);
        }
        return false;
    }
    m_lastPong = pong;
    m_consecutiveFails = 0;
    m_lastFailTick = 0;
    m_hardFailUntil = 0;
    m_dictNotReadyUntil = pong.dictReady ? 0 : (GetTickCount64() + kDictNotReadyBackoffMs);
    return true;
}

} // namespace taishen
