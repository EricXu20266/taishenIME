# SPEC: 呈现层（Root #8）— Direct2D 候选窗口渲染

> 对应 ARCHITECT.md Root #8「呈现层 — 长什么样」
> 关联 DEV-TRACKER: 0.1.6 Direct2D 候选窗口渲染（一期 Windows 原生）

---

## 一、需求

实现输入法候选窗口：用户在目标应用输入拼音时，光标附近弹出候选词列表，随按键实时刷新。一期用 Direct2D + DirectWrite（零第三方依赖，Windows 原生）。

**合约**：
- 候选窗口为置顶无边框透明窗口（WS_POPUP + WS_EX_TOPMOST + WS_EX_NOACTIVATE + WS_EX_LAYERED）
- 数据来源：TSF 层已通过 FFI 拉取的 `m_pinyin` + `m_candidates`（tsf_module.cpp::RefreshState）
- 定位：TSF 编辑会话中获取当前选区 ITfRange 的屏幕坐标（ITfContextOwnerServices::GetScreenExt）
- 显示逻辑：拼音非空且有候选 → 显示；拼音空 → 隐藏
- 不拦截鼠标（候选窗口仅显示，点击选词是二期；一期选词走数字键/空格）

**不做**：
- 鼠标点击候选词——二期
- 皮肤/主题系统（0.2.4）
- Skia 跨平台渲染——二期
- 候选窗口跟随光标平滑动画——二期

## 二、组件结构

```
platform/windows/
├── include/
│   ├── engine_bridge.h        # 已有
│   ├── tsf_keyevent.h         # 已有
│   └── candidate_window.h     # 新增：候选窗口类声明
├── src/
│   ├── tsf_module.cpp         # 已有，集成候选窗口调用
│   ├── tsf_keyevent.cpp       # 已有
│   └── candidate_window.cpp   # 新增：Direct2D 渲染实现
└── CMakeLists.txt             # 接入新文件 + d2d1/dwrite 链接
```

## 三、接口

### CCandidateWindow（candidate_window.h）

```cpp
class CCandidateWindow {
public:
    CCandidateWindow();
    ~CCandidateWindow();

    // 创建窗口 + D2D 资源（延迟到首次显示，降低冷启动成本）
    bool Initialize();

    // 更新内容并定位显示。candidates 为空或 pinyin 为空时隐藏。
    void UpdateState(const std::string& pinyin,
                     const std::vector<std::string>& candidates,
                     const RECT& caretRect);

    // 隐藏窗口
    void Hide();

    // 当前候选索引（数字键/空格选词时引擎侧已处理，这里仅用于高亮显示）
    void SetSelectedIndex(int index);

private:
    bool CreateDeviceResources();   // ID2D1Factory/HwndRenderTarget/Brush/Font
    void Render();                  // 绘制背景、拼音、候选词
    void PositionWindow(const RECT& caretRect); // 计算窗口位置（光标下方）

    HWND m_hwnd;
    // D2D 资源
    ID2D1Factory* m_pD2DFactory;
    ID2D1HwndRenderTarget* m_pRenderTarget;
    ID2D1SolidColorBrush* m_pBgBrush;
    ID2D1SolidColorBrush* m_pTextBrush;
    ID2D1SolidColorBrush* m_pHighlightBrush;
    IDWriteFactory* m_pDWriteFactory;
    IDWriteTextFormat* m_pTextFormat;
    // 状态
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    int m_selectedIndex;
    bool m_visible;
};
```

### 布局规格

```
+---------------------------------------+
| 拼音串 (16px, 灰色)                    |   ← 上边距 6px
|  1.候选一  2.候选二  3.候选三  4.候选四  |   ← 水平排布，16px
+---------------------------------------+
```

- 窗口宽度 = 拼音串宽 + 候选行宽 + 边距，自适应
- 每候选 `序号.词` 占宽 = 序号(14px) + 词宽 + 词间距(16px)
- 选中候选高亮：浅蓝背景圆角矩形
- 背景：半透明深色（ARGB 0xF22E2E2E），圆角 4px
- 字体：微软雅黑，16px（DW_CF_HEIGHT 规格）

### 定位规则

```
窗口左上角 = (caretRect.left, caretRect.bottom + 4)
屏幕超界修正：窗口超出右/下边界时向左/上回缩
```

## 四、数据流

```
OnKeyDown(VK_*)
  → tsf_keyevent::HandleKeyDown → FFI engine_process_key
  → CTextService::OnKeyDown
    → RefreshState() 拉取 m_pinyin + m_candidates
    → 请求编辑会话（TF_ES_SYNC）获取光标屏幕坐标 caretRect
      → ITfContext::GetSelection → ITfRange
      → ITfContextOwnerServices::GetScreenExt(range, &caretRect)
    → m_pCandidateWindow->UpdateState(m_pinyin, m_candidates, caretRect)
      → 候选空？Hide() : PositionWindow() + Render()
```

## 五、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | candidate_window.h 类声明 | include/candidate_window.h | 编译 |
| 2 | candidate_window.cpp：窗口创建 + D2D 资源 + Render | src/candidate_window.cpp | 编译 |
| 3 | 布局计算：PositionWindow + 宽度自适应 | src/candidate_window.cpp | 编译 |
| 4 | tsf_module 集成：OnKeyDown 刷新 + 光标定位 | src/tsf_module.cpp | 编译 |
| 5 | CMake 接入 + d2d1/dwrite 链接 | CMakeLists.txt | DLL 构建成功 |
| 6 | 验证：DLL 导出 + 注册 + 引擎回归 | — | 构建通过 + 10 测试 + regsvr32 |

## 六、风险与依赖

- **真机验证**：渲染效果需用户启用输入法后实测——本步骤交付「可构建、可注册、逻辑完整」的 DLL，视觉效果由用户确认
- **光标定位失败降级**：编辑会话失败或 GetScreenExt 失败时，窗口显示在屏幕左上角(0,0)并记录日志，不崩溃
- **依赖 0.1.5**：候选数据来自已完成的 RefreshState；选词上屏（0.1.7）本期只更新高亮索引，不落盘
- **D2D 延迟初始化**：首次 UpdateState 时才 CreateDeviceResources，避免输入法加载即初始化开销
