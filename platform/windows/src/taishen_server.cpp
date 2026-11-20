/// 泰深输入法进程外渲染服务（V4 D2：`taishen_server.exe`）
///
/// 职责：
///   1. 以当前用户普通权限常驻（单实例），绕开 AppContainer 内 D2D 建窗阻塞；
///   2. 监听命名管道 `\\.\pipe\taishen-ime-render`（DACL 含 AC + LPAC，SACL 含 LW）；
///   3. 100% 复用现有 `CCandidateWindow` + `ui_*` 控件库与 `config.ini` 主题/字体/布局；
///   4. 线程模型：后台管道接收线程只负责读帧并 PostMessage 到 UI 线程，渲染零锁竞争。

#include <windows.h>
#include <sddl.h>

#include <atomic>
#include <cstdio>
#include <memory>
#include <string>
#include <thread>

#include "candidate_window.h"
#include "config_reader.h"
#include "debug_log.h"
#include "dpi_util.h"
#include "render_ipc.h"
#include "theme.h"

namespace {

constexpr UINT WM_APP_RENDER_CMD = WM_APP + 101;
constexpr wchar_t kServerMsgClass[] = L"TaishenImeServerMsgWindow";

struct ServerState {
    taishen::CCandidateWindow candidateWindow;
    std::wstring exeDir;
    uint64_t activeSessionId = 0;
    taishen::ipc::RenderCommand lastShow;
    bool hasLastShow = false;
    std::atomic<bool> shuttingDown{false};
    HWND msgHwnd = nullptr;
};

ServerState* g_state = nullptr;

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

void ApplyServerConfig(ServerState& st)
{
    const taishen::ImeConfig cfg = taishen::LoadConfig(st.exeDir);
    taishen::SetDebugLogEnabled(cfg.debug_log);
    st.candidateWindow.SetTheme(cfg.theme);
    st.candidateWindow.SetFollowSystemTheme(!cfg.userThemeExplicit);
    st.candidateWindow.SetFont(cfg.font_face, cfg.font_size);
    st.candidateWindow.SetLabelFormat(cfg.label_format);
    st.candidateWindow.SetLayout(cfg.corner_radius, cfg.hilite_corner_radius,
                                 cfg.padding, cfg.candidate_spacing);
    st.candidateWindow.SetInlinePreedit(cfg.inline_preedit);
}

/// 末级异常处理（审计 P1-2）：server 崩溃时必须留下可见证据。
/// 没有这个 filter 时，server 会被 WER 静默回收，日志里什么都没有，
/// 表现为「容器内突然没有候选窗」且无法归因。
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
    // 不弹 WER 对话框（避免在用户桌面上弹窗），直接终止
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
            ApplyServerConfig(*g_state);
            break;
        default:
            break;
        }
        return 0;
    }
    if (msg == WM_SETTINGCHANGE && g_state != nullptr) {
        g_state->candidateWindow.OnSystemThemeChanged();
        return 0;
    }
    if (msg == WM_DESTROY) {
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
}

void HandleClientConnection(HANDLE hPipe, ServerState* st)
{
    std::string payload;
    while (!st->shuttingDown.load()) {
        if (!taishen::ipc::ReadPipeFrame(hPipe, payload)) {
            break;
        }
        auto cmd = std::make_unique<taishen::ipc::RenderCommand>();
        if (!taishen::ipc::DecodeCommand(payload, *cmd)) {
            continue;
        }
        if (cmd->op == taishen::ipc::RenderOp::Ping) {
            taishen::ipc::WritePipeFrame(hPipe, "{\"v\":1,\"op\":\"pong\"}");
            continue;
        }
        if (st->msgHwnd != nullptr) {
            if (!PostMessageW(st->msgHwnd, WM_APP_RENDER_CMD, 0,
                              reinterpret_cast<LPARAM>(cmd.get()))) {
                continue;
            }
            cmd.release();
        }
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

    ApplyServerConfig(state);
    // P1-4：进程外候选窗绑定鼠标点击选词与滚轮翻页，通过 SendInput 转发至前台宿主 TSF
    state.candidateWindow.SetClickCallback([](int index) {
        if (index >= 0 && index < 9) {
            SendVirtualKey(static_cast<WORD>('1' + index));
        }
    });
    state.candidateWindow.SetPageCallback([](int delta) {
        SendVirtualKey(delta < 0 ? VK_NEXT : VK_PRIOR);
    });
    taishen::DebugLog("taishen_server: started, listening on \\\\.\\pipe\\taishen-ime-render");

    std::thread acceptor(PipeAcceptorLoop, &state);
    acceptor.detach();

    MSG msg{};
    while (GetMessageW(&msg, nullptr, 0, 0) > 0) {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    state.shuttingDown.store(true);
    state.candidateWindow.Hide();
    g_state = nullptr;
    CloseHandle(hMutex);
    return 0;
}
