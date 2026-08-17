/// 音频采集（WASAPI）— 实现
///
/// 对应 SPEC: docs/modules/voice-input/SPEC.md 5.2
/// COM 调用链（SPEC 5.2）：
///   CoInitializeEx(COINIT_MULTITHREADED)
///     → IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture, eConsole)
///     → IMMDevice::Activate(IID_IAudioClient, CLSCTX_ALL)
///     → IAudioClient::Initialize(shared, 0, 1000000, 0, &fmt, NULL)
///     → GetService(IID_IAudioCaptureClient)
///     → IAudioClient::Start()
///     → [loop] GetBuffer() → 回调 → ReleaseBuffer()
///     → IAudioClient::Stop()
///
/// 设计要点（SPEC 10.7 泰深经验 + 审查 S3/M3 修复）：
///   - COM 初始化在采集线程内做（避免宿主进程线程模型冲突）
///   - COM 接口用 ComPtr RAII 管理，采集线程退出自动 Release（S3 修复）
///   - m_capturing 原子标志跨线程（M3 修复）
///   - Stop() 等待线程退出后才置空回调（防悬空 std::function 调用）

#include "audio_capture.h"

#include <audioclient.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <avrt.h>

#include <stdexcept>
#include <vector>

#include "debug_log.h"

using Microsoft::WRL::ComPtr;

namespace taishen {

namespace {

/// WAVEFORMATEX：16kHz / mono / 16bit PCM
WAVEFORMATEX MakeFormat()
{
    WAVEFORMATEX fmt = {};
    fmt.wFormatTag = WAVE_FORMAT_PCM;
    fmt.nChannels = 1;
    fmt.nSamplesPerSec = 16000;
    fmt.wBitsPerSample = 16;
    fmt.nBlockAlign = 2;
    fmt.nAvgBytesPerSec = 16000 * 2;
    fmt.cbSize = 0;
    return fmt;
}

/// HRESULT → 可读错误文本
std::wstring HrToString(HRESULT hr)
{
    wchar_t buf[256] = {0};
    FormatMessageW(FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                   nullptr, static_cast<DWORD>(hr), 0, buf, 256, nullptr);
    if (buf[0] == L'\0') {
        swprintf_s(buf, L"HRESULT 0x%08X", static_cast<unsigned>(hr));
    }
    return std::wstring(buf);
}

} // namespace

AudioCapture::AudioCapture() = default;

AudioCapture::~AudioCapture()
{
    Stop();
}

void AudioCapture::Start(Callback cb)
{
    if (m_capturing.load()) {
        Stop();
    }
    m_cb = std::move(cb);
    m_capturing = true;

    // 采集线程：COM 初始化 + 循环 GetBuffer（避免跨线程 COM 模型冲突）
    m_thread = CreateThread(nullptr, 0, CaptureThreadProc, this, 0, nullptr);
    if (!m_thread) {
        m_capturing = false;
        m_cb = nullptr;
        m_lastError = L"CreateThread failed";
        throw std::runtime_error("CreateThread failed");
    }
}

void AudioCapture::Stop()
{
    m_capturing = false;
    if (m_thread) {
        // M4 修复：等待线程退出（设备挂起时最多等 3s，超时记录日志但继续——
        // 线程退出路径在 CaptureLoop 末尾统一释放 COM，不依赖 Stop 时序）
        const DWORD wait = WaitForSingleObject(m_thread, 3000);
        if (wait == WAIT_OBJECT_0) {
            CloseHandle(m_thread);
            m_thread = nullptr;
        } else {
            DebugLog("AudioCapture: thread did not exit within 3s (device hung?)");
        }
    }
    // 回调仅在采集线程已退出（或放弃等待）后置空——采集线程不再引用
    m_cb = nullptr;
}

DWORD WINAPI AudioCapture::CaptureThreadProc(LPVOID param)
{
    auto* self = static_cast<AudioCapture*>(param);
    // 提升线程优先级到 MMCSS Pro Audio（对齐 SPEC 10.10 帧间隔 32ms±2ms）
    DWORD taskIndex = 0;
    HANDLE mmcss = AvSetMmThreadCharacteristicsW(L"Audio", &taskIndex);

    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (SUCCEEDED(hr)) {
        self->CaptureLoop();
        CoUninitialize();
    } else {
        self->m_lastError = HrToString(hr);
        DebugLog("AudioCapture: CoInitializeEx failed");
    }

    if (mmcss) {
        AvRevertMmThreadCharacteristics(mmcss);
    }
    return 0;
}

void AudioCapture::CaptureLoop()
{
    HRESULT hr = S_OK;

    // 1. 设备枚举器
    ComPtr<IMMDeviceEnumerator> enumerator;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr,
                          CLSCTX_ALL, IID_PPV_ARGS(&enumerator));
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: enumerator failed"); return; }

    // 2. 默认捕获设备（eCapture/eConsole = 默认麦克风）
    ComPtr<IMMDevice> device;
    hr = enumerator->GetDefaultAudioEndpoint(eCapture, eConsole, &device);
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: GetDefaultAudioEndpoint failed"); return; }

    // 3. 激活 IAudioClient
    ComPtr<IAudioClient> audioClient;
    hr = device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                          reinterpret_cast<void**>(audioClient.GetAddressOf()));
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: Activate IAudioClient failed"); return; }

    // 4. 初始化（共享模式，1s 缓冲，16kHz mono 16bit）
    WAVEFORMATEX fmt = MakeFormat();
    hr = audioClient->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1000000, 0, &fmt, nullptr);
    if (FAILED(hr)) {
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: Initialize failed");
        return; // ComPtr RAII 释放
    }

    // 5. 获取 IAudioCaptureClient
    ComPtr<IAudioCaptureClient> captureClient;
    hr = audioClient->GetService(IID_PPV_ARGS(&captureClient));
    if (FAILED(hr)) {
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: GetService IAudioCaptureClient failed");
        return;
    }

    // 6. 开始
    hr = audioClient->Start();
    if (FAILED(hr)) {
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: Start failed");
        return;
    }

    DebugLog("AudioCapture: started 16kHz mono 16bit");

    // 7. 采集循环
    UINT32 packetLength = 0;
    while (m_capturing.load()) {
        hr = captureClient->GetNextPacketSize(&packetLength);
        if (FAILED(hr)) { break; }
        if (packetLength == 0) {
            Sleep(5);
            continue;
        }

        BYTE* data = nullptr;
        UINT32 frames = 0;
        DWORD flags = 0;
        hr = captureClient->GetBuffer(&data, &frames, &flags, nullptr, nullptr);
        if (FAILED(hr)) { break; }

        // 静音帧（AUDCLNT_BUFFERFLAGS_SILENT）→ 全 0
        const bool silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT) != 0;

        // 16bit PCM → f32 归一化，喂回调
        if (m_cb && frames > 0) {
            // 每帧 2 字节（16bit mono），样本数 = 帧数
            std::vector<float> f32(static_cast<size_t>(frames));
            if (silent) {
                std::fill(f32.begin(), f32.end(), 0.0f);
            } else {
                const int16_t* pcm = reinterpret_cast<const int16_t*>(data);
                for (UINT32 i = 0; i < frames; ++i) {
                    f32[i] = pcm[i] / 32768.0f;
                }
            }
            m_cb(f32.data(), static_cast<int>(frames));
        }

        captureClient->ReleaseBuffer(frames);
    }

    // 8. 停止（ComPtr RAII 在函数退出统一释放——S3 修复）
    audioClient->Stop();
    DebugLog("AudioCapture: stopped");
}

} // namespace taishen
