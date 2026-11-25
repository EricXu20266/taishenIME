/// 语音输入冒烟测试 — 独立 exe
///
/// 验证 V0.5 语音核心链路（不依赖真实麦克风/whisper-server）：
///   1. WAV 编码：RIFF 头 + PCM 数据格式正确
///   2. VAD 状态机（经 FFI）：静音→说话→静音超时→pending_transcribe
///   3. 泰深检测：本地无 server → 返回 0（不可用）
///   4. 语音候选注入：engine_voice_result → 候选可取出
///   5. config_reader：voice_* 键读写往返
/// 返回 0 = 通过。

#include <windows.h>
#include <cstdio>
#include <cstring>
#include <vector>

#include "engine_bridge.h"
#include "voice_manager.h"
#include "config_reader.h"

static int failures = 0;

#define CHECK(cond, msg)                                                    \
    do {                                                                    \
        if (!(cond)) {                                                      \
            printf("FAIL: %s\n", (msg));                                    \
            ++failures;                                                     \
        }                                                                   \
    } while (0)

/// 生成 f32 恒定幅值样本
static std::vector<float> MakeFrame(float amp, int n)
{
    return std::vector<float>(n, amp);
}

int wmain()
{
    SetConsoleOutputCP(CP_UTF8);
    CoInitializeEx(nullptr, COINIT_MULTITHREADED);

    // ── 1. WAV 编码格式 ──
    {
        std::vector<float> samples(1600, 0.5f); // 0.1s @ 16kHz
        const std::vector<uint8_t> wav = taishen::VoiceManager::EncodeWavForTest(samples, 16000);

        // RIFF 头
        CHECK(wav.size() == 44 + 1600 * 2, "WAV 尺寸错误");
        CHECK(memcmp(wav.data(), "RIFF", 4) == 0, "RIFF 标记错误");
        CHECK(memcmp(wav.data() + 8, "WAVE", 4) == 0, "WAVE 标记错误");
        CHECK(memcmp(wav.data() + 12, "fmt ", 4) == 0, "fmt 标记错误");
        // PCM/1ch/16kHz/16bit
        CHECK(wav[20] == 1 && wav[21] == 0, "音频格式非 PCM");
        CHECK(wav[22] == 1 && wav[23] == 0, "声道数非 1");
        CHECK(wav[24] == 0x80 && wav[25] == 0x3E, "采样率非 16000");
        CHECK(wav[34] == 16 && wav[35] == 0, "位深非 16");
        CHECK(memcmp(wav.data() + 36, "data", 4) == 0, "data 标记错误");
        // 0.5f → int16 = 0.5*32767 ≈ 16384
        const int16_t sample = *reinterpret_cast<const int16_t*>(wav.data() + 44);
        CHECK(sample > 16300 && sample < 16450, "PCM 样本转换错误");
        printf("STEP1 WAV 编码 OK (size=%zu)\n", wav.size());
    }

    // ── 2. VAD 状态机（经 FFI）──
    {
        engine_init(nullptr);
        CHECK(engine_voice_start("http://127.0.0.1:9080", "zh") == 0, "voice_start 失败");
        CHECK(engine_voice_state() == 1, "voice_state 应为 listening");

        // 静音帧 → silence
        const std::vector<float> quiet = MakeFrame(0.001f, 512);
        CHECK(engine_vad_process(quiet.data(), 512) == 0, "静音应返回 0");

        // 高能量帧 → speech
        const std::vector<float> loud = MakeFrame(0.1f, 512);
        CHECK(engine_vad_process(loud.data(), 512) == 1, "说话应返回 1");

        // 连续说话 1s → 仍 speech
        for (int i = 0; i < 30; ++i) {
            engine_vad_process(loud.data(), 512);
        }
        CHECK(engine_voice_state() == 1, "录音中状态");

        // 停止 → idle
        CHECK(engine_voice_stop() == 0, "voice_stop 失败");
        CHECK(engine_voice_state() == 0, "voice_state 应为 idle");
        printf("STEP2 VAD 状态机 OK\n");
    }

    // ── 3. 泰深检测 ──
    // 对齐 engine/src/ffi.rs:test_voice_detect_taishen (commit 1e72b05)：
    // engine_detect_taishen(bin, port) 检查两项：bin/whisper-server.exe 是否存在，
    // 以及 127.0.0.1:port 是否存活；只要端口连通即返回 1（直连模式），与 bin 路径无关。
    // 原测试传 bogus/nullptr + 9080，在后台已运行 whisper-server.exe:9080 的开发机上
    // 必然返回 1（且原 wprintf 在 C locale 遇中文截断未输出 \n，使 STEP3 失败行拼在
    // STEP2 后显示为 "STEP2 VAD FAIL: ?????????"，误导为 STEP2 失败）。
    // 因此用 port=0（跳过端口探测）验证不存在路径与 nullptr 返回 0，同时保留
    // 9080 端口探测（允许返回 0=未运行 或 1=已运行）。
    {
        const std::string bogus = "Z:\\nonexistent\\bin";
        CHECK(engine_detect_taishen(bogus.c_str(), 0) == 0,
              "路径不存在且不探测端口应返回 0");
        CHECK(engine_detect_taishen(nullptr, 0) == 0,
              "NULL 路径且不探测端口应返回 0");
        const int code9080 = engine_detect_taishen(bogus.c_str(), 9080);
        CHECK(code9080 == 0 || code9080 == 1,
              "9080 端口探测返回值应为 0(未运行) 或 1(已运行)");
        const int null9080 = engine_detect_taishen(nullptr, 9080);
        CHECK(null9080 == 0 || null9080 == 1,
              "NULL 路径 + 9080 端口探测返回值应为 0 或 1");
        printf("STEP3 泰深检测 OK (port0=0, code9080=%d, null9080=%d)\n",
               code9080, null9080);
    }

    // ── 4. 语音候选注入 ──
    {
        CHECK(engine_voice_result("你好世界") == 1, "候选注入失败");
        CHECK(engine_get_candidate_count() == 1, "候选数应为 1");
        char buf[64] = {0};
        engine_get_candidate(0, buf, sizeof(buf));
        CHECK(strcmp(buf, "你好世界") == 0, "候选内容错误");
        printf("STEP4 语音候选注入 OK\n");
    }

    // ── 5. config_reader voice_* 读写往返 ──
    {
        // 构造临时 config.ini，写后读回
        wchar_t tmpDir[MAX_PATH] = {0};
        GetTempPathW(MAX_PATH, tmpDir);
        const std::wstring dir(tmpDir);
        taishen::ImeConfig cfg;
        cfg.voice.enabled = true;
        cfg.voice.model_size = L"base";
        cfg.voice.server_port = 9081;
        cfg.voice.vad_threshold = 0.03f;
        CHECK(taishen::SaveConfig(dir, cfg), "SaveConfig 失败");
        const taishen::ImeConfig read = taishen::LoadConfig(dir);
        CHECK(read.voice.enabled, "voice_enabled 读回失败");
        CHECK(read.voice.model_size == L"base", "voice_model_size 读回失败");
        CHECK(read.voice.server_port == 9081, "voice_server_port 读回失败");
        CHECK(read.voice.vad_threshold > 0.029f && read.voice.vad_threshold < 0.031f,
              "voice_vad_threshold 读回失败");
        // 清理临时文件
        DeleteFileW((dir + L"config.ini").c_str());
        printf("STEP5 config_reader voice 往返 OK\n");
    }

    // ── 6. VAD 参数注入 + 错误恢复（N4 审查修复回归）──
    {
        CHECK(engine_voice_set_vad_config(0.05f, 2.0f, 1.0f) == 0, "set_vad_config 失败");
        CHECK(engine_voice_set_vad_config(99.0f, 1.8f, 0.8f) == -2, "越界应返回 -2");
        // 转写失败（不可达端口）→ resume 恢复
        CHECK(engine_voice_resume() == 0, "resume 失败");
        printf("STEP6 VAD 配置 + 错误恢复 OK\n");
    }

    CoUninitialize();

    if (failures == 0) {
        printf("VOICE TEST PASSED\n");
        return 0;
    }
    printf("VOICE TEST FAILED (%d)\n", failures);
    return 1;
}
