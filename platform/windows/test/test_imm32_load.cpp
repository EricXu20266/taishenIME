/// IMM32 兼容层加载冒烟测试（V0.6）
///
/// 验证：
/// 1. DLL 可加载 + 全部 IMM32 导出符号存在
/// 2. ImeInquire 能力声明正确
/// 3. ImeProcessKey + ImeToAsciiEx 完整输入链（组合 → 候选 → 上屏）
/// 使用 ImmCreateContext 创建虚拟 HIMC（无需真实窗口）。

#include <windows.h>
#include <imm.h>
#include <immdev.h>  // IMEINFO / TRANSMSGLIST 等 IME 开发者类型
#include <cstdio>
#include <string>

#pragma comment(lib, "imm32.lib")

// IMM32 IME 导出函数原型（immdev.h 声明，测试中直接声明避免依赖）
typedef BOOL  (WINAPI* PFN_ImeInquire)(LPIMEINFO, LPWSTR, DWORD);
typedef BOOL  (WINAPI* PFN_ImeProcessKey)(HIMC, UINT, LPARAM, CONST LPBYTE);
typedef UINT  (WINAPI* PFN_ImeToAsciiEx)(UINT, UINT, CONST LPBYTE, LPTRANSMSGLIST, UINT, HIMC);
typedef BOOL  (WINAPI* PFN_ImeSelect)(HIMC, BOOL);
typedef BOOL  (WINAPI* PFN_ImeSetActiveContext)(HIMC, BOOL);
typedef DWORD (WINAPI* PFN_ImeConversionList)(HIMC, LPCWSTR, LPCANDIDATELIST, DWORD, UINT);

static int g_fail = 0;

static void Check(bool ok, const char* name)
{
    printf("%s %s\n", ok ? "[PASS]" : "[FAIL]", name);
    if (!ok) { ++g_fail; }
}

static HMODULE LoadIme()
{
    wchar_t dllPath[MAX_PATH] = {0};
    GetModuleFileNameW(nullptr, dllPath, MAX_PATH);
    std::wstring dir(dllPath);
    const size_t slash = dir.find_last_of(L"\\/");
    dir = dir.substr(0, slash + 1);
    const std::wstring full = dir + L"taishen_ime_imm32.ime";
    return LoadLibraryW(full.c_str());
}

int main()
{
    // 无缓冲输出：崩溃时也能定位到崩溃点
    setvbuf(stdout, nullptr, _IONBF, 0);
    setvbuf(stderr, nullptr, _IONBF, 0);

    const HMODULE h = LoadIme();
    Check(h != nullptr, "LoadLibrary taishen_ime_imm32.ime");
    if (h == nullptr) {
        printf("GetLastError=%lu\n", GetLastError());
        return 1;
    }

    // 1. 导出符号检查
    const char* names[] = {
        "ImeInquire", "ImeConversionList", "ImeConfigure", "ImeDestroy",
        "ImeEscape", "ImeProcessKey", "ImeSelect", "ImeSetActiveContext",
        "ImeToAsciiEx", "ImeGetImeMenuItems", "ImeRegisterWord",
        "ImeUnregisterWord", "ImeGetRegisterWordStyle", "ImeEnumRegisterWord",
        "ImeSetCompositionString", "ImeSetCompositionFont",
        "ImeSetCompositionWindow", "ImeNotify", "UIWndProc", "ImeUIWndProc",
        "NotifyIME", "DllRegisterServer", "DllUnregisterServer",
    };
    bool allExports = true;
    for (const char* n : names) {
        if (GetProcAddress(h, n) == nullptr) {
            printf("  缺失导出: %s\n", n);
            allExports = false;
        }
    }
    Check(allExports, "全部 23 个 IMM32 导出符号");

    const auto pImeInquire = reinterpret_cast<PFN_ImeInquire>(
        GetProcAddress(h, "ImeInquire"));
    const auto pImeProcessKey = reinterpret_cast<PFN_ImeProcessKey>(
        GetProcAddress(h, "ImeProcessKey"));
    const auto pImeToAsciiEx = reinterpret_cast<PFN_ImeToAsciiEx>(
        GetProcAddress(h, "ImeToAsciiEx"));
    const auto pImeSelect = reinterpret_cast<PFN_ImeSelect>(
        GetProcAddress(h, "ImeSelect"));
    const auto pImeSetActiveContext = reinterpret_cast<PFN_ImeSetActiveContext>(
        GetProcAddress(h, "ImeSetActiveContext"));
    const auto pImeConversionList = reinterpret_cast<PFN_ImeConversionList>(
        GetProcAddress(h, "ImeConversionList"));

    // 2. ImeInquire 能力声明
    IMEINFO info = {};
    wchar_t wndClass[16] = {0};
    const BOOL inqOk = pImeInquire(&info, wndClass, 0);
    Check(inqOk != FALSE && wndClass[0] != L'\0', "ImeInquire 能力声明");
    if (inqOk) {
        printf("  fdwProperty=0x%lX (期望含 AT_CARET/UNICODE/SPECIAL_UI), wndClass=%ls\n",
               info.fdwProperty, wndClass);
        Check((info.fdwProperty & IME_PROP_AT_CARET) != 0, "  AT_CARET 声明");
        Check((info.fdwProperty & IME_PROP_UNICODE) != 0, "  UNICODE 声明");
    }

    // 3. 虚拟 HIMC + 完整输入链
    const HIMC himc = ImmCreateContext();
    Check(himc != nullptr, "ImmCreateContext 虚拟 HIMC");
    if (himc == nullptr) {
        return 1;
    }

    pImeSelect(himc, TRUE);
    pImeSetActiveContext(himc, TRUE);

    BYTE keyState[256] = {0};
    TRANSMSGLIST msgList = {};
    msgList.TransMsg[0].message = 0;

    // 'n' → 组合开始（ImeProcessKey 应消费）
    const BOOL eatN = pImeProcessKey(himc, 'N', 0, keyState);
    Check(eatN != FALSE, "ImeProcessKey('n') 应消费");
    const UINT n1 = pImeToAsciiEx('N', 0, keyState, &msgList, 0, himc);
    Check(n1 > 0, "ImeToAsciiEx('n') 产生消息（组合开始）");

    // 'i' → 组合更新
    const UINT n2 = pImeToAsciiEx('I', 0, keyState, &msgList, 0, himc);
    Check(n2 > 0, "ImeToAsciiEx('i') 产生消息（组合更新）");

    // 候选列表应可用
    BYTE candBuf[4096] = {0};
    auto* candList = reinterpret_cast<LPCANDIDATELIST>(candBuf);
    const DWORD candRet = pImeConversionList(himc, L"ni", candList,
                                             sizeof(candBuf), 0);
    Check(candRet != 0 && candList->dwCount > 0,
          "ImeConversionList 返回候选");
    if (candRet != 0) {
        printf("  候选数=%lu 首个=", candList->dwCount);
        if (candList->dwCount > 0) {
            const wchar_t* first = reinterpret_cast<const wchar_t*>(
                reinterpret_cast<const BYTE*>(candList) + candList->dwOffset[0]);
            wprintf(L"%ls\n", first);
        }
    }

    // 空格 → 选默认候选上屏（WM_IME_CHAR）
    const UINT n3 = pImeToAsciiEx(VK_SPACE, 0, keyState, &msgList, 0, himc);
    bool hasChar = false;
    for (UINT i = 0; i < n3 && i < 8; ++i) {
        if (msgList.TransMsg[i].message == WM_IME_CHAR) {
            hasChar = true;
            wprintf(L"  上屏字符: %lc\n", static_cast<wchar_t>(msgList.TransMsg[i].wParam));
            break;
        }
    }
    Check(hasChar, "空格选词产生 WM_IME_CHAR 上屏");

    // 组合结束（选词后拼音清空 → ENDCOMPOSITION）
    bool hasEnd = false;
    for (UINT i = 0; i < n3 && i < 8; ++i) {
        if (msgList.TransMsg[i].message == WM_IME_ENDCOMPOSITION) {
            hasEnd = true;
        }
    }
    Check(hasEnd, "选词后 ENDCOMPOSITION");

    pImeSetActiveContext(himc, FALSE);
    pImeSelect(himc, FALSE);
    ImmDestroyContext(himc);

    printf("\n%s\n", g_fail == 0 ? "全部通过" : "存在失败项");
    return g_fail == 0 ? 0 : 1;
}
