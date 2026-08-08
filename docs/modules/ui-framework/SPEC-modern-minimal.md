# SPEC: 极简扁平视觉现代化（V0.3.6，微信输入法式）

> 对应 ARCHITECT.md Root #8「呈现层 — 长什么样」
> 关联 DEV-TRACKER: 0.3.6（在 0.3.0-0.3.4 自研窗体系统基础上做视觉打磨）
> 前置：0.3.0-0.3.4 已完成（UIWindow + 控件库 + 候选窗/工具栏/设置三组件迁移）
> 状态：设计确认（2026-08-08，用户确认：极简扁平方向 + 候选窗不透明圆角卡片 + toggle 开关 + accent 保持蓝色）

---

## 一、需求

### 1.1 现状痛点

0.3.0-0.3.4 完成了 UI 架构统一（D2D 自研窗体系统），但视觉仍偏"工程味"：

| 组件 | 现状 | 问题 |
|------|------|------|
| 候选窗 | 透明背景 + 1px 细边框 + 圆角 4px + 选中整列 accent 底 | 无卡片感；整列高亮太重；序号与正文同号无层次 |
| 设置页 | 560×440 平铺表单 + 左侧导航整块 accent 底 + 方块 checkbox | 无分组卡片；导航选中态过重；控件样式陈旧（方块勾选框）；间距紧凑 |

### 1.2 目标（用户确认的三项决策）

- **风格**：极简扁平（对标微信输入法 Windows）
- **候选窗**：不透明圆角卡片背景（白底/深灰底）+ 细边框，选中项胶囊高亮，拼音串弱化
- **设置页**：卡片分组布局 + 左侧导航左边界条选中态 + checkbox 升级为 iOS 风格 toggle 开关
- **强调色**：保持现有蓝色系（深色 4C8DFF / 浅色 0078D4）

### 1.3 非目标

- 工具栏（banner）视觉不动（本期聚焦候选窗 + 设置页）
- 动画/过渡（平滑动画接口已预留，本期不做）
- 毛玻璃/亚克力半透明（用户选择不透明卡片，规避 TSF 兼容风险）
- 皮肤系统/用户自定义主题导入

## 二、候选窗设计

### 2.1 视觉规格

| 项 | 当前 | 目标 |
|----|------|------|
| 背景 | 透明（仅边框） | **不透明圆角卡片**：浅色 `#FFFFFF` / 深色 `#1E1E1E`，圆角 **8px**，边框 1px（浅色 `#E5E5E5` / 深色 `#333333`） |
| 拼音串 | 正文同号、dim 色 | 小一号（fontSize - 2px）、dim 色、顶部留白（与卡片边缘 8px） |
| 候选序号 | 与正文同号 | 弱化：小一号灰字（dim），选中时白色；保持 label_format 可配 |
| 选中项 | 整列 accent 圆角矩形 | **胶囊高亮**：仅选中词条绘制 accent 圆角底（圆角 = 高亮圆角配置），序号 + 文字变白 |
| 悬停项 | 整列 mark 色 | 浅灰胶囊（hoverBg），圆角同高亮 |
| 内边距 | 可配（默认 8） | 默认 10-12px（通过 config padding 可调） |
| 词间距 | 可配（默认 14） | 默认 12px（通过 config candidate_spacing 可调） |
| 高亮圆角 | 可配（默认 3） | 默认 6px（config hilite_corner_radius 可调） |

### 2.2 绘制逻辑变更（CandidatePanel::Draw）

```
原：FillRoundedRect(窗口, corner, 无)  →  仅 DrawRoundedRect(边框)
新：FillRoundedRect(窗口, corner*scale, theme.bg 不透明)  +  DrawRoundedRect(边框)
```

- 背景填充：`r.FillRoundedRect(rc, m_corner * scale, m_theme.bg)`（bg 已是不透明卡片色）
- 选中胶囊：`FillRoundedRect({x, y, x+itemW, y+candH}, m_hilite*scale, m_theme.highlight_bg)` —— 保持现有绘制，仅调整颜色与圆角 token
- 拼音：`DrawText(..., m_fontSize - 2, m_theme.dim)`
- 序号：`DrawText(label, ..., m_fontSize - 2, selected ? m_theme.highlight_label : m_theme.label)`

### 2.3 主题色映射（CandidateTheme 复用，不新增字段）

| CandidateTheme 字段 | 浅色值 | 深色值 |
|---------------------|--------|--------|
| bg（卡片背景） | FFFFFF | 1E1E1E |
| text | 1A1A1A | E8E8E8 |
| label（序号） | 8A8A8A（弱化灰） | 9A9A9A |
| comment | 6B6B6B | 8A8A8A |
| border | E5E5E5 | 333333 |
| highlight_bg（胶囊） | 0078D4 | 4C8DFF |
| highlight_text | FFFFFF | FFFFFF |
| highlight_label | FFFFFF | FFFFFF |
| dim | 9A9A9A | 8A8A8A |
| mark（悬停） | E8F0FE（浅蓝灰） | 3A3A3A |

> 默认主题（CandidateTheme::Default）为深色；LightTheme() 为浅色。config 显式配置仍可覆盖。

## 三、设置页设计

### 3.1 窗口与整体骨架

| 项 | 当前 | 目标 |
|----|------|------|
| 窗口尺寸 | 560×440 | **640×480**（Run() 内 Create 参数） |
| 标题栏 | 36px cardBg + ✕ | 保持 36px；✕ 增加 hover 状态（浅灰圆角背景） |
| 左侧导航 | 选中 = 整块 accent 底白字 | **accent 左边界条（4px 竖条）+ hoverBg 背景 + accent 色文字**；导航项圆角 6px、间距 6px |
| 右侧内容 | 表单平铺 | **卡片分组**：每页按逻辑分类包圆角卡片（cardBg + 圆角 8px + 内边距 16px），卡片内分组标题（fontSizeTitle 粗体）+ 控件行；卡片间距 14px |
| 底部按钮 | 6px 圆角 | 圆角 8px；主按钮（确定）accent 底，次按钮 hoverBg 底 |

### 3.2 页面卡片分组规划

| 页 | 卡片 | 内容 |
|----|------|------|
| 基础 | 候选设置 | 候选数量 / 字体 / 字号 / 标签格式 |
| 基础 | 输入行为 | 行内预编辑 |
| 输入 | 输入模式 | 模糊音 / 智能纠错 / 中英混输 / 简繁转换 / 双拼模式 + 双拼方案 / 快捷短语 / 英文标点 / Emoji |
| 输入 | 智能候选 | 候选排序 / 上下文联想 |
| 外观 | 主题 | 主题模式 / 10 色配色器 |
| 外观 | 窗口 | 圆角 / 高亮圆角 / 内边距 / 候选间距 |
| 高级 | 应用级配置 | 应用列表（+ 添加程序） |
| 高级 | 路径 | 系统词库 / 用户词库 / 短语路径 |

### 3.3 Toggle 开关（UICheckBox 改造）

- 新增 `void SetSwitchMode(bool)`——**不改类名**，复用 UICheckBox 交互逻辑（OnClick/OnKeyDown/onChanged）
- Switch 渲染（switch 模式下替代原勾选框绘制）：
  - 尺寸：40×22 胶囊（圆角 = 高 / 2 = 11px）
  - 关闭态：轨道 = border/hoverBg 色，滑块（18px 圆）靠左，白色
  - 开启态：轨道 = accent 色，滑块靠右，白色
  - 文字渲染逻辑不变（右侧）
- 设置页所有 `CheckRow()` 调用处改为 `SetSwitchMode(true)`

### 3.4 主题 token 扩展（ui_theme.h/cpp）

```cpp
struct UITheme {
    // 新增
    D2D1_COLOR_F switchTrackOn;    // 开关开启轨道（= accent）
    D2D1_COLOR_F switchTrackOff;   // 开关关闭轨道（浅灰）
    D2D1_COLOR_F switchThumb;      // 滑块（白）
    // 调整几何默认值
    // cornerRadius: 4 → 8；cardRadius: 8 → 10
};
```

深/浅两套同步扩展；UIThemeCurrent/跟随系统逻辑不变。

## 四、实现计划

| 步骤 | 内容 | 文件 | 验证 |
|------|------|------|------|
| 1 | UITheme 扩展 switch token + 圆角/配色调优 | ui_theme.h/.cpp | 编译 |
| 2 | UICheckBox 增加 SetSwitchMode + switch 渲染 | ui_checkbox.h/.cpp | test_ui_controls 冒烟 |
| 3 | 候选窗：不透明卡片背景 + 拼音/序号弱化 + 胶囊高亮色 | candidate_window.cpp、theme.cpp（深浅默认色） | test_candidate_window 视觉回归 |
| 4 | 设置页：640×480 + 导航左边界条 + 卡片分组 + toggle 接入 | settings_window.cpp | test_settings_dialog 预览 |
| 5 | 回归：cargo test + biome check + 全部测试 exe | — | 全绿 |

**预计工时**：10h（步骤 3/4 为主）

## 五、风险与依赖

- **候选窗视觉回归**：背景从透明变不透明是主观视觉变化（用户明确要求）；选中样式从整列 → 胶囊只改绘制，命中逻辑（CandidateAt）不变
- **Toggle 渲染**：新增渲染分支不影响原 checkbox 模式（工具栏/其他使用处不受影响）
- **卡片分组高度溢出**：外观页（10 色 + 4 数字行）与高级页（应用列表 + 3 路径）内容可能超出 480px——页面根挂 UIScrollBar（已有控件），内容可滚动
- **配置兼容**：候选窗视觉相关 config 键（corner_radius/hilite_corner_radius/padding/candidate_spacing/theme_*）全部保留，语义不变

