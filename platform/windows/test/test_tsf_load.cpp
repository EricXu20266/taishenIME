// TSF 加载模拟测试 — 用 CoCreateInstance 完整创建 TextService
// 验证 DLL 能否被系统 COM 按注册表 CLSID 正常实例化
// 返回 0 = 通过（DLL 可被系统加载）

#include <windows.h>
#include <objbase.h>
#include <msctf.h>
#include <cstdio>

// {7D77E4AA-276E-4582-B952-94B6EFAADA28}
static const CLSID CLSID_TAISHEN = {
    0x7D77E4AA, 0x276E, 0x4582,
    {0xB9, 0x52, 0x94, 0xB6, 0xEF, 0xAA, 0xDA, 0x28}};

int wmain()
{
    HRESULT hr = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(hr)) {
        wprintf(L"CoInitializeEx 失败: 0x%08X\n", hr);
        return 1;
    }

    // 1. 通过注册表 CLSID 创建（模拟 TSF 加载路径）
    ITfTextInputProcessorEx* pTip = nullptr;
    hr = CoCreateInstance(CLSID_TAISHEN, nullptr, CLSCTX_INPROC_SERVER,
                          IID_ITfTextInputProcessorEx,
                          reinterpret_cast<void**>(&pTip));
    if (FAILED(hr)) {
        wprintf(L"CoCreateInstance 失败: 0x%08X (0x80040154=未注册 0x8007007E=模块找不到)\n", hr);
        CoUninitialize();
        return 2;
    }
    wprintf(L"CoCreateInstance 成功: 类工厂+TextService 实例化 OK\n");

    // 2. 调用 QueryInterface 验证接口完整性
    ITfKeyEventSink* pKeySink = nullptr;
    hr = pTip->QueryInterface(IID_ITfKeyEventSink,
                              reinterpret_cast<void**>(&pKeySink));
    if (SUCCEEDED(hr) && pKeySink != nullptr) {
        wprintf(L"ITfKeyEventSink OK\n");
        pKeySink->Release();
    } else {
        wprintf(L"ITfKeyEventSink 获取失败: 0x%08X\n", hr);
    }

    pTip->Release();
    CoUninitialize();
    wprintf(L"LOAD TEST PASSED\n");
    return 0;
}
