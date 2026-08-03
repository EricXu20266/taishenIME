# SPEC: 候选窗字体/字号可配（Root #8 #7，V0.2.21）

> 对应 ARCHITECT.md Root #8「候选窗口」+ Root #7「配置系统」
> 关联 DEV-TRACKER: 0.2.21 候选窗字体/字号可配

---

## 一、需求

config.ini 新增 `font_face` / `font_size`，候选窗 DWrite 读取配置。

| 配置键 | 默认值 | 说明 |
|--------|--------|------|
| font_face | Microsoft YaHei | 字体名（任意已安装字体） |
| font_size | 16 | 正文字号（px，逻辑像素，DPI 自适应缩放） |

**用户价值**：高分屏/视觉偏好用户可自定义候选窗字体与大小。

**合约**：
- font_face 非法/不存在 → 回退 Microsoft YaHei（DWrite 自动回退）
- font_size 范围 12-32，非法 → 回退 16
- 字号变化影响候选行高/拼音行高（布局随字号缩放，非仅字体）
- DPI 缩放继续生效（font_size × dpiScale）

**不做**：
- 运行时装 UI（改配置需重启输入法）——后续
- 拼音行独立字号——拼音字号随正文比例缩放

## 二、数据模型

### ImeConfig 新增

```cpp
std::wstring font_face = L"Microsoft YaHei";  // 字体名
float font_size = 16.0f;                       // 正文字号（px）
```

### 布局联动

```
候选行高 = font_size + 6（留字距）
拼音行高 = font_size * 0.85（约 14px @ 16 字号）
字号缩放 = font_size / 16.0f（基准缩放，叠加 DPI 缩放）
```

## 三、接口

### config_reader.cpp

```
font_face=Microsoft YaHei   # 字体名（UTF-8 中文兼容）
font_size=16                # 正文字号（12-32）
```

### candidate_window

```
CCandidateWindow::SetFont(face, size)  // 存成员 + 重建 TextFormat
CreateDeviceResources() 使用 m_fontFace / m_fontSize 替代常量 kFontSize
CalculateSize() 行高随字号缩放
Render() 绘制使用缩放后字号
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | config_reader：font_face/font_size 解析 | config_reader.cpp/.h | 编译 |
| 2 | candidate_window：SetFont + 布局缩放 | candidate_window.cpp/.h | 编译 |
| 3 | tsf_module：ActivateEx 传配置 | tsf_module.cpp | 编译 |
| 4 | 全链路验证 | — | build + test |

## 五、测试用例

- 默认配置 → Microsoft YaHei 16px（与现状一致）
- font_size=20 → 行高/字号放大
- font_size=5 → 回退 16（范围外）
- font_face=楷体 → 使用楷体（若系统存在）
