/// 语音管理器 — 实现
///
/// 双路径架构（SPEC 2.1）：
///   优先路径：泰深 whisper-server 在跑 → 直连（零资源占用）
///   自管路径：无泰深 → 需引擎+模型（v0.5.5 下载管理，本期先直连；
///             自管 server 启动在 0.5.6，此处留扩展点）

#include "voice_manager.h"

#include <algorithm>
#include <stdexcept>
#include <shlobj.h>

#include "config_reader.h"
#include "debug_log.h"
#include "engine_bridge.h"

namespace taishen {

namespace {

/// 本地 APPDATA 路径
std::wstring LocalAppData()
{
    wchar_t buf[MAX_PATH] = {0};
    if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_LOCAL_APPDATA, nullptr, 0, buf))) {
        return std::wstring(buf);
    }
    return L"C:\\Users\\Public\\AppData\\Local";
}

/// 检测 whisper-server.exe 是否存在
bool FileExists(const std::wstring& path)
{
    const DWORD attr = GetFileAttributesW(path.c_str());
    return attr != INVALID_FILE_ATTRIBUTES && !(attr & FILE_ATTRIBUTE_DIRECTORY);
}

} // namespace

VoiceManager::~VoiceManager()
{
    Stop();
}

VoiceManager& VoiceManager::Instance()
{
    static VoiceManager inst;
    return inst;
}

void VoiceManager::SetCallbacks(ResultCallback onResult, StateCallback onState)
{
    m_onResult = std::move(onResult);
    m_onState = std::move(onState);
}

void VoiceManager::LoadConfig()
{
    // 配置来自 config.ini voice_* 键（DLL 目录）
    wchar_t dllPath[MAX_PATH] = {0};
    GetModuleFileNameW(nullptr, dllPath, MAX_PATH);
    std::wstring dllDir(dllPath);
    const size_t slash = dllDir.find_last_of(L"\\/");
    if (slash != std::wstring::npos) {
        dllDir = dllDir.substr(0, slash + 1);
    }
    const ImeConfig cfg = taishen::LoadConfig(dllDir);
    m_port = cfg.voice.server_port > 0 ? cfg.voice.server_port : 9080;
    m_language = cfg.voice.language.empty() ? L"zh" : cfg.voice.language;
}

TaishenDetection VoiceManager::DetectTaishen(int port)
{
    TaishenDetection d;
    // ① ~/.taishen/bin/whisper-server.exe 存在？
    wchar_t userProfile[MAX_PATH] = {0};
    if (SUCCEEDED(SHGetFolderPathW(nullptr, CSIDL_PROFILE, nullptr, 0, userProfile))) {
        d.bin_path = std::wstring(userProfile) + L"\\.taishen\\bin";
        d.installed = FileExists(d.bin_path + L"\\whisper-server.exe");
    }
    // ② 端口连通（engine_detect_taishen 内部 TCP probe）
    const std::wstring url = L"http://127.0.0.1:" + std::to_wstring(port);
    d.server_url = url;
    // 转 UTF-8 传 FFI
    const std::string binUtf8 = [&]() {
        if (d.bin_path.empty()) return std::string();
        const int len = WideCharToMultiByte(CP_UTF8, 0, d.bin_path.c_str(),
                                            static_cast<int>(d.bin_path.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len <= 0) return std::string();
        std::string s(static_cast<size_t>(len), '\0');
        WideCharToMultiByte(CP_UTF8, 0, d.bin_path.c_str(),
                            static_cast<int>(d.bin_path.size()), &s[0], len, nullptr, nullptr);
        return s;
    }();
    const std::string urlUtf8 = [&]() {
        const int len = WideCharToMultiByte(CP_UTF8, 0, url.c_str(),
                                            static_cast<int>(url.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len <= 0) return std::string();
        std::string s(static_cast<size_t>(len), '\0');
        WideCharToMultiByte(CP_UTF8, 0, url.c_str(),
                            static_cast<int>(url.size()), &s[0], len, nullptr, nullptr);
        return s;
    }();

    const int code = engine_detect_taishen(
        binUtf8.empty() ? nullptr : binUtf8.c_str(), port);
    d.server_running = (code == 1);
    // ② 返回 2 = exe 存在但未启动；installed 仍为 true（可尝试自启）
    return d;
}

std::wstring VoiceManager::Start()
{
    LoadConfig();
    if (m_listening.load()) {
        return L"";
    }

    // ① 泰深检测：server 在跑 → 直连（优先路径）
    const TaishenDetection det = DetectTaishen(m_port);
    if (det.server_running) {
        m_serverUrl = det.server_url;
        DebugLog("VoiceManager: 直连泰深 whisper-server " + std::to_string(m_port));
    } else {
        // 自管路径：本期要求引擎+模型已就绪（0.5.5/0.5.6 后续实现自动启动）
        m_serverUrl = L"http://127.0.0.1:" + std::to_wstring(m_port);
        DebugLog("VoiceManager: 自管路径（0.5.6 实现 server 自启，本期需手动启动 whisper-server）");
    }

    // ② 引擎侧：voice_start（传 server_url + language）
    const std::string urlUtf8 = [&]() {
        const int len = WideCharToMultiByte(CP_UTF8, 0, m_serverUrl.c_str(),
                                            static_cast<int>(m_serverUrl.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len <= 0) return std::string();
        std::string s(static_cast<size_t>(len), '\0');
        WideCharToMultiByte(CP_UTF8, 0, m_serverUrl.c_str(),
                            static_cast<int>(m_serverUrl.size()), &s[0], len, nullptr, nullptr);
        return s;
    }();
    const std::string langUtf8 = [&]() {
        const int len = WideCharToMultiByte(CP_UTF8, 0, m_language.c_str(),
                                            static_cast<int>(m_language.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len <= 0) return std::string("zh");
        std::string s(static_cast<size_t>(len), '\0');
        WideCharToMultiByte(CP_UTF8, 0, m_language.c_str(),
                            static_cast<int>(m_language.size()), &s[0], len, nullptr, nullptr);
        return s;
    }();
    engine_voice_start(urlUtf8.c_str(), langUtf8.c_str());

    // ③ 转写线程
    {
        std::lock_guard<std::mutex> lk(m_queueMutex);
        if (!m_transcribeRunning) {
            m_transcribeRunning = true;
            m_transcribeThread = std::thread([this]() { TranscribeLoop(); });
        }
    }

    // ④ 采集（回调 = VAD 喂帧 + 段累积）
    m_listening = true;
    m_uiState = VoiceUiState::Listening;
    if (m_onState) m_onState(m_uiState);
    try {
        m_capture.Start([this](const float* samples, int count) {
            OnAudio(samples, count);
        });
    } catch (const std::exception& e) {
        m_listening = false;
        m_uiState = VoiceUiState::Error;
        if (m_onState) m_onState(m_uiState);
        return L"麦克风启动失败: " + std::wstring(e.what(), e.what() + strlen(e.what()));
    }
    return L"";
}

void VoiceManager::Stop()
{
    if (!m_listening.load()) {
        return;
    }
    m_listening = false;
    m_capture.Stop();

    // VAD flush：剩余语音段（用户说话中途停止）
    const int flush = engine_voice_flush();
    {
        std::lock_guard<std::mutex> lk(m_segMutex);
        if (flush == 1 && !m_segment.empty()) {
            m_queue.push_back(std::move(m_segment));
            m_segment.clear();
            m_queueCv.notify_one();
        }
    }

    m_uiState = VoiceUiState::Idle;
    if (m_onState) m_onState(m_uiState);
    engine_voice_stop();
}

void VoiceManager::OnAudio(const float* samples, int count)
{
    if (!m_listening.load()) {
        return;
    }
    // ① VAD 状态码（引擎侧状态机，阈值/静音超时参数取 config）
    const int code = engine_vad_process(samples, count);
    // ② 段缓冲管理
    std::lock_guard<std::mutex> lk(m_segMutex);
    switch (code) {
        case 0: // silence
            if (m_inSpeech) {
                // 静音中（VAD 在等静音超时）——继续累积，VAD 触发 pending 时转写
                m_segment.insert(m_segment.end(), samples, samples + count);
            }
            break;
        case 1: // speech
            if (!m_inSpeech) {
                m_inSpeech = true;
                m_segment.clear();
            }
            m_segment.insert(m_segment.end(), samples, samples + count);
            break;
        case 2: // pending_transcribe：VAD 判定一段语音结束 → 转写
            m_segment.insert(m_segment.end(), samples, samples + count);
            if (!m_segment.empty()) {
                m_queue.push_back(std::move(m_segment));
                m_segment.clear();
                m_queueCv.notify_one();
            }
            m_inSpeech = false;
            break;
        default:
            break;
    }
}

void VoiceManager::TranscribeLoop()
{
    while (true) {
        std::vector<float> segment;
        {
            std::unique_lock<std::mutex> lk(m_queueMutex);
            m_queueCv.wait(lk, [this]() { return !m_queue.empty() || !m_transcribeRunning; });
            if (!m_transcribeRunning && m_queue.empty()) {
                return; // 线程退出
            }
            if (m_queue.empty()) {
                continue;
            }
            segment = std::move(m_queue.front());
            m_queue.erase(m_queue.begin());
        }

        // 转写中状态
        m_uiState = VoiceUiState::Transcribing;
        if (m_onState) m_onState(m_uiState);

        // WAV 编码（16kHz mono 16bit）
        const std::vector<uint8_t> wav = EncodeWav(segment, 16000);

        // 阻塞 HTTP 转写
        char result[4096] = {0};
        const int rc = engine_voice_transcribe(
            wav.data(), static_cast<int>(wav.size()), result, sizeof(result));

        if (rc == 0) {
            // 注入候选（引擎侧 engine_voice_result）
            engine_voice_result(result);
            if (m_onResult) {
                m_onResult(std::string(result));
            }
            m_uiState = VoiceUiState::Listening;
        } else {
            DebugLog("VoiceManager: transcribe failed rc=" + std::to_string(rc));
            m_uiState = VoiceUiState::Listening;
        }
        if (m_onState) m_onState(m_uiState);
    }
}

std::vector<uint8_t> VoiceManager::EncodeWav(const std::vector<float>& samples, int sampleRate)
{
    // RIFF WAV 头（44 字节）+ PCM int16 数据
    const uint32_t dataSize = static_cast<uint32_t>(samples.size() * 2);
    std::vector<uint8_t> wav;
    wav.reserve(44 + dataSize);

    auto push = [&wav](const void* p, size_t n) {
        const auto* b = static_cast<const uint8_t*>(p);
        wav.insert(wav.end(), b, b + n);
    };

    // RIFF 头
    const uint32_t riffSize = 36 + dataSize;
    push("RIFF", 4);
    push(&riffSize, 4);
    push("WAVE", 4);
    push("fmt ", 4);
    const uint32_t fmtSize = 16;
    push(&fmtSize, 4);
    const uint16_t audioFmt = 1;        // PCM
    const uint16_t channels = 1;        // mono
    const uint32_t bytesPerSec = sampleRate * 2;
    const uint16_t blockAlign = 2;
    const uint16_t bitsPerSample = 16;
    push(&audioFmt, 2);
    push(&channels, 2);
    push(&sampleRate, 4);
    push(&bytesPerSec, 4);
    push(&blockAlign, 2);
    push(&bitsPerSample, 2);
    push("data", 4);
    push(&dataSize, 4);

    // PCM 数据（f32 → int16，clip）
    for (float s : samples) {
        s = std::clamp(s, -1.0f, 1.0f);
        int16_t v = static_cast<int16_t>(s * 32767.0f);
        push(&v, 2);
    }
    return wav;
}

} // namespace taishen
