# SPEC: 自研窗体系统（Root #8）— UI 全面重构

> 对应 ARCHITECT.md Root #8「呈现层 — 长什么样」
> 关联 DEV-TRACKER: V0.3（0.3.0 底座 → 0.3.5 验证）
> 状态：架构设计（2026-08-04，用户确认：全量一步到位 + 现代化重设计）

---

## 一、需求

### 1.1 现状痛点

泰深当前 UI 由三套互不相干的技术拼成：

| 组件 | 现状 | 技术 | 问题 |
|------|------|------|------|
| 候选窗 | CCandidateWindow（31KB） | Direct2D + DirectWrite 自绘 | 渲染能力已验证，但窗口/主题/事件逻辑与控件耦合 |
| 工具栏 | CBannerWindow（15KB） | GDI 全量绘制（OnPaint） | 与候选窗两套渲染栈；按钮态/悬停全靠手写 |
| 设置对话框 | settings_dialog + settings.rc（21KB） | Win32 对话框资源 + 标准控件 | .rc 依赖 BOM（B-10 乱码坑）；标准控件样式陈旧，无法深度定制 |

三套 UI 无法共享主题、控件、事件、布局——每次加一个视觉特性要改三处。

### 1.2 目标

- **统一渲染引擎**：全部 UI 组件基于 Direct2D + DirectWrite（候选窗已验证成熟、零第三方依赖、抗锯齿、硬件加速）
- **自研控件库**：Label/Button/CheckBox/ComboBox/Edit/Tab/ColorPicker 全自绘，摆脱 Win32 标准控件与 .rc 资源
- **现代化设置界面**：左侧导航 + 右侧内容面板、卡片式分组、圆角/阴影、深浅主题跟随系统（对标搜狗/微信输入法设置）
- **全量迁移**：候选窗、工具栏、设置对话框三组件全部迁移到新框架（用户确认：一步到位）

### 1.3 非目标

- 跨平台渲染（Skia 二期仍保留，本框架为 Windows 原生）
- 动画/过渡效果（平滑动画预留接口，本期不做）
- 皮肤商店/用户自定义主题导入（主题 token 化已支持，皮肤包后续）
- 触摸/触笔支持

## 二、架构分层

```
┌─────────────────────────────────────────────┐
│  应用组件层                                   │
│  CCandidateWindow / CBannerWindow /          │
│  CSettingsWindow（现代化设置窗体）             │
├─────────────────────────────────────────────┤
│  控件库层  ui_control.h                      │
│  Label  Button  CheckBox  ComboBox  Edit    │
│  Tab  ColorPicker  ScrollBar                 │
├─────────────────────────────────────────────┤
│  布局层    ui_layout.h                       │
│  VBox / HBox / Grid（流式 + 绝对坐标混合）     │
├─────────────────────────────────────────────┤
│  窗口框架层  ui_window.h                     │
│  无边框透明窗口封装 / 消息分发 / 渲染循环      │
├─────────────────────────────────────────────┤
│  渲染基础层  ui_render.h                     │
│  D2D 工厂 / 渲染目标管理 / 画刷缓存 / 字体管理 │
│  UITheme token（深/浅两套，跟随系统）          │
└─────────────────────────────────────────────┘
```

**依赖方向**：上层依赖下层，控件库不感知应用组件；应用组件只组合控件与布局。

## 三、模块设计

### 3.1 渲染基础层 `ui_render.h/cpp`

从候选窗抽取的 D2D 公共能力：

```cpp
class UIRenderer {
public:
    bool Ensure(HWND hwnd);                 // 延迟创建工厂+渲染目标
    void BeginDraw(); void EndDraw();
    void FillRoundedRect(const D2D1_RECT_F& rc, float radius, D2D1_COLOR_F color);
    void DrawText(const std::wstring& text, const D2D1_RECT_F& rc,
                  float size, D2D1_COLOR_F color, bool bold = false);
    D2D1_SIZE_F MeasureText(const std::wstring& text, float size);
    // 画刷缓存：同色不重建
    ID2D1SolidColorBrush* Brush(D2D1_COLOR_F color);
private:
    ID2D1Factory* m_factory;
    ID2D1HwndRenderTarget* m_rt;
    IDWriteFactory* m_dwrite;
    std::map<DWORD, ID2D1SolidColorBrush*> m_brushes;  // key: COLORREF
    std::map<std::pair<std::wstring,float>, IDWriteTextFormat*> m_formats;
};
```

要点：
- 画刷/字体格式按内容缓存，避免每次绘制重建（候选窗已验证的模式）
- 渲染目标随 WM_SIZE/DPI 变化重建，窗口移动不重建（HWND 渲染目标天然处理）
- DPI 感知：Per-Monitor V2，所有尺寸按 dpiScale 换算

### 3.2 主题 token `ui_theme.h`

```cpp
struct UITheme {
    // 颜色
    D2D1_COLOR_F bg;            // 窗口背景
    D2D1_COLOR_F cardBg;        // 卡片背景
    D2D1_COLOR_F text;          // 主文字
    D2D1_COLOR_F textDim;       // 次要文字
    D2D1_COLOR_F accent;        // 强调色（选中/主按钮）
    D2D1_COLOR_F accentText;    // 强调色上文字
    D2D1_COLOR_F border;        // 边框
    D2D1_COLOR_F hoverBg;       // 悬停背景
    D2D1_COLOR_F pressedBg;     // 按下背景
    D2D1_COLOR_F checkmark;     // 复选框勾
    // 几何
    float cornerRadius;         // 控件圆角
    float cardRadius;           // 卡片圆角
    int   padding;              // 页面内边距
    int   gap;                  // 控件间距
    // 字号
    float fontSize;             // 正文
    float fontSizeTitle;        // 分组标题
};
UITheme UIThemeDark();  UITheme UILightTheme();
UITheme UIThemeCurrent();       // 跟随系统（AppsUseLightTheme）
void UIThemeFollowSystem(bool follow);  // WM_SETTINGCHANGE 时重取
```

深色主题参考当前候选窗深色（0xF22E2E2E 背景族）；浅色参考 Windows 11 风格（白底 + 浅灰卡片 + 蓝强调）。

### 3.3 窗口框架 `ui_window.h/cpp`

```cpp
class UIWindow {
public:
    virtual ~UIWindow();
    bool Create(const std::wstring& title, int w, int h,
                DWORD exStyle, bool noActivate);
    void Show(); void Hide(); bool IsVisible();
    void SetTheme(const UITheme& t);
    void SetRoot(UIControl* root);      // 控件树根
    void Close(int result = 0);         // 模态结束时调用
    // 模态运行（设置窗体用；候选窗/工具栏不走模态）
    int RunModal();                     // 内部 GetMessage 循环

protected:
    virtual LRESULT HandleMessage(UINT msg, WPARAM wp, LPARAM lp);
    virtual void OnRender(UIRenderer& r);       // 默认清背景
    // 事件回调（子类覆写）
    virtual void OnKeyDown(int vk, bool ctrl, bool shift, bool alt);
    // 控件树消息分发：命中检测 → OnMouseDown/OnMouseMove/OnClick/OnKeyDown
    void DispatchMouse(UINT msg, LPARAM lp);
    void DispatchKey(UINT msg, WPARAM wp);

    HWND m_hwnd;
    UIRenderer m_renderer;
    UITheme m_theme;
    UIControl* m_root;      // 控件树根（负责布局+绘制+命中）
    UIControl* m_focus;     // 当前焦点控件
    bool m_modal;
};
```

窗口风格：
- 候选窗/工具栏：WS_POPUP + WS_EX_TOPMOST + WS_EX_NOACTIVATE + WS_EX_TOOLWINDOW（无边框不抢焦点）
- 设置窗体：WS_POPUP + WS_CAPTION 自绘标题栏 + 模态（RunModal）
- 窗口类注册：每个类独立 hCursor/hIcon，WndProc 统一转 UIWindow*

消息分发规则：
- WM_PAINT → UIRenderer::Ensure → BeginDraw → 控件树 Draw → EndDraw
- WM_MOUSEMOVE/LBUTTONDOWN/LBUTTONUP → 命中检测（自上而下）→ 控件状态更新 + 重绘
- WM_KEYDOWN → 焦点控件优先，未处理则窗口级
- WM_SETTINGCHANGE → 主题跟随系统重取 → 全树重绘

### 3.4 控件基类 `ui_control.h/cpp`

```cpp
enum class UIState { Normal, Hover, Pressed, Disabled, Focused };

class UIControl {
public:
    UIControl() = default;
    virtual ~UIControl() = default;

    void SetRect(const RECT& rc);       // 绝对坐标（布局容器负责分配）
    RECT Rect() const;
    void SetVisible(bool v); void SetEnabled(bool e);
    void SetParent(UIControl* p);       // 布局/命中用

    virtual void Draw(UIRenderer& r, const UITheme& t) = 0;
    virtual bool HitTest(int x, int y) const;   // 默认矩形命中
    // 交互（UIWindow 分发进来，默认空实现）
    virtual void OnMouseMove(int x, int y);
    virtual void OnMouseLeave();
    virtual void OnMouseDown(int x, int y, bool left);
    virtual void OnMouseUp(int x, int y, bool left);
    virtual void OnClick(int x, int y);
    virtual void OnKeyDown(int vk, bool ctrl, bool shift, bool alt);
    virtual void OnFocus(bool focused);

    bool IsEnabled() const; UIState State() const;
    void SetId(int id); int Id() const;     // 应用层区分控件

protected:
    void Invalidate();  // 触发所属窗口重绘

    RECT m_rect{};
    UIControl* m_parent = nullptr;
    UIState m_state = UIState::Normal;
    bool m_visible = true; bool m_enabled = true;
    bool m_hovered = false; bool m_focused = false;
    int m_id = 0;
};
```

状态机：Normal → Hover（鼠标悬停）→ Pressed（按下）→ Disabled（禁用）；Focused 独立维度。
所有交互变更自动 Invalidate → 所在窗口 WM_PAINT → 全树重绘（小窗口树，全量重绘成本可忽略）。

### 3.5 控件库

| 控件 | 行为 | 关键实现 |
|------|------|---------|
| UILabel | 只读文本 | DrawText + 可选对齐（左/中/右），支持换行 |
| UIButton | 点击 | 圆角矩形 + 状态底色（accent 主按钮/hoverBg 普通按钮）；文字居中；Click 回调 |
| UICheckBox | 开关 | 左侧圆角方块 + 勾（accent），右侧文字；点击切换 + OnChanged(int checked) |
| UIComboBox | 下拉单选 | 收起态：圆角框 + 当前项 + ▾ 箭头；展开态：弹出层（同窗口内绘制下拉面板）列出全部项，命中选择；回车/空格同点击 |
| UIEdit | 单行文本 | 圆角框 + 光标闪烁（定时器）+ 文字渲染；键盘输入：可见字符/退格/删除/方向键/Home/End；IME 组合输入（WM_IME_* + Composition）——复用 TSF 组合经验；可配数字校验（仅数字/范围） |
| UITab | 标签页 | 顶部横排标签（选中下划线 accent）+ 内容区；点击切换 |
| UIColorSwatch | 颜色选择 | 色块 + 十六进制文本；点击弹出自绘色板（预设 16×8 网格 + 自定义 RGB 滑杆）；OnColorChanged(D2D1_COLOR_F) |
| UIScrollBar | 滚动 | 细滚动条，拖拽/滚轮；内容超出时出现（设置页内容区用） |

**IME 组合输入（UIEdit）**：TSF 输入法自己的编辑框是焦点应用侧，UIEdit 是设置窗体内的编辑框——用户在设置窗体的编辑框里输入中文时，泰深作为 IME 向自己上屏（TSF 组合流程：GetSelection → InsertAtSelection）。关键：编辑框必须正确处理 WM_IME_STARTCOMPOSITION/COMPOSITION/ENDCOMPOSITION 与 WM_CHAR 双路径，光标位置随组合文本移动。

### 3.6 布局 `ui_layout.h/cpp`

```cpp
class UILayout : public UIControl {      // 布局也是控件（可嵌套）
public:
    enum class Dir { V, H, Grid };
    void SetDir(Dir d); void SetGap(int gap); void SetPadding(int pad);
    void Add(UIControl* c);             // 子控件
    void Layout();                       // 重算子控件 Rect（父 Rect 变化时调用）
protected:
    void Draw(UIRenderer& r, const UITheme& t) override { /* 透明，仅布局 */ }
    std::vector<UIControl*> m_children;
};
```

- VBox：子控件垂直堆叠，固定高度控件（Label/Button）按内容高，弹性控件（Edit/ComboBox）按剩余空间
- HBox：水平堆叠（设置页底部按钮栏）
- Grid：等宽列（配色器色板、应用级配置列表行）
- 绝对坐标仍可用：SetRect 直接指定（复杂定制区域）

### 3.7 应用组件迁移

**CCandidateWindow → 候选窗（0.3.2）**
- 窗口：UIWindow（WS_EX_TOPMOST + NOACTIVATE + TOOLWINDOW）
- 内容：根控件 = 自定义 UIControl（候选行绘制，非标准控件——保持现有布局算法：拼音行/候选行/翻页指示/多行网格/高亮/悬停/点击/滚轮）
- 主题：UITheme 替换 CandidateTheme（或适配层，避免 config 结构大改）
- 保留全部现有功能：鼠标点击选词、滚轮翻页、多行展开、标签格式、字号配置

**CBannerWindow → 工具栏（0.3.3）**
- 窗口：UIWindow（TOPMOST + NOACTIVATE）
- 内容：HBox + 4 个 UIButton（中英/简繁/双拼/设置）
- 按钮状态：文字 + 当前态高亮（accent 底 = 功能开启）
- 保留：前台线程激活判定、托盘开关、右下角定位

**settings_dialog → CSettingsWindow（0.3.4，现代化）**
- 窗口：UIWindow 模态（RunModal）+ 自绘标题栏（标题 + 关闭按钮 + 可拖动）
- 布局：左侧导航（垂直列表 4 项：基础/输入/外观/高级，选中项 accent 高亮）+ 右侧内容面板（卡片分组，超出滚动）
- 内容映射：现有 4 Tab 页 20+ 配置项全部迁移（表格见 settings-ui/SPEC.md）
- 配色子对话框 → 自绘配色面板（ColorSwatch 网格）
- 应用级配置：3+1 文本输入框 → 结构化列表（进程名 + 行为下拉 + 行内预编辑复选，完成 0.2.34 未竟的 UI）
- 移除 settings.rc / resource.h 依赖（.rc BOM 坑彻底消失）；保留 test_settings_dialog 预览 exe 模式

## 四、文件结构

```
platform/windows/
├── include/
│   ├── ui_render.h  ui_theme.h  ui_window.h  ui_control.h
│   ├── ui_label.h  ui_button.h  ui_checkbox.h  ui_combobox.h
│   ├── ui_edit.h  ui_tab.h  ui_colorpicker.h  ui_scrollbar.h
│   └── ui_layout.h
├── src/
│   ├── ui_render.cpp  ui_theme.cpp  ui_window.cpp  ui_control.cpp
│   ├── ui_label.cpp  ui_button.cpp  ui_checkbox.cpp  ui_combobox.cpp
│   ├── ui_edit.cpp  ui_tab.cpp  ui_colorpicker.cpp  ui_scrollbar.cpp
│   └── ui_layout.cpp
│   ├── candidate_window.cpp   # 重构（0.3.2）
│   ├── banner_window.cpp      # 重构（0.3.3）
│   └── settings_window.cpp    # 新（0.3.4，替代 settings_dialog.cpp + settings.rc）
└── CMakeLists.txt             # 接入新文件；移除 settings.rc
```

## 五、实施计划

| 阶段 | 内容 | 交付 | 验证 |
|------|------|------|------|
| 0.3.0 | ui_render/ui_theme/ui_window/ui_control/ui_layout 底座 | 编译通过 + 冒烟测试（test_ui_framework：创建窗口/主题切换/布局计算/命中检测） | 编译 + 冒烟 |
| 0.3.1 | 控件库 8 个控件 | 编译 + 控件冒烟测试（test_ui_controls：按钮点击/复选切换/下拉选择/编辑输入/IME 组合） | 编译 + 冒烟 |
| 0.3.2 | 候选窗迁移 | 视觉回归（深/浅主题、多行、翻页、悬停、点击） | 冒烟 + 装机实测 |
| 0.3.3 | 工具栏迁移 | 四按钮状态切换 + 前台显隐 | 装机实测 |
| 0.3.4 | 设置窗体现代化 | 全部 20+ 配置项可编辑保存 + 深浅主题 + 自绘配色 | 预览 exe + 装机实测 |
| 0.3.5 | 回归 | cargo test + 全测试 exe + 装机 | 全绿 |

## 六、风险与依赖

- **UIEdit 的 IME 组合**（最高风险）：泰深作为输入法向自己窗体的编辑框上屏中文，组合流程（TSF GetSelection/InsertAtSelection）需在设置窗体上下文复用，候选窗逻辑不适用（候选窗只读）。预留：若 TSF 自组合困难，降级为 WM_CHAR 透传（英文 + 系统 IME 兜底），中文配置项（词库路径等）暂用英文/路径输入
- **模态循环与 TSF 共存**：RunModal 的 GetMessage 循环不能阻塞 TSF 消息泵；设置窗体打开期间输入法仍需正常工作。方案：模态循环内处理 WM_QUIT 不退出进程，TSF 回调线程独立不受影响
- **候选窗迁移视觉回归**：布局算法（宽度自适应/多行网格/DPI）直接从现实现搬运，不重写；风险集中在主题 token 映射（CandidateTheme → UITheme 适配层）
- **移除 .rc 的资源**：IDD_SETTINGS/IDC_* 全部由代码构建替代，resource.h 仅保留托盘/工具栏所需（如有）；B-10 类问题根除
