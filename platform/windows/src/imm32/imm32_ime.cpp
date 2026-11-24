/// 泰深输入法 IMM32 兼容层 — Server IPC 薄壳（V3.2）
///
/// 背景：老游戏（LOL Scaleform GFxIME / War3 / 帝国时代等）通过 IMM32 协议
/// 调用输入法，不激活 TSF。V3.2 起本 DLL 摘除内置 Rust 引擎与 D2D/DWrite，
/// 仅保留标准 IMM32 导出、HIMC 组合/结果串状态机与 `TRANSMSGLIST` 消息泵，
/// 词库查询与候选窗渲染统一经命名管道交由 `taishen_server.exe` 完成。
///
/// 安全铁律（BUG-2026-10-31-01）：
/// - winlogon/LogonUI/consent/csrss/smss/services 登录会话宿主直接拒绝服务；
/// - `DllMain(DLL_PROCESS_DETACH)` 严禁任何 UI / IPC / 非平凡析构，客户端句柄
///   采用堆上按需创建、进程退出由系统回收，绝不在 loader lock 下阻塞或触碰已卸载代码。

#include <windows.h>
#include <immdev.h>
#include <imm.h>

#include <algorithm>
#include <cstdio>
#include <cstring>
#include <cwctype>
#include <deque>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

#include "config_reader.h"
#include "app_state.h"
#include "debug_log.h"
#include "ipc_render_client.h"
#include "render_ipc.h"
#include "tsf_keyevent.h"

#pragma comment(lib, "imm32.lib")

namespace {

/// DLL 模块句柄（DllMain 设置）
HMODULE g_hModule = nullptr;

/// IME 窗口类名（ImeInquire 返回，系统为每个 HIMC 创建该类的窗口）
constexpr wchar_t kImeWndClass[] = L"TaishenImeWindow";

/// IPC 客户端（堆上单例、进程退出由 OS 回收，避免 loader lock 内析构）
std::mutex g_clientPtrMutex;
taishen::CRenderIpcClient* g_clientPtr = nullptr;

taishen::CRenderIpcClient& Client()
{
    std::lock_guard<std::mutex> lock(g_clientPtrMutex);
    if (g_clientPtr == nullptr) {
        g_clientPtr = new taishen::CRenderIpcClient();
    }
    return *g_clientPtr;
}

/// 初始化锁 + 全局运行态（受 g_ipcMutex 保护）
std::mutex g_initMutex;
bool g_clientInitialized = false;

std::mutex g_ipcMutex;
uint64_t g_renderSessionId = 0;
uint64_t g_keySeq = 0;
ULONGLONG g_configStamp = 0;
taishen::ImeConfig g_appCfg;
taishen::RemoteEngineMirror g_remote;
bool g_inlinePreedit = true;
bool g_multiRowExpanded = false;
bool g_quoteOpen = false;
bool g_dquoteOpen = false;

/// 组合与消息状态
HIMC g_activeHimc = nullptr;
bool g_composing = false;
bool g_candidateOpen = false;
bool g_candidateNotified = false;

/// GCS_COMPSTR / GCS_RESULTSTR（GFxIME 通过 ImmGetCompositionString 读取）
std::mutex g_strMutex;
std::wstring g_compString;
std::wstring g_resultString;

/// TRANSMSGLIST 待投递队列
constexpr size_t kMaxPendingMessagesPerHimc = 256;
std::mutex g_messageMutex;
std::unordered_map<HIMC, std::deque<TRANSMSG>> g_pendingMessages;

void ClearPendingMessages(HIMC hIMC)
{
    std::lock_guard<std::mutex> lock(g_messageMutex);
    g_pendingMessages.erase(hIMC);
}

std::wstring GetDllDir()
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

std::wstring GetSelfProcessName()
{
    wchar_t path[MAX_PATH] = {0};
    const DWORD len = GetModuleFileNameW(nullptr, path, MAX_PATH);
    if (len == 0) {
        return std::wstring();
    }
    std::wstring p(path, len);
    const size_t slash = p.find_last_of(L"\\/");
    std::wstring file = (slash == std::wstring::npos) ? p : p.substr(slash + 1);
    for (wchar_t& c : file) {
        c = static_cast<wchar_t>(std::towlower(c));
    }
    return file;
}

/// 登录/系统会话宿主拒绝服务（BUG-2026-10-31-01）
bool IsSystemLogonHost()
{
    static const bool isLogonHost = []() {
        const std::wstring exe = GetSelfProcessName();
        return exe == L"winlogon.exe" || exe == L"logonui.exe" ||
               exe == L"consent.exe" || exe == L"csrss.exe" ||
               exe == L"smss.exe" || exe == L"services.exe";
    }();
    return isLogonHost;
}

void EnsureImeWndClass()
{
    static bool registered = false;
    if (registered || IsSystemLogonHost()) {
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

bool KeyDownInState(int vk, CONST LPBYTE lpbKeyState)
{
    if (lpbKeyState != nullptr) {
        return (lpbKeyState[vk & 0xFF] & 0x80) != 0;
    }
    return (GetKeyState(vk) & 0x8000) != 0;
}

bool CapsLockInState(CONST LPBYTE lpbKeyState)
{
    if (lpbKeyState != nullptr) {
        return (lpbKeyState[VK_CAPITAL] & 0x01) != 0;
    }
    return (GetKeyState(VK_CAPITAL) & 0x0001) != 0;
}

void ApplyConfigToLocalStateLocked(const taishen::ImeConfig& cfg)
{
    g_appCfg = cfg;
    g_inlinePreedit =
        taishen::AppStateInlinePreeditForProcess(cfg, GetSelfProcessName());
    taishen::SetPairPunctEnabled(cfg.pair_punct);
    taishen::SetDebugLogEnabled(cfg.debug_log);

    taishen::KeyBindingState kb;
    kb.toggleAsciiCtrlSpace = cfg.keys.toggle_ascii_ctrl_space;
    kb.toggleAsciiShift = cfg.keys.toggle_ascii_shift;
    kb.toggleFullwidth = cfg.keys.toggle_fullwidth;
    kb.togglePunct = cfg.keys.toggle_punct;
    kb.toggleTraditionalKey = cfg.keys.toggle_traditional_key;
    kb.pageCommaPeriod = cfg.keys.page_comma_period;
    kb.pageMinusEqual = cfg.keys.page_minus_equal;
    kb.pageBrackets = cfg.keys.page_brackets;
    kb.pageNumpad = cfg.keys.page_numpad;
    taishen::SetKeyBindings(kb);

    g_remote.asciiPunct = cfg.ascii_punct;
    g_remote.traditional = cfg.traditional_enabled;
    g_remote.pageSize = (std::max)(1, cfg.candidate_count);
}

ULONGLONG ReadConfigStamp(const std::wstring& dllDir)
{
    if (dllDir.empty()) {
        return 0;
    }
    const std::wstring cfgPath = taishen::ResolveConfigPath(dllDir);
    WIN32_FILE_ATTRIBUTE_DATA attrs = {};
    if (GetFileAttributesExW(cfgPath.c_str(), GetFileExInfoStandard, &attrs) == 0) {
        return 0;
    }
    return (static_cast<ULONGLONG>(attrs.ftLastWriteTime.dwHighDateTime) << 32) |
           attrs.ftLastWriteTime.dwLowDateTime;
}

bool EnsureClientReady()
{
    if (IsSystemLogonHost()) {
        return false;
    }
    std::lock_guard<std::mutex> initLock(g_initMutex);
    if (g_clientInitialized) {
        return true;
    }

    const std::wstring dllDir = GetDllDir();
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    const ULONGLONG stamp = ReadConfigStamp(dllDir);

    {
        std::lock_guard<std::mutex> ipcLock(g_ipcMutex);
        if (g_renderSessionId == 0) {
            g_renderSessionId =
                (static_cast<uint64_t>(GetCurrentProcessId()) << 32) ^
                GetTickCount64();
            if (g_renderSessionId == 0) {
                g_renderSessionId = 1;
            }
        }
        g_configStamp = stamp;
        ApplyConfigToLocalStateLocked(cfg);
        taishen::AppStateApplyRemote(g_appCfg, g_remote.asciiMode);
        Client().EnsureServerRunning(dllDir);
    }

    g_clientInitialized = true;
    taishen::DebugLog("imm32: thin IPC client ready, inline_preedit=" +
                      std::to_string(g_inlinePreedit ? 1 : 0));
    return true;
}

void ReloadConfigIfChangedLocked()
{
    const std::wstring dllDir = GetDllDir();
    const ULONGLONG stamp = ReadConfigStamp(dllDir);
    if (stamp == 0 || stamp == g_configStamp) {
        return;
    }
    g_configStamp = stamp;
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    ApplyConfigToLocalStateLocked(cfg);
    taishen::AppStateApplyRemote(g_appCfg, g_remote.asciiMode);
    Client().SendReloadConfig();
    taishen::DebugLog("imm32: config.ini changed, reloaded");
}

void SendRemoteResetCommitLocked()
{
    taishen::ipc::CommitRequest req;
    req.sessionId = g_renderSessionId;
    req.hostPid = GetCurrentProcessId();
    req.index = -1;
    Client().SendCommit(req);
}

void SendRemoteCommitLocked(const std::string& pinyin,
                            const std::string& word,
                            int32_t index)
{
    taishen::ipc::CommitRequest req;
    req.sessionId = g_renderSessionId;
    req.hostPid = GetCurrentProcessId();
    req.pinyin = pinyin;
    req.word = word;
    req.index = index;
    Client().SendCommit(req);
}

void ApplyRemoteResponseLocked(const taishen::ipc::CandidateListResponse& resp)
{
    g_remote.rawPinyin = resp.pinyin;
    g_remote.preedit = resp.preedit;
    g_remote.syllableDisplay =
        resp.syllable.empty() ? resp.pinyin : resp.syllable;
    g_remote.candidates = resp.candidates;
    g_remote.page = resp.page;
    g_remote.totalPages = resp.totalPages;
    g_remote.totalCandidates = resp.totalCandidates;
    if (resp.pageSize > 0) {
        g_remote.pageSize = resp.pageSize;
    }
    g_remote.selectedIndex = resp.selectedIndex;
    g_remote.cursor = resp.cursor;
    g_remote.inCompose = resp.inCompose;
    g_remote.composePrefix = resp.composePrefix;
    g_remote.inputMode = resp.inputMode;
    g_remote.isSymbolPrefix = resp.symbolPrefix;
    g_remote.semicolonKeyActive = resp.semicolonActive;
}

std::wstring BuildDisplayPinyinWideLocked()
{
    std::string combined;
    if (g_remote.inCompose) {
        combined = g_remote.composePrefix + g_remote.syllableDisplay;
    } else if (!g_remote.syllableDisplay.empty()) {
        combined = g_remote.syllableDisplay;
    } else if (!g_remote.rawPinyin.empty()) {
        combined = g_remote.rawPinyin;
    } else {
        combined = g_remote.preedit;
    }
    return taishen::Utf8ToWide(combined);
}

RECT GetCaretRect()
{
    RECT caretRect = {0, 0, 0, 0};
    POINT pt = {};
    HWND hwnd = GetFocus();
    if (hwnd != nullptr && GetCaretPos(&pt)) {
        ClientToScreen(hwnd, &pt);
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.right = pt.x;
        caretRect.bottom = pt.y;
        return caretRect;
    }
    if (GetCursorPos(&pt)) {
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.right = pt.x;
        caretRect.bottom = pt.y;
    }
    return caretRect;
}

void UpdateRemoteCandidateUiLocked(const std::wstring& displayPinyin)
{
    if (g_remote.candidates.empty() ||
        (displayPinyin.empty() && g_remote.candidates.empty())) {
        Client().SendHide(g_renderSessionId);
        g_candidateOpen = false;
        return;
    }

    taishen::ipc::RenderCommand cmd;
    cmd.op = taishen::ipc::RenderOp::Show;
    cmd.sessionId = g_renderSessionId;
    cmd.hostPid = GetCurrentProcessId();
    cmd.pinyin = taishen::WideToUtf8(displayPinyin);
    cmd.candidates.reserve(g_remote.candidates.size());
    for (const auto& c : g_remote.candidates) {
        cmd.candidates.push_back(c.text);
    }
    cmd.caret = GetCaretRect();
    cmd.page = g_remote.page;
    cmd.totalPages = g_remote.totalPages;
    cmd.selectedIndex = g_remote.selectedIndex;
    cmd.cursorPos = g_remote.inCompose ? -1 : g_remote.cursor;
    cmd.inlinePreedit = g_inlinePreedit;
    cmd.multiRow = g_multiRowExpanded;
    Client().SendShow(cmd);
    g_candidateOpen = true;
}

void SyncComposition(HIMC himc, const std::wstring& pinyin,
                     std::vector<TRANSMSG>& msgs)
{
    const bool wasComposing = g_composing;
    const bool nowComposing = !pinyin.empty();
    g_composing = nowComposing;

    if (!nowComposing) {
        {
            std::lock_guard<std::mutex> lock(g_strMutex);
            g_compString.clear();
        }
        if (himc != nullptr && wasComposing) {
            ImmSetCompositionStringW(himc, SCS_SETSTR, nullptr, 0, nullptr, 0);
        }
        if (wasComposing) {
            // GFxIME 需要先收到空 GCS_COMPSTR 再收 WM_IME_ENDCOMPOSITION，
            // 否则上屏字符会被继续高亮为组合串。
            TRANSMSG clearMsg = {};
            clearMsg.message = WM_IME_COMPOSITION;
            clearMsg.lParam = GCS_COMPSTR | GCS_COMPATTR | GCS_CURSORPOS;
            msgs.push_back(clearMsg);

            TRANSMSG endMsg = {};
            endMsg.message = WM_IME_ENDCOMPOSITION;
            msgs.push_back(endMsg);
        }
        return;
    }

    {
        std::lock_guard<std::mutex> lock(g_strMutex);
        g_compString = pinyin;
    }
    if (himc != nullptr) {
        ImmSetCompositionStringW(
            himc, SCS_SETSTR, const_cast<wchar_t*>(pinyin.c_str()),
            static_cast<DWORD>(pinyin.size() * sizeof(wchar_t)), nullptr, 0);
    }

    if (!wasComposing) {
        TRANSMSG startMsg = {};
        startMsg.message = WM_IME_STARTCOMPOSITION;
        msgs.push_back(startMsg);
    }
    TRANSMSG compMsg = {};
    compMsg.message = WM_IME_COMPOSITION;
    compMsg.lParam = GCS_COMPSTR | GCS_COMPATTR | GCS_CURSORPOS;
    msgs.push_back(compMsg);
}

void AppendCommitMessages(const std::wstring& text, std::vector<TRANSMSG>& msgs)
{
    if (text.empty()) {
        return;
    }
    {
        std::lock_guard<std::mutex> lock(g_strMutex);
        g_compString.clear();
        g_resultString = text;
    }
    TRANSMSG m = {};
    m.message = WM_IME_COMPOSITION;
    m.wParam = static_cast<WPARAM>(text[0]);
    m.lParam = GCS_RESULTSTR | GCS_RESULTCLAUSE;
    msgs.push_back(m);
    taishen::DebugLog("imm32 commit: text=" + taishen::WideToUtf8(text) +
                      " msgs=" + std::to_string(msgs.size()));
}

void SyncCandidateNotify(bool open, std::vector<TRANSMSG>& msgs)
{
    if (open == g_candidateNotified) {
        return;
    }
    g_candidateNotified = open;
    TRANSMSG m = {};
    m.message = WM_IME_NOTIFY;
    m.wParam = open ? IMN_OPENCANDIDATE : IMN_CLOSECANDIDATE;
    m.lParam = 1;
    msgs.push_back(m);
}

void CommitFinalText(HIMC himc, const std::wstring& rawText,
                     std::vector<TRANSMSG>& msgs)
{
    std::wstring text = rawText;
    if (taishen::IsFullWidthMode()) {
        text = taishen::ToFullWidth(text);
    }
    if (!text.empty()) {
        const wchar_t last = text.back();
        taishen::g_lastCommitEndsWithDigit = (last >= L'0' && last <= L'9');
        AppendCommitMessages(text, msgs);
    }
    SyncComposition(himc, L"", msgs);
    g_candidateOpen = false;
    SyncCandidateNotify(false, msgs);
}

void ConsumePrefixForPunctLocked(std::string& outPrefixUtf8)
{
    outPrefixUtf8.clear();
    if (g_remote.inCompose) {
        outPrefixUtf8 = g_remote.composePrefix;
        SendRemoteResetCommitLocked();
        g_remote.ClearInput();
    } else if (!g_remote.candidates.empty()) {
        outPrefixUtf8 = g_remote.candidates[0].text;
        const bool isPrefix =
            (g_remote.candidates[0].reasons & taishen::ipc::kReasonPrefixWord) != 0;
        SendRemoteCommitLocked(g_remote.rawPinyin, outPrefixUtf8,
                               isPrefix ? -1 : 0);
        g_remote.ClearInput();
    } else if (!g_remote.rawPinyin.empty()) {
        SendRemoteResetCommitLocked();
        g_remote.ClearInput();
    }
}

UINT FlushMessagesToList(HIMC hIMC, LPTRANSMSGLIST lpTransMsgList,
                         const std::vector<TRANSMSG>& msgs)
{
    const UINT capacity = lpTransMsgList->uMsgCount;
    std::lock_guard<std::mutex> lock(g_messageMutex);
    auto& pending = g_pendingMessages[hIMC];
    if (msgs.size() > kMaxPendingMessagesPerHimc -
                          (std::min)(pending.size(), kMaxPendingMessagesPerHimc)) {
        pending.clear();
        taishen::DebugLog("ImeToAsciiEx: pending message queue overflow, reset");
    }
    for (const TRANSMSG& msg : msgs) {
        if (pending.size() >= kMaxPendingMessagesPerHimc) {
            break;
        }
        pending.push_back(msg);
    }
    const UINT count = (std::min)(capacity, static_cast<UINT>(pending.size()));
    for (UINT i = 0; i < count; ++i) {
        lpTransMsgList->TransMsg[i] = pending.front();
        pending.pop_front();
    }
    lpTransMsgList->uMsgCount = count;
    if (pending.empty()) {
        g_pendingMessages.erase(hIMC);
    }
    return count;
}

std::wstring GetLayoutRegPath()
{
    return L"SYSTEM\\CurrentControlSet\\Control\\Keyboard Layouts\\E0C00804";
}

bool RegSetString(HKEY root, const std::wstring& subKey,
                  const std::wstring& name, const std::wstring& value)
{
    HKEY hKey = nullptr;
    if (RegCreateKeyExW(root, subKey.c_str(), 0, nullptr, 0, KEY_WRITE,
                        nullptr, &hKey, nullptr) != ERROR_SUCCESS) {
        return false;
    }
    const DWORD bytes = static_cast<DWORD>((value.size() + 1) * sizeof(wchar_t));
    const LSTATUS ret = RegSetValueExW(
        hKey, name.c_str(), 0, REG_SZ,
        reinterpret_cast<const BYTE*>(value.c_str()), bytes);
    RegCloseKey(hKey);
    return ret == ERROR_SUCCESS;
}

} // namespace

// ---------------------------------------------------------------------------
// IMM32 标准导出函数
// ---------------------------------------------------------------------------

BOOL WINAPI ImeInquire(LPIMEINFO lpIMEInfo, LPWSTR lpszWndCls,
                       DWORD /*dwSystemInfoFlags*/)
{
    if (IsSystemLogonHost()) {
        return FALSE;
    }
    EnsureImeWndClass();
    if (lpIMEInfo != nullptr) {
        ZeroMemory(lpIMEInfo, sizeof(IMEINFO));
        lpIMEInfo->dwPrivateDataSize = 0;
        lpIMEInfo->fdwProperty =
            IME_PROP_AT_CARET | IME_PROP_UNICODE | IME_PROP_SPECIAL_UI;
        lpIMEInfo->fdwConversionCaps = IME_CMODE_NATIVE | IME_CMODE_FULLSHAPE;
        lpIMEInfo->fdwSentenceCaps = IME_SMODE_NONE;
        lpIMEInfo->fdwUICaps = 0;
        lpIMEInfo->fdwSCSCaps = 0;
        lpIMEInfo->fdwSelectCaps = 0;
    }
    if (lpszWndCls != nullptr) {
        wcsncpy_s(lpszWndCls, IME_UI_CLASS_NAME_SIZE, kImeWndClass, _TRUNCATE);
    }
    return TRUE;
}

DWORD WINAPI ImeConversionList(HIMC /*hIMC*/, LPCWSTR /*lpSrc*/,
                               LPCANDIDATELIST lpDst, DWORD dwBufLen,
                               UINT /*uFlag*/)
{
    if (IsSystemLogonHost() || !EnsureClientReady()) {
        return 0;
    }

    std::vector<std::wstring> cands;
    int selectedIndex = 0;
    {
        std::lock_guard<std::mutex> lock(g_ipcMutex);
        if (g_remote.candidates.empty()) {
            return 0;
        }
        cands.reserve(g_remote.candidates.size());
        for (const auto& item : g_remote.candidates) {
            cands.push_back(taishen::Utf8ToWide(item.text));
        }
        selectedIndex = g_remote.selectedIndex;
    }

    DWORD needed = sizeof(CANDIDATELIST) +
                   static_cast<DWORD>((cands.size() - 1) * sizeof(DWORD));
    for (const auto& w : cands) {
        needed += static_cast<DWORD>((w.size() + 1) * sizeof(wchar_t));
    }
    if (lpDst == nullptr || dwBufLen < needed) {
        return 0;
    }

    lpDst->dwSize = needed;
    lpDst->dwStyle = 0;
    lpDst->dwCount = static_cast<DWORD>(cands.size());
    lpDst->dwSelection =
        (selectedIndex >= 0 && static_cast<size_t>(selectedIndex) < cands.size())
            ? static_cast<DWORD>(selectedIndex)
            : 0;
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

BOOL WINAPI ImeConfigure(HKL /*hKL*/, HWND /*hWnd*/, DWORD /*dwMode*/,
                         LPVOID /*lpData*/)
{
    if (IsSystemLogonHost() || !EnsureClientReady()) {
        return FALSE;
    }
    std::lock_guard<std::mutex> lock(g_ipcMutex);
    return Client().SendOpenSettings(0) ? TRUE : FALSE;
}

BOOL WINAPI ImeDestroy(UINT_PTR /*uForce*/)
{
    if (IsSystemLogonHost()) {
        return TRUE;
    }
    g_composing = false;
    g_candidateOpen = false;
    g_candidateNotified = false;
    g_activeHimc = nullptr;
    {
        std::lock_guard<std::mutex> strLock(g_strMutex);
        g_compString.clear();
        g_resultString.clear();
    }
    std::lock_guard<std::mutex> ipcLock(g_ipcMutex);
    g_remote.ClearInput();
    if (g_clientPtr != nullptr) {
        g_clientPtr->SendHide(g_renderSessionId);
        g_clientPtr->Disconnect();
    }
    return TRUE;
}

LRESULT WINAPI ImeEscape(HIMC /*hIMC*/, UINT uEscape, LPVOID /*lpData*/)
{
    return (uEscape == IME_ESC_QUERY_SUPPORT) ? 1 : 0;
}

BOOL WINAPI ImeProcessKey(HIMC /*hIMC*/, UINT uVirKey, LPARAM /*lParam*/,
                          CONST LPBYTE lpbKeyState)
{
    if (IsSystemLogonHost() || !EnsureClientReady()) {
        return FALSE;
    }
    const int rawVk = static_cast<int>(uVirKey);
    const int effVk = taishen::NormalizeKeypad(rawVk);
    const bool ctrlDown = KeyDownInState(VK_CONTROL, lpbKeyState);
    const bool altDown = KeyDownInState(VK_MENU, lpbKeyState);
    const bool shiftDown = KeyDownInState(VK_SHIFT, lpbKeyState) ||
                           taishen::KeypadShiftEquivalent(rawVk);

    std::lock_guard<std::mutex> lock(g_ipcMutex);
    if (Client().IsProtocolIncompatible()) {
        Client().TryReconnectIfMismatchDue();
    }
    if (Client().IsProtocolIncompatible() || Client().IsInHardBackoff() ||
        !Client().IsServerDictReady()) {
        if (g_remote.rawPinyin.empty() && !g_remote.inCompose) {
            return FALSE;
        }
    }

    const auto& kb = taishen::GetKeyBindings();
    if (effVk == VK_SPACE && ctrlDown && kb.toggleAsciiCtrlSpace) {
        return TRUE;
    }
    if (effVk == VK_SPACE && shiftDown && !ctrlDown && kb.toggleFullwidth == 1) {
        return TRUE;
    }
    if (effVk == VK_OEM_PERIOD && ctrlDown && !shiftDown && kb.togglePunct == 1) {
        return TRUE;
    }
    if (kb.toggleTraditionalKey != 0 && effVk == kb.toggleTraditionalKey &&
        ctrlDown && shiftDown) {
        return TRUE;
    }
    if (taishen::AppStateIsVimForeground(g_appCfg) &&
        taishen::IsVimModeKey(rawVk)) {
        return FALSE;
    }

    const bool vimPass = taishen::AppStateIsVimForeground(g_appCfg);
    const bool eat = taishen::ShouldEatKeyRemote(
        effVk, vimPass, rawVk, g_remote, Client().IsInHardBackoff(),
        Client().IsProtocolIncompatible());
    if (!eat) {
        taishen::UpdateDigitFollowOnPassthrough(effVk, rawVk, shiftDown,
                                                ctrlDown, altDown);
    }
    return eat ? TRUE : FALSE;
}

BOOL WINAPI ImeSelect(HIMC hIMC, BOOL fSelect)
{
    if (IsSystemLogonHost()) {
        return FALSE;
    }
    if (fSelect) {
        if (!EnsureClientReady()) {
            return FALSE;
        }
        g_activeHimc = hIMC;
        std::lock_guard<std::mutex> lock(g_ipcMutex);
        ReloadConfigIfChangedLocked();
        taishen::AppStateApplyRemote(g_appCfg, g_remote.asciiMode);
        Client().EnsureServerRunning(GetDllDir());
    } else {
        ClearPendingMessages(hIMC);
        g_composing = false;
        g_candidateOpen = false;
        g_candidateNotified = false;
        {
            std::lock_guard<std::mutex> strLock(g_strMutex);
            g_compString.clear();
            g_resultString.clear();
        }
        std::lock_guard<std::mutex> ipcLock(g_ipcMutex);
        if (!g_remote.rawPinyin.empty() || g_remote.inCompose) {
            SendRemoteResetCommitLocked();
        }
        g_remote.ClearInput();
        Client().SendHide(g_renderSessionId);
        g_activeHimc = nullptr;
    }
    return TRUE;
}

BOOL WINAPI ImeSetActiveContext(HIMC hIMC, BOOL fActivate)
{
    if (IsSystemLogonHost()) {
        return TRUE;
    }
    if (!fActivate) {
        ClearPendingMessages(hIMC);
        g_composing = false;
        g_candidateOpen = false;
        g_candidateNotified = false;
        {
            std::lock_guard<std::mutex> strLock(g_strMutex);
            g_compString.clear();
            g_resultString.clear();
        }
        if (g_clientInitialized) {
            std::lock_guard<std::mutex> ipcLock(g_ipcMutex);
            if (!g_remote.rawPinyin.empty() || g_remote.inCompose) {
                SendRemoteResetCommitLocked();
            }
            g_remote.ClearInput();
            Client().SendHide(g_renderSessionId);
        }
    }
    return TRUE;
}

UINT WINAPI ImeToAsciiEx(UINT uVirKey, UINT /*uChar*/,
                         CONST LPBYTE lpbKeyState,
                         LPTRANSMSGLIST lpTransMsgList, UINT /*fuState*/,
                         HIMC hIMC)
{
    if (IsSystemLogonHost() || !EnsureClientReady() || lpTransMsgList == nullptr) {
        return 0;
    }

    {
        std::lock_guard<std::mutex> lock(g_strMutex);
        g_resultString.clear();
    }

    const int rawVk = static_cast<int>(uVirKey);
    const int effVk = taishen::NormalizeKeypad(rawVk);
    const bool keypad = taishen::IsKeypadKey(rawVk);
    const bool ctrlDown = KeyDownInState(VK_CONTROL, lpbKeyState);
    const bool altDown = KeyDownInState(VK_MENU, lpbKeyState);
    const bool shiftRaw = KeyDownInState(VK_SHIFT, lpbKeyState);
    const bool shiftDown = shiftRaw || taishen::KeypadShiftEquivalent(rawVk);
    const bool capsLock = CapsLockInState(lpbKeyState);

    std::vector<TRANSMSG> msgs;
    bool needSyncComp = false;
    std::wstring compText;
    bool needUiUpdate = false;
    std::wstring uiPinyin;
    bool needUiHide = false;
    bool notifyOpen = g_candidateNotified;
    std::wstring commitText;
    bool hasCommit = false;

    {
        std::lock_guard<std::mutex> lock(g_ipcMutex);
        ReloadConfigIfChangedLocked();
        taishen::AppStateApplyRemote(g_appCfg, g_remote.asciiMode);
        const auto& kb = taishen::GetKeyBindings();

        auto scheduleClearUi = [&]() {
            needSyncComp = true;
            compText.clear();
            needUiHide = true;
            notifyOpen = false;
        };

        auto scheduleCommit = [&](const std::wstring& text) {
            commitText = text;
            hasCommit = true;
            needSyncComp = true;
            compText.clear();
            needUiHide = true;
            notifyOpen = false;
        };

        // 1) 本地快捷键
        if (effVk == VK_SPACE && ctrlDown && kb.toggleAsciiCtrlSpace) {
            if (!g_remote.rawPinyin.empty() || g_remote.inCompose) {
                SendRemoteResetCommitLocked();
                g_remote.ClearInput();
            }
            g_remote.asciiMode = !g_remote.asciiMode;
            taishen::AppStateSetAsciiRemote(g_remote.asciiMode);
            scheduleClearUi();
        } else if (effVk == VK_SPACE && shiftDown && !ctrlDown &&
                   kb.toggleFullwidth == 1) {
            taishen::SetFullWidthMode(!taishen::IsFullWidthMode());
        } else if (effVk == VK_OEM_PERIOD && ctrlDown && !shiftDown &&
                   kb.togglePunct == 1) {
            g_remote.asciiPunct = !g_remote.asciiPunct;
        } else if (kb.toggleTraditionalKey != 0 &&
                   effVk == kb.toggleTraditionalKey && ctrlDown && shiftDown) {
            g_remote.traditional = !g_remote.traditional;
        } else if ((effVk == VK_UP || effVk == VK_DOWN) &&
                   !g_remote.candidates.empty()) {
            g_multiRowExpanded = (effVk == VK_DOWN);
            needUiUpdate = true;
            uiPinyin = BuildDisplayPinyinWideLocked();
        } else if (effVk == VK_ESCAPE) {
            if (!g_remote.rawPinyin.empty() || g_remote.inCompose) {
                SendRemoteResetCommitLocked();
                g_remote.ClearInput();
                scheduleClearUi();
            }
        } else if (effVk == VK_RETURN && !g_remote.inCompose &&
                   !g_remote.rawPinyin.empty()) {
            const std::wstring pyWide = taishen::Utf8ToWide(g_remote.rawPinyin);
            SendRemoteResetCommitLocked();
            g_remote.ClearInput();
            scheduleCommit(pyWide);
        } else if (effVk == VK_RETURN && !g_remote.inCompose &&
                   g_remote.rawPinyin.empty()) {
            // 透传回车
        } else if (Client().IsProtocolIncompatible()) {
            if (!g_remote.rawPinyin.empty()) {
                scheduleCommit(taishen::Utf8ToWide(g_remote.rawPinyin));
            } else {
                scheduleClearUi();
            }
            g_remote.ClearInput();
        } else {
            // 2) 判断是否需要优先进服务端（组词退格回车/特殊模式/翻页/[ ]/双拼分号）
            bool routeToServer = false;
            bool isPaginationKey = false;
            bool collapseMultiRow = false;

            if (g_remote.inCompose &&
                (effVk == VK_RETURN || (effVk == VK_BACK && !ctrlDown))) {
                routeToServer = true;
            }
            if (!routeToServer &&
                (g_remote.inputMode == 1 || g_remote.inputMode == 2 ||
                 g_remote.inputMode == 3)) {
                const bool isSpecialDigit =
                    (!shiftRaw || effVk == '8' || effVk == '9' || effVk == '0') &&
                    (effVk >= '0' && effVk <= '9');
                const bool isSpecialCalcOp =
                    (g_remote.inputMode == 1 || g_remote.inputMode == 2) &&
                    ((keypad &&
                      (rawVk == VK_ADD || rawVk == VK_SUBTRACT ||
                       rawVk == VK_MULTIPLY || rawVk == VK_DIVIDE ||
                       rawVk == VK_DECIMAL || rawVk == VK_SEPARATOR)) ||
                     (!shiftRaw &&
                      (effVk == VK_OEM_PERIOD || effVk == VK_OEM_PLUS ||
                       effVk == VK_OEM_MINUS || effVk == VK_OEM_2 ||
                       effVk == VK_OEM_5)) ||
                     (shiftRaw && effVk == '6'));
                if (isSpecialDigit || isSpecialCalcOp) {
                    routeToServer = true;
                }
            }
            if (!routeToServer && !g_remote.candidates.empty()) {
                const bool plusMinus =
                    (effVk == VK_OEM_PLUS || effVk == VK_OEM_MINUS) &&
                    (keypad ? kb.pageNumpad : kb.pageMinusEqual);
                const bool commaPeriod =
                    (effVk == VK_OEM_COMMA || effVk == VK_OEM_PERIOD) &&
                    !keypad && !shiftDown && kb.pageCommaPeriod;
                if (effVk == VK_PRIOR || effVk == VK_NEXT || plusMinus ||
                    commaPeriod) {
                    routeToServer = true;
                    isPaginationKey = true;
                }
            }
            if (!routeToServer && (effVk == VK_OEM_4 || effVk == VK_OEM_6) &&
                !shiftDown && kb.pageBrackets && !g_remote.candidates.empty()) {
                routeToServer = true;
            }
            if (!routeToServer && effVk == VK_OEM_1 && !shiftRaw &&
                g_remote.semicolonKeyActive) {
                routeToServer = true;
                collapseMultiRow = true;
            }

            auto tryPunct = [&]() -> bool {
                if (g_remote.asciiMode || g_remote.asciiPunct || ctrlDown ||
                    altDown) {
                    return false;
                }
                if (taishen::IsDigitFollowPunctKey(effVk, shiftDown)) {
                    return false;
                }
                if (effVk == VK_OEM_7) {
                    std::string prefix;
                    ConsumePrefixForPunctLocked(prefix);
                    std::wstring pair;
                    int caretOffset = -1;
                    if (taishen::IsPairPunctEnabled()) {
                        taishen::ExpandPairPunct(shiftDown ? L"“" : L"‘", pair,
                                                 caretOffset);
                    } else if (shiftDown) {
                        pair = g_dquoteOpen ? L"”" : L"“";
                        g_dquoteOpen = !g_dquoteOpen;
                    } else {
                        pair = g_quoteOpen ? L"’" : L"‘";
                        g_quoteOpen = !g_quoteOpen;
                    }
                    scheduleCommit(taishen::Utf8ToWide(prefix) + pair);
                    return true;
                }
                const wchar_t* punct =
                    taishen::MapFullWidthPunctForKey(effVk, shiftDown);
                if (punct != nullptr) {
                    std::string prefix;
                    ConsumePrefixForPunctLocked(prefix);
                    std::wstring pair;
                    int caretOffset = -1;
                    taishen::ExpandPairPunct(std::wstring(punct), pair,
                                             caretOffset);
                    scheduleCommit(taishen::Utf8ToWide(prefix) + pair);
                    return true;
                }
                return false;
            };

            bool doneLocally = false;
            if (!routeToServer) {
                if (tryPunct()) {
                    doneLocally = true;
                } else if (effVk >= 'A' && effVk <= 'Z') {
                    if (!ctrlDown && !altDown && !g_remote.asciiMode) {
                        if (capsLock || shiftRaw) {
                            std::string prefix;
                            ConsumePrefixForPunctLocked(prefix);
                            const wchar_t ch = static_cast<wchar_t>(
                                (capsLock && shiftRaw) ? (effVk + ('a' - 'A'))
                                                       : effVk);
                            std::wstring out = taishen::Utf8ToWide(prefix);
                            out.push_back(ch);
                            scheduleCommit(out);
                            doneLocally = true;
                        } else {
                            routeToServer = true;
                            collapseMultiRow = true;
                        }
                    }
                } else if (effVk == VK_SPACE && !g_remote.candidates.empty()) {
                    const bool canFast =
                        !g_remote.inCompose && g_remote.inputMode == 0 &&
                        !g_remote.isSymbolPrefix &&
                        ((g_remote.candidates[0].reasons &
                          taishen::ipc::kReasonPrefixWord) == 0);
                    if (canFast) {
                        const std::string chosen = g_remote.candidates[0].text;
                        const std::string rawPy = g_remote.rawPinyin;
                        SendRemoteCommitLocked(rawPy, chosen, 0);
                        g_remote.ClearInput();
                        scheduleCommit(taishen::Utf8ToWide(chosen));
                        doneLocally = true;
                    } else {
                        routeToServer = true;
                    }
                } else if (!keypad && effVk >= '1' && effVk <= '9') {
                    if (g_remote.isSymbolPrefix) {
                        routeToServer = true;
                    } else {
                        const size_t idx = static_cast<size_t>(effVk - '1');
                        if (idx < g_remote.candidates.size()) {
                            const bool canFast =
                                !g_remote.inCompose &&
                                g_remote.inputMode == 0 &&
                                ((g_remote.candidates[idx].reasons &
                                  taishen::ipc::kReasonPrefixWord) == 0);
                            if (canFast) {
                                const std::string chosen =
                                    g_remote.candidates[idx].text;
                                const std::string rawPy = g_remote.rawPinyin;
                                SendRemoteCommitLocked(
                                    rawPy, chosen, static_cast<int32_t>(idx));
                                g_remote.ClearInput();
                                scheduleCommit(taishen::Utf8ToWide(chosen));
                                doneLocally = true;
                            } else {
                                routeToServer = true;
                            }
                        }
                    }
                } else if (effVk == VK_BACK &&
                           (!g_remote.rawPinyin.empty() || g_remote.inCompose)) {
                    routeToServer = true;
                    collapseMultiRow = true;
                } else if (effVk == VK_TAB && !g_remote.rawPinyin.empty()) {
                    routeToServer = true;
                    collapseMultiRow = true;
                } else if ((effVk == VK_LEFT || effVk == VK_RIGHT ||
                            effVk == VK_HOME || effVk == VK_END) &&
                           !g_remote.rawPinyin.empty() && !altDown) {
                    routeToServer = true;
                } else if (effVk == VK_DELETE && ctrlDown &&
                           !g_remote.candidates.empty()) {
                    routeToServer = true;
                    collapseMultiRow = true;
                }
            }

            if (!doneLocally && routeToServer) {
                taishen::ipc::KeyInputRequest req;
                req.seq = ++g_keySeq;
                req.sessionId = g_renderSessionId;
                req.hostPid = GetCurrentProcessId();
                req.vk = static_cast<uint32_t>(effVk);
                req.rawVk = static_cast<uint32_t>(rawVk);
                req.modifiers =
                    (shiftDown ? taishen::ipc::kModShift : 0u) |
                    (ctrlDown ? taishen::ipc::kModCtrl : 0u) |
                    (altDown ? taishen::ipc::kModAlt : 0u) |
                    (capsLock ? taishen::ipc::kModCapsLock : 0u);
                req.asciiMode = g_remote.asciiMode;
                req.fresh =
                    g_remote.rawPinyin.empty() && !g_remote.inCompose;
                req.flags = taishen::ipc::kFlagHasTraditional |
                            taishen::ipc::kFlagHasShuangpin |
                            (g_remote.traditional
                                 ? taishen::ipc::kFlagTraditional
                                 : 0u) |
                            (g_appCfg.shuangpin_mode
                                 ? taishen::ipc::kFlagShuangpin
                                 : 0u);

                taishen::ipc::CandidateListResponse resp;
                const bool ipcOk = Client().SendKeyInput(req, resp, 50);
                if (!ipcOk) {
                    if (!g_remote.rawPinyin.empty()) {
                        scheduleCommit(taishen::Utf8ToWide(g_remote.rawPinyin));
                    } else {
                        scheduleClearUi();
                    }
                    g_remote.ClearInput();
                } else if (isPaginationKey && !resp.eaten) {
                    tryPunct();
                } else if (resp.eaten) {
                    if (collapseMultiRow) {
                        g_multiRowExpanded = false;
                    }
                    ApplyRemoteResponseLocked(resp);
                    if (!resp.committed.empty() && !resp.inCompose) {
                        g_remote.ClearInput();
                        scheduleCommit(taishen::Utf8ToWide(resp.committed));
                    } else if (!g_remote.rawPinyin.empty() ||
                               g_remote.inCompose ||
                               !g_remote.candidates.empty()) {
                        const std::wstring displayPy =
                            BuildDisplayPinyinWideLocked();
                        needSyncComp = true;
                        compText = g_inlinePreedit ? displayPy : std::wstring();
                        needUiUpdate = true;
                        uiPinyin = displayPy;
                        notifyOpen = !g_remote.candidates.empty();
                    } else {
                        g_remote.ClearInput();
                        scheduleClearUi();
                    }
                }
            }
        }

        if (needUiHide) {
            Client().SendHide(g_renderSessionId);
            g_candidateOpen = false;
        } else if (needUiUpdate) {
            UpdateRemoteCandidateUiLocked(uiPinyin);
            notifyOpen = g_candidateOpen;
        }
    }

    if (hasCommit) {
        CommitFinalText(hIMC, commitText, msgs);
    } else {
        if (needSyncComp) {
            SyncComposition(hIMC, compText, msgs);
        }
        SyncCandidateNotify(notifyOpen, msgs);
    }

    return FlushMessagesToList(hIMC, lpTransMsgList, msgs);
}

LRESULT WINAPI ImeGetImeMenuItems(HIMC /*hIMC*/, DWORD /*dwFlags*/,
                                  DWORD /*dwType*/,
                                  LPIMEMENUITEMINFO /*lpImeParentMenu*/,
                                  LPIMEMENUITEMINFO /*lpImeMenu*/,
                                  DWORD /*dwSize*/)
{
    return 0;
}

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

BOOL WINAPI ImeSetCompositionString(HIMC /*hIMC*/, DWORD /*dwIndex*/,
                                    LPCVOID /*lpComp*/, DWORD /*dwCompSize*/,
                                    LPCVOID /*lpRead*/, DWORD /*dwReadSize*/)
{
    return TRUE;
}

LONG WINAPI ImeGetCompositionStringW(HIMC /*hIMC*/, DWORD dwIndex,
                                     LPVOID lpBuf, DWORD dwBufLen)
{
    std::lock_guard<std::mutex> lock(g_strMutex);
    switch (dwIndex) {
    case GCS_CURSORPOS:
        return static_cast<LONG>(g_compString.size());
    case GCS_DELTASTART:
        return 0;
    case GCS_COMPSTR: {
        const DWORD bytes =
            static_cast<DWORD>(g_compString.size() * sizeof(wchar_t));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(bytes);
        }
        const DWORD copyLen = (std::min)(dwBufLen, bytes);
        memcpy(lpBuf, g_compString.data(), copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_RESULTSTR: {
        const DWORD bytes =
            static_cast<DWORD>(g_resultString.size() * sizeof(wchar_t));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(bytes);
        }
        const DWORD copyLen = (std::min)(dwBufLen, bytes);
        memcpy(lpBuf, g_resultString.data(), copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_COMPATTR: {
        const DWORD count = static_cast<DWORD>(g_compString.size());
        if (count == 0) {
            return 0;
        }
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(count);
        }
        const DWORD copyLen = (std::min)(dwBufLen, count);
        memset(lpBuf, 0, copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_COMPCLAUSE: {
        if (g_compString.empty()) {
            return 0;
        }
        const DWORD clause[2] = {0, static_cast<DWORD>(g_compString.size())};
        constexpr DWORD needed = static_cast<DWORD>(sizeof(clause));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(needed);
        }
        const DWORD copyLen = (std::min)(dwBufLen, needed);
        memcpy(lpBuf, clause, copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_RESULTCLAUSE: {
        if (g_resultString.empty()) {
            return 0;
        }
        const DWORD clause[2] = {0, static_cast<DWORD>(g_resultString.size())};
        constexpr DWORD needed = static_cast<DWORD>(sizeof(clause));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(needed);
        }
        const DWORD copyLen = (std::min)(dwBufLen, needed);
        memcpy(lpBuf, clause, copyLen);
        return static_cast<LONG>(copyLen);
    }
    default:
        return 0;
    }
}

LONG WINAPI ImeGetCompositionStringA(HIMC /*hIMC*/, DWORD dwIndex,
                                     LPVOID lpBuf, DWORD dwBufLen)
{
    std::wstring compWide;
    std::wstring resultWide;
    {
        std::lock_guard<std::mutex> lock(g_strMutex);
        compWide = g_compString;
        resultWide = g_resultString;
    }

    auto toAnsi = [](const std::wstring& w) -> std::string {
        if (w.empty()) {
            return std::string();
        }
        const int need = WideCharToMultiByte(CP_ACP, 0, w.c_str(),
                                             static_cast<int>(w.size()),
                                             nullptr, 0, nullptr, nullptr);
        if (need <= 0) {
            return std::string();
        }
        std::string s(static_cast<size_t>(need), '\0');
        WideCharToMultiByte(CP_ACP, 0, w.c_str(), static_cast<int>(w.size()),
                            &s[0], need, nullptr, nullptr);
        return s;
    };

    switch (dwIndex) {
    case GCS_CURSORPOS:
        return static_cast<LONG>(toAnsi(compWide).size());
    case GCS_DELTASTART:
        return 0;
    case GCS_COMPSTR: {
        const std::string s = toAnsi(compWide);
        const DWORD bytes = static_cast<DWORD>(s.size());
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(bytes);
        }
        const DWORD copyLen = (std::min)(dwBufLen, bytes);
        memcpy(lpBuf, s.data(), copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_RESULTSTR: {
        const std::string s = toAnsi(resultWide);
        const DWORD bytes = static_cast<DWORD>(s.size());
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(bytes);
        }
        const DWORD copyLen = (std::min)(dwBufLen, bytes);
        memcpy(lpBuf, s.data(), copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_COMPATTR: {
        const std::string s = toAnsi(compWide);
        const DWORD count = static_cast<DWORD>(s.size());
        if (count == 0) {
            return 0;
        }
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(count);
        }
        const DWORD copyLen = (std::min)(dwBufLen, count);
        memset(lpBuf, 0, copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_COMPCLAUSE: {
        const std::string s = toAnsi(compWide);
        if (s.empty()) {
            return 0;
        }
        const DWORD clause[2] = {0, static_cast<DWORD>(s.size())};
        constexpr DWORD needed = static_cast<DWORD>(sizeof(clause));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(needed);
        }
        const DWORD copyLen = (std::min)(dwBufLen, needed);
        memcpy(lpBuf, clause, copyLen);
        return static_cast<LONG>(copyLen);
    }
    case GCS_RESULTCLAUSE: {
        const std::string s = toAnsi(resultWide);
        if (s.empty()) {
            return 0;
        }
        const DWORD clause[2] = {0, static_cast<DWORD>(s.size())};
        constexpr DWORD needed = static_cast<DWORD>(sizeof(clause));
        if (lpBuf == nullptr || dwBufLen == 0) {
            return static_cast<LONG>(needed);
        }
        const DWORD copyLen = (std::min)(dwBufLen, needed);
        memcpy(lpBuf, clause, copyLen);
        return static_cast<LONG>(copyLen);
    }
    default:
        return 0;
    }
}

BOOL WINAPI ImeSetCompositionFont(HIMC /*hIMC*/, LPLOGFONTW /*lpLogFont*/)
{
    return TRUE;
}

BOOL WINAPI ImeSetCompositionWindow(HIMC /*hIMC*/, LPCANDIDATEFORM /*lpCompForm*/)
{
    return TRUE;
}

LRESULT WINAPI ImeNotify(HIMC /*hIMC*/, DWORD /*dwCommand*/, DWORD /*dwData*/)
{
    return 0;
}

LRESULT WINAPI UIWndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    return DefWindowProcW(hWnd, msg, wParam, lParam);
}

LRESULT WINAPI ImeUIWndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    return DefWindowProcW(hWnd, msg, wParam, lParam);
}

BOOL WINAPI NotifyIME(HIMC /*hIMC*/, DWORD /*dwAction*/, DWORD /*dwIndex*/,
                      DWORD /*dwValue*/)
{
    return FALSE;
}

STDAPI DllRegisterServer(void)
{
    const std::wstring keyPath = GetLayoutRegPath();
    HKEY hKey = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, keyPath.c_str(), 0, KEY_READ,
                      &hKey) == ERROR_SUCCESS) {
        wchar_t existing[MAX_PATH] = {0};
        DWORD size = sizeof(existing);
        const LSTATUS q = RegQueryValueExW(
            hKey, L"Layout File", nullptr, nullptr,
            reinterpret_cast<LPBYTE>(existing), &size);
        RegCloseKey(hKey);
        if (q == ERROR_SUCCESS && existing[0] != L'\0' &&
            wcsstr(existing, L"taishen_ime_imm32") == nullptr) {
            return E_ACCESSDENIED;
        }
    }

    wchar_t dllPath[MAX_PATH] = {0};
    GetModuleFileNameW(g_hModule, dllPath, MAX_PATH);
    if (!RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"Layout File", dllPath) ||
        !RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"IME File", dllPath) ||
        !RegSetString(HKEY_LOCAL_MACHINE, keyPath, L"Layout Text", L"泰深拼音")) {
        return E_FAIL;
    }
    return S_OK;
}

STDAPI DllUnregisterServer(void)
{
    const std::wstring keyPath = GetLayoutRegPath();
    HKEY hKey = nullptr;
    if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, keyPath.c_str(), 0, KEY_READ,
                      &hKey) == ERROR_SUCCESS) {
        wchar_t existing[MAX_PATH] = {0};
        DWORD size = sizeof(existing);
        const LSTATUS q = RegQueryValueExW(
            hKey, L"Layout File", nullptr, nullptr,
            reinterpret_cast<LPBYTE>(existing), &size);
        RegCloseKey(hKey);
        if (q == ERROR_SUCCESS && existing[0] != L'\0' &&
            wcsstr(existing, L"taishen_ime_imm32") == nullptr) {
            return S_OK;
        }
    }
    RegDeleteTreeW(HKEY_LOCAL_MACHINE, keyPath.c_str());
    return S_OK;
}

BOOL APIENTRY DllMain(HMODULE hModule, DWORD ul_reason_for_call, LPVOID)
{
    switch (ul_reason_for_call) {
    case DLL_PROCESS_ATTACH:
        g_hModule = hModule;
        DisableThreadLibraryCalls(hModule);
        break;
    case DLL_PROCESS_DETACH:
        // 严禁在此销毁 IPC 客户端或触碰系统资源（BUG-2026-10-31-01）
        break;
    default:
        break;
    }
    return TRUE;
}
