/// V4 D2 候选窗进程外渲染 IPC 客户端 — 声明
///
/// 供 TSF DLL 调用，通过命名管道将候选帧/坐标/隐藏指令发送给 `taishen_server.exe`。
/// 硬约束（ARCHITECT-V4 V1/V4）：
///   1. AppContainer 进程内只连不拉（绝不调用 CreateProcess）；
///   2. 普通权限宿主进程激活时自动确保 `taishen_server.exe` 已启动；
///   3. 断连快速退避（不阻塞 UI 线程），单次发送严格受短超时保护。

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

    /// 通知 server 热重载 config.ini（主题/字体/布局变化）
    bool SendReloadConfig();

    /// 同步健康检查（发 ping 并读取 pong，供冒烟测试与诊断使用）
    bool Ping();

    /// 主动断开管道
    void Disconnect();

private:
    bool EnsureConnected();
    bool SendPayload(const std::string& payload);

    HANDLE m_pipe = INVALID_HANDLE_VALUE;
    ULONGLONG m_lastFailTick = 0;
    ULONGLONG m_lastSpawnTick = 0;
};

} // namespace taishen
