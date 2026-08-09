/// 泰深输入法 IMM32 兼容层 — 老游戏/老应用适配
///
/// 背景：LOL 聊天框使用 Scaleform GFxIME（IMM32 协议）+ IMEConfig.xml 白名单，
/// 纯 TSF 输入法不被激活。本 DLL 注册为 IMM32 IME（Keyboard Layouts），
/// 复用 Rust 引擎 + TSF 侧按键逻辑（tsf_keyevent），行为与 TSF 通道一致。
///
/// 对应 SPEC: docs/modules/imm32-layer/SPEC.md
/// 覆盖 DEV-TRACKER: V0.6.1 / 0.6.2 / 0.6.3

#include <windows.h>
#include <immdev.h>   // IMM32 IME 开发者接口（IMEINFO/LPIMEINFO 等）
#include <imm.h>
#include <string>
#include <vector>
#include <mutex>

#include "engine_bridge.h"
#include "config_reader.h"
#include "candidate_window.h"
#include "tsf_keyevent.h"
#include "app_state.h"
#include "debug_log.h"
#include "theme.h"

#pragma comment(lib, "imm32.lib")

// ---------------------------------------------------------------------------
// 全局状态
// ---------------------------------------------------------------------------

/// DLL 模块句柄（DllMain 设置）
static HMODULE g_hModule = nullptr;

/// IME 窗口类名（ImeInquire 返回，系统为每个 HIMC 创建该类的窗口）
static const wchar_t kImeWndClass[] = L"TaishenImeWindow";

/// 引擎初始化锁 + 状态（IMM32 函数在 UI 线程调用，防御多线程）
static std::mutex g_engineMutex;
static bool g_engineReady = false;
static bool g_engineInitFailed = false;

/// 引号交替状态（对齐 tsf_module：' 单引号 / " 双引号开闭交替）
static bool g_quoteOpen = false;    // 单引号当前是否"开"
static bool g_dquoteOpen = false;   // 双引号当前是否"开"

/// 候选窗（进程单例，IMM32 自绘；LOL 白名单命中时 GFxIME 游戏内渲染）
static taishen::CCandidateWindow g_candidateWindow;

/// 组合状态（当前活动 HIMC）
static HIMC g_activeHimc = nullptr;
static bool g_composing = false;   // 是否处于组合状态（有拼音串）
static bool g_candidateOpen = false; // 候选窗是否打开

// ---------------------------------------------------------------------------
// 内部工具
// ---------------------------------------------------------------------------

/// UTF-8 → UTF-16（复用 tsf_keyevent 实现）
static std::wstring Utf8ToWide(const std::string& utf8)
{
    return taishen::Utf8ToWide(utf8);
}

/// 获取 DLL 所在目录（带尾分隔符）
static std::wstring GetDllDir()
{
    wchar_t path[MAX_PATH] = {0};
    const DWORD len = GetModuleFileNameW(g_hModule, path, MAX_PATH);
    if (len == 0) {
        return std::wstring();
    }
    std::wstring p(path, len);
    const size_t slash = p.find_last_of(L"\\/");
    if (slash == std::wstring::npos) {
        return std::wstring();
    }
    return p.substr(0, slash + 1);
}

/// 注册 IME 窗口类（系统为每个 HIMC 创建该类的窗口）
static void EnsureImeWndClass()
{
    static bool registered = false;
    if (registered) {
        return;
    }
    WNDCLASSW wc = {0};
    wc.style = CS_IME;
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = g_hModule;
    wc.lpszClassName = kImeWndClass;
    if (RegisterClassW(&wc) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS) {
        registered = true;
    }
}

/// 初始化引擎（对齐 tsf_module ActivateEx）：config.ini → 词库 → 用户词库 → 配置应用
static bool EnsureEngineReady()
{
    std::lock_guard<std::mutex> lock(g_engineMutex);
    if (g_engineReady) {
        return true;
    }
    if (g_engineInitFailed) {
        return false;
    }

    const std::wstring dllDir = GetDllDir();
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);

    // 系统词库
    const std::wstring dictPath = taishen::ResolveDictPath(cfg, dllDir);
    std::string dictUtf8;
    if (!dictPath.empty()) {
        const int len = WideCharToMultiByte(CP_UTF8, 0, dictPath.c_str(),
                                            static_cast<int>(dictPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            dictUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, dictPath.c_str(),
                                static_cast<int>(dictPath.size()),
                                &dictUtf8[0], len, nullptr, nullptr);
        }
    }
    const int initRet = engine_init(dictUtf8.empty() ? nullptr : dictUtf8.c_str());
    taishen::DebugLog("imm32: engine_init ret=" + std::to_string(initRet));

    // 用户词库
    const std::wstring userDictPath = taishen::ResolveUserDictPath(cfg, dllDir);
    std::string userDictUtf8;
    if (!userDictPath.empty()) {
        const int len = WideCharToMultiByte(CP_UTF8, 0, userDictPath.c_str(),
                                            static_cast<int>(userDictPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            userDictUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, userDictPath.c_str(),
                                static_cast<int>(userDictPath.size()),
                                &userDictUtf8[0], len, nullptr, nullptr);
        }
    }
    engine_set_user_dict_path(userDictUtf8.empty() ? nullptr : userDictUtf8.c_str());

    // 应用配置到引擎
    engine_set_candidate_count(cfg.candidate_count);
    engine_set_sort_mode(cfg.sort_mode);
    engine_set_context_assoc(cfg.context_assoc ? 1 : 0);
    engine_set_fuzzy(cfg.fuzzy_enabled ? 1 : 0);
    engine_set_shuangpin(cfg.shuangpin_mode ? 1 : 0);
    engine_set_shuangpin_scheme(cfg.shuangpin_scheme.c_str());
    engine_set_correction(cfg.correction_enabled ? 1 : 0);
    engine_set_mix_mode(cfg.mix_mode_enabled ? 1 : 0);
    engine_set_traditional(cfg.traditional_enabled ? 1 : 0);
    engine_set_ascii_punct(cfg.ascii_punct ? 1 : 0);
    engine_set_emoji(cfg.emoji_enabled ? 1 : 0);
    engine_set_phrase_enabled(cfg.phrase_enabled ? 1 : 0);
    if (!cfg.phrase_path.empty()) {
        std::string p;
        const int len = WideCharToMultiByte(CP_UTF8, 0, cfg.phrase_path.c_str(),
                                            static_cast<int>(cfg.phrase_path.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            p.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, cfg.phrase_path.c_str(),
                                static_cast<int>(cfg.phrase_path.size()),
                                &p[0], len, nullptr, nullptr);
        }
        engine_set_phrase_path(p.empty() ? nullptr : p.c_str());
    }
    // 拆字反查词库
    {
        const std::wstring radicalPath = dllDir + L"radical_pinyin.dict.yaml";
        std::string p;
        const int len = WideCharToMultiByte(CP_UTF8, 0, radicalPath.c_str(),
                                            static_cast<int>(radicalPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            p.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, radicalPath.c_str(),
                                static_cast<int>(radicalPath.size()),
                                &p[0], len, nullptr, nullptr);
        }
        engine_set_radical_path(p.empty() ? nullptr : p.c_str());
    }

    // 配对符号开关同步（tsf_keyevent 全局）
    taishen::SetPairPunctEnabled(cfg.pair_punct);
    taishen::SetDebugLogEnabled(cfg.debug_log);

    // 候选窗主题/布局/字体
    taishen::CandidateTheme theme;
    bool followSystem = taishen::ApplyThemeWithSystem(theme, cfg);
    g_candidateWindow.SetFollowSystemTheme(followSystem);
    g_candidateWindow.SetTheme(theme);
    g_candidateWindow.SetFont(cfg.font_face, cfg.font_size);
    g_candidateWindow.SetLabelFormat(cfg.label_format);
    g_candidateWindow.SetLayout(cfg.corner_radius, cfg.hilite_corner_radius,
                                cfg.padding, cfg.candidate_spacing);
    g_candidateWindow.SetInlinePreedit(cfg.inline_preedit);
    g_candidateWindow.SetClickCallback([](int index) {
        // 鼠标点击选词：提交第 index 个候选（上屏）
        char buf[512] = {0};
        const int len = engine_select_candidate(index, buf, sizeof(buf));
        if (len > 0) {
            // 通过 WM_IME_CHAR 上屏（组合窗）
            const std::wstring w = Utf8ToWide(buf);
            for (wchar_t ch : w) {
                PostMessageW(GetFocus(), WM_IME_CHAR, ch, 0);
            }
            engine_reset();
            g_composing = false;
            g_candidateWindow.Hide();
        }
    });

    g_engineReady = (initRet == 0);
    g_engineInitFailed = !g_engineReady;
    taishen::DebugLog(g_engineReady
        ? "imm32: engine ready"
        : "imm32: engine init FAILED ret=" + std::to_string(initRet));
    return g_engineReady;
}

/// 获取当前拼音串（UTF-16）
static std::wstring GetPinyinWide()
{
    const int len = engine_get_pinyin_str(nullptr, 0);
    if (len <= 1) { // 空串 = 1（含 null）
        return std::wstring();
    }
    std::string utf8(static_cast<size_t>(len - 1), '\0');
    engine_get_pinyin_str(&utf8[0], len);
    return Utf8ToWide(utf8);
}

/// 收集当前页候选（UTF-16）
static std::vector<std::wstring> CollectCandidates()
{
    std::vector<std::wstring> result;
    const int count = engine_get_candidate_count();
    for (int i = 0; i < count; ++i) {
        char buf[512] = {0};
        const int len = engine_get_candidate(i, buf, sizeof(buf));
        if (len > 0) {
            result.push_back(Utf8ToWide(std::string(buf, static_cast<size_t>(len - 1))));
        }
    }
    return result;
}

/// 更新候选窗（组合变化后调用）：拼音空或候选空 → 隐藏；否则显示
static void UpdateCandidateWindow()
{
    const std::wstring pinyin = GetPinyinWide();
    const std::vector<std::wstring> candidates = CollectCandidates();

    if (pinyin.empty() || candidates.empty()) {
        g_candidateWindow.Hide();
        g_candidateOpen = false;
        return;
    }

    // 定位：优先光标（GetCaretPos + ClientToScreen），失败降级鼠标
    RECT caretRect = {0, 0, 0, 0};
    POINT pt;
    HWND hwnd = GetFocus();
    if (hwnd != nullptr && GetCaretPos(&pt)) {
        ClientToScreen(hwnd, &pt);
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.right = pt.x;
        caretRect.bottom = pt.y;
    } else if (GetCursorPos(&pt)) {
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.right = pt.x;
        caretRect.bottom = pt.y;
    }

    // 候选转 UTF-8（CCandidateWindow 接口）
    std::vector<std::string> candsUtf8;
    for (const auto& w : candidates) {
        const int len = WideCharToMultiByte(CP_UTF8, 0, w.c_str(),
                                            static_cast<int>(w.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            std::string s(static_cast<size_t>(len), '\0');
            WideCharToMultiByte(CP_UTF8, 0, w.c_str(),
                                static_cast<int>(w.size()),
                                &s[0], len, nullptr, nullptr);
            candsUtf8.push_back(s);
        }
    }

    const int page = engine_get_current_page();
    const int total = engine_get_total_pages();
    g_candidateWindow.UpdateState(
        taishen::WideToUtf8(pinyin), candsUtf8, caretRect, page, total);
    g_candidateOpen = true;
}

/// 更新组合串到系统（ImmGetCompositionString 缓存），并生成 WM_IME_COMPOSITION 消息
/// @param msgs 消息列表（可能为空）
/// @param pinyin 当前拼音串（空 = 组合结束）
static void SyncComposition(HIMC himc, const std::wstring& pinyin,
                            std::vector<TRANSMSG>& msgs)
{
    const bool wasComposing = g_composing;
    const bool nowComposing = !pinyin.empty();
    g_composing = nowComposing;

    if (!nowComposing) {
        if (wasComposing) {
            // 组合结束
            TRANSMSG m = {};
            m.message = WM_IME_ENDCOMPOSITION;
            msgs.push_back(m);
        }
        return;
    }

    // 更新系统组合串缓存（应用 ImmGetCompositionString 读取）
    if (himc != nullptr && !pinyin.empty()) {
        ImmSetCompositionStringW(himc, SCS_SETSTR,
                                 const_cast<wchar_t*>(pinyin.c_str()),
                                 static_cast<DWORD>(pinyin.size() * sizeof(wchar_t)),
                                 nullptr, 0);
    }

    if (!wasComposing) {
        TRANSMSG m = {};
        m.message = WM_IME_STARTCOMPOSITION;
        msgs.push_back(m);
    }
    {
        TRANSMSG m = {};
        m.message = WM_IME_COMPOSITION;
        m.lParam = GCS_COMPSTR | GCS_COMPATTR | GCS_CURSORPOS;
        msgs.push_back(m);
    }
}

/// 上屏文本：逐字符 WM_IME_CHAR（处理 UTF-16 代理对）
static void AppendCommitMessages(const std::wstring& text,
                                 std::vector<TRANSMSG>& msgs)
{
    for (wchar_t ch : text) {
        TRANSMSG m = {};
        m.message = WM_IME_CHAR;
        m.wParam = ch;
        msgs.push_back(m);
    }
}

// ---------------------------------------------------------------------------
// 导出函数
// ---------------------------------------------------------------------------

/// ImeInquire — 能力声明 + IME 窗口类名
BOOL WINAPI ImeInquire(LPIMEINFO lpIMEInfo, LPWSTR lpszWndCls,
                       DWORD dwSystemInfoFlags)
{
    EnsureImeWndClass();
    if (lpIMEInfo != nullptr) {
        ZeroMemory(lpIMEInfo, sizeof(IMEINFO));
        lpIMEInfo->dwPrivateDataSize = 0;
        // AT_CARET：候选窗定位在光标处（GFxIME 依赖）；UNICODE：输出 Unicode；
        // SPECIAL_UI：IME 自绘候选 UI（CCandidateWindow）
        lpIMEInfo->fdwProperty =
            IME_PROP_AT_CARET | IME_PROP_UNICODE | IME_PROP_SPECIAL_UI;
        lpIMEInfo->fdwConversionCaps = IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE;
        lpIMEInfo->fdwSentenceCaps = IME_SMODE_NONE;
        lpIMEInfo->fdwUICaps = 0;
        lpIMEInfo->fdwSCSCaps = 0;
        lpIMEInfo->fdwSelectCaps = 0;
    }
    if (lpszWndCls != nullptr) {
        // SDK：缓冲区固定 IME_UI_CLASS_NAME_SIZE(16)
        wcsncpy_s(lpszWndCls, IME_UI_CLASS_NAME_SIZE, kImeWndClass, _TRUNCATE);
    }
    return TRUE;
}

/// ImeConversionList — 返回当前候选列表（LOL GFxIME 经 ImmGetCandidateList 调用）
DWORD WINAPI ImeConversionList(HIMC /*hIMC*/, LPCWSTR /*lpSrc*/,
                               LPCANDIDATELIST lpDst, DWORD dwBufLen, UINT /*uFlag*/)
{
    if (!EnsureEngineReady()) {
        return 0;
    }
    const int count = engine_get_candidate_count();
    if (count <= 0) {
        return 0;
    }
    const std::vector<std::wstring> cands = CollectCandidates();
    if (cands.empty()) {
        return 0;
    }

    // 所需字节 = 结构 + 每候选 (size+1) * sizeof(wchar_t)
    DWORD needed = sizeof(CANDIDATELIST) +
                   static_cast<DWORD>((cands.size() - 1) * sizeof(DWORD));
    for (const auto& w : cands) {
        needed += static_cast<DWORD>((w.size() + 1) * sizeof(wchar_t));
    }
    if (lpDst == nullptr || dwBufLen < needed) {
        return 0; // 空间不足（ImmGetConversionList 两次调用：先探测）
    }

    lpDst->dwSize = needed;
    lpDst->dwStyle = 0;
    lpDst->dwCount = static_cast<DWORD>(cands.size());
    lpDst->dwSelection = 0;
    lpDst->dwPageStart = 0;
    lpDst->dwPageSize = static_cast<DWORD>(cands.size());
    DWORD offset = static_cast<DWORD>(sizeof(CANDIDATELIST) +
                                      (cands.size() - 1) * sizeof(DWORD));
    for (size_t i = 0; i < cands.size(); ++i) {
        lpDst->dwOffset[i] = offset;
        wchar_t* dst = reinterpret_cast<wchar_t*>(
            reinterpret_cast<BYTE*>(lpDst) + offset);
        const size_t avail = (dwBufLen - offset) / sizeof(wchar_t);
        wcsncpy_s(dst, avail, cands[i].c_str(), _TRUNCATE);
        offset += static_cast<DWORD>((cands[i].size() + 1) * sizeof(wchar_t));
    }
    return 1;
}

/// ImeConfigure — 打开设置（暂不支持，后续接 settings_window）
BOOL WINAPI ImeConfigure(HKL /*hKL*/, HWND /*hWnd*/, DWORD /*dwMode*/,
                         LPVOID /*lpData*/)
{
    return FALSE;
}

/// ImeDestroy — 释放（引擎为进程单例，保留）
BOOL WINAPI ImeDestroy(UINT_PTR /*uForce*/)
{
    g_composing = false;
    g_candidateWindow.Hide();
    return TRUE;
}

/// ImeEscape — 查询支持
LRESULT WINAPI ImeEscape(HIMC /*hIMC*/, UINT uEscape, LPVOID /*lpData*/)
{
    switch (uEscape) {
    case IME_ESC_QUERY_SUPPORT:
        return 1;
    default:
        return 0;
    }
}

/// ImeProcessKey — 按键预判（宽松：精确判定在 ImeToAsciiEx，返回 0 消息=透传）
BOOL WINAPI ImeProcessKey(HIMC /*hIMC*/, UINT uVirKey, LPARAM /*lParam*/,
                          CONST LPBYTE /*lpbKeyState*/)
{
    if (!EnsureEngineReady()) {
        return FALSE;
    }
    const int vk = taishen::NormalizeKeypad(static_cast<int>(uVirKey));
    if (vk >= 'A' && vk <= 'Z') {
        return TRUE;
    }
    if (vk >= '0' && vk <= '9') {
        return TRUE;
    }
    switch (vk) {
    case VK_SPACE: case VK_BACK: case VK_RETURN: case VK_ESCAPE:
    case VK_PRIOR: case VK_NEXT: case VK_UP: case VK_DOWN:
    case VK_TAB: case VK_DELETE:
    case VK_OEM_1: case VK_OEM_2: case VK_OEM_3: case VK_OEM_4:
    case VK_OEM_5: case VK_OEM_6: case VK_OEM_7: case VK_OEM_8:
    case VK_OEM_COMMA: case VK_OEM_PERIOD:
    case VK_OEM_MINUS: case VK_OEM_PLUS:
        return TRUE;
    default:
        return FALSE;
    }
}

/// ImeSelect — 输入法被选中/取消
BOOL WINAPI ImeSelect(HIMC hIMC, BOOL fSelect)
{
    if (fSelect) {
        if (!EnsureEngineReady()) {
            return FALSE;
        }
        g_activeHimc = hIMC;
        // 应用前台进程的初始/记忆状态（app_options：终端默认英文等）
        const std::wstring dllDir = GetDllDir();
        const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        taishen::AppStateApply(cfg);
    } else {
        g_composing = false;
        g_candidateOpen = false;
        g_candidateWindow.Hide();
        g_activeHimc = nullptr;
    }
    return TRUE;
}

/// ImeSetActiveContext — 激活/失活
BOOL WINAPI ImeSetActiveContext(HIMC /*hIMC*/, BOOL fActivate)
{
    if (!fActivate) {
        g_composing = false;
        g_candidateOpen = false;
        g_candidateWindow.Hide();
    }
    return TRUE;
}

/// ImeToAsciiEx — 按键处理主入口：引擎 → 组合/候选/上屏消息列表
UINT WINAPI ImeToAsciiEx(UINT uVirKey, UINT /*uChar*/,
                         CONST LPBYTE /*lpbKeyState*/,
                         LPTRANSMSGLIST lpTransMsgList, UINT /*fuState*/,
                         HIMC hIMC)
{
    if (!EnsureEngineReady() || lpTransMsgList == nullptr) {
        return 0;
    }

    const int effVk = taishen::NormalizeKeypad(static_cast<int>(uVirKey));
    taishen::KeyEventResult result;
    const bool eat = taishen::HandleKeyDown(effVk, 0, result);
    if (!eat) {
        return 0; // 透传给应用
    }

    // 配对引号：开闭交替 / 成对输出（对齐 tsf_module OnKeyDown）
    std::wstring committed = result.committed;
    if (result.punct_quote == 1) {
        if (taishen::IsPairPunctEnabled()) {
            int off = -1;
            std::wstring out;
            taishen::ExpandPairPunct(L"‘", out, off);
            committed = out;
        } else {
            committed = g_quoteOpen ? L"’" : L"‘";
            g_quoteOpen = !g_quoteOpen;
        }
    } else if (result.punct_quote == 2) {
        if (taishen::IsPairPunctEnabled()) {
            int off = -1;
            std::wstring out;
            taishen::ExpandPairPunct(L"“", out, off);
            committed = out;
        } else {
            committed = g_dquoteOpen ? L"”" : L"“";
            g_dquoteOpen = !g_dquoteOpen;
        }
    }

    std::vector<TRANSMSG> msgs;

    // 上屏文本（选词/标点/大写等）
    if (!committed.empty()) {
        AppendCommitMessages(committed, msgs);
        // P1-2 数字分隔符状态同步（对齐 tsf_module 提交后更新）
        const wchar_t last = committed.back();
        taishen::g_lastCommitEndsWithDigit = (last >= L'0' && last <= L'9');
    }

    // 同步组合状态（引擎拼音串）
    const std::wstring pinyin = GetPinyinWide();
    SyncComposition(hIMC, pinyin, msgs);

    // 候选窗更新（自绘；LOL 白名单命中时 GFxIME 游戏内渲染，此窗不干扰）
    UpdateCandidateWindow();

    const UINT count = static_cast<UINT>(msgs.size());
    if (count > 0) {
        lpTransMsgList->uMsgCount = count;
        for (UINT i = 0; i < count; ++i) {
            lpTransMsgList->TransMsg[i] = msgs[i];
        }
    }
    return count;
}

/// ImeGetImeMenuItems — 无菜单
LRESULT WINAPI ImeGetImeMenuItems(HIMC /*hIMC*/, DWORD /*dwFlags*/,
                                  DWORD /*dwType*/,
                                  LPIMEMENUITEMINFO /*lpImeParentMenu*/,
                                  LPIMEMENUITEMINFO /*lpImeMenu*/, DWORD /*dwSize*/)
{
    return 0;
}

/// ImeRegisterWord / ImeUnregisterWord / ImeGetRegisterWordStyle / ImeEnumRegisterWord
/// — 词组注册 API，本 IME 不支持（返回失败/0）
BOOL WINAPI ImeRegisterWord(LPCTSTR /*lpszReading*/, DWORD /*dwStyle*/,
                            LPCTSTR /*lpszString*/)
{
    return FALSE;
}

BOOL WINAPI ImeUnregisterWord(LPCTSTR /*lpszReading*/, DWORD /*dwStyle*/,
                              LPCTSTR /*lpszString*/)
{
    return FALSE;
}

UINT WINAPI ImeGetRegisterWordStyle(UINT /*nItem*/, LPSTYLEBUFW /*lpStyleBuf*/)
{
    return 0;
}

UINT WINAPI ImeEnumRegisterWord(REGISTERWORDENUMPROCW /*lpfnEnumProc*/,
                                LPCWSTR /*lpszReading*/, DWORD /*dwStyle*/,
                                LPCWSTR /*lpszString*/, LPVOID /*lpData*/)
{
    return 0;
}

/// ImeSetCompositionString — 系统/应用设置组合串（引擎为真相源，仅记录）
BOOL WINAPI ImeSetCompositionString(HIMC /*hIMC*/, DWORD /*dwIndex*/,
                                    LPCVOID /*lpComp*/, DWORD /*dwCompSize*/,
                                    LPCVOID /*lpRead*/, DWORD /*dwReadSize*/)
{
    return TRUE;
}

/// ImeSetCompositionFont / ImeSetCompositionWindow — 接受设置
BOOL WINAPI ImeSetCompositionFont(HIMC /*hIMC*/, LPLOGFONTW /*lpLogFont*/)
{
    return TRUE;
}

BOOL WINAPI ImeSetCompositionWindow(HIMC /*hIMC*/, LPCANDIDATEFORM /*lpCompForm*/)
{
    return TRUE;
}

/// ImeNotify — 系统通知（忽略）
LRESULT WINAPI ImeNotify(HIMC /*hIMC*/, DWORD /*dwCommand*/, DWORD /*dwData*/)
{
    return 0;
}

/// UIWndProc / ImeUIWndProc — IME 窗口过程（默认处理）
LRESULT WINAPI UIWndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    return DefWindowProcW(hWnd, msg, wParam, lParam);
}

LRESULT WINAPI ImeUIWndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    return DefWindowProcW(hWnd, msg, wParam, lParam);
}

/// NotifyIME — 无操作
BOOL WINAPI NotifyIME(HIMC /*hIMC*/, DWORD /*dwAction*/, DWORD /*dwIndex*/,
                      DWORD /*dwValue*/)
{
    return FALSE;
}

// ---------------------------------------------------------------------------
// 注册（Keyboard Layouts，HKLM 需要管理员）
// ---------------------------------------------------------------------------

/// KLID：厂商自定义区间 E0C0-0804（落地前检查冲突）
static const wchar_t kLayoutId[] = L"E0C00804";

/// 获取 Keyboard Layouts 注册表键路径
static std::wstring GetLayoutRegPath()
{
    return std::wstring(
        L"SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\") + kLayoutId;
}

/// 写注册表字符串值
static bool RegSetString(HKEY root, const std::wstring& subKey,
                         const std::wstring& name, const std::wstring& value)
{
    HKEY hKey = nullptr;
    if (RegCreateKeyExW(root, subKey.c_str(), 0, nullptr, 0, KEY_WRITE,
                        nullptr, &hKey, nullptr) != ERROR_SUCCESS) {
        return false;
    }
    const DWORD bytes = static_cast<DWORD>((value.size() + 1) * sizeof(wchar_t));
    const LSTATUS ret = RegSetValueExW(hKey, name.c_str(), 0, REG_SZ,
                                       reinterpret_cast<const BYTE*>(value.c_str()),
                                       bytes);
    RegCloseKey(hKey);
    return ret == ERROR_SUCCESS;
}

/// DllRegisterServer — 注册 IMM32 IME（Keyboard Layouts）
STDAPI DllRegisterServer(void)
{
    // KLID 冲突检测：已存在且不是本 IME → 拒绝注册
    const std::wstring keyPath = GetLayoutRegPath();
    HKEY hKey = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, keyPath.c_str(), 0, KEY_READ,
                      &hKey) == ERROR_SUCCESS) {
        wchar_t existing[MAX_PATH] = {0};
        DWORD size = sizeof(existing);
        const LSTATUS q = RegQueryValueExW(hKey, L"Layout File", nullptr, nullptr,
                                           reinterpret_cast<LPBYTE>(existing), &size);
        RegCloseKey(hKey);
        if (q == ERROR_SUCCESS && existing[0] != L'\0' &&
            wcsstr(existing, L"taishen_ime_imm32") == nullptr) {
            return E_ACCESSDENIED; // KLID 被其他输入法占用
        }
    }

    // DLL 完整路径（Layout File 用完整路径，第三方 IME 通行做法）
    wchar_t dllPath[MAX_PATH] = {0};
    GetModuleFileNameW(g_hModule, dllPath, MAX_PATH);

    if (!RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"Layout File", dllPath) ||
        !RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"IME File", dllPath) ||
        !RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"Layout Text", L"泰深拼音")) {
        return E_FAIL;
    }
    return S_OK;
}

/// DllUnregisterServer — 移除 Keyboard Layouts 注册
STDAPI DllUnregisterServer(void)
{
    const std::wstring keyPath = GetLayoutRegPath();
    // 仅当指向本 IME 时删除（避免误删他人）
    HKEY hKey = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, keyPath.c_str(), 0, KEY_READ,
                      &hKey) == ERROR_SUCCESS) {
        wchar_t existing[MAX_PATH] = {0};
        DWORD size = sizeof(existing);
        const LSTATUS q = RegQueryValueExW(hKey, L"Layout File", nullptr, nullptr,
                                           reinterpret_cast<LPBYTE>(existing), &size);
        RegCloseKey(hKey);
        if (q == ERROR_SUCCESS && existing[0] != L'\0' &&
            wcsstr(existing, L"taishen_ime_imm32") == nullptr) {
            return S_OK; // 非本 IME 注册，不删
        }
    }
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, keyPath.c_str());
    return S_OK;
}

// ---------------------------------------------------------------------------
// DLL 入口
// ---------------------------------------------------------------------------

BOOL APIENTRY DllMain(HMODULE hModule, DWORD ul_reason_for_call, LPVOID)
{
    switch (ul_reason_for_call) {
    case DLL_PROCESS_ATTACH:
        g_hModule = hModule;
        // 引擎初始化不在 DllMain（Loader Lock 禁止重活），由 ImeSelect 首次触发
        break;
    case DLL_PROCESS_DETACH:
        g_candidateWindow.Hide();
        break;
    default:
        break;
    }
    return TRUE;
}
