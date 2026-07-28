# 泰深输入法 — 架构设计

## 项目定位

一款跨平台 PC 端中文拼音输入法。核心引擎用 Rust 实现跨平台复用，平台层（IME 框架对接）用各平台原生语言。

## 技术选型

| 层 | 技术 | 理由 |
|---|------|------|
| 核心引擎 | Rust | 零成本抽象、内存安全、C FFI 导出天然跨平台 |
| Windows 平台层 | C++/COM | TSF（Text Services Framework）原生接口 |
| macOS 平台层（预留） | Swift | Input Method Kit 原生接口 |
| FFI 桥接 | C ABI | Rust `extern "C"` ↔ C++ `extern "C"`，最稳定 |
| 构建系统引擎 | Cargo | Rust 生态标准 |
| 构建系统平台 | CMake | 跨平台 C++ 构建 |

## 整体架构

```
┌─────────────────────────────────────────────┐
│                  应用程序                      │
│          (记事本 / 浏览器 / VS Code ...)       │
└──────────────────┬──────────────────────────┘
                   │ 文本提交 (Composition)
┌──────────────────┴──────────────────────────┐
│              平台层 (Platform Layer)          │
│  ┌─────────────────────┐  ┌───────────────┐  │
│  │   Windows TSF DLL   │  │  macOS InputKit│  │
│  │   (C++ / COM)       │  │  (Swift)       │  │
│  │                     │  │   【预留】      │  │
│  │ - KeyEvent 捕获     │  │               │  │
│  │ - 候选窗口 UI       │  │               │  │
│  │ - 文本提交          │  │               │  │
│  │ - 状态管理          │  │               │  │
│  └──────────┬──────────┘  └───────────────┘  │
└─────────────┼────────────────────────────────┘
              │ C ABI (extern "C")
┌─────────────┴────────────────────────────────┐
│             核心引擎 (Engine)                  │
│              Rust Library (.dll / .dylib)     │
│                                              │
│  ┌──────────┐ ┌──────────┐ ┌──────────────┐  │
│  │ Pinyin   │ │Dictionary│ │ Configuration│  │
│  │ 拼音引擎  │ │ 词库     │ │ 配置管理      │  │
│  │          │ │          │ │              │  │
│  │ 全拼→汉字 │ │ 前缀查询 │ │ 用户设置      │  │
│  │ 双拼→汉字 │ │ 词频排序 │ │ 词库加载      │  │
│  │ 【二期】  │ │ 用户词库 │ │ 【二期】      │  │
│  │          │ │ 【二期】  │ │              │  │
│  └──────────┘ └──────────┘ └──────────────┘  │
└──────────────────────────────────────────────┘
```

## 数据流 — 第一期 MVP

```
用户按键→TSF KeyEvent→C FFI→pinyin_engine_process_key()
                                   ↓
                          累积拼音串 (如 "zhong")
                                   ↓
                          dictionary_query("zhong")
                                   ↓
                          候选词列表 ["中","重","钟"...]
                                   ↓
                ← C FFI 返回 ←
        ↓
TSF 候选窗口展示
        ↓
用户选择(数字键/Space)→TSF 提交文本到应用
```

## 第一期 MVP 范围

- Windows TSF 输入法框架对接
- 全拼拼音输入
- 基础词库（内置 5-10 万词条）
- 候选词展示与选择
- 中英文切换

## 第二期范围

- 双拼支持
- 用户词库（学习用户输入习惯）
- 云输入候选
- 皮肤/主题系统
- macOS 平台适配

## 项目目录结构

```
taishenIME/
├── engine/                  # Rust 核心引擎
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs           # 库入口
│       ├── ffi.rs           # C FFI 导出
│       ├── pinyin/          # 拼音处理
│       │   └── mod.rs
│       └── dictionary/      # 词库
│           └── mod.rs
├── platform/
│   └── windows/             # Windows TSF 实现
│       ├── CMakeLists.txt
│       ├── src/
│       │   ├── dllmain.cpp  # DLL 入口
│       │   ├── tsf_module.cpp  # TSF 模块实现
│       │   └── engine_bridge.cpp  # Rust FFI 桥接
│       └── include/
│           └── engine_bridge.h  # FFI 头文件
├── resources/               # 词库等数据文件
├── docs/                    # 设计文档
│   └── architecture.md
├── .gitignore
└── taishenIME.md
```

## Rust FFI 接口设计（初稿）

```c
// 引擎初始化
int engine_init(const char* dict_path);

// 处理按键，返回候选词数量。拼音串内部累积
int engine_process_key(int key_code, int modifier);

// 获取当前拼音串
int engine_get_pinyin_str(char* buf, int buf_len);

// 获取候选词（index 从 0 开始）
int engine_get_candidate(int index, char* buf, int buf_len);

// 获取候选词总数
int engine_get_candidate_count();

// 选择候选词（提交），返回提交文本长度
int engine_select_candidate(int index, char* buf, int buf_len);

// 清空状态（ESC / 回车提交后）
void engine_reset();

// 引擎销毁
void engine_destroy();
```
