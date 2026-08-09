﻿# SPEC: IMM32 兼容层（老游戏/老应用适配，Root #3 #8）

> 对应 ARCHITECT.md Root #3「接口层」+ #8「呈现层」
> 关联 DEV-TRACKER: V0.6 IMM32 兼容层
> 调研依据: LOL 输入法根因实锤（Scaleform GFxIME + IMEConfig.xml 白名单）+ Weasel weasel.ime 设计对照（2026-08-09）

---

## 一、需求

**背景（根因实锤）**：LOL 游戏内聊天框使用 Scaleform GFx 渲染 + GFxIME 接口，走 **IMM32 协议**（ImmGetContext/ImmGetCompositionString/WM_IME_*），且维护输入法白名单 `Game\DATA\Menu\IMEConfig.xml`（含 `<imeName>/<displayName>/<Tag>` 三字段）。只有白名单内输入法 LOL 才激活并渲染候选。泰深纯 TSF 不在白名单 → LOL 从不激活（实测 LOL 进程从未加载 taishen_ime.dll）。

**目标**：泰深注册 IMM32 IME 身份（仿 weasel.ime / 搜狗 QQ 双协议），使 LOL、War3、帝国时代等 IMM32 老游戏/老应用能枚举、激活、输入中文。

**对照结论**：
- 微软拼音可用 = LOL 官方在 IMEConfig.xml 适配 + TSF-IMM32 系统适配器路由
- 搜狗/QQ 可用 = 完整 IMM32 IME 身份 + 被 LOL 白名单收录
- 小狼毫 weasel = TSF（weasel.dll）+ IMM32（weasel.ime）双实现，但 Win10 默认不加载 weasel.ime，需 ImmInstallIME 手动注册（rime/home #744）

## 二、现状代码事实

| 位置 | 现状 |
|------|------|
| `platform/windows/src/dllmain.cpp` | DLL 入口，DllRegisterServer 取路径 |
| `platform/windows/src/tsf_module.cpp` | 纯 TSF：DllGetClassObject/DllCanUnloadNow/DllRegisterServer/DllUnregisterServer（.def 导出），注册 CLSID + CTF\TIP + LanguageProfile + Category |
| `platform/windows/taishen_ime.def` | 仅 TSF 四导出 |
| Rust engine staticlib | FFI 桥接（engine_bridge），引擎状态/词库/配置与平台解耦 |
| 候选窗 | CCandidateWindow → UIWindow + CandidatePanel（D2D 自绘，ui-framework） |

**差距**：无 IMM32 注册（Keyboard Layouts）、无 ImeInquire/ImeProcessKey 等导出、无 IMM32 消息处理。

## 三、IMM32 IME 机制（调研事实）

1. IMM32 IME 是经典 DLL（非 COM），注册到 `HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\<KLID>`（Layout File + Layout Text），或运行时 `ImmInstallIME(dll, name)` 安装到用户层。
2. 必须导出：`ImeInquire`（能力声明）/`ImeProcessKey`（按键消费）/`ImeConversionList`（候选）/`ImeToAsciiEx`（转换上屏）/`ImeSetActiveContext`/`ImeSelect`/`ImeEscape` 等 10+ 函数。
3. 宿主应用调 `ImmGetContext(hwnd)` 拿 HIMC → 输入法窗口过程接收 `WM_IME_*` 消息族（STARTCOMPOSITION/COMPOSITION/ENDCOMPOSITION/CHAR/NOTIFY/SETCONTEXT/SELECT/CONTROL）→ IME 通过 `ImmGetCompositionString`/`ImmGetCandidateList` 被应用轮询组合串与候选。
4. 候选渲染双路径：GFxIME 白名单内 → 游戏内自绘（LOL 用 Tag 匹配）；否则 IME 自绘弹窗（可复用泰深 UIWindow）。
5. Win10 对 64 位进程走 64 位 IME DLL；32 位老游戏需 32 位 IME DLL（weasel 提供 x86/x64 双包）。

## 四、架构设计

**决策：独立 DLL `taishen_ime_imm32.ime`（仿 weasel.ime），与 TSF DLL 分离**。理由：职责隔离、可分别注册/卸载、避免 TSF in-proc 与 IMM32 消息循环互相干扰。引擎复用 Rust staticlib + engine_bridge FFI（同一引擎状态，中英/候选/词库天然共享）。

### 4.1 模块划分

| 模块 | 文件 | 职责 |
|------|------|------|
| 导出层 | `imm32_ime.cpp` | ImeInquire/ImeSelect/ImeSetActiveContext/ImeProcessKey/ImeToAsciiEx/ImeConversionList/ImeEscape/ImeDestroy 等 |
| 消息层 | `imm32_message.cpp` | WM_IME_* 消息处理：组合状态机、候选列表、上屏、光标定位 |
| 候选层 | `imm32_candidate.cpp` | 非白名单应用自绘候选窗（复用 UIWindow 基类 + CandidatePanel 渲染逻辑） |
| 桥接 | 复用 `engine_bridge` | Rust 引擎 FFI（process_key/query/commit） |

### 4.2 注册设计

- KLID 选择：厂商自定义区间 `E0xx0804`（避开微软保留 E001-E020 与主流厂商），落地时先查本机占用再定。
- 注册位置：`Keyboard Layouts\<KLID>`（Layout File = 显示名 .txt，Layout Text = 泰深拼音）+ `Preload`（可选，不抢占默认）。
- 安装器（install.ps1/NSIS）集成注册与卸载清理；提供 `ImmInstallIME` 运行时安装为 fallback（对标 weasel.ime 手动注册路径）。
- 与 LOL 联动：注册后玩家在 `IMEConfig.xml` 添加泰深条目即可走 GFxIME 游戏内候选（displayName 用 IMM32 Layout Text）。

### 4.3 数据流

```
宿主按键 → ImeProcessKey → 消费？→ engine_bridge(process_key) → 引擎
  ├─ 组合串 → WM_IME_COMPOSITION(GCS_COMPSTR) 通知 → 宿主 ImmGetCompositionString 轮询
  ├─ 候选   → ImeConversionList / WM_IME_NOTIFY(IMN_OPENCANDIDATE) → GFxIME 白名单渲染 或 自绘
  └─ 上屏   → ImeToAsciiEx → WM_IME_CHAR → 宿主收到字 → 用户词库学习（复用引擎）
```

## 五、实施计划（每步独立验证 + commit）

| # | 阶段 | 内容 | 工时 | 验证 |
|---|------|------|------|------|
| 1 | 骨架 | DLL 工程 + .def 导出桩 + Keyboard Layouts 注册 + ImeInquire 能力声明（含 64 位） | 4h | ImmInstallIME/注册表能枚举；notepad 出现泰深 IMM32 项 |
| 2 | 输入链 | ImeProcessKey → 引擎 → 组合串消息 + ImeConversionList 候选 + ImeToAsciiEx 上屏 | 8h | notepad IMM32 路径全拼音输入可用 |
| 3 | 候选窗 | 自绘候选（复用 UIWindow + CandidatePanel），组合窗口跟随 | 4h | 非白名单应用候选窗正常 |
| 4 | 集成验证 | LOL 实测（IMEConfig.xml 注册后）+ 32 位 IME DLL 构建 + 老游戏回归 | 4h | LOL 可切换泰深并出候选 |
| 5 | 安装器 | install.ps1/NSIS 注册 KLID + 卸载清理 + 32/64 双包 | 2h | 全新安装/卸载干净 |

**总工时**：~22h

## 六、验证标准

- `cargo build` + `cargo test` + `biome check` 全绿
- notepad 通过 IMM32 路径完整输入（组合/候选/选词/退格）
- LOL 安装后：Win+Space 可切泰深，聊天框出候选（GFxIME 或自绘）
- 64 位 + 32 位宿主双架构验证

## 七、风险与对策

| 风险 | 对策 |
|------|------|
| IMM32 规范文档少 | 主参照 weasel.ime 源码（rime/weasel，开源）+ 微软 IME 规范 + 搜狗/QQ 行为逆向 |
| Win10 对 IMM32 兼容层有已知 bug（微软拼音 IMM32 通道事件不完整） | IMM32 通道仅作为老游戏 fallback，TSF 为主通道 |
| KLID 冲突 | 落地前枚举本机 Keyboard Layouts 避让 |
| 32 位 IME DLL 需额外构建链 | CMake 增加 Win32 平台配置（weasel 已有先例） |
| LOL 白名单 Tag 匹配需试错 | Phase 4 实测验证，displayName 对齐 IMM32 Layout Text |
