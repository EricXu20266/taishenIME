# SPEC: 接口层（Root #3）— TSF Text Service + KeyEvent + FFI 对接

> 对应 ARCHITECT.md Root #3「接口层 — 怎么跟它打交道」
> 关联 DEV-TRACKER: 0.1.2 Windows TSF DLL 骨架 + 0.1.5 TSF KeyEvent 捕获与 FFI 对接

---

## 一、需求

当前 `platform/windows/src/tsf_module.cpp` 是空白占位实现（CLSID 为 0、COM 导出函数全部 TODO）。本 SPEC 将其建成可被 Windows TSF 框架加载、激活、接收按键的 Text Service。

**合约**：
- DLL 导出标准 COM 入口：DllGetClassObject / DllCanUnloadNow / DllRegisterServer / DllUnregisterServer
- 实现 IClassFactory + ITfTextInputProcessorEx（激活/停用）+ ITfKeyEventSink（按键）+ ITfThreadMgrEventSink（焦点）
- OnKeyDown 拦截：英文字母 → `engine_process_key`；退格 → `engine_backspace`；数字/空格 → 暂存选词意图（上屏在 0.1.7）
- 处理后的拼音串与候选词通过 FFI 从引擎取回，暂存供候选窗口（0.1.6）使用
- 注册：写 HKCU 注册表（调试期无需管理员），包含 CTF\TIP 键、LanguageProfile、InprocServer32

**不做**：
- 候选窗口渲染（0.1.6 单独做）
- 文本上屏/组合编辑（0.1.7 单独做）
- 配置系统（0.1.8）、中英文切换（0.1.9）、FFI panic 守卫（0.1.10）

## 二、CLSID 与注册表设计

### CLSID（正式生成，替换占位 0）

```
CLSID_TAISHEN_IME    = {B7E8F2C4-5A3D-4E9F-8B2A-1C6D4F0E9A35}
GUID_LANGPROFILE     = {D1A2B3C4-5E6F-4789-9A1B-2C3D4E5F6071}
```

### 注册表布局（HKCU，调试期）

```
HKCU\Software\Classes\CLSID\{B7E8F2C4-...}\InprocServer32
    (Default)   = 完整 DLL 路径
    ThreadingModel = "Apartment"

HKCU\Software\Microsoft\CTF\TIP\{B7E8F2C4-...}
HKCU\Software\Microsoft\CTF\TIP\{B7E8F2C4-...}\LanguageProfile\0x00000804\{D1A2B3C4-...}
    Description = "泰深输入法"
    Enable      = 1
    IconFile / IconIndex（二期）
```

### 类别注册（TSF 识别为输入法）

TSF 通过 Category 项识别 TIP 类型，注册 GUID_TFCAT_TIP_KEYBOARD：
```
HKCU\Software\Microsoft\CTF\TIP\{CLSID}\Category\{GUID_TFCAT_TIP_KEYBOARD}
```

## 三、组件结构

```
platform/windows/src/
├── dllmain.cpp         # DllMain + 模块引用计数
├── tsf_module.cpp      # DLL 导出 + 类工厂 + TextService 实现（本 SPEC 重写）
├── tsf_keyevent.cpp    # 按键→引擎 FFI 的键码映射与处理逻辑（新增，可单测）
└── engine_bridge.cpp   # FFI 桥（已存在，保持）
```

## 四、接口

### C++ 类

```cpp
class CClassFactory : public IClassFactory {
    // CreateInstance → new CTextService
    // LockServer → 模块引用计数
};

class CTextService : public ITfTextInputProcessorEx,
                     public ITfKeyEventSink,
                     public ITfThreadMgrEventSink {
    // ITfTextInputProcessorEx
    //   ActivateEx(ptim, tid, dwFlags) → 线程管理器就绪，注册事件接收器
    //   Deactivate() → 注销接收器，清理
    // ITfKeyEventSink
    //   OnKeyDown(wParam, lParam) → 处理或放行（S_OK 吞键 / S_FALSE 透传）
    //   OnKeyUp / OnTestKeyDown / OnTestKeyUp → 透传
    // ITfThreadMgrEventSink
    //   OnSetFocus → 记录焦点上下文
};
```

### 按键处理规则（tsf_keyevent.cpp，纯逻辑可单测）

```cpp
// 返回 true 表示吞掉该键
bool HandleKeyDown(int vk, uint32_t lparam, KeyEventResult& out);
```

| 按键 | 动作 | 结果 |
|------|------|------|
| VK_A..VK_Z | `engine_process_key('a'..'z')` | 吞键 |
| VK_BACK | `engine_backspace()` | 吞键 |
| VK_SPACE | 候选数>0 时选第 0 个（暂存） | 吞键 |
| VK_1..VK_9 | 候选数>索引 时选该候选（暂存） | 吞键 |
| 其他 | 透传给应用 | 不吞 |

## 五、数据流

```
TSF 框架加载 DLL → DllGetClassObject → CClassFactory
  → ITfTextInputProcessorEx::ActivateEx
    → 注册 ITfKeyEventSink / ITfThreadMgrEventSink
      ↓
OnKeyDown(VK_*)
  → tsf_keyevent::HandleKeyDown
    → engine_bridge → FFI engine_process_key / engine_backspace / engine_select_candidate
    → 通过 FFI 取回拼音串 + 候选词，存入 m_composition（供 0.1.6 候选窗口）
  → S_OK（吞键）或 S_FALSE（透传）
```

## 六、实施计划

| 步骤 | 描述 | 涉及文件 | 验证 |
|------|------|---------|------|
| 1 | 生成正式 CLSID + GUID，替换占位 | tsf_module.cpp | 编译 |
| 2 | 实现 DLL 导出 + 类工厂 + 模块计数 | tsf_module.cpp | 编译 |
| 3 | 实现 CTextService：Activate/Deactivate + 事件注册 | tsf_module.cpp | 编译 |
| 4 | 实现 ITfKeyEventSink OnKeyDown 接线 | tsf_module.cpp | 编译 |
| 5 | 新增 tsf_keyevent.cpp 键码映射逻辑 | tsf_keyevent.cpp + .h | 单元测试（可选） |
| 6 | DllRegisterServer/DllUnregisterServer 注册表写读 | tsf_module.cpp | 注册后 reg query 验证 |
| 7 | CMake 接入新源文件 + Rust staticlib 链接 | CMakeLists.txt | cmake + msbuild 构建出 DLL |
| 8 | 全链路验证 | — | 构建成功 + engine 回归测试 + biome |

## 七、风险与依赖

- **TSF 加载验证**：编译成功 ≠ TSF 能激活。真实验证需注册后在系统输入法设置中启用——由用户手动验证，本步骤交付「可注册、可激活、可吞键」的 DLL
- **依赖 0.1.7**：选词暂存后上屏逻辑在 0.1.7 实现，本步骤数字/空格选词只更新内部状态
- **Rust 链接**：CMake 链接 `engine/target/release/taishen_engine.lib`（staticlib 在 MSVC 下产出 .lib），需先 cargo build --release
