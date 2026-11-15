# SPEC: 呈现层（Root #8）— 自绘 UI 与候选窗

> 对应 ARCHITECT.md Root #8「呈现层 — 长什么样」
> 初版：2026-08-02（0.1.6 Direct2D 候选窗口）
> 现行修订：2026-11-16（候选窗拼音行同源、行内可读性判定、UIElement 兜底）

---

## 一、需求

在光标附近呈现输入状态与候选，零第三方依赖，浅/深色主题跟随系统。

**合约**：

- 候选窗为无边框、置顶、不抢焦点的弹层，内容自绘
- 数据来源唯一：引擎当前候选页 + 拼音显示串
- 拼音行与文档组合串同源，二者不能出现「文档写了但窗里没有」或反之
- 候选窗位置跟随光标；多屏与高 DPI 下坐标正确
- 宿主自己渲染候选时（游戏），不自绘叠窗
- 渲染异常不阻塞输入：窗口创建失败时记录日志并降级，不崩溃

**不做**：

- 跨平台渲染（Skia 预留，当前 Windows 全自绘）
- 候选窗口动画
- 皮肤市场

## 二、组件结构

```
platform/windows/src/
├── candidate_window.cpp      # CCandidateWindow（UIWindow + CandidatePanel）
├── ui_window.cpp             # 无边框置顶窗口基类（消息分发）
├── ui_render.cpp             # Direct2D / DirectWrite 渲染封装
├── ui_control.cpp            # 控件基类（命中测试/布局/绘制）
├── ui_layout.cpp             # 流式布局
├── ui_theme.cpp / theme.cpp  # 主题 token（浅/深/自定义）
├── ui_label/button/edit/...  # 基础控件
├── banner_window.cpp         # 工具栏（中英/简繁/设置入口）
├── settings_window.cpp       # 设置窗
└── dpi_util.cpp              # DPI 与坐标换算
```

候选窗由 `CCandidateWindow` 持有 `CandidatePanel`；面板负责测量、命中测试与绘制，
窗口只负责创建/定位/显隐。

## 三、布局规格

```
+---------------------------------------------+
| 拼音行（小一号，弱化；组词时 = 已选 + 剩余）     |
|  1.候选一  2.候选二  3.候选三  4.候选四 …      |
+---------------------------------------------+
```

- 宽度按内容自适应：拼音行宽 + 候选列宽 + 内边距
- 单行紧凑列，列宽取该列最大项宽；超出屏幕时多行展开
- 选中项高亮圆角矩形；翻页指示显示当前页/总页数
- 字体与字号走主题 token，不硬编码（DPI 缩放由 `dpi_util` 统一换算）

## 四、数据来源与刷新

```
按键 → RefreshState()
  ├─ m_pinyin   ← engine_get_preedit_str（普通输入：原始拼音）
  ├─ m_candidates ← engine_get_candidate_count / engine_get_candidate
  └─ 候选 UIElement MarkUpdated（计数/选中/文本/页码）
        ↓
UpdateCandidateWindow()
  ├─ displayPinyin = m_pinyin
  │    组词态 → BuildCompositionText(engine_syllable_display)
  │             = 已选前缀 + 剩余音节
  ├─ SetInlinePreedit(per-app inline_preedit && g_inlineCompositionReadable)
  ├─ 光标坐标：TSF 编辑会话优先，失败降级鼠标位置
  └─ UpdateState(pinyin, candidates, caretRect, page, totalPages)
```

**拼音行可见性判据**（2026-11-16）：

```
候选窗画拼音行 = per-app inline_preedit 开启 && 组合串在宿主中可读
```

`g_inlineCompositionReadable` 由 `StartComposition` 后的 range 回读探测得出：
读得回文本说明宿主接收了组合串，候选窗不重复画拼音；读不回（游戏自绘聊天框）
则候选窗必须画，否则用户看不到自己打了什么。判据不按进程名特判。

## 五、定位与 DPI

- 优先 TSF 编辑会话取光标屏幕坐标；失败降级当前鼠标位置（不钉左上角）
- 多屏：`MonitorFromPoint` 取目标屏工作区，超出右/下边界时向左/上回缩
- 高 DPI：窗口坐标与字体尺寸都经 `dpi_util` 换算，避免系统缩放下的错位

## 六、宿主自绘与兜底

TSF 候选数据通过 `ITfCandidateListUIElement` 提供（详见 composition/SPEC.md 第六节）：

- 每次 `BeginUIElement` 无条件注册，宿主可读
- 宿主读过候选数据 → 认为它自己会画 → 不再叠自绘窗
- 未读过 → `CCandidateWindow` 兜底
- per-app 配置 `app_self_draw` / `app_no_self_draw` 可强制覆盖判定

IMM32 路径没有 UIElement 概念，候选始终由 `CCandidateWindow` 自绘
（LOL 白名单场景由 GFxIME 读取 `GCS_CANDIDATELIST` 自行渲染）。

## 七、验证清单

| 判据 | 方式 |
|------|------|
| 候选窗跟随光标，不遮挡输入行 | 实机多应用 |
| 组词态拼音行显示「已选前缀 + 剩余音节」 | 实机 + `CandidateWindow: UpdateState` 日志 |
| 宿主不接收组合串时拼音行仍显示 | 游戏宿主实测 |
| 宿主自绘候选时不出现双窗 | `EnsureCandidateUiElement` 日志 |
| 深/浅色主题切换即时生效 | 设置页切换 |
| 多屏 + 125%/150% 缩放位置正确 | 实机 |

## 八、演进记录

| 日期 | 变更 |
|------|------|
| 2026-08-02 | 初版：D2D/DWrite 候选窗，拼音 + 候选 + 翻页 |
| 2026-08-04 | 自研 UI 框架：`ui_*` 控件库，候选窗/工具栏/设置窗统一渲染 |
| 2026-08-08 | 多屏定位与 DPI 对齐；主题系统 |
| 2026-11-13 | 候选 UIElement 全局语义；拼音行不再依赖宿主声明 |
| 2026-11-15 | 候选窗拼音行与组合串同源（组词显示已选前缀） |
| 2026-11-16 | 自绘兜底按「宿主是否读过候选数据」判定；行内可读性回读探测 |
