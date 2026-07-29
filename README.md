# 泰深输入法 (taishenIME)

> Rust 核心引擎 + C++ TSF 平台层 · 跨平台中文拼音输入法

[![Rust](https://img.shields.io/badge/Rust-1.97+-orange.svg)](https://www.rust-lang.org)
[![C++](https://img.shields.io/badge/C++-17-blue.svg)](https://isocpp.org)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

PC 中文拼音输入法。Rust 实现核心引擎（音节切分、词库查询），C++ 对接 Windows TSF (Text Services Framework) 平台层。个人项目，AI 辅助开发。

---

## 架构

```
┌─────────────────────────────────┐
│         应用层（未来）            │
│   候选窗 UI · 配置工具 · 词库管理  │
├─────────────────────────────────┤
│       C++ TSF 平台层             │
│   按键拦截 → 拼音串 → 候选上屏     │
├─────────────────────────────────┤
│       Rust 核心引擎 (FFI)        │
│   音节切分 · 词库查询 · 状态机     │
├─────────────────────────────────┤
│        SQLite 系统词库            │
└─────────────────────────────────┘
```

## 技术栈

| 层 | 技术 | 说明 |
|---|------|------|
| 核心引擎 | Rust (cdylib) | 平台无关的拼音处理逻辑 |
| Windows 平台 | C++17 + TSF | 系统输入法框架对接 |
| 构建 | Cargo + CMake + MSVC | Rust 编译为动态库，C++ 链接调用 |
| 代码质量 | Biome + rustfmt + clippy | 格式化 + Lint |

## 开发状态

- [x] 项目架构骨架 (ARCHITECT.md)
- [x] 系统词库 SQLite 外部加载 (203 条)
- [x] 竞品调研 (RIME / PIME / Skia)
- [ ] TSF 平台层实现
- [ ] 候选窗口渲染 (Direct2D)
- [ ] 用户词库学习

详细进度见 [DEV-TRACKER.md](docs/DEV-TRACKER.md)。

## 快速开始

### 环境要求

- Windows 10+
- Rust 1.97+ (`rustup`)
- CMake 3.26+
- Visual Studio Build Tools (MSVC)

### 构建引擎

```bash
cd engine
cargo build --release
```

### 构建平台层（待实现）

```bash
cd platform/windows
cmake -B build
cmake --build build --config Release
```

## 项目结构

```
taishenIME/
├── engine/                 # Rust 核心引擎
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # Engine 状态机
│       ├── ffi.rs          # C ABI 接口
│       ├── pinyin/         # 音节表 + 切分算法
│       └── dictionary/     # 词库查询
├── platform/
│   ├── windows/            # Windows TSF 实现
│   └── macos/              # 预留
├── resources/              # 词库 / 配置
├── docs/                   # 设计文档
│   ├── ARCHITECT.md        # 架构骨架
│   ├── DEV-TRACKER.md      # 需求看板
│   ├── business-flow.md    # 核心数据流
│   └── modules/            # 模块 SPEC
└── taishenIME.md           # L2 宪法（开发环境与工作流）
```

## 文档

- [架构设计](docs/ARCHITECT.md)
- [开发需求跟踪](docs/DEV-TRACKER.md)
- [核心数据流](docs/business-flow.md)
- [项目宪法](taishenIME.md) — 开发环境、Git 策略、工具链约定

## License

MIT
