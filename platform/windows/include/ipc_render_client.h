/// V4 D2/D3 进程外渲染与引擎 IPC 客户端 — 声明
///
/// 供 TSF DLL 调用，通过命名管道与 `taishen_server.exe` 通信：
///   - D2：候选窗渲染帧/坐标/隐藏/配置重载
///   - D3：按键查询（KeyInput → CandidateList，<=50ms 超时透传）与异步选词提交（Commit）
/// 硬约束（ARCHITECT-V4 V1/V4）：
///   1. AppContainer 进程内只连不拉（绝不调用 CreateProcess）；
///   2. 普通权限宿主进程激活时自动确保 `taishen_server.exe` 已启动；
///   3. 断连快速退避（不阻塞 UI 线程），单次发送与往返严格受短超时保护；
///   4. 握手严格校验协议版本号（kRenderIpcVersion），不匹配拒绝会话并透传。

#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

#include "render_ipc.h"

namespace taishen {

class CRenderIpcClient {
public:
    CRenderIpcClient();
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

    /// V4 D3：同步发送按键并取回候选列表（总往返预算 <= budgetMs，默认 50ms；超时立即断管并透传）
    bool SendKeyInput(const ipc::KeyInputRequest& req,
                      ipc::CandidateListResponse& outResp,
                      DWORD budgetMs = 50);

    /// V4 D3：异步发送已选词提交指令（无返回，供 server 落盘学习与上下文更新）
    bool SendCommit(const ipc::CommitRequest& req);

    /// 同步健康检查（发 ping 并校验 V2 pong，供冒烟测试与诊断使用）
    bool Ping();

    /// 是否正处于硬失败退避窗口内（超时/断管/版本不匹配后短期快速透传）
    bool IsInHardBackoff() const;

    /// 最近一次握手/Ping 收到的服务端元信息
    const ipc::PongInfo& LastPongInfo() const { return m_lastPong; }

    /// 主动断开管道
    void Disconnect();

private:
    bool EnsureConnected(bool ignoreBackoff = false, DWORD budgetMs = 45);
    bool PerformHandshake(HANDLE hPipe, DWORD timeoutMs, ipc::PongInfo& outPong);
    bool SendPayload(const std::string& payload);
    /// 发送失败后的自愈：清句柄 → 进入硬失败退避 → 普通宿主进程尝试重新拉起 server
    void HandleSendFailure();

    HANDLE m_pipe = INVALID_HANDLE_VALUE;
    ULONGLONG m_lastFailTick = 0;
    ULONGLONG m_lastSpawnTick = 0;
    /// 硬失败退避截止时刻（超时/写失败/版本错后短期不再尝试，避免每个按键都吃一次超时）
    ULONGLONG m_hardFailUntil = 0;
    /// 连续拉起 server 的次数与熔断截止时刻（防止「崩溃-重拉」热循环）
    unsigned m_spawnStreak = 0;
    ULONGLONG m_circuitOpenUntil = 0;
    /// 首次 EnsureServerRunning 传入的安装目录，供失败后重新拉起使用
    std::wstring m_dllDir;
    ipc::PongInfo m_lastPong;
};

} // namespace taishen
