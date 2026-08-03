# SPEC: 深色模式跟随系统（Root #8，V0.2.20）

> 对应 ARCHITECT.md Root #8「候选窗口」+ Root #7「配置系统」
> 关联 DEV-TRACKER: 0.2.20 深色模式跟随系统
> Windows 10 1809+ 支持（Windows 设置 → 个性化 → 颜色 → 应用模式）

---

## 一、需求

系统深色/浅色模式变化时，候选窗 + 状态横幅自动切换主题色。

**用户价值**：系统深色模式用户不再被刺眼白底/黑底窗口打扰。

**合约**：
- 检测方式：注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
  （0=深色，1=浅色）；同时监听 `WM_SETTINGCHANGE` 实时切换
- 主题策略：**用户显式配置 theme_* 时固定使用**（不跟随系统）；
  **未配置时跟随系统**——深色 → 深色主题（现默认），浅色 → 浅色主题
- 浅色主题配色：白底黑字（与深色镜像）
- 切换时机：窗口创建时检测 + `WM_SETTINGCHANGE` 实时重绘

**不做**：
- 自定义深浅色主题配色——用默认两套
- 跟随「标题栏/任务栏」模式（AppsUseLightTheme 已覆盖）

## 二、数据模型

```
无新配置键——沿用 theme_*（用户显式配置优先）+ 新增系统检测
```

### 浅色主题默认值

```
bg: 0xF5F5F5 (白底)
text: 0x1A1A1A (黑字)
highlight: 0x1E6FFF (蓝色选中，同深色)
dim: 0x8A8A8A (灰序号)
```

## 三、接口

### 平台层

```cpp
// 检测系统应用模式：0=深色，1=浅色，-1=未知（读注册表失败）
int GetSystemAppTheme();

// 应用主题策略（候选窗/横幅共用）：
// 用户显式配置 theme_* → 固定用户主题
// 否则 → 按系统模式选 深色/浅色 默认
void ApplyThemeWithSystem(CandidateTheme& out, const ImeConfig& cfg,
                          bool userExplicit);
```

### 窗口处理

```
WM_SETTINGCHANGE + 参数含 "ImmersiveColorSet" 或 "AppsUseLightTheme"
  → 重新检测系统主题 → 若未显式配置 → 切换默认主题 → 重绘
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | GetSystemAppTheme() 注册表检测 | theme.cpp（新建）| 编译 |
| 2 | ApplyThemeWithSystem 策略 | theme.cpp | 编译 |
| 3 | candidate_window WM_SETTINGCHANGE 监听 | candidate_window.cpp | 编译 |
| 4 | banner_window 同步 | banner_window.cpp | 编译 |
| 5 | 全链路验证 | — | build + test |

## 五、测试用例

- 未配置 theme_* + 系统深色 → 深色主题
- 未配置 theme_* + 系统浅色 → 浅色主题
- 配置 theme_bg 后 → 固定用户主题（不随系统）
- WM_SETTINGCHANGE 触发 → 实时切换
