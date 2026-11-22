/// 泰深输入法进程外引擎与渲染服务（V4 D2+D3：`taishen_server.exe`）
///
/// 职责：
///   1. 以当前用户普通权限常驻（单实例），绕开 AppContainer 内 D2D 建窗与文件代理阻塞；
///   2. 启动时一次性加载全量词库（system_dict.db / mmap 容器）与 %APPDATA% 用户词库；
///   3. 监听命名管道 `\\.\pipe\taishen-ime-render`（DACL 含 AC + LPAC，SACL 含 LW）；
///   4. 线程模型：
///      - 管道工作线程持 `engineMutex` 处理 `KeyInput`/`Commit`（不阻塞 UI/D2D 渲染）；
///      - 渲染指令（`Show`/`Move`/`Hide`/`ReloadConfig`）PostMessage 到 UI 线程执行；
///      - UI 线程 2s `WM_TIMER` 负责热重载 `config.ini` 与周期落盘用户学习队列。

#include <windows.h>
#include <sddl.h>

#include <atomic>
#include <cstdio>
#include <memory>
#include <mutex>
#include <string>
#include <thread>

#include "candidate_window.h"
#include "config_apply.h"
#include "config_reader.h"
#include "debug_log.h"
#include "dpi_util.h"
#include "engine_bridge.h"
#include "engine_text.h"
#include "render_ipc.h"
#include "theme.h"

namespace {

constexpr UINT WM_APP_RENDER_CMD = WM_APP + 101;
constexpr UINT_PTR kTimerMaintenanceId = 1;
constexpr UINT kTimerMaintenanceMs = 2000;
constexpr wchar_t kServerMsgClass[] = L"TaishenImeServerMsgWindow";

struct ServerState {
    taishen::CCandidateWindow candidateWindow;
    std::wstring exeDir;
    std::wstring dictDir;
    std::wstring configDir;
    std::wstring configPath;
    FILETIME configMtime = {};
    taishen::ImeConfig cfg;

    // 渲染活跃会话（UI 线程独占）
    uint64_t activeSessionId = 0;
    taishen::ipc::RenderCommand lastShow;
    bool hasLastShow = false;

    // 引擎互斥锁与活跃输入会话（管道工作线程 + 维护定时器共用）
    std::mutex engineMutex;
    uint64_t activeInputSessionId = 0;
    std::atomic<bool> engineReady{false};
    std::atomic<bool> shuttingDown{false};
    HWND msgHwnd = nullptr;
};

ServerState* g_state = nullptr;

std::string WideToUtf8(const std::wstring& wide)
{
    if (wide.empty()) {
        return std::string();
    }
    const int len = WideCharToMultiByte(
        CP_UTF8, 0, wide.c_str(), static_cast<int>(wide.size()),
        nullptr, 0, nullptr, nullptr);
    if (len <= 0) {
        return std::string();
    }
    std::string out(static_cast<size_t>(len), '\0');
    WideCharToMultiByte(
        CP_UTF8, 0, wide.c_str(), static_cast<int>(wide.size()),
        &out[0], len, nullptr, nullptr);
    return out;
}

bool FileExistsW(const std::wstring& path)
{
    const DWORD attrs = GetFileAttributesW(path.c_str());
    return attrs != INVALID_FILE_ATTRIBUTES && (attrs & FILE_ATTRIBUTE_DIRECTORY) == 0;
}

std::wstring GetExeDirectory()
{
    wchar_t path[MAX_PATH] = {0};
    const DWORD len = GetModuleFileNameW(nullptr, path, MAX_PATH);
    if (len == 0 || len >= MAX_PATH) {
        return L"";
    }
    std::wstring s(path, len);
    const size_t slash = s.find_last_of(L"\\/");
    return (slash != std::wstring::npos) ? s.substr(0, slash + 1) : L"";
}

/// 解析词库目录：优先同目录；缺失时回退 %LOCALAPPDATA%\TaishenIME\（适配 uiAccess 部署到 Program Files）
/// 或开发期仓库 resources\ 目录。
std::wstring ResolveServerDictDir(const std::wstring& exeDir)
{
    if (FileExistsW(exeDir + L"system_dict.db")) {
        return exeDir;
    }
    wchar_t localBuf[MAX_PATH] = {0};
    const DWORD n = GetEnvironmentVariableW(L"LOCALAPPDATA", localBuf, MAX_PATH);
    if (n > 0 && n < MAX_PATH) {
        const std::wstring installed = std::wstring(localBuf) + L"\\TaishenIME\\";
        if (FileExistsW(installed + L"system_dict.db")) {
            return installed;
        }
    }
    const std::wstring devResources = exeDir + L"..\\..\\..\\resources\\";
    if (FileExistsW(devResources + L"system_dict.db")) {
        return devResources;
    }
    return exeDir;
}

std::wstring ResolveServerConfigDir(const std::wstring& exeDir, const std::wstring& dictDir)
{
    if (FileExistsW(dictDir + L"config.ini")) {
        return dictDir;
    }
    return exeDir;
}

/// 合成单个虚拟键按下与抬起（中完整性 server 投递给前台宿主，包括 AppContainer/UWP）
void SendVirtualKey(WORD vk)
{
    INPUT inputs[2]{};
    inputs[0].type = INPUT_KEYBOARD;
    inputs[0].ki.wVk = vk;
    inputs[1].type = INPUT_KEYBOARD;
    inputs[1].ki.wVk = vk;
    inputs[1].ki.dwFlags = KEYEVENTF_KEYUP;
    SendInput(2, inputs, sizeof(INPUT));
}

void ApplyServerUiConfig(ServerState& st, const taishen::ImeConfig& cfg)
{
    taishen::SetDebugLogEnabled(cfg.debug_log);
    st.candidateWindow.SetTheme(cfg.theme);
    st.candidateWindow.SetFollowSystemTheme(!cfg.userThemeExplicit);
    st.candidateWindow.SetFont(cfg.font_face, cfg.font_size);
    st.candidateWindow.SetLabelFormat(cfg.label_format);
    st.candidateWindow.SetLayout(cfg.corner_radius, cfg.hilite_corner_radius,
                                 cfg.padding, cfg.candidate_spacing);
    st.candidateWindow.SetInlinePreedit(cfg.inline_preedit);
}

void ApplyServerEngineConfig(const taishen::ImeConfig& cfg, const std::wstring& dictDir)
{
    taishen::ApplyConfigToEngine(cfg);
    engine_set_phrase_enabled(cfg.phrase_enabled ? 1 : 0);
    if (!cfg.phrase_path.empty()) {
        std::wstring p = cfg.phrase_path;
        if (p.size() < 2 || p[1] != L':') {
            p = dictDir + p;
        }
        const std::string utf8 = WideToUtf8(p);
        engine_set_phrase_path(utf8.empty() ? nullptr : utf8.c_str());
    }
}

void ApplyServerConfigAll(ServerState& st)
{
    const taishen::ImeConfig cfg = taishen::LoadConfig(st.configDir);
    ApplyServerUiConfig(st, cfg);
    {
        std::lock_guard<std::mutex> lock(st.engineMutex);
        st.cfg = cfg;
        if (st.engineReady.load() || engine_get_ascii_mode() >= 0) {
            ApplyServerEngineConfig(cfg, st.dictDir);
        }
    }
}

void InitConfigWatch(ServerState& st)
{
    st.configPath = st.configDir + L"config.ini";
    WIN32_FILE_ATTRIBUTE_DATA attrs{};
    if (GetFileAttributesExW(st.configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        st.configMtime = attrs.ftLastWriteTime;
    }
}

void ReloadServerConfigIfChanged(ServerState& st)
{
    if (st.configPath.empty()) {
        return;
    }
    WIN32_FILE_ATTRIBUTE_DATA attrs{};
    if (!GetFileAttributesExW(st.configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        return;
    }
    if (attrs.ftLastWriteTime.dwHighDateTime == st.configMtime.dwHighDateTime &&
        attrs.ftLastWriteTime.dwLowDateTime == st.configMtime.dwLowDateTime) {
        return;
    }
    st.configMtime = attrs.ftLastWriteTime;
    ApplyServerConfigAll(st);
    taishen::DebugLog("taishen_server: config.ini reloaded");
}

void InitServerEngineWorker(ServerState* st)
{
    const ULONGLONG t0 = GetTickCount64();

    taishen::ImeConfig cfg;
    {
        std::lock_guard<std::mutex> lock(st->engineMutex);
        cfg = st->cfg;
    }

    std::wstring dictPath = taishen::ResolveDictPath(cfg, st->dictDir);
    if (dictPath.empty() && FileExistsW(st->dictDir + L"system_dict.db")) {
        dictPath = st->dictDir + L"system_dict.db";
    }
    const std::string dictPathUtf8 = WideToUtf8(dictPath);
    const std::wstring userDictPath = taishen::ResolveUserDictPath(cfg, st->dictDir);
    const std::string userDictUtf8 = WideToUtf8(userDictPath);

    {
        std::lock_guard<std::mutex> lock(st->engineMutex);
        const int initRet = engine_init(dictPathUtf8.empty() ? nullptr : dictPathUtf8.c_str());
        if (initRet != 0) {
            taishen::ForceLog("taishen_server: engine_init failed ret=" + std::to_string(initRet));
            return;
        }
        engine_set_user_dict_path(userDictUtf8.empty() ? nullptr : userDictUtf8.c_str());
        ApplyServerEngineConfig(cfg, st->dictDir);

        const std::wstring priorPath = st->dictDir + L"context_prior.db";
        if (FileExistsW(priorPath)) {
            const std::string priorUtf8 = WideToUtf8(priorPath);
            engine_set_context_prior_path(priorUtf8.c_str());
        }
        const std::wstring bigramPath = st->dictDir + L"bigram.db";
        if (FileExistsW(bigramPath)) {
            const std::string bigramUtf8 = WideToUtf8(bigramPath);
            engine_set_bigram_path(bigramUtf8.c_str());
        }
    }

    // 拆字词表（2.1MB YAML）仅操作 RADICAL 锁，放锁外加载不阻塞首键查询
    const std::wstring radicalPath = st->dictDir + L"radical_pinyin.dict.yaml";
    if (FileExistsW(radicalPath)) {
        const std::string radicalUtf8 = WideToUtf8(radicalPath);
        engine_set_radical_path(radicalUtf8.c_str());
    }

    int status = engine_dict_load_status();
    while (status == 1 && (GetTickCount64() - t0) < 30000 && !st->shuttingDown.load()) {
        Sleep(10);
        status = engine_dict_load_status();
    }

    const int entries = engine_dict_entry_count();
    const bool ready = (engine_dict_ready() == 1);
    const ULONGLONG elapsed = GetTickCount64() - t0;
    st->engineReady.store(ready);

    char buildBuf[128] = {0};
    engine_build_id(buildBuf, static_cast<int>(sizeof(buildBuf)));

    taishen::ForceLog(
        "taishen_server: engine ready=" + std::string(ready ? "1" : "0") +
        " status=" + std::to_string(status) +
        " entries=" + std::to_string(entries) +
        " elapsed=" + std::to_string(elapsed) + "ms" +
        " build=" + std::string(buildBuf[0] != '\0' ? buildBuf : "unknown") +
        " dict=" + (dictPathUtf8.empty() ? std::string("(builtin)") : dictPathUtf8));
}

bool IsServerKeypadKey(uint32_t rawVk)
{
    if (rawVk >= VK_NUMPAD0 && rawVk <= VK_NUMPAD9) {
        return true;
    }
    switch (rawVk) {
    case VK_MULTIPLY:
    case VK_ADD:
    case VK_SEPARATOR:
    case VK_SUBTRACT:
    case VK_DECIMAL:
    case VK_DIVIDE:
        return true;
    default:
        return false;
    }
}

bool ServerKeypadShiftEquivalent(uint32_t rawVk)
{
    return rawVk == VK_ADD || rawVk == VK_MULTIPLY;
}

void PopulateCandidateResponse(taishen::ipc::CandidateListResponse& resp)
{
    taishen::ReadEngineUtf8(
        [](char* buf, int cap) { return engine_get_preedit_str(buf, cap); },
        resp.preedit);
    taishen::ReadEnginePinyin(resp.pinyin);
    taishen::ReadEngineSyllableDisplay(resp.syllable);

    resp.inCompose = (engine_in_compose() == 1);
    if (resp.inCompose) {
        taishen::ReadEngineUtf8(
            [](char* buf, int cap) { return engine_compose_prefix(buf, cap); },
            resp.composePrefix);
    } else {
        resp.composePrefix.clear();
    }

    resp.page = engine_get_current_page();
    resp.totalPages = engine_get_total_pages();
    resp.totalCandidates = engine_get_total_candidate_count();
    resp.pageSize = engine_get_page_size();
    resp.selectedIndex = 0;
    resp.inputMode = engine_input_mode();
    resp.symbolPrefix = (engine_is_symbol_prefix() == 1);
    resp.semicolonActive = (engine_semicolon_key_active() == 1);

    resp.candidates.clear();
    const int count = engine_get_candidate_count();
    for (int i = 0; i < count; ++i) {
        taishen::ipc::CandidateItem item;
        if (!taishen::ReadEngineCandidate(i, item.text)) {
            continue;
        }
        EngineCandidateMeta meta{};
        if (engine_get_candidate_meta(i, &meta) == 0) {
            item.tier = meta.tier;
            item.reasons = meta.reasons;
            item.hasScore = (meta.has_score != 0);
            item.score = meta.score;
        }
        resp.candidates.push_back(std::move(item));
    }
}

/// 处理来自 DLL 的单次按键请求（须在持有 `st->engineMutex` 下调用）
void ProcessServerKeyInputLocked(ServerState& st,
                                 const taishen::ipc::KeyInputRequest& req,
                                 taishen::ipc::CandidateListResponse& resp)
{
    resp = taishen::ipc::CandidateListResponse{};
    resp.seq = req.seq;
    resp.sessionId = req.sessionId;

    // 切换活跃会话或客户端无未完成拼音时重置，杜绝跨会话拼音串串扰；
    // 保留 last_committed / context_map / pin_words / demoted_words，确保跨宿主排序一致。
    if (st.activeInputSessionId != req.sessionId) {
        engine_reset();
        st.activeInputSessionId = req.sessionId;
    } else if (req.fresh && engine_get_pinyin_str(nullptr, 0) > 1) {
        engine_reset();
    }

    engine_set_ascii_mode(req.asciiMode ? 1 : 0);

    const uint32_t effVk = req.vk;
    const uint32_t rawVk = (req.rawVk != 0) ? req.rawVk : req.vk;
    const bool shift = (req.modifiers & taishen::ipc::kModShift) != 0;
    const bool ctrl = (req.modifiers & taishen::ipc::kModCtrl) != 0;
    const bool alt = (req.modifiers & taishen::ipc::kModAlt) != 0;
    const bool capsLock = (req.modifiers & taishen::ipc::kModCapsLock) != 0;
    const bool keypad = IsServerKeypadKey(rawVk);
    const bool shiftDown = shift || ServerKeypadShiftEquivalent(rawVk);
    const taishen::KeyBindings& keys = st.cfg.keys;

    bool handled = false;
    bool eaten = false;

    // 1. 组词中间态回车 / 退格优先接管
    if (effVk == VK_RETURN && engine_in_compose() == 1) {
        std::string prefix;
        taishen::ReadEngineComposePrefix(prefix);
        if (!prefix.empty()) {
            engine_reset();
            resp.committed = std::move(prefix);
            eaten = true;
            handled = true;
        }
    }
    if (!handled && effVk == VK_BACK && !ctrl && engine_in_compose() == 1) {
        std::string prefix;
        taishen::ReadEngineComposePrefix(prefix);
        if (!prefix.empty()) {
            std::string undone;
            taishen::ReadEngineComposeUndo(undone);
            eaten = true;
            handled = true;
        }
    }

    // 2. 特殊模式（计算器 c / 数字大写 R / Unicode U）
    if (!handled) {
        const int mode = engine_input_mode();
        if (mode == 1 || mode == 2 || mode == 3) {
            if (keypad && (mode == 1 || mode == 2)) {
                char kp = 0;
                switch (rawVk) {
                case VK_ADD:       kp = '+'; break;
                case VK_SUBTRACT:  kp = '-'; break;
                case VK_MULTIPLY:  kp = '*'; break;
                case VK_DIVIDE:    kp = '/'; break;
                case VK_DECIMAL:   kp = '.'; break;
                case VK_SEPARATOR: kp = ','; break;
                default: break;
                }
                if (kp != 0) {
                    engine_process_key(kp);
                    eaten = true;
                    handled = true;
                }
            }
            if (!handled && effVk >= '0' && effVk <= '9') {
                char ch = static_cast<char>(effVk);
                bool badShift = false;
                if (shift && effVk == '8') ch = '*';
                else if (shift && effVk == '9') ch = '(';
                else if (shift && effVk == '0') ch = ')';
                else if (shift) badShift = true;
                if (!badShift) {
                    engine_process_key(ch);
                    eaten = true;
                }
                handled = true;
            }
            if (!handled && (mode == 1 || mode == 2)) {
                char ch = 0;
                if (effVk == VK_OEM_PERIOD && !shift) ch = '.';
                else if (effVk == VK_OEM_PLUS && !shift) ch = '+';
                else if (effVk == VK_OEM_MINUS && !shift) ch = '-';
                else if (effVk == VK_OEM_2 && !shift) ch = '/';
                else if (effVk == VK_OEM_5 && !shift) ch = '%';
                else if (effVk == '6' && shift) ch = '^';
                if (ch != 0) {
                    engine_process_key(ch);
                    eaten = true;
                    handled = true;
                }
            }
        }
    }

    // 3. 翻页（优先于标点语义）
    if (!handled && engine_get_candidate_count() > 0) {
        const bool plusMinus =
            (effVk == VK_OEM_PLUS || effVk == VK_OEM_MINUS) &&
            (keypad ? keys.page_numpad : keys.page_minus_equal);
        const bool commaPeriod =
            (effVk == VK_OEM_COMMA || effVk == VK_OEM_PERIOD) &&
            !keypad && !shiftDown && keys.page_comma_period;
        if (effVk == VK_PRIOR || effVk == VK_NEXT || plusMinus || commaPeriod) {
            const bool forward =
                (effVk == VK_NEXT || effVk == VK_OEM_PLUS || effVk == VK_OEM_PERIOD);
            if (engine_page(forward ? 1 : -1) > 0) {
                eaten = true;
            }
            handled = true;
        }
    }

    // 4. 以词定字（[ 首字 / ] 末字）
    if (!handled && (effVk == VK_OEM_4 || effVk == VK_OEM_6) && !shiftDown &&
        keys.page_brackets && engine_get_candidate_count() > 0) {
        std::string selected;
        if (taishen::ReadEngineTakenChar(effVk == VK_OEM_4, selected)) {
            resp.committed = std::move(selected);
            eaten = true;
            handled = true;
        }
    }

    // 5. 双拼分号编码键
    if (!handled && effVk == VK_OEM_1 && !shift &&
        engine_semicolon_key_active() == 1) {
        engine_process_key(';');
        eaten = true;
        handled = true;
    }

    // 6. 字母 A-Z
    if (!handled && effVk >= 'A' && effVk <= 'Z') {
        if (!ctrl && !alt && !req.asciiMode && !capsLock && !shift) {
            const char ch = static_cast<char>(effVk + ('a' - 'A'));
            engine_process_key(static_cast<int>(ch));
            eaten = true;
        }
        handled = true;
    }

    // 7. Ctrl+BackSpace 删音节
    if (!handled && effVk == VK_BACK && ctrl) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            engine_backspace_syllable();
            eaten = true;
        }
        handled = true;
    }

    // 8. Tab / Shift+Tab 移光标
    if (!handled && effVk == VK_TAB) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            engine_move_cursor(shift ? -1 : 1);
            eaten = true;
        }
        handled = true;
    }

    // 9. Ctrl+Delete 删候选
    if (!handled && effVk == VK_DELETE && ctrl) {
        if (engine_get_candidate_count() > 0) {
            engine_delete_candidate(0);
            eaten = true;
        }
        handled = true;
    }

    // 10. 退格
    if (!handled && effVk == VK_BACK) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            engine_backspace();
            eaten = true;
        }
        handled = true;
    }

    // 11. 空格选第 0 候选（组词态中间音节继续留组词，末音节返回整词 committed）
    if (!handled && effVk == VK_SPACE) {
        if (engine_get_candidate_count() > 0) {
            std::string selected;
            const bool selectedOk = taishen::ReadEngineSelectedCandidate(0, selected);
            if (selectedOk || engine_in_compose() == 1) {
                if (engine_in_compose() != 1 && selectedOk) {
                    resp.committed = std::move(selected);
                }
                eaten = true;
            }
        }
        handled = true;
    }

    // 12. v 前缀数字分类别名（v1-v9）
    if (!handled && !keypad && effVk >= '1' && effVk <= '9' &&
        engine_is_symbol_prefix() == 1) {
        engine_process_key(static_cast<int>(effVk));
        eaten = true;
        handled = true;
    }

    // 13. 数字键 1-9 选词
    if (!handled && !keypad && effVk >= '1' && effVk <= '9') {
        const int index = static_cast<int>(effVk - '1');
        if (engine_get_candidate_count() > index) {
            std::string selected;
            const bool selectedOk = taishen::ReadEngineSelectedCandidate(index, selected);
            if (selectedOk || engine_in_compose() == 1) {
                if (engine_in_compose() != 1 && selectedOk) {
                    resp.committed = std::move(selected);
                }
                eaten = true;
            }
        } else if (engine_in_compose() == 1 || engine_get_pinyin_str(nullptr, 0) > 1) {
            eaten = true;
        }
        handled = true;
    }

    // 14. 输入进行中按 0 或小键盘数字
    if (!handled && effVk >= '0' && effVk <= '9' &&
        (engine_in_compose() == 1 || engine_get_pinyin_str(nullptr, 0) > 1)) {
        eaten = true;
        handled = true;
    }

    // 15. 回车提交原拼音串
    if (!handled && effVk == VK_RETURN) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            std::string py;
            taishen::ReadEnginePinyin(py);
            engine_reset();
            resp.committed = std::move(py);
            eaten = true;
        }
        handled = true;
    }

    // 16. Esc 取消
    if (!handled && effVk == VK_ESCAPE) {
        if (engine_in_compose() == 1 || engine_get_pinyin_str(nullptr, 0) > 1) {
            engine_reset();
        }
        eaten = false;
        handled = true;
    }

    resp.eaten = eaten;
    PopulateCandidateResponse(resp);
}

void HandleServerCommit(ServerState& st, const taishen::ipc::CommitRequest& req)
{
    std::lock_guard<std::mutex> lock(st.engineMutex);
    if (req.word.empty()) {
        if (st.activeInputSessionId == req.sessionId) {
            engine_reset();
        }
        return;
    }
    if (st.activeInputSessionId == req.sessionId && req.index >= 0) {
        std::string curPy;
        std::string curWord;
        taishen::ReadEnginePinyin(curPy);
        const bool gotCand = taishen::ReadEngineCandidate(req.index, curWord);
        if (curPy == req.pinyin && gotCand && curWord == req.word) {
            std::string dummy;
            taishen::ReadEngineSelectedCandidate(req.index, dummy);
            return;
        }
    }
    engine_commit_word(req.pinyin.c_str(), req.word.c_str());
}

void PostHideForSession(ServerState* st, uint64_t sid)
{
    if (st->msgHwnd == nullptr || sid == 0) {
        return;
    }
    auto cmd = std::make_unique<taishen::ipc::RenderCommand>();
    cmd->op = taishen::ipc::RenderOp::Hide;
    cmd->sessionId = sid;
    if (PostMessageW(st->msgHwnd, WM_APP_RENDER_CMD, 0, reinterpret_cast<LPARAM>(cmd.get()))) {
        cmd.release();
    }
}

/// 末级异常处理（审计 P1-2）：server 崩溃时必须留下可见证据。
LONG WINAPI ServerCrashFilter(EXCEPTION_POINTERS* info)
{
    DWORD code = 0;
    const void* addr = nullptr;
    if (info != nullptr && info->ExceptionRecord != nullptr) {
        code = info->ExceptionRecord->ExceptionCode;
        addr = info->ExceptionRecord->ExceptionAddress;
    }
    char buf[128] = {0};
    sprintf_s(buf, "taishen_server: FATAL unhandled exception code=0x%08lX addr=%p",
              static_cast<unsigned long>(code), addr);
    taishen::ForceLog(buf);
    taishen::ForceLog("taishen_server: terminated by crash filter, will be relaunched on next activation");
    return EXCEPTION_EXECUTE_HANDLER;
}

LRESULT CALLBACK ServerMsgWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    if (msg == WM_APP_RENDER_CMD && g_state != nullptr) {
        std::unique_ptr<taishen::ipc::RenderCommand> cmd(
            reinterpret_cast<taishen::ipc::RenderCommand*>(lParam));
        if (!cmd) {
            return 0;
        }
        switch (cmd->op) {
        case taishen::ipc::RenderOp::Show:
            g_state->activeSessionId = cmd->sessionId;
            g_state->lastShow = *cmd;
            g_state->hasLastShow = true;
            g_state->candidateWindow.SetInlinePreedit(cmd->inlinePreedit);
            g_state->candidateWindow.SetSelectedIndex(cmd->selectedIndex);
            g_state->candidateWindow.SetMultiRow(cmd->multiRow);
            g_state->candidateWindow.UpdateState(
                cmd->pinyin, cmd->candidates, cmd->caret, cmd->page, cmd->totalPages);
            break;
        case taishen::ipc::RenderOp::Move:
            if (g_state->hasLastShow && g_state->activeSessionId == cmd->sessionId &&
                g_state->candidateWindow.IsVisible()) {
                g_state->lastShow.caret = cmd->caret;
                g_state->candidateWindow.UpdateState(
                    g_state->lastShow.pinyin,
                    g_state->lastShow.candidates,
                    g_state->lastShow.caret,
                    g_state->lastShow.page,
                    g_state->lastShow.totalPages);
            }
            break;
        case taishen::ipc::RenderOp::Hide:
            if (cmd->sessionId == 0 || g_state->activeSessionId == cmd->sessionId) {
                g_state->hasLastShow = false;
                g_state->candidateWindow.Hide();
            }
            break;
        case taishen::ipc::RenderOp::ReloadConfig:
            ApplyServerConfigAll(*g_state);
            break;
        default:
            break;
        }
        return 0;
    }
    if (msg == WM_TIMER && wParam == kTimerMaintenanceId && g_state != nullptr) {
        ReloadServerConfigIfChanged(*g_state);
        engine_flush_user_writes(0);
        return 0;
    }
    if (msg == WM_SETTINGCHANGE && g_state != nullptr) {
        g_state->candidateWindow.OnSystemThemeChanged();
        return 0;
    }
    if (msg == WM_DESTROY) {
        KillTimer(hwnd, kTimerMaintenanceId);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

void HandleClientConnection(HANDLE hPipe, ServerState* st)
{
    std::string payload;
    uint64_t connLastSid = 0;

    while (!st->shuttingDown.load()) {
        if (!taishen::ipc::ReadPipeFrame(hPipe, payload)) {
            break;
        }

        uint32_t ver = 0;
        std::string op;
        if (!taishen::ipc::PeekEnvelope(payload, ver, op)) {
            taishen::ForceLog("taishen_server: malformed IPC envelope");
            break;
        }
        if (ver != taishen::ipc::kRenderIpcVersion) {
            taishen::ForceLog("taishen_server: protocol version mismatch got=" +
                              std::to_string(ver) + " expected=" +
                              std::to_string(taishen::ipc::kRenderIpcVersion) +
                              " op=" + op);
            if (op == "ping" || op == "key_input") {
                taishen::ipc::WritePipeFrame(hPipe, taishen::ipc::EncodeVersionErrorReply(ver));
            }
            break;
        }

        if (op == "ping") {
            char buildBuf[128] = {0};
            engine_build_id(buildBuf, static_cast<int>(sizeof(buildBuf)));
            const std::string reply = taishen::ipc::EncodePongReply(
                engine_dict_ready() == 1,
                engine_dict_entry_count(),
                buildBuf[0] != '\0' ? std::string(buildBuf) : std::string("unknown"));
            if (!taishen::ipc::WritePipeFrame(hPipe, reply)) {
                break;
            }
            continue;
        }

        if (op == "key_input") {
            taishen::ipc::KeyInputRequest req;
            if (!taishen::ipc::DecodeKeyInput(payload, req)) {
                continue;
            }
            connLastSid = req.sessionId;
            taishen::ipc::CandidateListResponse resp;
            {
                std::lock_guard<std::mutex> lock(st->engineMutex);
                ProcessServerKeyInputLocked(*st, req, resp);
            }
            if (!taishen::ipc::WritePipeFrame(hPipe, taishen::ipc::EncodeCandidateList(resp))) {
                break;
            }
            continue;
        }

        if (op == "commit") {
            taishen::ipc::CommitRequest commitReq;
            if (taishen::ipc::DecodeCommit(payload, commitReq)) {
                connLastSid = commitReq.sessionId;
                HandleServerCommit(*st, commitReq);
            }
            continue;
        }

        auto cmd = std::make_unique<taishen::ipc::RenderCommand>();
        if (!taishen::ipc::DecodeCommand(payload, *cmd)) {
            continue;
        }
        if (cmd->sessionId != 0) {
            connLastSid = cmd->sessionId;
        }
        if (st->msgHwnd != nullptr) {
            if (!PostMessageW(st->msgHwnd, WM_APP_RENDER_CMD, 0,
                              reinterpret_cast<LPARAM>(cmd.get()))) {
                continue;
            }
            cmd.release();
        }
    }

    if (connLastSid != 0) {
        {
            std::lock_guard<std::mutex> lock(st->engineMutex);
            if (st->activeInputSessionId == connLastSid) {
                engine_reset();
                st->activeInputSessionId = 0;
            }
        }
        PostHideForSession(st, connLastSid);
    }

    FlushFileBuffers(hPipe);
    DisconnectNamedPipe(hPipe);
    CloseHandle(hPipe);
}

HANDLE CreateAppContainerPipeInstance()
{
    PSECURITY_DESCRIPTOR pSd = nullptr;
    SECURITY_ATTRIBUTES sa{};
    sa.nLength = sizeof(sa);
    sa.bInheritHandle = FALSE;

    if (ConvertStringSecurityDescriptorToSecurityDescriptorW(
            taishen::ipc::kRenderPipeSddl, SDDL_REVISION_1, &pSd, nullptr)) {
        sa.lpSecurityDescriptor = pSd;
    } else {
        taishen::ForceLog("taishen_server: ConvertStringSD failed err=" +
                          std::to_string(GetLastError()));
    }

    HANDLE hPipe = CreateNamedPipeW(
        taishen::ipc::kRenderPipeName,
        PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        PIPE_UNLIMITED_INSTANCES,
        taishen::ipc::kMaxFrameBytes,
        taishen::ipc::kMaxFrameBytes,
        0,
        pSd != nullptr ? &sa : nullptr);

    if (pSd != nullptr) {
        LocalFree(pSd);
    }
    return hPipe;
}

void PipeAcceptorLoop(ServerState* st)
{
    while (!st->shuttingDown.load()) {
        HANDLE hPipe = CreateAppContainerPipeInstance();
        if (hPipe == INVALID_HANDLE_VALUE) {
            taishen::ForceLog("taishen_server: CreateNamedPipe failed err=" +
                              std::to_string(GetLastError()));
            Sleep(500);
            continue;
        }

        const BOOL connected =
            ConnectNamedPipe(hPipe, nullptr) ? TRUE : (GetLastError() == ERROR_PIPE_CONNECTED);
        if (!connected || st->shuttingDown.load()) {
            CloseHandle(hPipe);
            continue;
        }

        std::thread(HandleClientConnection, hPipe, st).detach();
    }
}

} // namespace

int WINAPI wWinMain(HINSTANCE hInstance, HINSTANCE, PWSTR, int)
{
    // Per-Monitor V2 DPI 感知，确保与宿主上报的物理像素光标坐标单位一致
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

    // 审计 P1-2：崩溃必须可归因（否则容器内表现为「候选窗突然消失」且无日志）
    SetUnhandledExceptionFilter(ServerCrashFilter);

    // 单实例保护
    HANDLE hMutex = CreateMutexW(nullptr, TRUE, taishen::ipc::kServerMutexName);
    if (hMutex == nullptr) {
        return 1;
    }
    if (GetLastError() == ERROR_ALREADY_EXISTS) {
        CloseHandle(hMutex);
        return 0;
    }

    ServerState state;
    state.exeDir = GetExeDirectory();
    state.dictDir = ResolveServerDictDir(state.exeDir);
    state.configDir = ResolveServerConfigDir(state.exeDir, state.dictDir);
    InitConfigWatch(state);
    g_state = &state;

    WNDCLASSEXW wc{};
    wc.cbSize = sizeof(wc);
    wc.lpfnWndProc = ServerMsgWndProc;
    wc.hInstance = hInstance;
    wc.lpszClassName = kServerMsgClass;
    RegisterClassExW(&wc);

    state.msgHwnd = CreateWindowExW(
        0, kServerMsgClass, L"", 0, 0, 0, 0, 0,
        HWND_MESSAGE, nullptr, hInstance, nullptr);
    if (state.msgHwnd == nullptr) {
        CloseHandle(hMutex);
        return 1;
    }

    ApplyServerConfigAll(state);
    // P1-4：进程外候选窗绑定鼠标点击选词与滚轮翻页，通过 SendInput 转发至前台宿主 TSF
    state.candidateWindow.SetClickCallback([](int index) {
        if (index >= 0 && index < 9) {
            SendVirtualKey(static_cast<WORD>('1' + index));
        }
    });
    state.candidateWindow.SetPageCallback([](int delta) {
        SendVirtualKey(delta < 0 ? VK_NEXT : VK_PRIOR);
    });
    SetTimer(state.msgHwnd, kTimerMaintenanceId, kTimerMaintenanceMs, nullptr);

    taishen::ForceLog("taishen_server: started v" +
                      std::to_string(taishen::ipc::kRenderIpcVersion) +
                      " dictDir=" + WideToUtf8(state.dictDir));

    std::thread acceptor(PipeAcceptorLoop, &state);
    acceptor.detach();

    std::thread engineInit(InitServerEngineWorker, &state);
    engineInit.detach();

    MSG msg{};
    while (GetMessageW(&msg, nullptr, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    state.shuttingDown.store(true);
    state.candidateWindow.Hide();
    {
        std::lock_guard<std::mutex> lock(state.engineMutex);
        engine_flush_user_writes(1);
        engine_destroy();
    }
    g_state = nullptr;
    CloseHandle(hMutex);
    return 0;
}
