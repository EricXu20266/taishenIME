/// V4 D2/D3 进程外渲染与引擎 IPC 客户端 — 声明
///
/// 供 TSF DLL 调用，通过命名管道与 `taishen_server.exe` 通信：
///   - D2：候选窗渲染帧/坐标/隐藏/配置重载
///   - D3：按键查询（KeyInput → CandidateList，<=50ms 超时透传）与异步选词提交（Commit）
/// 硬约束（ARCHITECT-V4 V1/V4）：
///   1. AppContainer 进程内只连不拉（绝不调用 CreateProcess）；
///   2. 普通权限宿主进程激活时自动确保 `taishen_server.exe` 已启动；
///   3. 断连快速退避（不阻塞 UI 线程），单次发送与往返严格受短超时保护；
///   4. 握手严格校验协议版本号（kRenderIpcVersion），不匹配拒绝会话并透传（普通宿主可自愈重启旧服务端）。

#pragma once

#include <windows.h>

#include <atomic>
#include <cstdint>
#include <string>

#include "render_ipc.h"

namespace taishen {

class CRenderIpcClient {
public:
    explicit CRenderIpcClient(std::wstring pipeName = ipc::kRenderPipeName);
    ~CRenderIpcClient();

    CRenderIpcClient(const CRenderIpcClient&) = delete;
    CRenderIpcClient& operator=(const CRenderIpcClient&) = delete;

    /// 普通宿主进程调用：若 server 未运行则以无窗口后台进程拉起 `dllDir + taishen_server.exe`。
    /// AppContainer 进程内直接跳过（V1 约束：容器子进程会继承容器令牌）。
    void EnsureServerRunning(const std::wstring& dllDir);

    /// 发送显示/刷新候选窗指令
    bool SendShow(const ipc::RenderCommand& cmd);

    /// 发送异步光标坐标更新指令
    bool SendMove(uint64_t sessionId, const RECT& caret);

    /// 发送隐藏候选窗指令
    bool SendHide(uint64_t sessionId);

    /// 通知 server 热重载 config.ini（主题/字体/布局/引擎开关变化）
    bool SendReloadConfig();

    /// V1.2：请求 server 打开设置窗（非阻塞，在 server 独立线程弹窗，不阻塞宿主 UI）
    bool SendOpenSettings(int tabIndex = 0, DWORD budgetMs = 150);

    /// V1.2：请求 server 切换语音输入开关
    bool SendToggleVoice(bool* outListening = nullptr, DWORD budgetMs = 150);

    /// V4 D3：同步发送按键并取回候选列表（总往返预算 <= budgetMs，默认 50ms；超时立即断管并透传）
    bool SendKeyInput(const ipc::KeyInputRequest& req,
                      ipc::CandidateListResponse& outResp,
                      DWORD budgetMs = 50);

    /// V4 D3：异步发送已选词提交指令（无返回，供 server 落盘学习与上下文更新）
    bool SendCommit(const ipc::CommitRequest& req);

    /// Q3.1 TSF 再转换：按选定汉字查询服务端同音候选列表
    bool SendReconvert(const ipc::ReconvertRequest& req,
                       ipc::ReconvertResponse& outResp,
                       DWORD budgetMs = 150);

    /// 同步健康检查（发 ping 并校验当前协议 pong，供冒烟测试与诊断使用）
    bool Ping();

    /// 是否正处于硬失败退避窗口内（超时/断管/版本不匹配后短期快速透传）
    bool IsInHardBackoff() const;

    /// V2.1 诊断：当前连续重连失败次数（握手/发送成功后清零）
    unsigned ConsecutiveFailCount() const { return m_consecutiveFails; }

    /// 服务端词库是否已就绪（F1：未就绪时 1s 窗口内零 IPC 透传，到期自动重探恢复）
    bool IsServerDictReady() const;

    /// 服务端与当前 DLL 协议版本是否不兼容（仅在握手成功时清除）
    bool IsProtocolIncompatible() const { return m_protocolMismatch.load(); }

    /// 协议失配状态下，若已满 30 秒重试间隔则尝试执行一次轻量握手重连；未到期零 IPC 直接返回 false
    bool TryReconnectIfMismatchDue();

    /// 当前是否持有已连接的管道句柄
    bool IsConnected() const { return m_pipe != INVALID_HANDLE_VALUE; }

    /// 最近一次握手/Ping 收到的服务端元信息
    const ipc::PongInfo& LastPongInfo() const { return m_lastPong; }

    /// 主动断开管道
    void Disconnect();

private:
    bool EnsureConnected(bool ignoreBackoff = false, DWORD budgetMs = 45, bool allowAutoSpawn = true);
    bool PerformHandshake(HANDLE hPipe, DWORD timeoutMs, ipc::PongInfo& outPong);
    bool TryHealVersionMismatch(ULONG serverPid, uint32_t serverVersion);
    bool SendPayload(const std::string& payload);
    bool SendControlCommand(const std::string& payload, ipc::AckReply& outAck, DWORD budgetMs);
    /// 发送失败后的自愈：清句柄 → 进入指数退避窗口 → 普通宿主进程尝试重新拉起 server
    void HandleSendFailure(bool alreadyCountedFail = false);

    HANDLE m_pipe = INVALID_HANDLE_VALUE;
    ULONGLONG m_lastFailTick = 0;
    ULONGLONG m_lastSpawnTick = 0;
    /// 连续重连失败计数（V2.1：驱动 50/150/500/1500/5000 ms 指数短探测）
    unsigned m_consecutiveFails = 0;
    /// 硬失败退避截止时刻（超时/写失败/版本错后短期不再尝试，避免每个按键都吃一次超时）
    ULONGLONG m_hardFailUntil = 0;
    /// F1：服务端 dictReady=0 时的短期透传截止时刻（1000ms 后允许重探恢复）
    ULONGLONG m_dictNotReadyUntil = 0;
    /// F8：版本失配主动替换旧 server 的 10s 冷却时间戳
    ULONGLONG m_lastMismatchHealTick = 0;
    /// 连续拉起 server 的次数与熔断截止时刻（防止「崩溃-重拉」热循环）
    unsigned m_spawnStreak = 0;
    ULONGLONG m_circuitOpenUntil = 0;
    /// 首次 EnsureServerRunning 传入的安装目录，供失败后重新拉起使用
    std::wstring m_dllDir;
    std::wstring m_pipeName;
    std::atomic<bool> m_protocolMismatch{false};
    ULONGLONG m_mismatchRetryAfter = 0;
    ULONGLONG m_lastMismatchLogTick = 0;
    ipc::PongInfo m_lastPong;
};

} // namespace taishen
