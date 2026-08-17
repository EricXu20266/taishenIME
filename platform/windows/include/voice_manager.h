/// 语音管理器 — 声明
///
/// 对应 SPEC: docs/modules/voice-input/SPEC.md
/// 覆盖 DEV-TRACKER: 0.5.1/0.5.2/0.5.3/0.5.4/0.5.7
///
/// 职责（双路径架构的 C++ 侧实现）：
///   1. 泰深检测（优先路径）：~/.taishen/bin/whisper-server.exe + 端口连通
///   2. WASAPI 采集：AudioCapture 16kHz mono → f32 帧
///   3. VAD 状态机：喂 engine_vad_process，按状态码管理音频段缓冲
///   4. WAV 编码：f32 段 → int16 + RIFF 头（16kHz mono 16bit）
///   5. 转写：engine_voice_transcribe（阻塞 HTTP，独立转写线程）
///   6. 候选注入：engine_voice_result(text) → 候选窗显示
///
/// 线程模型：
///   - 采集线程：只做 VAD 喂帧 + 段缓冲累积（轻量，不阻塞）
///   - 转写线程：WAV 编码 + HTTP POST（阻塞 5-15s，不占采集线程）
///   - 结果经回调通知 UI 层（候选注入）

#pragma once

#include <atomic>
#include <condition_variable>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "audio_capture.h"

namespace taishen {

/// 泰深检测结果
struct TaishenDetection {
    bool installed = false;       // ~/.taishen/bin/whisper-server.exe 存在
    bool server_running = false;  // 127.0.0.1:port 可连通
    std::wstring bin_path;        // whisper-server.exe 所在目录
    std::wstring server_url;      // http://127.0.0.1:port
};

/// 语音状态（对外暴露，UI 按钮显示用）
enum class VoiceUiState {
    Idle,
    Listening,
    Transcribing,
    Error,
};

/// 语音管理器（进程级单例）
class VoiceManager
{
public:
    /// 结果回调：转写完成（text 已注入候选，UI 刷新）
    using ResultCallback = std::function<void(const std::string& text)>;
    /// 状态回调：UI 状态变化（按钮图标刷新）
    using StateCallback = std::function<void(VoiceUiState state)>;

    static VoiceManager& Instance();

    /// 启动语音（点 Mic）。返回 0=成功 / 错误消息。
    /// 自动执行：泰深检测 → 确定 server_url → 采集+VAD+转写链路。
    std::wstring Start();

    /// 停止语音（再点 Mic）。冲刷 VAD 剩余语音，等待转写完成。
    void Stop();

    /// 是否正在录音
    bool IsListening() const { return m_listening.load(); }

    /// 当前 UI 状态
    VoiceUiState UiState() const { return m_uiState.load(); }

    /// 当前 server URL（直连泰深或自管）
    std::wstring ServerUrl() const { return m_serverUrl; }

    /// 设置结果/状态回调（UI 层注入）
    void SetCallbacks(ResultCallback onResult, StateCallback onState);

    /// 泰深检测（0.5.3，独立可测）
    static TaishenDetection DetectTaishen(int port = 9080);

    /// WAV 编码（f32 → 16kHz mono 16bit RIFF WAV，测试/诊断用）
    static std::vector<uint8_t> EncodeWavForTest(const std::vector<float>& samples, int sampleRate)
    {
        return EncodeWav(samples, sampleRate);
    }

private:
    VoiceManager() = default;
    ~VoiceManager();

    /// 配置回调（config.ini 的 voice 段 → 参数）
    void LoadConfig();

    /// 音频回调（采集线程）
    void OnAudio(const float* samples, int count);

    /// 转写线程入口
    void TranscribeLoop();

    /// 把一段 f32 段编码为 WAV 字节
    static std::vector<uint8_t> EncodeWav(const std::vector<float>& samples, int sampleRate);

    AudioCapture m_capture;                 // WASAPI 采集器
    std::atomic<bool> m_listening{false};   // 录音中
    std::atomic<VoiceUiState> m_uiState{VoiceUiState::Idle};
    std::wstring m_serverUrl;               // whisper-server URL
    std::wstring m_language = L"zh";        // 识别语言
    int m_port = 9080;                      // server 端口

    // VAD 段缓冲（采集线程写，转写线程读，互斥保护）
    std::mutex m_segMutex;
    std::vector<float> m_segment;           // 当前累积段（f32）
    bool m_inSpeech = false;                // 是否在语音段中

    // 转写队列
    std::mutex m_queueMutex;
    std::condition_variable m_queueCv;
    std::vector<std::vector<float>> m_queue; // 待转写段（f32）
    std::thread m_transcribeThread;
    bool m_transcribeRunning = false;

    ResultCallback m_onResult;
    StateCallback m_onState;
};

} // namespace taishen
