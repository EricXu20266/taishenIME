/// Windows TSF IME — DLL 入口点
///
/// 输入法是一个 COM DLL，需要导出标准入口函数。
/// 第一期 MVP：仅注册 DLL，TSF 框架通过 COM 接口调用。

#include <windows.h>

BOOL APIENTRY DllMain(HMODULE hModule,
                      DWORD   ul_reason_for_call,
                      LPVOID  lpReserved)
{
    switch (ul_reason_for_call)
    {
    case DLL_PROCESS_ATTACH:
        // 初始化引擎——加载系统词库（相对 DLL 路径），失败则回退内置词库
        // engine_init("system_dict.db");
        break;
    case DLL_PROCESS_DETACH:
        // engine_destroy();
        break;
    case DLL_THREAD_ATTACH:
    case DLL_THREAD_DETACH:
        break;
    }
    return TRUE;
}
