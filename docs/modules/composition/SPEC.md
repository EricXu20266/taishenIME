# SPEC: 组合区与上屏（TSF / IMM32）

> 对应 ARCHITECT.md Root #3「接口层」+ Root #8「呈现层」
> 初版：2026-08-02（0.1.7 选词上屏）
> 现行修订：2026-11-16（组词组合串、分段显示属性、UIElement 协商、IMM32 对齐）

---

## 一、需求

在目标应用的文档中维护一段「正在编辑」的组合文本，支持拼音累积、组词已选前缀、
候选上屏与取消，并在 TSF 与 IMM32 两条链路上表现一致。

**合约**：

- 输入字母时创建组合，组合内容为当前输入串（普通输入 = 拼音显示串）
- 组词态组合串 = 已选前缀（汉字）+ 剩余音节（带分隔）
- 选词时把组合替换为所选文本并结束组合
- 退格：组词态先回退已选词，无已选词才删拼音字母
- ESC：取消组合，不落任何文本
- 组合被宿主外部终止时，输入法同步清理内部状态
- 所有文档修改在 ITfEditSession 内执行

**不做**：

- 云输入、联想整段续写
- 非 TSF/IMM32 的第三方输入协议（如 UWP Text Services 的独立路径）

## 二、状态归属（现行核心约束）

**引擎是组合状态的唯一真相源**。平台层只读、不存副本：

| 状态 | 来源 | 说明 |
|------|------|------|
| 是否组词 | `engine_in_compose()` | 1 = 组词态 |
| 已选前缀 | `engine_compose_prefix()` | 已选汉字串 |
| 待选/剩余音节 | `engine_syllable_display()` | 组词态为剩余音节，普通输入为完整拼音分隔串 |
| 回退 | `engine_compose_undo()` | 回退最后一个已选字 |

历史教训（B-22 / 2026-11-15）：前缀副本曾由平台层维护，翻页、退格穿透、
候选窗刷新三条路径不同步，表现为「已选字消失、重新选字后又出现、提交重复」。
现在任何入口都重新向引擎读取，组合串必然与引擎状态一致。

## 三、组合串拼装：单一入口

### TSF

```cpp
// tsf_module.cpp
std::string BuildCompositionText(const std::string& pending) const;
void SyncComposition(ITfContext* pic, const std::string& pending,
                     bool forceStart = false);
```

- `BuildCompositionText`：组词态返回 `compose_prefix + pending`，否则返回 `pending`
- `SyncComposition`：`pending` 为空时自动读 `engine_syllable_display()`；
  组合已激活走 `Update`，否则 `Start`
- 已选前缀的宽字符数经 `MultiByteToWideChar` 换算后传入编辑会话，
  用于分段显示属性（按宽字符位置切分，不能按 UTF-8 字节数）

### IMM32

```cpp
// imm32_ime.cpp
static std::wstring GetDisplayPinyinWide();
static void SyncComposition(HIMC himc, const std::wstring& pinyin,
                            std::vector<TRANSMSG>& msgs);
```

- `GetDisplayPinyinWide`：与 TSF 同构——组词态 = 已选前缀 + 剩余音节
- 组合串写 `GCS_COMPSTR`，同时缓存到 DLL 内部供 `ImeGetCompositionStringW` 返回
- 中间音节不落文档：只更新组合串；最后一个音节提交整词
- 组合结束前先推一条空 `GCS_COMPSTR` 刷新消息，再发 `WM_IME_ENDCOMPOSITION`——
  GFxIME（LOL 聊天框）只在这条消息路径上重绘组合区

**两条通道的差异只在「怎么写进宿主」**，组合串内容规则与引擎状态完全共用。

## 四、TSF 组合模型

```
ITfContextComposition::StartComposition(ec, range, sink, &composition)
  → ITfComposition（可 SetText / ShiftStart / ShiftEnd）
  → EndComposition(ec) 结束
```

| 接口 | 用途 |
|------|------|
| ITfContextComposition | 创建/枚举组合 |
| ITfComposition | 组合对象与生命周期 |
| ITfCompositionSink | 组合被外部终止回调（`OnCompositionTerminated`） |
| ITfEditSession | 文档修改载体 |
| ITfRange | 文本范围 SetText/GetText |

编辑会话调度：`CEditSessionComposition` 携带 `Op { Start, Update, Commit }`
+ 文本 + `convertedChars` + `caretOffset`，`RequestEditSession(TF_ES_SYNC)` 同步执行。

**Commit 路径不写分段属性**：即将进文档的文本不能再挂 `TF_ATTR_INPUT`
（会带虚线下划线且属性残留）。

## 五、分段显示属性

两个自定义 GUID（注册端与应用端必须一致）：

| GUID | 语义 | 样式 |
|------|------|------|
| `GUID_TAISHEN_ATTR_CONFIRMED` | 已选汉字前缀 | 无额外标记（`TF_LS_NONE` / `TF_ATTR_TARGET_CONVERTED`） |
| `GUID_TAISHEN_ATTR_PENDING` | 待选拼音 | 虚线下划线（`TF_LS_DOT` / `TF_ATTR_INPUT`） |

实现 `tsf_composition.cpp::ReplaceCompositionText` →
`display_attributes.cpp::ApplyCompositionDisplayAttributes(ec, pic, range, convertedChars, totalChars)`。

应用顺序：先把整段标为 pending，再用 confirmed 覆盖前 `convertedChars` 个字符。
前缀切分失败时整段至少还是「待选」样式，不会变成无属性。

## 六、候选 UIElement 协商与自绘兜底

TSF 候选数据通过 `ITfCandidateListUIElement` 提供给宿主：

- 每次更新候选都 `BeginUIElement` 注册（`show` 由宿主决定），`UpdateUIElement` 刷新
- `show = FALSE` 只表示**不让系统画**，不代表宿主自己会画——
  游戏经 msctfime 桥接读取候选数据后自行渲染
- 自绘兜底判定 `ShouldSelfDrawWhileSuppressed()`：
  1. per-app 配置 `app_self_draw` / `app_no_self_draw` 优先
  2. 未配置时看宿主是否调用过 `GetCount` / `GetString` / `GetSelection` /
     `GetPageIndex` / `GetCurrentPage`（`m_uiElementDataRead`）
  3. 读过 → 宿主自己画，不再叠自绘窗；没读过 → `CCandidateWindow` 兜底

数据接口：`GetCount` / `GetString` / `GetSelection` / `GetPageIndex` /
`SetPageIndex` / `GetCurrentPage`。`SetPageIndex` 只记录请求，
不在 UIElement 回调中重入引擎状态机。

## 七、数据流

```
按键
  → 引擎状态推进（pinyin_buf / compose_*）
  → SyncComposition(pending = "")
      → BuildCompositionText(engine_syllable_display)
      → Start / Update（带 convertedChars）
      → ApplyCompositionDisplayAttributes
  → 候选刷新（同时 UpdateUIElement）
      → 宿主自绘 或 CCandidateWindow 兜底

选词（中间音节）
  → engine_select_candidate → 引擎推进 compose_idx
  → SyncComposition（组合串 = 新前缀 + 剩余音节，不落文档）

选词（最后音节 / 普通词 / 整句）
  → engine_select_candidate 返回整段文本
  → Commit（不写分段属性）
  → CollapseSelectionToEnd（防宿主把组合 range 当选区继续高亮）
  → 候选窗隐藏，引擎 reset
```

## 八、验证清单

| 判据 | 方式 |
|------|------|
| 组词选首字后翻页，组合区仍显示已选字 | 实机 Notepad++ / 记事本 |
| 组词退格先回退已选词，拼音不丢 | 实机 + `SyncComposition` 日志 |
| 已选前缀无下划线、剩余拼音虚线下划线 | 实机视觉 |
| 上屏文本无属性残留、不保持选中 | 实机连续输入 |
| 宿主不接收组合串时候选窗仍显示拼音 | `g_inlineCompositionReadable` 回读探测 |
| 游戏读候选数据时不自绘叠窗 | `EnsureCandidateUiElement` 日志 show=0 + 数据读取标记 |
| IMM32 组词提交整词、不逐字落文档 | LOL 聊天框 / 魔兽世界实测 |

## 九、演进记录

| 日期 | 变更 |
|------|------|
| 2026-08-02 | 初版：TSF Start/Update/Commit + 编辑会话 |
| 2026-08-09 | IMM32 兼容层：GCS_COMPSTR / GCS_RESULTSTR / IMN_OPENCANDIDATE |
| 2026-11-15 | 组合串收敛到 `BuildCompositionText` + `SyncComposition`；已选前缀归属引擎 |
| 2026-11-16 | 分段显示属性（confirmed/pending）；UIElement 全局注册 + 数据读取判定兜底；IMM32 组词对齐（前缀进组合串、整词一次提交、退格回退） |
