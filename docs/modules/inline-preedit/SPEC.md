# SPEC: 候选窗口行内预编辑（Root #8，V0.2.18）

> 对应 ARCHITECT.md Root #8「候选窗口」+ docs/modules/composition/SPEC.md
> 关联 DEV-TRACKER: 0.2.18 候选窗口行内预编辑（inline_preedit）

---

## 一、需求

拼音串显示在光标位置（组合文本），而非候选窗内重复显示。

**现状**（已部分实现）：组合机制已把拼音写入光标位置（`StartComposition(pinyin)`），
但候选窗口顶部**也**绘制拼音行 → 光标处和候选窗顶部重复显示。

**本期目标**：
- 新增 `inline_preedit` 配置（默认开，对齐 Weasel/搜狗）
- 开启：候选窗不绘制拼音行（行内已有，避免重复）；拼音仍写入组合
- 关闭：候选窗顶部显示拼音行（回退旧行为——部分应用组合显示异常时可用）

**用户价值**：拼音跟随光标（打字位置所见即所得），候选窗更紧凑。

**不做**：
- ITfContext::GetTextExt 精确光标追踪改造（现有 GetCaretRectFromContext 已可用，候选窗跟随光标）
- 高亮组合内已匹配音节（TSF 无标准 API，Weasel 亦未做）

## 二、数据模型

### ImeConfig 新增

```cpp
bool inline_preedit = true;  // 行内预编辑（默认开）
```

### config.ini

```
inline_preedit=1    # 1=拼音行内显示（候选窗不重复），0=候选窗显示拼音行
```

## 三、接口

### candidate_window

```cpp
void SetInlinePreedit(bool enable);  // true=不绘制拼音行；false=绘制
bool m_inlinePreedit = true;         // 成员
```

### Render() / CalculateSize()

```
m_inlinePreedit 时跳过拼音行（高度、绘制）
```

### tsf_module

```
ActivateEx：读 cfg.inline_preedit → m_candidateWindow.SetInlinePreedit(...)
```

## 四、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | config_reader：inline_preedit 解析 | config_reader.cpp/.h | 编译 |
| 2 | candidate_window：SetInlinePreedit + 条件跳过拼音行 | candidate_window.cpp/.h | 编译 |
| 3 | tsf_module：ActivateEx 传配置 | tsf_module.cpp | 编译 |
| 4 | 全链路验证 | — | build + test |

## 五、测试用例

- inline_preedit=1（默认）：候选窗无拼音行，高度更紧凑
- inline_preedit=0：候选窗显示拼音行（旧行为）
- 组合文本仍含拼音（光标位置显示不受影响）
