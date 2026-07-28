# 泰深输入法

一款跨平台 PC 端中文拼音输入法。

## 技术栈

- **核心引擎**: Rust (cdylib + staticlib, edition 2024)
- **Windows 平台层**: C++17 + CMake, TSF (Text Services Framework)
- **UI 渲染**: Skia 跨平台 2D 图形
- **数据库**: SQLite (系统词库) + 文本文件 (用户词库)
- **代码检查**: Biome 2.x

## 项目结构

```
taishenIME/
├── engine/               Rust 核心引擎
├── platform/windows/     Windows TSF 实现
├── platform/macos/       macOS InputKit (预留)
├── resources/            静态资源 (词库/配置)
└── docs/                 设计文档
```

## 常用命令

```powershell
# 引擎编译与测试
cd engine && cargo build && cargo test

# 代码检查
.\node_modules\.bin\biome.cmd check --write .

# Git
git log --oneline
```
