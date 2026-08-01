# SPEC: 呈现层（Root #8）— 选词上屏（TSF 文本提交）

> 对应 ARCHITECT.md Root #3「接口层」+ Root #8「呈现层」
> 关联 DEV-TRACKER: 0.1.7 选词上屏（候选→TSF 文本提交）

---

## 一、需求

打通输入法完整链路：用户在目标应用输入拼音 → 候选窗口显示 → 数字键/空格选词 → 汉字上屏到目标应用。

**合约**：
- 输入字母时在目标应用创建 TSF 组合（ITfContextComposition），组合内容为拼音串（如 "zhong"）
- 选词（数字 1-9 / 空格）时，将组合内拼音替换为所选汉字（ITfRange::SetText），然后结束组合
- 退格：组合内删除最后一个拼音字符（更新组合文本）
- ESC：取消组合（回退为未输入状态）
- 光标位置变化（ITfContextOwnerServices::OnLayoutChange 或 ITfThreadMgrEventSink 事件）时更新候选窗口位置
- 所有编辑操作在编辑会话（ITfEditSession）内执行——TSF 要求对文档的修改必须在编辑会话中

**不做**：
- 鼠标点击选词——二期
- 用户词库学习（0.2.2）
- 云输入（0.2.3）
- 双拼（0.2.1）

## 二、TSF 组合模型

TSF 组合（Composition）是输入法在文档中标记的一段"正在编辑的文本"：

```
ITfContextComposition::StartComposition(ec, range, sink, &composition)
  → 创建组合，返回 ITfComposition 指针
  → 组合内文本可被 SetText 修改
  → ITfComposition::EndComposition(ec) 结束组合（上屏）
```

**关键接口**：
| 接口 | 用途 |
|------|------|
| ITfContextComposition | 创建/枚举组合（从 ITfContext 获取） |
| ITfComposition | 组合对象：ShiftStart/ShiftEnd/EndComposition |
| ITfCompositionSink | 组合被外部终止时回调（如应用撤销） |
| ITfEditSession | 编辑会话回调，所有文档修改的载体 |
| ITfRange | 文本范围：SetText/GetText/InsertText |

## 三、组件结构

```
platform/windows/
├── include/
│   ├── engine_bridge.h        # 已有
│   ├── tsf_keyevent.h         # 已有
│   ├── candidate_window.h     # 已有
│   └── tsf_composition.h      # 新增：组合管理类声明
├── src/
│   ├── tsf_module.cpp         # 已有，集成组合管理
│   ├── tsf_keyevent.cpp       # 已有
│   ├── candidate_window.cpp   # 已有
│   └── tsf_composition.cpp    # 新增：TSF 组合实现
└── CMakeLists.txt             # 接入新文件
```

## 四、接口

### CTsfComposition（tsf_composition.h）

```cpp
class CTsfComposition : public ITfCompositionSink {
public:
    CTsfComposition();
    ~CTsfComposition();

    // 在编辑会话中启动组合（输入首个字母时调用）
    HRESULT StartComposition(TfEditCookie ec, ITfContext* pic,
                             const std::wstring& pinyin);

    // 在编辑会话中更新组合文本（每次按键后调用）
    HRESULT UpdateComposition(TfEditCookie ec, ITfContext* pic,
                              const std::wstring& pinyin);

    // 在编辑会话中提交组合（选词/ESC 时调用），text 为要上屏的汉字
    HRESULT CommitComposition(TfEditCookie ec, ITfContext* pic,
                              const std::wstring& text);

    // 组合是否活跃
    bool IsActive() const { return m_composition != nullptr; }

    // ITfCompositionSink — 组合被外部结束（如应用撤销）时回调
    STDMETHODIMP OnEndComposition(ITfComposition* pComposition) override;

private:
    ITfComposition* m_composition;   // 当前组合（nullptr = 无组合）
    LONG m_cRef;
};
```

### 数据流

```
按键 → HandleKeyDown → engine FFI → RefreshState()
  ├─ 拼音累积中（无选词）:
  │    → CTsfComposition::StartComposition(ec, pic, pinyin)  // 首次
  │    → CTsfComposition::UpdateComposition(ec, pic, pinyin)  // 后续
  └─ 选词命中（数字/空格）:
       → engine_select_candidate → 得到汉字
       → CTsfComposition::CommitComposition(ec, pic, hanzi)
       → 候选窗口隐藏
```

### 编辑会话调度

所有组合操作需要编辑会话。现有 CTextService::CEditSessionGetCaret 模式复用：
新建 CEditSessionComposition 编辑会话类，携带操作类型 + 数据：

```cpp
enum class CompOp { Start, Update, Commit };
class CEditSessionComposition : public ITfEditSession {
    // 携带 pic + op + pinyin/commitText
    // DoEditSession 内根据 op 调 CTsfComposition 对应方法
};
```

CTextService::OnKeyDown 中统一走 `RequestEditSession(TF_ES_SYNC)` 同步执行。

## 五、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | tsf_composition.h 声明 + CTsfComposition 骨架 | include/tsf_composition.h | 编译 |
| 2 | tsf_composition.cpp：Start/Update/Commit 实现 | src/tsf_composition.cpp | 编译 |
| 3 | ITfCompositionSink::OnEndComposition 实现 | src/tsf_composition.cpp | 编译 |
| 4 | CTextService 集成：CEditSessionComposition + OnKeyDown 调度 | src/tsf_module.cpp | 编译 |
| 5 | 退格/ESC/光标变化事件处理 | src/tsf_module.cpp | 编译 |
| 6 | CMake 接入 | CMakeLists.txt | DLL 构建 |
| 7 | 验证：构建 + 导出 + 引擎回归 + 冒烟测试 | — | 全链路通过 |

## 六、风险与依赖

- **真机验证**：实际上屏效果需用户启用输入法后在任意应用（记事本）输入验证——本步骤交付完整逻辑，最终效果由用户确认
- **组合冲突**：应用主动结束组合（OnEndComposition）时，输入法必须同步清理内部状态（置空 m_composition、隐藏候选窗口）
- **编辑会话嵌套**：RequestEditSession 同步调用在 OnKeyDown 的 UI 线程执行，TSF 保证顺序，无竞态
- **依赖 0.1.5/0.1.6**：拼音/候选数据来自 RefreshState；候选窗口已有，上屏后隐藏
