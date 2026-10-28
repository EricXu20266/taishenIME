# SPEC — IMM32 候选数据接口与全屏降级

> 模块：platform/windows（IMM32 链路）
> 日期：2026-10-28
> 状态：设计完成，待在场实测后实施

## 一、背景

泰深有两条平台链路，能力不对等：

| 检查项 | 现状 | 位置 |
|--------|------|------|
| TSF 候选数据结构 | 已实现 `ITfCandidateListUIElement` 的 9 个方法 | `tsf_module.cpp` |
| TSF 候选注册 | 已通过 `ITfUIElementMgr::BeginUIElement` | `tsf_module.cpp` |
| IMM32 候选窗 | 自绘 `CCandidateWindow` | `imm32_ime.cpp` |
| IMM32 候选数据 | **未实现**：IME 从不填充 `INPUTCONTEXT::hCandInfo` | `imm32_ime.cpp` 全文无 `IMC_` / `ImmLockIMC` |
| 全屏检测 | **无** | `platform/windows/src` 无相关调用 |
| 内联拼音 | 已有（`SetInlinePreedit`） | `imm32_ime.cpp` |

结论：TSF 链路已具备「应用自绘」能力，IMM32 链路不具备。独占全屏应用走 IMM32，正好落在缺的一侧。

## 二、目标

1. 老应用能用标准流程取到候选并自绘：`ImmGetContext → ImmGetCompositionString(GCS_COMPSTR) → ImmGetCandidateList → 自绘 → ImmNotifyIME 选字`
2. 提供 `ui_mode` 开关，允许在全屏场景关闭自绘候选窗，只维护引擎状态并对外提供数据
3. 全屏自动降级，保证「能打字」优先

**不做**：检测到全屏就禁用输入法（倒退）。

## 三、候选数据协议

IMM32 下候选列表由 IME 写入输入上下文，IMM 读取后返回给应用。实现落在 `ImeToAsciiEx` 生成消息的位置：

```
INPUTCONTEXT* lpIMC = ImmLockIMC(hIMC);
HIMCC hCand = ImmReSizeIMCC(lpIMC->hCandInfo, totalSize);
CANDIDATEINFO* lpCandInfo = (CANDIDATEINFO*)ImmLockIMCC(hCand);
lpCandInfo->dwCount = 1;                       // 一个候选列表
lpCandInfo->dwOffset[0] = sizeof(CANDIDATEINFO); // CANDIDATELIST 相对偏移
CANDIDATELIST* lpList = (CANDIDATELIST*)((BYTE*)lpCandInfo + lpCandInfo->dwOffset[0]);
lpList->dwCount = n;
lpList->dwSelection = sel;
lpList->dwPageStart = pageStart;
lpList->dwPageSize = pageSize;
lpList->dwOffset[i] = 相对 lpList 的字符串偏移;
// 字符串区：UTF-16，连续存放
ImmUnlockIMCC(hCand);
ImmUnlockIMC(hIMC);
```

**字节量计算**：`sizeof(CANDIDATEINFO) + sizeof(CANDIDATELIST) + (dwCount) * sizeof(DWORD) + 字符串区总字节`。
每次候选变化都要重算，`ImmReSizeIMCC` 会重新分配。

**通知消息**：候选打开/变化/关闭各发一次
`WM_IME_NOTIFY` + `IMN_OPENCANDIDATE` / `IMN_CHANGECANDIDATE` / `IMN_CLOSECANDIDATE`。

**组合串**：走 `lpIMC->hCompStr`（`COMPOSITIONSTRING`）或每条 TRANSMSG 里的 `WM_IME_COMPOSITION` + `GCS_COMPSTR`，二选一保持一致，不要两条都设。

## 四、ui_mode 开关

`config.ini` 增加 `ui_mode`：

| 值 | 行为 |
|----|------|
| `window` | 当前行为：自绘候选窗 + 数据接口同时可用 |
| `data` | 不创建候选窗、不 `ShowWindow`，只维护引擎状态并填 `hCandInfo` |
| `auto`（默认） | 常规桌面走 `window`；检测到独占全屏走 `data` |

数据接口在两种模式下都要填——`window` 模式下填了也不冲突，宿主愿意自绘就自绘。

## 五、全屏检测

用 `SHQueryUserNotificationState()`（`shell32.dll`）：

| 返回值 | 处理 |
|--------|------|
| `QUNS_RUNNING_D3D_FULL_SCREEN` | 切 `data` 模式 |
| `QUNS_PRESENTATION_MODE` | 切 `data` 模式 |
| 其他 | 恢复 `window` 模式 |

比枚举窗口 + 判断尺寸可靠。检测时机：输入法激活时 + 每次候选窗显示前（节流到 1 秒一次）。

**注意**：切到 `data` 模式后，如果宿主不主动取数据自绘，用户仍看不到候选。兜底是「首候选直接上屏 + 拼音串走组合串」，至少能打字。

## 六、验收

| 判据 | 读取方式 | 级别 |
|------|---------|------|
| 自绘应用能取到 N 条候选 | 最小 IMM32 测试程序调 `ImmGetCandidateList`，打印 `dwCount` 与首条字符串 | 人工（需装机） |
| 分页与选中项同步 | 翻页后重取，`dwPageStart`/`dwSelection` 变化正确 | 人工 |
| `data` 模式下不创建候选窗 | Spy++ 或 `desktop_status` 查宿主进程窗口列表，无候选窗类名 | 人工 |
| 全屏时能上屏 | 独占全屏 D3D 程序输入 `nihao`，空格上屏「你好」 | 人工 |
| `window` 模式行为不变 | 常规桌面应用候选窗位置、样式与当前版本一致 | 人工 |

全部判据需要真实安装与宿主应用，无法在无人值守下完成。

## 七、风险

- **崩溃面**：`hCandInfo` 字节量算错或 `ImmLockIMCC` 未配对释放，会让宿主进程崩溃（输入法联动所有应用）。改动必须逐个应用验证，不能一次铺开。
- **类名不可变**：IME 窗口类名一旦被 IMM 记录，改名会导致加载失败（需重启）。本方案不改类名。
- **通知风暴**：每次按键都重填候选 + 发 `IMN_CHANGECANDIDATE` 会拖慢宿主。需按「候选内容哈希变化」节流。
- **回滚**：`ui_mode=window` 且不填 `hCandInfo` 即回到当前行为，代码必须保证这条路径不变。

## 八、实施顺序

| 步 | 内容 | 验证 |
|----|------|------|
| 1 | `hCandInfo` 填充 + `IMN_*` 通知（仅 `window` 模式也填） | 最小 IMM32 测试程序 |
| 2 | `ui_mode` 配置项与 `data` 模式（跳过窗口创建/显示） | Spy++ 查窗口 |
| 3 | `SHQueryUserNotificationState` 检测 + `auto` 切换 | 全屏程序实测 |
| 4 | 节流（候选哈希 + 1 秒全屏检测间隔） | 长按连续输入不卡 |

`window` 模式下第 1 步也要生效，这样自绘宿主（不做全屏）也能直接用。
