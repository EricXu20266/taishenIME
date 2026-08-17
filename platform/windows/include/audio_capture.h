/// 音频采集（WASAPI）— 声明
///
/// 对应 SPEC: docs/modules/voice-input/SPEC.md 5.2
/// 覆盖 DEV-TRACKER: 0.5.1 音频采集模块
///
/// WASAPI 共享模式采集默认麦克风：16kHz / mono / 16bit PCM。
/// 内部工作线程循环 GetBuffer → 回调（f32 归一化样本）→ ReleaseBuffer。
/// 回调在采集线程执行，耗时操作（转写）必须移到外部，不得阻塞回调。

#pragma once

#include <windows.h>
#include <atomic>
#include <functional>
#include <memory>
#include <string>

#include <wrl/client.h>

namespace taishen {

/// WASAPI 音频采集器（16kHz mono 16bit PCM）
/// 回调签名：const float* samples（归一化 -1.0~1.0）, int count（样本数）
class AudioCapture
{
public:
    using Callback = std::function<void(const float* samples, int count)>;

    AudioCapture();
    ~AudioCapture();

    // 禁止拷贝
    AudioCapture(const AudioCapture&) = delete;
    AudioCapture& operator=(const AudioCapture&) = delete;

    /// 开始录音。失败抛 std::runtime_error（附 Win32 错误描述）。
    /// @param cb 音频回调（采集线程调用，必须快速返回）
    void Start(Callback cb);

    /// 停止录音并释放设备（可重复调用，幂等）。
    /// 等待采集线程退出并释放 COM 接口（S3 修复：防泄漏）。
    void Stop();

    /// 是否正在录音
    bool IsCapturing() const { return m_capturing.load(); }

    /// 当前采样率（16kHz，调试用）
    int SampleRate() const { return 16000; }

    /// 最近一次错误描述（调试日志用）
    std::wstring LastError() const { return m_lastError; }

private:
    /// 采集线程入口
    static DWORD WINAPI CaptureThreadProc(LPVOID param);
    /// 实际采集循环（COM 已初始化）
    void CaptureLoop();

    Callback m_cb;                     // 音频回调（采集线程使用，Stop join 后置空）
    std::atomic<bool> m_capturing{false};  // 采集中（M3 修复：原子跨线程）
    std::wstring m_lastError;          // 最近错误

    HANDLE m_thread = nullptr;         // 采集线程句柄
};

} // namespace taishen
