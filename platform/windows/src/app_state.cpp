/// per-app 状态记忆实现（V0.2.33）

#include "app_state.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>
#include <mutex>
#include <unordered_map>

#include "config_reader.h"
#include "debug_log.h"
#ifndef TAISHEN_THIN_DLL
#include "engine_bridge.h"
#endif
#include "tsf_keyevent.h"  // WideToUtf8

namespace taishen {

namespace {

/// 单进程记忆条目
struct AppState {
    bool ascii = false;  ///< 记忆的中英状态
    bool init = false;   ///< 是否已记录（区分「未进入过」与「记忆为中文」）
};

std::mutex g_appStateMutex;
std::unordered_map<std::wstring, AppState> g_appStates;  ///< key: 小写进程名
std::wstring g_currentProc;    ///< 最近一次应用过状态的进程（OnKeyDown 兜底幂等）
bool g_currentInlineHit = false;  ///< 当前进程是否命中 app_inline
bool g_currentVimHit = false;     ///< 当前进程是否命中 app_vim

/// 出厂程序兼容表（V0.2.35，对标搜狗内置兼容数据库）：
/// 终端类/编辑器类默认英文——出厂即用。用户 app_ascii 配置叠加生效（并集）。
/// 均为合理行为（终端输入命令天然英文），无副作用，不需要用户可关闭。
const wchar_t* const kBuiltinAppAscii[] = {
    L"cmd.exe", L"powershell.exe", L"pwsh.exe",
    L"wt.exe", L"windowsterminal.exe", L"conhost.exe", L"mintty.exe",
    L"nvim-qt.exe",
};

/// 命中出厂兼容表（小写进程名）
bool InBuiltinAppAscii(const std::wstring& proc)
{
    for (const wchar_t* name : kBuiltinAppAscii) {
        if (proc == name) {
            return true;
        }
    }
    return false;
}

/// 出厂兼容表：不渲染 TSF 组合串的宿主（小写进程名）。
/// 当前为空——不按宿主特调。拼音可见性由候选窗承担：
/// 候选窗的拼音行无条件绘制，任何宿主画不出行内拼音时，
/// 用户仍能在候选窗看到自己打了什么。
const wchar_t* const kBuiltinNoInline[] = {
    L"",  // 占位：永不匹配
};

bool InBuiltinNoInline(const std::wstring& proc)
{
    for (const wchar_t* name : kBuiltinNoInline) {
        if (proc == name) {
            return true;
        }
    }
    return false;
}

/// 出厂强制自绘候选窗的宿主（小写进程名）。
/// 当前为空——魔兽世界实测会通过 msctfime 桥接自己画候选，
/// 强制自绘会造成「游戏候选 + 泰深候选」双窗。保留机制供真不画的宿主。
const wchar_t* const kBuiltinSelfDraw[] = {
    L"",  // 占位：永不匹配
};

bool InBuiltinSelfDraw(const std::wstring& proc)
{
    for (const wchar_t* name : kBuiltinSelfDraw) {
        if (proc == name) {
            return true;
        }
    }
    return false;
}

/// 获取当前前台窗口进程名（小写，如 "cmd.exe"）。失败返回空串。
std::wstring GetForegroundProcessName()
{
    const HWND fg = GetForegroundWindow();
    if (fg == nullptr) {
        return std::wstring();
    }
    DWORD pid = 0;
    GetWindowThreadProcessId(fg, &pid);
    if (pid == 0) {
        return std::wstring();
    }
    HANDLE h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (h == nullptr) {
        return std::wstring();
    }
    wchar_t name[MAX_PATH] = {0};
    DWORD sz = MAX_PATH;
    QueryFullProcessImageNameW(h, 0, name, &sz);
    CloseHandle(h);
    std::wstring full(name, sz);
    const size_t slash = full.find_last_of(L"\\/");
    std::wstring file = (slash == std::wstring::npos) ? full : full.substr(slash + 1);
    std::transform(file.begin(), file.end(), file.begin(), ::towlower);
    return file;
}

}  // namespace

bool AppStateIsVimForeground(const ImeConfig& cfg)
{
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return false;
    }
    return std::find(cfg.app_vim_list.begin(), cfg.app_vim_list.end(), proc) !=
           cfg.app_vim_list.end();
}

bool AppStateInlinePreeditForProcess(const ImeConfig& cfg,
                                     const std::wstring& proc)
{
    if (!proc.empty()) {
        // 显式配置优先：强制开 > 强制关
        if (std::find(cfg.app_inline_list.begin(), cfg.app_inline_list.end(),
                      proc) != cfg.app_inline_list.end()) {
            return true;
        }
        if (std::find(cfg.app_no_inline_list.begin(),
                      cfg.app_no_inline_list.end(),
                      proc) != cfg.app_no_inline_list.end()) {
            return false;
        }
        // 出厂兼容表：这些宿主把 TSF 组合串当正文插入，行内拼音会漏进输入框
        if (InBuiltinNoInline(proc)) {
            return false;
        }
    }
    return cfg.inline_preedit;
}

bool AppStateInlinePreeditEnabled(const ImeConfig& cfg)
{
    return AppStateInlinePreeditForProcess(cfg, GetForegroundProcessName());
}

bool AppStateShouldSelfDrawWhenSuppressed(const ImeConfig& cfg, bool hostReadData)
{
    const std::wstring proc = GetForegroundProcessName();
    if (!proc.empty()) {
        // 显式配置优先：强制画 / 禁止画
        if (std::find(cfg.app_self_draw_list.begin(),
                      cfg.app_self_draw_list.end(),
                      proc) != cfg.app_self_draw_list.end()) {
            DebugLog("SelfDraw: " + WideToUtf8(proc) + " forced by app_self_draw");
            return true;
        }
        if (std::find(cfg.app_no_self_draw_list.begin(),
                      cfg.app_no_self_draw_list.end(),
                      proc) != cfg.app_no_self_draw_list.end()) {
            DebugLog("SelfDraw: " + WideToUtf8(proc) + " disabled by app_no_self_draw");
            return false;
        }
        // 出厂强制自绘：msctfime 桥接替宿主读过数据，自动判据失效，但宿主自己不画
        if (InBuiltinSelfDraw(proc)) {
            DebugLog("SelfDraw: " + WideToUtf8(proc) + " forced by builtin");
            return true;
        }
    }
    // 自动：宿主读过候选数据说明它自己会画；没读过就是一概不管（游戏）
    return !hostReadData;
}

AppStateResult ApplyAppStateCore(const ImeConfig& cfg, bool& ioAscii, bool syncLocalEngine)
{
    AppStateResult result;
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return result;
    }
    result.proc = proc;

    const bool currentAscii =
#ifndef TAISHEN_THIN_DLL
        syncLocalEngine ? (engine_get_ascii_mode() == 1) :
#endif
                        ioAscii;
    bool ascii = currentAscii;
    {
        std::lock_guard<std::mutex> lock(g_appStateMutex);
        if (proc == g_currentProc) {
            result.inline_hit = g_currentInlineHit;
            result.vim_hit = g_currentVimHit;
            if (!syncLocalEngine) {
                const auto it = g_appStates.find(proc);
                if (it != g_appStates.end() && it->second.init && it->second.ascii != ioAscii) {
                    ioAscii = it->second.ascii;
                    result.ascii_changed = true;
                }
            }
            return result;
        }
        g_currentProc = proc;
        g_currentInlineHit =
            std::find(cfg.app_inline_list.begin(), cfg.app_inline_list.end(),
                      proc) != cfg.app_inline_list.end();
        result.inline_hit = g_currentInlineHit;
        g_currentVimHit =
            std::find(cfg.app_vim_list.begin(), cfg.app_vim_list.end(), proc) !=
            cfg.app_vim_list.end();
        result.vim_hit = g_currentVimHit;

        const auto it = g_appStates.find(proc);
        if (it != g_appStates.end() && it->second.init) {
            ascii = it->second.ascii;
        } else {
            const bool inAscii =
                std::find(cfg.app_ascii_list.begin(), cfg.app_ascii_list.end(),
                          proc) != cfg.app_ascii_list.end() ||
                InBuiltinAppAscii(proc);
            const bool inCn =
                std::find(cfg.app_cn_list.begin(), cfg.app_cn_list.end(), proc) !=
                cfg.app_cn_list.end();
            if (inAscii) {
                ascii = true;
            } else if (inCn) {
                ascii = false;
            } else {
                ascii = currentAscii;
            }
            g_appStates[proc] = {ascii, true};
        }
    }

    if (ascii != currentAscii) {
#ifndef TAISHEN_THIN_DLL
        if (syncLocalEngine) {
            // V0.3.x：set_ascii_mode 不再清空拼音（用户 shift 切换保留已打内容），
            // 进程记忆恢复是全新环境 → 这里显式清空，避免残留拼音跨应用。
            engine_reset();
            engine_set_ascii_mode(ascii ? 1 : 0);
        }
#endif
        ioAscii = ascii;
        result.ascii_changed = true;
        DebugLog(std::string(syncLocalEngine ? "AppStateApply: " : "AppStateApplyRemote: ") +
                 WideToUtf8(proc) + " -> " + (ascii ? "ascii" : "cn"));
    }
    return result;
}

#ifndef TAISHEN_THIN_DLL
AppStateResult AppStateApply(const ImeConfig& cfg)
{
    bool dummyAscii = false;
    return ApplyAppStateCore(cfg, dummyAscii, true);
}
#endif

AppStateResult AppStateApplyRemote(const ImeConfig& cfg, bool& ioAsciiMode)
{
    return ApplyAppStateCore(cfg, ioAsciiMode, false);
}

void UpdateProcAsciiMemory(bool ascii, const char* logTag)
{
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return;
    }
    {
        std::lock_guard<std::mutex> lock(g_appStateMutex);
        g_appStates[proc] = {ascii, true};
    }
    DebugLog(std::string(logTag) + ": " + WideToUtf8(proc) + " -> " +
             (ascii ? "ascii" : "cn"));
}

#ifndef TAISHEN_THIN_DLL
void AppStateSetAscii(bool ascii)
{
    // 中英文模式切换结束当前拼音事务，避免中文模式残留组合串在英文模式
    // 继续存在；同模式重复设置不应清空用户正在输入的内容。
    if (ascii != (engine_get_ascii_mode() == 1)) {
        engine_reset();
        engine_set_ascii_mode(ascii ? 1 : 0);
    }
    UpdateProcAsciiMemory(ascii, "AppStateSetAscii");
}
#endif

void AppStateSetAsciiRemote(bool ascii)
{
    UpdateProcAsciiMemory(ascii, "AppStateSetAsciiRemote");
}

bool AppStateGetAsciiRemote(bool fallback)
{
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return fallback;
    }
    std::lock_guard<std::mutex> lock(g_appStateMutex);
    const auto it = g_appStates.find(proc);
    if (it != g_appStates.end() && it->second.init) {
        return it->second.ascii;
    }
    return fallback;
}

}  // namespace taishen
