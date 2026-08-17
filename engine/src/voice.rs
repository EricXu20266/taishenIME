/// 语音输入模块（V0.5）— VAD 语音活动检测 + Whisper HTTP 转写
///
/// 对应 SPEC: docs/modules/voice-input/SPEC.md
/// 覆盖 DEV-TRACKER: 0.5.2 VAD / 0.5.4 HTTP 转写
///
/// 设计（与 SPEC 4.1 对齐但简化 FFI 内存管理）：
/// - VAD 只做检测：process_frame 返回状态码（0=silence 1=speech 2=pending_transcribe），
///   不输出音频段——音频缓冲累积与 WAV 编码由 C++ 平台层负责（WASAPI 本来就是 C++ 的活）。
/// - 转写：C++ 拼好 WAV 字节 → engine_voice_transcribe → 内部 HTTP POST whisper-server。
/// - 泰深检测：engine_detect_taishen 检查 ~/.taishen/bin/whisper-server.exe 与端口连通。
///
/// 参考实现: 泰深 src/renderer/utils/vad.ts（VAD 逻辑移植）、
///          electron/ipc/whisper.ts（HTTP 转写协议）

/// VAD 配置（默认值取自泰深四轮迭代收敛值，SPEC 10.5）
#[derive(Clone, Copy, Debug)]
pub struct VadConfig {
    /// 采样率（whisper 标准 16000，不要改）
    pub sample_rate: u32,
    /// 静音判定能量阈值（RMS），默认 0.02
    pub energy_threshold: f32,
    /// 静音超时（秒）：语音结束后等待多久触发转写，默认 1.8
    pub silence_timeout_sec: f32,
    /// 最短语音段长度（秒）：低于此的片段视为噪音丢弃，默认 0.8
    pub min_speech_duration_sec: f32,
    /// 最长语音段长度（秒）：超过强制触发转写，默认 15
    pub max_speech_duration_sec: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16000,
            energy_threshold: 0.02,
            silence_timeout_sec: 1.8,
            min_speech_duration_sec: 0.8,
            max_speech_duration_sec: 15.0,
        }
    }
}

/// VAD 状态
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VadState {
    Silence,
    Speech,
    PendingTranscribe,
}

/// VAD 处理结果
pub struct VadResult {
    pub state: VadState,
    /// 是否需要触发转写（pending 触发或 flush 剩余语音）
    pub should_transcribe: bool,
}

/// 语音活动检测器（移植泰深 TS 版 VoiceActivityDetector）
pub struct VoiceActivityDetector {
    config: VadConfig,
    state: VadState,
    /// 静音持续时间（采样数）
    silence_samples: usize,
    /// 当前语音段持续时间（采样数）
    speech_samples: usize,
}

impl VoiceActivityDetector {
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            state: VadState::Silence,
            silence_samples: 0,
            speech_samples: 0,
        }
    }

    /// 处理一帧音频（16kHz f32），返回状态与是否触发转写
    /// 状态转移（对齐 SPEC 3.2 / vad.ts）：
    ///   SILENCE --energy>threshold--> SPEECH
    ///   SPEECH  --静音持续 silence_timeout_sec--> PendingTranscribe（触发）
    ///   SPEECH  --段长超 max_speech_duration_sec--> PendingTranscribe（强制触发）
    pub fn process_frame(&mut self, frame: &[f32]) -> VadResult {
        if frame.is_empty() {
            return VadResult {
                state: self.state,
                should_transcribe: false,
            };
        }
        let rms = compute_rms(frame);
        let is_speech = rms > self.config.energy_threshold;
        let frame_len = frame.len();

        match self.state {
            VadState::Silence => {
                if is_speech {
                    self.state = VadState::Speech;
                    self.speech_samples = frame_len;
                    self.silence_samples = 0;
                }
            }
            VadState::Speech => {
                if is_speech {
                    self.speech_samples += frame_len;
                    self.silence_samples = 0;
                    // 超过最大语音段长度 → 强制触发转写
                    let max_samples = (self.config.max_speech_duration_sec
                        * self.config.sample_rate as f32)
                        as usize;
                    if self.speech_samples >= max_samples {
                        self.state = VadState::Silence;
                        return VadResult {
                            state: VadState::PendingTranscribe,
                            should_transcribe: true,
                        };
                    }
                } else {
                    self.silence_samples += frame_len;
                    // 静音足够久 → 检查最短段长
                    let silence_threshold =
                        (self.config.silence_timeout_sec * self.config.sample_rate as f32) as usize;
                    if self.silence_samples >= silence_threshold {
                        let min_samples = (self.config.min_speech_duration_sec
                            * self.config.sample_rate as f32)
                            as usize;
                        if self.speech_samples >= min_samples {
                            self.state = VadState::Silence;
                            return VadResult {
                                state: VadState::PendingTranscribe,
                                should_transcribe: true,
                            };
                        }
                        // 太短，丢弃
                        self.state = VadState::Silence;
                        self.speech_samples = 0;
                        self.silence_samples = 0;
                    }
                }
            }
            VadState::PendingTranscribe => {
                // pending 不应收到新帧（C++ 侧触发转写后才继续喂），保守回 silence
                self.state = VadState::Silence;
                self.speech_samples = 0;
                self.silence_samples = 0;
            }
        }

        VadResult {
            state: self.state,
            should_transcribe: false,
        }
    }

    /// 强制结束当前段（手动停止录音时处理剩余语音）
    /// 返回 true 表示有剩余段需要转写（段长 >= min_speech_duration）
    pub fn flush_remaining(&mut self) -> bool {
        let had_speech = self.state == VadState::Speech;
        let min_samples =
            (self.config.min_speech_duration_sec * self.config.sample_rate as f32) as usize;
        let enough = self.speech_samples >= min_samples;
        self.state = VadState::Silence;
        self.speech_samples = 0;
        self.silence_samples = 0;
        had_speech && enough
    }

    /// 当前状态
    pub fn state(&self) -> VadState {
        self.state
    }
}

/// 计算 RMS 能量（窗长任意，泰深 vad.ts 同款）
pub fn compute_rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for &s in frame {
        sum += (s as f64) * (s as f64);
    }
    (sum / frame.len() as f64).sqrt() as f32
}

// ── HTTP 转写（0.5.4）──

/// 转写错误
#[derive(Debug)]
pub enum VoiceError {
    /// 网络错误（连接失败/请求失败）
    Network(String),
    /// 服务端返回非 200
    Http(u16, String),
    /// 超时
    Timeout,
    /// 空结果
    Empty,
}

impl std::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoiceError::Network(e) => write!(f, "网络错误: {e}"),
            VoiceError::Http(code, body) => write!(f, "HTTP {code}: {body}"),
            VoiceError::Timeout => write!(f, "转写超时"),
            VoiceError::Empty => write!(f, "识别结果为空"),
        }
    }
}

/// 转写 WAV 音频（阻塞调用）。
///
/// 协议与泰深 whisper-server 一致（SPEC 4.3）：
/// POST {server_url}/inference  multipart/form-data
///   字段: file=WAV 16kHz mono 16bit PCM, language="zh"|"auto"
/// 返回 200 {"text": "..."}。客户端总超时 120s（泰深同款）。
pub fn transcribe(wav: &[u8], server_url: &str, language: &str) -> Result<String, VoiceError> {
    let url = format!("{}/inference", server_url.trim_end_matches('/'));
    let lang = if language.is_empty() { "zh" } else { language };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| VoiceError::Network(e.to_string()))?;

    let form = reqwest::blocking::multipart::Form::new()
        .part(
            "file",
            reqwest::blocking::multipart::Part::bytes(wav.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| VoiceError::Network(e.to_string()))?,
        )
        .text("language", lang.to_string());

    let resp = client.post(&url).multipart(form).send().map_err(|e| {
        if e.is_timeout() {
            VoiceError::Timeout
        } else {
            VoiceError::Network(e.to_string())
        }
    })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(VoiceError::Http(status.as_u16(), body));
    }

    let text: serde_json::Value = resp
        .json()
        .map_err(|e| VoiceError::Network(e.to_string()))?;
    let text = text
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(VoiceError::Empty);
    }
    Ok(text)
}

// ── 语音会话状态（FFI 层共享）──

/// 语音模式
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VoiceMode {
    /// 空闲
    Idle,
    /// 录音中
    Listening,
    /// 转写中
    Transcribing,
    /// 错误
    Error,
}

/// 语音会话（全局单例，ffi.rs 持有 Mutex）
pub struct VoiceSession {
    mode: VoiceMode,
    /// whisper-server URL（如 http://127.0.0.1:9080）
    server_url: String,
    /// 识别语言（zh / auto）
    language: String,
    vad: VoiceActivityDetector,
}

impl VoiceSession {
    /// 创建会话。server_url 为空表示自管路径待定（start 时再确认）。
    pub fn new(server_url: &str, language: &str) -> Self {
        Self {
            mode: VoiceMode::Idle,
            server_url: server_url.to_string(),
            language: if language.is_empty() {
                "zh".to_string()
            } else {
                language.to_string()
            },
            vad: VoiceActivityDetector::new(VadConfig::default()),
        }
    }

    pub fn mode(&self) -> VoiceMode {
        self.mode
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// 开始录音
    pub fn start(&mut self) {
        self.mode = VoiceMode::Listening;
        self.vad = VoiceActivityDetector::new(VadConfig::default());
        crate::log::info(&format!(
            "voice start url={} lang={}",
            self.server_url, self.language
        ));
    }

    /// 停止录音（VAD flush 剩余段由 C++ 侧按 vad_process 返回码处理）
    pub fn stop(&mut self) {
        self.mode = VoiceMode::Idle;
        self.vad = VoiceActivityDetector::new(VadConfig::default());
        crate::log::info("voice stop");
    }

    /// 处理一帧 PCM（f32）。返回 VAD 状态码：0=silence 1=speech 2=pending_transcribe
    pub fn process_frame(&mut self, samples: &[f32]) -> i32 {
        if self.mode != VoiceMode::Listening {
            return 0;
        }
        let result = self.vad.process_frame(samples);
        if result.should_transcribe {
            self.mode = VoiceMode::Transcribing;
            return 2;
        }
        match result.state {
            VadState::Silence => 0,
            VadState::Speech => 1,
            VadState::PendingTranscribe => 2,
        }
    }

    /// 转写完成后回到 listening（可继续说话）
    pub fn on_transcribe_done(&mut self) {
        if self.mode == VoiceMode::Transcribing {
            self.mode = VoiceMode::Listening;
        }
    }

    /// 转写出错
    pub fn on_transcribe_error(&mut self) {
        self.mode = VoiceMode::Error;
    }

    /// VAD flush 剩余语音（engine_voice_flush 用）。
    /// 返回 true = 有剩余段需要转写（平台层应转写其累积的音频缓冲）。
    pub fn flush_remaining_segment(&mut self) -> bool {
        let had = self.vad.flush_remaining();
        if had {
            crate::log::info("voice flush: remaining segment");
        }
        had
    }
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VadConfig {
        VadConfig {
            sample_rate: 16000,
            energy_threshold: 0.02,
            silence_timeout_sec: 1.8,
            min_speech_duration_sec: 0.8,
            max_speech_duration_sec: 15.0,
        }
    }

    /// 构造一帧全为恒定幅值的音频
    fn tone_frame(amp: f32, len: usize) -> Vec<f32> {
        vec![amp; len]
    }

    #[test]
    fn rms_computes_correctly() {
        // 全 0 → 0
        assert_eq!(compute_rms(&vec![0.0; 512]), 0.0);
        // 恒定 0.1 → RMS = 0.1
        let rms = compute_rms(&vec![0.1; 512]);
        assert!((rms - 0.1).abs() < 1e-6, "rms={rms}");
        // 混合正负 → 平方均值开方
        let frame = vec![1.0, -1.0];
        let rms = compute_rms(&frame);
        assert!((rms - 1.0).abs() < 1e-6);
    }

    #[test]
    fn silence_stays_silence() {
        let mut vad = VoiceActivityDetector::new(config());
        for _ in 0..100 {
            let r = vad.process_frame(&tone_frame(0.001, 512));
            assert_eq!(r.state, VadState::Silence);
            assert!(!r.should_transcribe);
        }
    }

    #[test]
    fn speech_enters_speech_state() {
        let mut vad = VoiceActivityDetector::new(config());
        let r = vad.process_frame(&tone_frame(0.1, 512));
        assert_eq!(r.state, VadState::Speech);
    }

    #[test]
    fn short_noise_is_discarded() {
        // 短促噪音（< 0.8s）→ 回到 silence，不触发转写
        let mut vad = VoiceActivityDetector::new(config());
        vad.process_frame(&tone_frame(0.1, 512)); // speech 开始
        // 静音 1.9s（> 1.8s 超时，但段长仅 512 样本 = 32ms < 0.8s）
        let silence_frames = (config().silence_timeout_sec * 16000.0 / 512.0).ceil() as usize + 2;
        let mut triggered = false;
        for _ in 0..silence_frames {
            let r = vad.process_frame(&tone_frame(0.001, 512));
            if r.should_transcribe {
                triggered = true;
            }
        }
        assert!(!triggered, "短噪音不应触发转写");
        assert_eq!(vad.state(), VadState::Silence);
    }

    #[test]
    fn silence_timeout_triggers_transcribe() {
        let mut vad = VoiceActivityDetector::new(config());
        // 说话 1s
        let speech_frames = (1.0 * 16000.0 / 512.0) as usize;
        for _ in 0..speech_frames {
            vad.process_frame(&tone_frame(0.1, 512));
        }
        assert_eq!(vad.state(), VadState::Speech);
        // 静音 1.9s → 触发
        let silence_frames = (config().silence_timeout_sec * 16000.0 / 512.0).ceil() as usize + 2;
        let mut triggered = false;
        for _ in 0..silence_frames {
            let r = vad.process_frame(&tone_frame(0.001, 512));
            if r.should_transcribe {
                triggered = true;
                break;
            }
        }
        assert!(triggered, "静音超时应触发转写");
        assert_eq!(vad.state(), VadState::Silence);
    }

    #[test]
    fn max_speech_duration_forces_transcribe() {
        let mut vad = VoiceActivityDetector::new(config());
        // 连续说话超过 max_speech_duration_sec → 强制触发
        let max_frames = (config().max_speech_duration_sec * 16000.0 / 512.0).ceil() as usize + 1;
        let mut triggered = false;
        for _ in 0..max_frames {
            let r = vad.process_frame(&tone_frame(0.1, 512));
            if r.should_transcribe {
                triggered = true;
                break;
            }
        }
        assert!(triggered, "超长语音应强制触发转写");
    }

    #[test]
    fn flush_remaining_returns_segment() {
        let mut vad = VoiceActivityDetector::new(config());
        // 没说话 → flush 无剩余
        assert!(!vad.flush_remaining());
        // 说话 1s → flush 有剩余
        let speech_frames = (1.0 * 16000.0 / 512.0) as usize;
        for _ in 0..speech_frames {
            vad.process_frame(&tone_frame(0.1, 512));
        }
        assert!(vad.flush_remaining());
        // flush 后回 silence
        assert_eq!(vad.state(), VadState::Silence);
    }

    #[test]
    fn empty_frame_is_noop() {
        let mut vad = VoiceActivityDetector::new(config());
        let r = vad.process_frame(&[]);
        assert_eq!(r.state, VadState::Silence);
        assert!(!r.should_transcribe);
    }

    /// 转写：mock HTTP server 返回文本（SPEC 8.1）
    /// 用 std::net::TcpListener 起本地 server 模拟 whisper-server /inference
    #[test]
    fn transcribe_mock_server_returns_text() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            // reqwest 发完 multipart 后保持连接等响应（不发 EOF）→ 读超时后回复
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(800)));
            let mut buf = [0u8; 8192];
            let mut total = 0usize;
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Err(_) => break, // 读超时 → 请求体已收完
                    Ok(n) => {
                        total += n;
                        if total > 100_000 {
                            break;
                        }
                    }
                }
            }
            // 响应 JSON（UTF-8 转义：你好世界）
            let body = b"{\"text\": \"\xE4\xBD\xA0\xE5\xA5\xBD\xE4\xB8\x96\xE7\x95\x8C\"}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.write_all(body);
        });

        // 构造最小 WAV 数据（伪样本，server 只回固定文本）
        let wav = vec![0u8; 1600];
        let url = format!("http://127.0.0.1:{port}");
        let text = transcribe(&wav, &url, "zh").expect("transcribe ok");
        assert_eq!(text, "你好世界");
        server.join().expect("server thread");
    }

    /// 转写：server 返回错误状态码 → Http 错误
    #[test]
    fn transcribe_mock_server_http_error() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
        });

        let url = format!("http://127.0.0.1:{port}");
        let err = transcribe(&[0u8; 1600], &url, "zh").expect_err("should fail");
        match err {
            VoiceError::Http(500, _) => {}
            other => panic!("期望 Http(500)，got {other:?}"),
        }
        server.join().expect("server thread");
    }
}
