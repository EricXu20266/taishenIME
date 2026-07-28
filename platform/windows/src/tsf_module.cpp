/// Windows TSF IME — TSF 模块实现（骨架）
///
/// 第一期 MVP：搭建 TSF Text Service 基础框架
/// 后续逐步填充 KeyEvent 处理、候选窗口、文本提交
///
/// TSF 输入法需要实现的核心 COM 接口：
///   - ITfTextInputProcessorEx  (键盘事件接收)
///   - ITfDisplayAttributeProvider (候选窗口显示属性)
///   - ITfCompositionSink (编辑会话文本提交)
///
/// 注册：通过 RegSvr32 或 DllRegisterServer 导出函数注册 COM 组件

#include <windows.h>
#include <msctf.h>

// TSF 输入法 CLSID（需要生成唯一 GUID，当前为占位符）
// 正式版本用 guidgen.exe 或 uuidgen 生成
// {XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}
const CLSID CLSID_TAISHEN_IME = {0};

// TSF Text Service 简要结构（MVP 骨架）
// 完整实现参考微软 TSF 示例：
// https://github.com/microsoft/TSF-samples

/// DllRegisterServer — 注册 COM 组件
STDAPI DllRegisterServer(void)
{
    // TODO: 注册 TSF Text Service
    // 1. 在 HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\CTF\TIP\ 下注册 CLSID
    // 2. 注册 CLSID → DLL 路径映射
    // 3. 注册语言配置文件（中文 zh-CN）
    return S_OK;
}

/// DllUnregisterServer — 注销 COM 组件
STDAPI DllUnregisterServer(void)
{
    // TODO: 删除注册表项
    return S_OK;
}

/// DllGetClassObject — COM 类工厂
STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, LPVOID* ppv)
{
    // TODO: 返回 TSF Text Service 的类工厂
    return CLASS_E_CLASSNOTAVAILABLE;
}

/// DllCanUnloadNow — 是否可以卸载
STDAPI DllCanUnloadNow(void)
{
    // 如果没有活动引用，可以卸载
    return S_FALSE;
}
