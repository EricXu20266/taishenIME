# SPEC: 多行候选面板（Root #8，V0.2.14）

> 对应 ARCHITECT.md Root #8「呈现层 — 候选窗口」
> 关联 DEV-TRACKER: 0.2.14 多行候选面板（↓ 展开）

---

## 一、需求

候选窗口支持多行网格布局：默认单行横排（现有），按下 ↓（VK_DOWN）展开为多行，
显示更多候选；再按 ↑（VK_UP）或 Esc 收起。

**用户价值**：候选多时一眼看全，不用反复翻页。

**合约**：
- 单行模式（默认）：现有水平排布，不变
- 多行模式（↓ 展开）：每行固定 N 个候选（N = 每行候选数，默认 5），行高复用候选行高
- 多行时选中项高亮跟随（行/列定位）
- ↓/↑ 键在候选非空时切换展开/收起；Esc 收起
- 多行模式高度自适应（行数 × 行高），宽度 = 每行内容宽
- 多行模式下翻页键仍可用（PgDn/PgUp）

**不做**：
- 候选分行滚动（行数超出窗口滚动）——后续
- 鼠标拖拽调整大小——后续
- 每行候选数可配置——后续（固定 5）

## 二、实现

### CCandidateWindow

```cpp
// 新增状态
bool m_multiRow;           // 多行展开状态
static constexpr int kPerRow = 5;  // 每行候选数

// 方法
void SetMultiRow(bool enabled);   // 切换布局（重算尺寸 + 重绘）
bool IsMultiRow() const;
```

### CalculateSize（多行分支）

```
多行模式：
  行数 rows = ceil(candidates.size() / kPerRow)
  宽度 = 每行内容宽（前 kPerRow 个候选横排宽）
  高度 = pad*2 + pinyinH(若有) + rows * candH
  单行模式：现有逻辑不变
```

### Render（多行分支）

```
候选词按 i → (row = i / kPerRow, col = i % kPerRow) 定位：
  x = padF + col * itemWidth
  y = padY + row * candH
选中高亮跟随行列
```

### 按键接入（tsf_keyevent）

```
VK_DOWN：候选非空 → 展开多行（eat，状态变化）
VK_UP：多行展开 → 收起（eat）；否则透传
Esc：多行展开 → 收起（eat）
```

## 三、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 候选窗口多行状态 + CalculateSize 多行 | candidate_window.h/.cpp | 冒烟测试 |
| 2 | Render 多行网格绘制 | candidate_window.h/.cpp | 冒烟测试 |
| 3 | tsf_keyevent ↓/↑/Esc 展开收起 | tsf_keyevent.cpp | 冒烟测试 |
| 4 | 全链路验证 | — | build + 冒烟 |

## 四、测试用例

- 默认单行布局（候选数 ≤ kPerRow 时单行）
- 候选 > kPerRow → ↓ 展开多行，窗口高度 = rows * 行高
- ↑ 收起回单行
- 选中项在多行模式下高亮正确
