# SPEC: 皮肤/主题系统（Root #8，V0.2.4）

> 对应 ARCHITECT.md Root #8「呈现层 — 候选窗口」
> 关联 DEV-TRACKER: 0.2.4 皮肤/主题系统
> 前置：0.1.6 Direct2D 候选窗口

---

## 一、需求

候选窗口颜色可配置：用户可通过 config.ini 调整背景/文本/高亮/序号四色，实现明暗主题切换。

**用户价值**：跟随系统深色/浅色偏好，或自定义配色（个人化）。

**合约**：
- 四个可配颜色：背景、主文本、选中高亮、序号/页码（灰色）
- config.ini 用 HEX 格式（RRGGBB，如 2E2E2E）
- 未配置的颜色回退默认（当前深色主题）
- 颜色变更需重启输入法生效（配置加载在 ActivateEx）
- 默认主题 = 当前深色（2E2E2E/E8E8E8/1E6FFF/9A9A9A）

**不做**：
- 多主题切换 UI（托盘菜单）——0.2.13 托盘做时考虑
- 字体/字号/圆角可配——后续
- 图片皮肤/动画——后续

## 二、数据模型

### CandidateTheme 结构（C++）

```cpp
struct CandidateTheme {
    D2D1_COLOR_F bg;         // 背景
    D2D1_COLOR_F text;       // 主文本
    D2D1_COLOR_F highlight;  // 选中高亮
    D2D1_COLOR_F dim;        // 序号/页码
    static CandidateTheme Default();  // 当前深色默认
};
```

### config.ini

```
theme_bg=2E2E2E        # 背景（HEX RRGGBB）
theme_text=E8E8E8      # 主文本
theme_highlight=1E6FFF # 选中高亮
theme_dim=9A9A9A       # 序号/页码
```

## 三、接口

### CCandidateWindow

```cpp
// 新增
void SetTheme(const CandidateTheme& theme);  // 渲染前应用（重建画刷）
```

### config_reader

```cpp
// ImeConfig 新增 theme 字段
CandidateTheme theme;  // 默认 Default()

// 解析 HEX "RRGGBB" → D2D1_COLOR_F（alpha=1）
```

### tsf_module

ActivateEx 读取配置后 `m_candidateWindow.SetTheme(cfg.theme)`。

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | CandidateTheme 结构 + Default + HEX 解析 | config_reader.h/.cpp | 冒烟测试 |
| 2 | CCandidateWindow::SetTheme + 渲染用主题色 | candidate_window.h/.cpp | 冒烟测试 |
| 3 | tsf_module 应用主题 | tsf_module.cpp | 冒烟测试 |
| 4 | config.ini.example 注释 | config.ini.example | — |
| 5 | 全链路验证 | — | build + 冒烟 |

## 五、测试用例

- 默认主题 = 深色（当前配色不变）
- 配置浅色主题（F5F5F5/333333/0078D4/999999）→ 窗口用新色
- 非法 HEX 值 → 回退该颜色默认
- 未配置 → 全部默认
