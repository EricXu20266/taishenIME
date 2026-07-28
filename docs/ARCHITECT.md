# 泰深输入法 — ARCHITECT.md

> 架构骨架。基于 app-project-architect 方法论：拓扑总纲 + Root 裁剪 + 结构映射。
> 生成于 2026-07-28，Step 3 产出。

---

## 一、拓扑分析（1+3+3+场）

```
心跳(1): TSF 事件循环（隐式） → 注入点 tsf_module.cpp:ITfKeyEventSink::OnKeyDown

三角:
  前台 — platform/windows/  TSF 候选窗口（Skia 渲染）
  后台 — engine/src/       拼音引擎 + 词库查询
  数据库 — resources/system_dict.db（SQLite）+ %APPDATA%/user_dict.txt

三条流:
  通道 — C FFI 9 函数（按键入/候选出）→ engine/src/ffi.rs
  跟踪 — tracing crate（Rust 侧）+ Windows EventLog（平台侧）
  呈现 — Skia 跨平台 2D 渲染 → platform/windows/src/candidate_ui.cpp

场:
  安全 — 输入内容不落盘明文，不联网泄露
  可靠 — FFI panic 守卫，词库损坏降级
  性能 — 候选查询 <5ms，冷启动 <500ms，内存 <50MB
  配置 — 三层配置（默认→安装→用户），YAML，热加载
  生命周期 — NSIS/MSI 安装器，自动更新
```

## 二、Root 裁剪

从参考十二 Root 裁剪出 11 个。砍掉 #5 身份与权限（桌面单用户不需要）。

| # | Root | 白话 | 物理位置 | 状态 |
|---|------|------|---------|------|
| 1 | 数据模型与持久化 | 存什么、怎么存 | `resources/system_dict.db` + `%APPDATA%/user_dict.txt` | 🔴 待实现 |
| 2 | 业务领域层 | 拼音→汉字 | `engine/src/pinyin/` + `engine/src/dictionary/` | 🟢 骨架已有 |
| 3 | 接口层 | 怎么跟它打交道 | `engine/src/ffi.rs` + `platform/*/` | 🟡 9 个 FFI 函数已定义，TSF 未实现 |
| 4 | 状态管理 | 此刻输入什么 | `engine/src/lib.rs` (Engine struct) | 🟢 骨架已有 |
| 5 | ~~身份与权限~~ | ~~谁能干什么~~ | — | ❌ 砍掉 |
| 6 | 安全边界 | 不能发生什么 | 场覆盖，不建实体 | 🔴 待设计 |
| 7 | 配置系统 | 不改代码怎么改行为 | `resources/default_config.yaml` | 🔴 待实现 |
| 8 | 呈现层 | 长什么样 | `platform/windows/src/candidate_ui.cpp` (Skia) | 🔴 待实现 |
| 9 | 可观测性 | 发生了什么 | tracing crate + 平台日志 | 🔴 待实现 |
| 10 | 可靠性 | 错了能回退 | FFI panic 守卫 + 降级策略 | 🟡 ffi.rs 有 unwrap() 隐患 |
| 11 | 性能 | 跑得快用得省 | 词库前缀索引 + 冷启动优化 | 🟡 索引已有，未压测 |
| 12 | 生命周期 | 怎么部署升级回滚 | NSIS/MSI 安装器 | 🔴 待实现 |

## 三、配置三个家

| 类型 | 位置 | 生效时机 | 内容 |
|------|------|---------|------|
| 代码库配置 | `resources/default_config.yaml` | 随 git，安装时拷贝 | 默认候选数、默认皮肤、音节表 |
| 环境配置 | 安装目录 `config.yaml` | 安装器写入，重启生效 | 词库路径、日志级别 |
| 用户数据 | `%APPDATA%/taishen-ime/` | 运行时读写，即时生效 | 用户词库、用户配置覆盖 |

## 四、数据分库

| 库 | 位置 | 量级 | 保留期 |
|----|------|------|--------|
| 系统词库 | `resources/system_dict.db` (SQLite) | ~10MB，只读 | 随版本更新 |
| 用户词库 | `%APPDATA%/taishen-ime/user_dict.txt` | 持续增长 | 永久，用户可手动清理 |
| 日志 | `%APPDATA%/taishen-ime/logs/` | ~1MB/天 | 滚动保留 7 天 |

## 五、数据流

```
用户按键
  ↓
Windows TSF (ITfKeyEventSink::OnKeyDown)
  ↓
engine_bridge.cpp → FFI: engine_process_key(char)
  ↓
Engine::process_key() → 累积拼音缓冲
  ↓
dictionary::query(prefix) → 前缀索引查询
  ↓
候选词列表 → FFI: engine_get_candidate()
  ↓
candidate_ui.cpp (Skia 渲染候选窗口)
  ↓
用户选择 (数字键/Space)
  ↓
FFI: engine_select_candidate() → 提交文本
  ↓
TSF 文本插入到目标应用
```

## 六、目录结构

```
taishenIME/
├── engine/                          # Root #2 业务领域 + #4 状态管理
│   ├── Cargo.toml                   # cdylib + staticlib
│   └── src/
│       ├── lib.rs                   # Engine 状态机
│       ├── ffi.rs                   # Root #3 C ABI 接口
│       ├── pinyin/mod.rs            # 音节表 + 切分算法
│       └── dictionary/mod.rs        # Root #1 词库查询
├── platform/
│   ├── windows/                     # Root #3 (TSF) + #8 (呈现)
│   │   ├── CMakeLists.txt
│   │   ├── include/engine_bridge.h
│   │   └── src/
│   │       ├── dllmain.cpp
│   │       ├── tsf_module.cpp       # TSF COM 实现
│   │       ├── candidate_ui.cpp     # Skia 候选窗口
│   │       └── engine_bridge.cpp    # FFI 桥接
│   └── macos/                       # 预留
├── resources/                       # 静态资源（随安装包）
│   ├── default_config.yaml
│   └── system_dict.db
├── docs/
│   ├── ARCHITECT.md                 # ← 本文件
│   ├── architecture-overview.md
│   ├── business-flow.md
│   ├── DEV-TRACKER.md
│   ├── index.md
│   ├── CHANGELOG
│   ├── modules/                     # 每 Root 一篇设计手册
│   └── archive/                     # 过时文档归档
├── biome.json
└── .gitignore
```

## 七、生效时机

| 资源 | 生效方式 | 说明 |
|------|---------|------|
| 系统词库 | 重启生效 | 引擎启动时加载到内存索引 |
| 默认配置 | 重启生效 | 安装时拷贝到安装目录 |
| 用户配置 | 热加载 | 改完立即生效，引擎检测文件变化 |
| 用户词库 | 即时生效 | 每次选词后实时写入 |
| 日志 | 即时生效 | 引擎启动即开始写入 |

## 八、运行时设计

| 维度 | 决策 |
|------|------|
| 启动 | 系统词库延迟加载（首次输入时才建索引），冷启动 <500ms |
| 状态 | 单进程、单用户。重启后从用户词库恢复学习结果 |
| 数据增长 | 用户词库文本追加，超过 10MB 触发压缩去重 |
| 并发 | 输入事件串行，无竞态。词库读写用 Mutex 保护 |
| 失败 | 系统词库损坏 → 降级到内置最小词库（硬编码 85 条）；用户词库损坏 → 丢弃，新建空文件 |
| 可观测 | tracing crate 分 INFO/WARN/ERROR 三级，平台层对接 Windows EventLog |
| 可回退 | 配置回退：删除用户配置即回默认。词库回退：系统词库随版本回滚 |
| 版本兼容 | 词库格式带版本号，引擎启动时检查→不兼容则重建索引 |

## 九、决策记录

| 决策 | 选项 A | 选项 B | 选择 | 原因 |
|------|--------|--------|------|------|
| 数据库 | 纯 SQLite | 纯文本文件 | SQLite（系统）+ 文本（用户） | 系统词库需要查询性能，用户词库需要可迁移 |
| UI 渲染 | 原生控件 | 跨平台框架 | Skia 跨平台 | 一套渲染代码覆盖 Windows + macOS |
| 配置系统 | 最小配置 | 完整分层配置 | 完整配置 | 为二期双拼/云输入/皮肤预留扩展点 |
