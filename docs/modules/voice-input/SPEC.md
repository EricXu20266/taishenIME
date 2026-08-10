# SPEC: 语音输入（V0.5）

> 对应 ARCHITECT.md Root #2「业务领域层」、#3「接口层」、#7「配置系统」、#8「呈现层」
> 关联 DEV-TRACKER: V0.5 语音输入 10 项子需求
> 参考实现: taishen-pisdk `electron/ipc/whisper.ts`、`electron/services/whisper-server-manager.ts`、`src/renderer/components/chat/VoiceInputButton.tsx`

---

## 一、需求

为泰深输入法增加本地语音输入能力。用户点击工具栏麦克风按钮开始说话，语音自动转为文字上屏。

核心约束：输入法是独立产品，不绑泰深工作台。有泰深时优先复用其 whisper-server（零额外资源），没有时自己下载引擎和模型。

### 1.1 功能清单

| 功能 | 说明 |
|------|------|
| 语音→文字 | 说话自动转写，VAD 自动分段，逐段上屏 |
| 工具栏切换 | 麦克风按钮一键开关，状态即时可见 |
| 优先路径 | 检测到泰深已安装 → 直连其 whisper-server |
| 自管路径 | 无泰深 → 输入法自行下载引擎+模型+启动服务 |
| 设置页 | 总开关、模型管理、引擎风格、VAD 参数全可视化 |

### 1.2 不做

- 云语音识别（纯本地，隐私优先）
- 语音合成 / TTS（那是泰深的活）
- 多语种同时识别（一期仅 zh）
- 语音命令（"换行""删除"等）

---

## 二、架构

### 2.1 双路径拓扑

```
工具栏 Mic 按钮
    │
    ▼
引擎 voice_start()
    │
    ├─ 泰深检测
    │   ├─ ~/.taishen/bin/whisper-server.exe 存在
    │   │   └─ 127.0.0.1:9080 连通？
    │   │       ├─ 是 → 【优先路径】直连
    │   │       └─ 否 → 尝试自启该 server → 仍不通则走自管
    │   │
    │   └─ 不存在 → 【自管路径】
    │
    ▼
WASAPI 录音 (16kHz mono PCM)
    │
    ▼
VAD 分段 (engine/voice.rs)
    │
    ▼
WAV 编码 → HTTP POST /inference
    │
    ▼
{ text } → engine_voice_result → 候选上屏
```

### 2.2 模块映射

```
┌─────────────────────────────────────────┐
│              C++ TSF 平台层               │
│                                         │
│  AudioCapture    ← WASAPI 音频采集        │
│  VoiceButton     ← 工具栏按钮             │
│  SettingsVoicePage ← 设置页              │
│  TaishenDetect   ← 泰深检测              │
├─────────────────────────────────────────┤
│            Rust 核心引擎 (FFI)            │
│                                         │
│  voice.rs        ← VAD / 转写 / 状态管理   │
│  ffi.rs          ← 新增 6 个语音 FFI      │
├─────────────────────────────────────────┤
│           Whisper 推理服务                │
│                                         │
│  whisper-server.exe (HTTP 127.0.0.1)    │
│  ggml 模型 (tiny ~ large-v3-turbo)       │
└─────────────────────────────────────────┘
```

### 2.3 自管路径存储布局

```
%LOCALAPPDATA%\TaishenIME\whisper\
├── bin\
│   ├── whisper-server.exe
│   ├── ggml.dll
│   ├── whisper.dll
│   └── ggml-cpu-*.dll / ggml-cuda-*.dll
├── models\
│   ├── ggml-tiny.bin              (~78MB)
│   ├── ggml-base.bin              (~148MB)
│   ├── ggml-small.bin             (~488MB)
│   ├── ggml-medium.bin            (~1.5GB)
│   └── ggml-large-v3-turbo.bin    (~1.5GB，推荐)
└── downloads\                     (临时下载文件)
```

---

## 三、数据流

### 3.1 完整时序

```
用户点击 Mic ──→ C++ VoiceButton.OnClick()
                      │
                      ▼
                 engine_voice_start()
                      │
                      ├─ [检测泰深] 读文件 + HTTP GET /
                      │   ├─ 优先路径: 记录 taishen_detected=true, 记录 server_url
                      │   └─ 自管路径: 检查本地 whisper-server + 模型, 缺则拒绝(提示下载)
                      │
                      ▼
                 C++ AudioCapture.Start()
                      │
                      ▼
                 ┌─ 录音循环 ──────────────────────────┐
                 │                                      │
                 │  WASAPI 回调 → PCM buffer (每 32ms)   │
                 │      │                                │
                 │      ▼                                │
                 │  engine_vad_process(samples)          │
                 │      │                                │
                 │      ├─ silence → 继续                │
                 │      ├─ speech  → 累积                │
                 │      └─ pending  → 触发转写           │
                 │           │                           │
                 │           ▼                           │
                 │  engine_voice_transcribe(wav)         │
                 │      │                                │
                 │      ▼                                │
                 │  POST /inference (multipart WAV)      │
                 │      │                                │
                 │      ▼                                │
                 │  { text: "识别结果" }                  │
                 │      │                                │
                 │      ▼                                │
                 │  engine_voice_result(text)            │
                 │      │                                │
                 │      ▼                                │
                 │  候选窗口展示 + 自动上屏               │
                 │                                      │
                 └──────────────────────────────────────┘
                      │
用户点击 Mic (关闭) ──→ engine_voice_stop()
                      │
                      ▼
                 AudioCapture.Stop() + VAD flush (剩余语音)
```

### 3.2 VAD 分段逻辑

参照泰深 `VoiceActivityDetector` (TS 版)，移植到 Rust：

```
状态机:
  SILENCE ──→ energy > threshold ──→ SPEECH (开始累积)
  SPEECH  ──→ energy < threshold 持续 silence_timeout_sec ──→ PENDING_TRANSCRIBE
  PENDING ──→ 转写完成后 ──→ SILENCE

最短语音: min_speech_duration_sec = 0.8s (防止杂音误触发)
静音超时: silence_timeout_sec = 1.8s (用户停顿即分段)
能量计算: RMS (root mean square) 滑窗, 窗长 512 samples
```

---

## 四、接口设计

### 4.1 Rust FFI 新增（engine/src/ffi.rs）

```rust
/// 启动语音输入。返回 0=成功, -1=引擎未安装, -2=模型未下载, -3=泰深未运行(优先路径)
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_start(
    server_url: *const c_char,   // whisper-server URL, NULL=自管路径
    model_path: *const c_char,   // 模型路径
    port: i32,                   // server 端口
) -> i32;

/// 停止语音输入。冲刷 VAD 剩余语音并等待转写完成。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_stop() -> i32;

/// 处理一帧 PCM 音频。返回状态: 0=silence, 1=speech, 2=pending_transcribe
/// segment_ptr/segment_len: pending 时输出音频段 Float32Array
#[unsafe(no_mangle)]
pub extern "C" fn engine_vad_process(
    samples: *const f32,
    sample_count: i32,
    segment_ptr: *mut *mut f32,    // 输出: 待转写音频段
    segment_len: *mut i32,         // 输出: 音频段长度
) -> i32;

/// 转写音频段（内部 HTTP POST whisper-server，阻塞调用）
/// 返回 0=成功(结果写入 result_buf), -1=网络错误, -2=超时
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_transcribe(
    wav_data: *const u8,
    wav_len: i32,
    result_buf: *mut c_char,       // 输出: 转写文本, 至少 4096 字节
    result_capacity: i32,
) -> i32;

/// 注入语音转写结果到候选列表。返回插入的候选数。
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_result(
    text: *const c_char,
) -> i32;

/// 获取语音输入状态: 0=idle, 1=listening, 2=transcribing, 3=error
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_state() -> i32;

/// 冲刷 VAD 剩余语音。返回 0=无剩余, 1=有待转写段
/// segment_ptr/segment_len 输出音频段（调用方负责用 engine_voice_transcribe 处理）
#[unsafe(no_mangle)]
pub extern "C" fn engine_voice_flush(
    segment_ptr: *mut *mut f32,
    segment_len: *mut i32,
) -> i32;

/// 检测泰深是否可连接。返回 0=不可用, 1=可用(直连模式), 2=server存在但未启动
#[unsafe(no_mangle)]
pub extern "C" fn engine_detect_taishen(
    taishen_bin: *const c_char,    // ~/.taishen/bin/ 路径
    port: i32,
) -> i32;
```

### 4.2 C++ 平台层新增

```cpp
// platform/windows/include/audio_capture.h

namespace taishen {

/// WASAPI 音频采集器
/// 16kHz, mono, 16bit PCM. 回调在内部线程执行.
class AudioCapture {
public:
    using Callback = std::function<void(const float* samples, int count)>;

    AudioCapture();
    ~AudioCapture();

    /// 开始录音。失败抛 std::runtime_error.
    void Start(Callback cb);
    /// 停止录音并释放设备。
    void Stop();
    /// 是否正在录音。
    bool IsCapturing() const;

private:
    // WASAPI COM 接口
    // IMMDeviceEnumerator → IMMDevice → IAudioClient → IAudioCaptureClient
};

/// 泰深检测
struct TaishenDetection {
    bool installed;          // ~/.taishen/bin/whisper-server.exe 存在
    bool server_running;     // 127.0.0.1:port 可连通
    std::wstring bin_path;   // whisper-server.exe 路径
};

TaishenDetection DetectTaishen(int port = 9080);

/// 设置页「语音」标签
/// 构造第 6 个导航页的内容面板
UIControl* BuildVoiceSettingsPage(ImeConfig& cfg);

}  // namespace taishen
```

### 4.3 HTTP 转写 API

与泰深 whisper-server 完全一致：

```
POST http://127.0.0.1:{port}/inference
Content-Type: multipart/form-data

字段:
  file:      音频文件 (WAV, 16kHz mono 16bit PCM)
  language:  "zh" | "auto" (默认 "zh")

响应 (200):
{
  "text": "识别结果文本"
}

错误:
  400 - 文件格式不支持
  500 - 推理失败
  503 - server 过载

超时: 30s (server 端), 120s (客户端总体)
```

---

## 五、模块设计

### 5.1 Rust: engine/src/voice.rs

```
pub struct VoiceState {
    mode: VoiceMode,         // Idle / Listening / Transcribing / Error
    taishen_detected: bool,  // 是否走优先路径
    server_url: String,      // whisper-server URL
    port: u16,
    vad: VoiceActivityDetector,
}

pub struct VoiceActivityDetector {
    sample_rate: u32,            // 16000
    energy_threshold: f32,       // 默认 0.02
    min_speech_duration_sec: f32, // 0.8
    silence_timeout_sec: f32,    // 1.8
    state: VadState,             // Silence / Speech / Pending
    speech_buffer: Vec<f32>,     // 累积语音帧
    silence_frames: u32,         // 连续静音帧计数
    speech_frames: u32,          // 连续语音帧计数
}

pub fn transcribe(wav: &[u8], server_url: &str) -> Result<String, VoiceError>;
```

### 5.2 C++: AudioCapture (WASAPI)

核心 COM 调用链：

```
CoInitializeEx(COINIT_MULTITHREADED)
    ↓
IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture, eConsole)
    ↓
IMMDevice::Activate(IID_IAudioClient, CLSCTX_ALL)
    ↓
IAudioClient::Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 1000000, 0, &fmt, NULL)
    ↓
IAudioClient::GetService(IID_IAudioCaptureClient)
    ↓
IAudioClient::Start()
    ↓
[循环] IAudioCaptureClient::GetBuffer() → 回调 → ReleaseBuffer()
    ↓
IAudioClient::Stop()
```

WAVEFORMATEX: 16000 Hz, 1 channel, 16 bits, PCM

### 5.3 工具栏按钮

现有工具栏在 `banner_window.cpp`，基于自研窗体系统（UIWindow + UIControl）。

新增 VoiceButton：
- 图标：Unicode 字符 🎤 (U+1F3A4) 或 D2D 自绘麦克风图形
- 状态：Idle(灰色) / Listening(红色脉冲动画) / Transcribing(橙色三点弹跳) / Error(黄色)
- 点击：`engine_voice_start()` / `engine_voice_stop()`
- 位置：工具栏现有按钮之后（中英切换/简繁/双拼/符号/设置 → 语音）

### 5.4 设置页「语音」

导航从 5 页扩到 6 页：`kNavNames[] = { L"基础", L"输入", L"外观", L"高级", L"符号", L"语音" }`，`m_navItems[6]`。

页面布局（VBox 卡片分组）：

**卡片 1：总开关与状态**
- 启用语音输入 (CheckBox) → `voice.enabled`
- 引擎状态标签：绿色「泰深已连接（直连模式）」或蓝色「本机独立运行」
- whisper-server 路径（只读文本，灰色）

**卡片 2：模型管理**
- 模型大小 (ComboBox): tiny / base / small / medium / large-v3-turbo
- 下载按钮 + 进度条（模型不存在时显示）
- 已安装标记：✓ 绿色 / ✗ 灰色
- 磁盘占用提示（如「将占用 ~1.5GB」）

**卡片 3：引擎风格**
- 引擎风格 (ComboBox): CPU / CUDA / BLAS
- GPU 检测按钮 → 调 nvidia-smi
- CUDA 运行时状态标签

**卡片 4：识别参数**
- 语言 (ComboBox): 自动 / 中文
- VAD 阈值 (Slider, 0.01-0.10, 步长 0.005)
- 静音超时 (Slider, 0.5-5.0s, 步长 0.1)

### 5.5 引擎与模型下载

自管路径的下载逻辑参照泰深 `electron/ipc/whisper.ts`：

1. 模型下载：CDN → `%LOCALAPPDATA%\TaishenIME\whisper\models\ggml-{size}.bin`
2. CLI 下载：CDN → ZIP → 提取 whisper-server.exe + 依赖 DLL 到 `whisper\bin\`
3. 校验：
   - 模型：文件 ≥ 10MB（防 HTML 错误页），GGML 格式头
   - CLI ZIP：PK 头 + ≥ 1MB
   - CLI exe：MZ PE 头 + ≥ 100KB
4. 进度：下载回调 → 设置页进度条
5. CDN 源与泰深相同（HuggingFace / GitHub Releases 镜像）

下载 UI 流程：
```
首次点击 Mic 按钮
    → engine_voice_start() 返回 -1 (引擎未安装) 或 -2 (模型未下载)
    → 弹窗: "语音输入需要下载 Whisper 引擎和模型。推荐 large-v3-turbo (~1.5GB)，也可选择更小的模型。"
    → [去设置下载] → 打开设置页「语音」标签 → 用户选择模型大小 → 点击下载
    → 下载完成 → 按钮状态变为 ✓ → 返回即可使用
```

---

## 六、配置

### 6.1 config.ini 新增段

```ini
[voice]
enabled = false
engine = whisper
model_size = large-v3-turbo
server_port = 9080
language = zh
engine_flavor = cpu
vad_threshold = 0.02
vad_silence_timeout_sec = 1.8
vad_min_speech_sec = 0.8
```

### 6.2 config_reader 新增字段

```cpp
struct VoiceConfig {
    bool enabled = false;
    std::string engine = "whisper";
    std::string model_size = "large-v3-turbo";
    int server_port = 9080;
    std::string language = "zh";
    std::string engine_flavor = "cpu";
    float vad_threshold = 0.02f;
    float vad_silence_timeout_sec = 1.8f;
    float vad_min_speech_sec = 0.8f;
};

// ImeConfig 新增成员
VoiceConfig voice;
```

### 6.3 运行时状态（不持久化）

```cpp
struct VoiceRuntimeState {
    bool taishen_detected = false;  // 优先路径可用
    std::wstring server_path;       // whisper-server.exe 路径
    bool model_downloaded = false;  // 当前模型大小已下载
    bool engine_downloaded = false; // whisper-server 已安装
    bool gpu_available = false;     // GPU 可检测到
    bool cuda_runtime_available = false;
};
```

---

## 七、构建与依赖

### 7.1 Rust 侧

`Cargo.toml` 新增依赖：

```toml
[dependencies]
reqwest = { version = "0.12", features = ["blocking", "multipart"] }
```

- `reqwest::blocking` — HTTP POST whisper-server（阻塞调用，转写时无其他操作）
- 不使用 async：输入法 FFI 调用天然同步，无需引入 tokio

### 7.2 C++ 侧

链接系统库（无需额外第三方）：

```
ole32.lib       — COM 基础
uuid.lib        — GUID 定义
```

WASAPI 头文件：
```cpp
#include <Audioclient.h>
#include <Mmdeviceapi.h>
```

均在 Windows SDK 中，无外部依赖。

---

## 八、测试计划

### 8.1 单元测试

| 模块 | 测试项 | 框架 |
|------|--------|------|
| voice.rs VAD | RMS 计算、状态转移、最短语音过滤、静音超时 | `cargo test` |
| voice.rs transcribe | mock HTTP server 返回文本 | `cargo test` |
| config_reader | VoiceConfig 读写往返 | `cargo test` |

### 8.2 集成测试

| 场景 | 前置条件 | 验证点 |
|------|----------|--------|
| 优先路径-直连 | 泰深已安装，whisper-server 在 9080 运行 | 麦克风→说话→候选上屏，无下载弹窗 |
| 优先路径-泰深未启动 | 泰深已安装但 server 未运行 | 提示"请先启动泰深"或尝试自启 |
| 自管路径-完整流程 | 无泰深，引擎+模型已下载 | 麦克风→说话→候选上屏 |
| 自管路径-首次使用 | 无泰深，无引擎 | 点击 Mic → 弹窗引导下载 |
| 安静环境短句 | 安静房间，"你好世界" | 准确转写，上屏"你好世界" |
| 嘈杂环境 | 背景噪音 60dB | VAD 不误触发 |
| 长句连续 | 10 秒以上连续说话 | 自动分段，逐段上屏 |

### 8.3 性能基准

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| 转写延迟（段尾→结果） | < 2s (tiny), < 5s (large-v3-turbo) | 计时 |
| 内存增量 | < 50MB (录音+VAD), < 2GB (whisper-server, turbo模型) | 任务管理器 |
| WASAPI 回调间隔 | 32ms (±2ms) | 帧计数 |

---

## 九、实施顺序

| 阶段 | 需求 | 依赖 | 预计 |
|------|------|------|------|
| 1 | 0.5.1 音频采集 | — | 先调通 WASAPI，能看到 PCM 波形 |
| 2 | 0.5.2 VAD | 0.5.1 | 能量检测，能正确分段 |
| 3 | 0.5.4 HTTP 转写 | 0.5.2 | 硬编码 URL 发请求，拿到 text |
| 4 | 0.5.7 候选注入 | 0.5.4 | 转写文本上屏，看到效果 |
| 5 | 0.5.3 泰深检测 | — | 并行开发，文件检查+端口探测 |
| 6 | 0.5.5 下载管理 | — | 并行开发，参照 taishen whisper.ts |
| 7 | 0.5.6 server 生命周期 | 0.5.5 | 启动/停止/健康检查 |
| 8 | 0.5.8 工具栏按钮 | 0.5.7 | 串联完整用户体验 |
| 9 | 0.5.9 设置页 | 0.5.5 | 可视化配置 |
| 10 | 0.5.10 集成测试 | 全部 | 四种场景全覆盖 |

阶段 1-4 是核心链路（先跑通再完善），5-6 可并行，7-10 是体验完善。

---

## 十、泰深经验

> 以下是泰深语音功能从 D34 到 D75 迭代四轮的全历程经验，含架构决策、下载方案、踩坑实录。
> 输入法不必从零踩坑。

### 10.1 架构演进（D34 → D75）

| 阶段 | 版本 | 方案 | 效果 | 教训 |
|------|------|------|------|------|
| D34 | MVP | 每次转录 spawn whisper-cli.exe，加载 1.5GB 模型→转写→退出 | 单次转录 6-7s 延迟，完全不可用 | 模型不能每次加载 |
| D75 P1 | 中期 | whisper-server 常驻 HTTP 进程，模型只加载一次 | 首次启动等 5-8s，后续转录毫秒级 | 常驻进程是关键 |
| D75 P2 | 当前 | whisper-server HTTP API + CLI spawn 降级 | server 不可用时自动回退 CLI | 要有降级方案 |

**结论**：输入法直接上 whisper-server 常驻模式，不要走 CLI spawn 弯路。自管路径启动 server 时用户可感知 5-8s 启动延迟（加载模型），此后每次转录即发即得。

### 10.2 下载资源

**CDN 源**：whisper.cpp GitHub Releases + HuggingFace 镜像

| 资源 | 大小 | 来源 | 校验方式 |
|------|------|------|----------|
| whisper-server.exe (ZIP) | ~8MB (仅本体) / ~278MB (含 CUDA 运行时) | `github.com/ggerganov/whisper.cpp/releases` | ZIP PK 头 + 文件数 + whisper-server.exe 存在检查 |
| ggml-tiny.bin | ~78MB | `huggingface.co/ggerganov/whisper.cpp` | 文件 ≥10MB + GGML 格式头 |
| ggml-base.bin | ~148MB | 同上 | 同上 |
| ggml-small.bin | ~488MB | 同上 | 同上 |
| ggml-medium.bin | ~1.5GB | 同上 | 同上 |
| ggml-large-v3-turbo.bin | ~1.5GB | 同上 | 同上（推荐，速度/精度最佳平衡） |

**下载方式**：HTTP GET 流式写入，边下边写磁盘，每收到一块更新进度。泰深用的是 `undici.fetch` 手动传 ProxyAgent（保证代理链可控）；输入法自管路径用 Rust `reqwest::blocking` + `std::io::Write`。

**代理支持**：`reqwest` 自动读取系统代理设置（`HTTP_PROXY` / `HTTPS_PROXY` 环境变量），与泰深 `getFetchProxyAgent()` 逻辑等价。

### 10.3 下载校验（防 CDN 镜像返回 HTML 错误页）

泰深踩过 CDN 返回 404/403 HTML 页面被当成二进制文件保存的坑。防御措施：

| 检查点 | 方法 | 来源 |
|--------|------|------|
| HTTP 状态码 | `response.status >= 400` → 终止下载 | `whisper.ts:610-624` |
| ZIP 完整性 | 文件头第 4 字节必须为 `0x50 0x4b` (PK)，≥1MB | `whisper.ts:445-457` |
| EXE 完整性 | 文件头必须为 `0x4d 0x5a` (MZ PE)，≥100KB | `whisper.ts:462-474` |
| 模型完整性 | ≥10MB，文件头不以 `<!do` 或 `<htm` 开头 | `whisper.ts:478-487` |
| Content-Length | 下载完成后校验字节数匹配 | `whisper.ts:626` |

错误处理：校验失败 → 删临时文件 → 提示用户"下载文件校验失败（可能镜像失效），请切换下载源重试"。**不重试同一 URL**。

### 10.4 GPU 检测与引擎风格选择

两级检测（`whisper.ts:254-299`）：

```
1. nvidia-smi --query-gpu=name --format=csv,noheader
   → 成功 → gpuType=nvidia，推荐 CUDA 引擎
2. 降级：PowerShell WMI
   Get-CimInstance Win32_VideoController | Where-Object Name -match "nvidia|radeon|arc"
   → 匹配到 → 对应 gpuType
3. 都不通 → gpuType=null，推荐 CPU 引擎
```

CUDA 运行时检测（`whisper.ts:302-332`）：`where cudart64_*.dll` + `CUDA_PATH` 环境变量 + `CUDA_HOME`。已装 CUDA 时跳过 ZIP 中 ~270MB CUDA 运行时 DLL，只下载 ~8MB 本体。

输入法自管路径的流程：
1. 首次配置时调 GPU 检测 → 自动推荐引擎风格
2. 系统已有 CUDA → 下载 ZIP 时 `skipCudaRuntime=true`（`whisper.ts:526-533` 的 `cudaRuntimePrefixes` 逻辑）
3. 没有 CUDA但有 NVIDIA GPU → 下载 CUDA 版本 ZIP（含运行时 ~278MB）
4. 都没有 → CPU 版本

### 10.5 VAD 参数经验

来自泰深 `VoiceActivityDetector` 实际使用验证的值（`VoiceInputButton.tsx:279-283`）：

| 参数 | 值 | 说明 |
|------|-----|------|
| sampleRate | 16000 | whisper 模型标准采样率，不要改 |
| energyThreshold | 0.02 | RMS 能量阈值，安静环境 0.01-0.02，嘈杂环境需调高 |
| minSpeechDurationSec | 0.8 | 最短语音时长，防止杂音（键盘声/呼吸）误触发 |
| silenceTimeoutSec | 1.8 | 静音超时判段尾，1.8s 是"自然停顿"的平衡点 |

**经验**：silenceTimeoutSec 太短（<1.0s）→ 频繁分段，一句话切成碎片；太长（>3.0s）→ 用户说完后等太久才有反应。1.8s 是四轮迭代后收敛的值。

### 10.6 Whisper 中文简繁问题

**现象**：Whisper zh 模型训练数据以繁体为主，转写结果倾向输出繁体中文（即使输入的是简体普通话）。

**泰深方案**：集成 OpenCC（`opencc-js`），在转写结果上调用简繁转换（`Converter({ from: 't', to: 'cn' })`），来源 `whisper.ts:56-66`。早版本是手工映射表（D74），后升级为 OpenCC 全量映射。

**输入法方案**：Rust 生态无成熟 OpenCC 库。两个选项：
1. 在 Rust 端调 C FFI 链接 OpenCC C 库（`opencc` crate 可用）
2. 转写结果直接上屏，不做转换——输入法场景下，用户可接受繁体候选（反而是一种能力）

建议先不做，等用户反馈。繁体输出对输入法用户不是缺陷，有些用户还会手动切繁体。如果后续确认需要，再集成 `opencc` crate。

### 10.7 音频资源管理（前端→C++ 对应）

泰深前端用 Web Audio API，采集是浏览器级的；输入法走 WASAPI，但以下前端经验仍适用：

| 泰深前端问题 | 表现 | 输入法对应 |
|-------------|------|-----------|
| 多个 AudioContext 竞争麦克风 | 新流静音（energy=0） | WASAPI 独占模式已有保护，但注意 TSF 进程可能被多个宿主加载 |
| 组件卸载时未清理 | 残留 AudioContext 占用设备 | `AudioCapture.Stop()` 必须可靠释放 COM 接口 |
| `abortedRef` 模式 | 用户点停止时，正在进行的转录结果应丢弃 | `engine_voice_stop()` 后忽略 pending 转录结果 |

**来源**：`VoiceInputButton.tsx:63-79`（cleanupAudio 逻辑）、`VoiceInputButton.tsx:215-219`（abortedRef 恢复逻辑）。

### 10.8 whisper-server 进程管理

常驻进程的坑（`whisper-server-manager.ts`）：

| 问题 | 方案 | 来源 |
|------|------|------|
| 并发 start() 竞态 | `startPromise` 锁，后续调用 await 同一个 Promise | L34-35 |
| 模型/端口切换 | 检测配置变更 → 先 stop 旧 server → 再 start 新配置 | L93-103 |
| 意外退出（崩溃） | 自动重启，最多 3 次，防止无限循环 | L163-182 |
| 端口冲突 | 读 stderr 检测 "address already in use" / "EADDRINUSE" | L265-276 |
| 健康检查 | 轮询 `GET /` 直到 200，最多等 30s | L279-298 |
| 优雅退出 | 先 SIGTERM（等 5s），未响应则 SIGKILL | L232-254 |
| 期望退出 vs 意外崩溃 | `expectedExit` 标记，stop() 设置 true 以免报"unexpected exit" | L36-37, L229 |

输入法自管路径的 server 生命周期管理应完全对齐以上逻辑。

### 10.9 转写 API 的 language 参数陷阱

**问题**：whisper-server 默认 `language=en`。不传 language 时，中文语音会被当作英语——不会拒绝，而是把中文发音"翻译"成英文单词。例如"你好"可能变成 "knee how"。

**结论**（`whisper.ts:195-199`）：始终传 `language` 参数，中文模式下固定 `"zh"`，自动模式下传 `"auto"`。输入法默认 `"zh"` 即可。

### 10.10 前端诊断日志体系（C++ 侧参考）

泰深前端有完整的语音诊断日志（`VoiceInputButton.tsx` 中以 `logVoice` 为前缀的调用），覆盖四个阶段：

| 阶段 | 诊断点 |
|------|--------|
| 麦克风流 | active / trackCount / label / constraints / settings |
| AudioContext | state / sampleRate / baseLatency / statechange 事件 |
| 音频帧 | 前 5 帧逐帧能量 + 每 100 帧心跳 |
| 转写 | 请求参数 + 耗时 + 结果长度 |

**超时检测**：200ms / 1s / 3s 三级超时检测 `frameFired` 标记（`VoiceInputButton.tsx:329-342`）——如果音频帧一直不到，逐级报 warning。

输入法 C++ 端应建同等诊断日志：AudioCapture 启动后记录设备名/采样率/缓冲区大小、每 N 帧记录一次能量/帧计数、转录计时。日志落 `DebugLog()`，开关由设置页「诊断日志」控制（已有功能）。

### 10.11 不要重蹈的坑

| 坑 | 现象 | 预防 |
|----|------|------|
| CLI spawn 每次加载模型 | 6-7s 延迟 | 直接用 whisper-server 常驻 |
| CDN 返回 HTML 存为二进制 | whisper-cli.exe 是 HTML 文件，启动失败 | 下载后校验文件头（10.3） |
| 端口写死 9080 | 用户机器端口冲突 | 设置页可改 `server_port` |
| zh 模型输出繁体 | 转写结果看不懂 | OpenCC 集成（10.6） |
| 大模型磁盘残留 | large-v3 升级 turbo 后 2.9GB 浪费 | 升级时自动清理旧模型（`whisper.ts:70-84`） |
| server 崩溃无限重启 | CPU 100% | crashRestartCount 上限 3 次 |
| 多个 AudioContext 抢麦克风 | 新录音静音 | AudioCapture 单例 + Start 前先 Stop 旧实例 |
