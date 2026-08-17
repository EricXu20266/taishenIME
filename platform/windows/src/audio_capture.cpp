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
/// 设计要点（SPEC 10.7 泰深经验）：
///   - COM 初始化在采集线程内做（避免宿主进程线程模型冲突）
///   - Stop() 必须可靠释放 COM 接口（防残留占用麦克风）
///   - 回调只做 VAD 投喂等轻量操作，阻塞工作（转写）在外部线程

#include "audio_capture.h"

#include <audioclient.h>
#include <mmdeviceapi.h>
#include <functiondiscoverykeys_devpkey.h>
#include <avrt.h>

#include <stdexcept>
#include <vector>

#include "debug_log.h"

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
    if (m_capturing) {
        Stop();
    }
    m_cb = std::move(cb);
    m_capturing = true;

    // 采集线程：COM 初始化 + 循环 GetBuffer（避免跨线程 COM 模型冲突）
    m_thread = CreateThread(nullptr, 0, CaptureThreadProc, this, 0, nullptr);
    if (!m_thread) {
        m_capturing = false;
        m_lastError = L"CreateThread 失败";
        throw std::runtime_error("CreateThread failed");
    }
    // 提升到 MMCSS（多媒体调度，防音频卡顿）
    // （线程内用 AvSetMmThreadCharacteristics，此处不阻塞等待）
}

void AudioCapture::Stop()
{
    m_capturing = false;
    if (m_thread) {
        WaitForSingleObject(m_thread, 2000);
        CloseHandle(m_thread);
        m_thread = nullptr;
    }
    // COM 接口由线程内 RAII 释放（CaptureLoop 退出时）
    m_audioClient = nullptr;
    m_captureClient = nullptr;
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
    IMMDeviceEnumerator* enumerator = nullptr;
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr,
                          CLSCTX_ALL, IID_PPV_ARGS(&enumerator));
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: enumerator failed"); return; }

    // 2. 默认捕获设备（eCapture/eConsole = 默认麦克风）
    IMMDevice* device = nullptr;
    hr = enumerator->GetDefaultAudioEndpoint(eCapture, eConsole, &device);
    enumerator->Release();
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: GetDefaultAudioEndpoint failed"); return; }

    // 3. 激活 IAudioClient
    IAudioClient* audioClient = nullptr;
    hr = device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                          reinterpret_cast<void**>(&audioClient));
    device->Release();
    if (FAILED(hr)) { m_lastError = HrToString(hr); DebugLog("AudioCapture: Activate IAudioClient failed"); return; }

    // 4. 初始化（共享模式，1s 缓冲，16kHz mono 16bit）
    WAVEFORMATEX fmt = MakeFormat();
    hr = audioClient->Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1000000, 0, &fmt, nullptr);
    if (FAILED(hr)) {
        // 设备可能不支持 16kHz 共享格式 → 尝试让系统混音（只读格式初始化失败即放弃）
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: Initialize failed");
        audioClient->Release();
        return;
    }

    // 5. 获取 IAudioCaptureClient
    IAudioCaptureClient* captureClient = nullptr;
    hr = audioClient->GetService(IID_PPV_ARGS(&captureClient));
    if (FAILED(hr)) {
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: GetService IAudioCaptureClient failed");
        audioClient->Release();
        return;
    }

    m_audioClient = audioClient;
    m_captureClient = captureClient;

    // 6. 开始
    hr = audioClient->Start();
    if (FAILED(hr)) {
        m_lastError = HrToString(hr);
        DebugLog("AudioCapture: Start failed");
        return; // 线程退出，接口由析构/Stop 释放
    }

    DebugLog("AudioCapture: started 16kHz mono 16bit");

    // 7. 采集循环
    UINT32 packetLength = 0;
    while (m_capturing) {
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

    // 8. 停止
    audioClient->Stop();
    DebugLog("AudioCapture: stopped");
}

} // namespace taishen
