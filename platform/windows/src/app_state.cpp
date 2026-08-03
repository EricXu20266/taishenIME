/// per-app 状态记忆实现（V0.2.33）

#include "app_state.h"

#include <windows.h>

#include <algorithm>
#include <cwctype>
#include <mutex>
#include <unordered_map>

#include "config_reader.h"
#include "debug_log.h"
#include "engine_bridge.h"
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

AppStateResult AppStateApply(const ImeConfig& cfg)
{
    AppStateResult result;
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return result;
    }
    result.proc = proc;

    bool ascii = false;
    {
        std::lock_guard<std::mutex> lock(g_appStateMutex);
        if (proc == g_currentProc) {
            // 进程未变化：引擎状态即该进程最新状态（手动切换已由
            // AppStateSetAscii 同步），无需重新应用，幂等返回。
            result.inline_hit = g_currentInlineHit;
            result.vim_hit = g_currentVimHit;
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
            // 已有记忆（用户手动切换过）：应用记忆状态
            ascii = it->second.ascii;
        } else {
            // 首次进入：按配置定初始状态
            const bool inAscii =
                std::find(cfg.app_ascii_list.begin(), cfg.app_ascii_list.end(),
                          proc) != cfg.app_ascii_list.end() ||
                InBuiltinAppAscii(proc);  // V0.2.35 出厂兼容表叠加
            const bool inCn =
                std::find(cfg.app_cn_list.begin(), cfg.app_cn_list.end(), proc) !=
                cfg.app_cn_list.end();
            if (inAscii) {
                ascii = true;
            } else if (inCn) {
                ascii = false;
            } else {
                // 未配置：继承当前引擎状态（无配置时行为与现状完全一致）
                ascii = (engine_get_ascii_mode() == 1);
            }
            g_appStates[proc] = {ascii, true};
        }
    }

    if (ascii != (engine_get_ascii_mode() == 1)) {
        engine_set_ascii_mode(ascii ? 1 : 0);
        result.ascii_changed = true;
        DebugLog("AppStateApply: " + WideToUtf8(proc) + " -> " +
                 (ascii ? "ascii" : "cn"));
    }
    return result;
}

void AppStateSetAscii(bool ascii)
{
    engine_set_ascii_mode(ascii ? 1 : 0);
    const std::wstring proc = GetForegroundProcessName();
    if (proc.empty()) {
        return;
    }
    {
        std::lock_guard<std::mutex> lock(g_appStateMutex);
        g_appStates[proc] = {ascii, true};
        // 注意：不动 g_currentProc / g_currentInlineHit——
        // 若缓存落后，下次 AppStateApply 会走「进程变化」分支重算；
        // 若一致，引擎状态刚被本函数更新，AppStateApply 幂等返回即可。
    }
    DebugLog("AppStateSetAscii: " + WideToUtf8(proc) + " -> " +
             (ascii ? "ascii" : "cn"));
}

}  // namespace taishen
