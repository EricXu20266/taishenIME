/// V4 D2 进程外渲染 IPC 冒烟测试（test_render_ipc）
///
/// 验证：
///   1. render_ipc.h JSON 编解码往返（含中文、转义字符、caret 坐标、页码）；
///   2. kRenderPipeSddl 含 AC (S-1-15-2-1) + LPAC (S-1-15-2-2) + 低完整性标签 (LW) 且 Win32 解析通过；
///   3. 实际拉起同目录 `taishen_server.exe`，命名管道连通：Ping → Show → Move → Hide 全部成功。

#include <windows.h>
#include <sddl.h>

#include <cstdio>
#include <string>

#include "ipc_render_client.h"
#include "render_ipc.h"

static int g_fails = 0;

static void Check(bool cond, const char* msg)
{
    if (cond) {
        std::printf("[PASS] %s\n", msg);
    } else {
        std::printf("[FAIL] %s\n", msg);
        ++g_fails;
    }
}

static std::wstring GetExeDir()
{
    wchar_t path[MAX_PATH] = {0};
    const DWORD len = GetModuleFileNameW(nullptr, path, MAX_PATH);
    std::wstring s(path, len);
    const size_t slash = s.find_last_of(L"\\/");
    return (slash != std::wstring::npos) ? s.substr(0, slash + 1) : L"";
}

int main()
{
    // 1. JSON 编解码往返
    {
        taishen::ipc::RenderCommand src;
        src.op = taishen::ipc::RenderOp::Show;
        src.sessionId = 0x123456789ABCDEF0ULL;
        src.hostPid = 4321;
        src.pinyin = "suan'fa";
        src.candidates = {"算法", "断\"法\\", "测试\t词"};
        src.caret = {100, 200, 102, 224};
        src.page = 1;
        src.totalPages = 4;
        src.selectedIndex = 2;
        src.inlinePreedit = false;
        src.multiRow = true;

        const std::string encoded = taishen::ipc::EncodeShowCommand(src);
        taishen::ipc::RenderCommand dst;
        const bool ok = taishen::ipc::DecodeCommand(encoded, dst);
        Check(ok, "EncodeShowCommand -> DecodeCommand round-trip ok");
        Check(dst.op == taishen::ipc::RenderOp::Show, "op == Show");
        Check(dst.sessionId == src.sessionId, "sessionId exact match");
        Check(dst.hostPid == src.hostPid, "hostPid exact match");
        Check(dst.pinyin == src.pinyin, "pinyin exact match");
        Check(dst.candidates == src.candidates, "candidates UTF-8 & escaping match");
        Check(dst.caret.left == 100 && dst.caret.top == 200 &&
              dst.caret.right == 102 && dst.caret.bottom == 224,
              "caret rect match");
        Check(dst.page == 1 && dst.totalPages == 4 && dst.selectedIndex == 2,
              "page/total/sel match");
        Check(!dst.inlinePreedit && dst.multiRow, "flags match");
    }

    // 2. SDDL 合法性（确保 AppContainer + LPAC + LW 标签在当前 Windows 上有效）
    {
        PSECURITY_DESCRIPTOR pSd = nullptr;
        const BOOL sdOk = ConvertStringSecurityDescriptorToSecurityDescriptorW(
            taishen::ipc::kRenderPipeSddl, SDDL_REVISION_1, &pSd, nullptr);
        Check(sdOk != FALSE && pSd != nullptr, "kRenderPipeSddl valid (AC + LPAC + LW)");
        if (pSd != nullptr) {
            LocalFree(pSd);
        }
    }

    // 3. 端到端管道测试（拉起同目录 taishen_server.exe，如未运行则临时启动并在结束时关闭）
    const std::wstring dir = GetExeDir();
    const std::wstring serverExe = dir + L"taishen_server.exe";
    HANDLE spawnedProc = nullptr;
    HANDLE existingMutex = OpenMutexW(SYNCHRONIZE, FALSE, taishen::ipc::kServerMutexName);
    if (existingMutex != nullptr) {
        CloseHandle(existingMutex);
        std::printf("[INFO] taishen_server.exe already running, reusing instance\n");
    } else if (GetFileAttributesW(serverExe.c_str()) != INVALID_FILE_ATTRIBUTES) {
        STARTUPINFOW si{};
        si.cb = sizeof(si);
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_HIDE;
        PROCESS_INFORMATION pi{};
        std::wstring cmd = L"\"" + serverExe + L"\"";
        if (CreateProcessW(serverExe.c_str(), &cmd[0], nullptr, nullptr, FALSE,
                           CREATE_NO_WINDOW, nullptr, dir.c_str(), &si, &pi)) {
            CloseHandle(pi.hThread);
            spawnedProc = pi.hProcess;
            Sleep(250); // 等待命名管道创建完成
        }
    }

    taishen::CRenderIpcClient client;
    bool pingOk = false;
    for (int i = 0; i < 10 && !pingOk; ++i) {
        pingOk = client.Ping();
        if (!pingOk) {
            Sleep(100);
        }
    }
    Check(pingOk, "Named pipe Ping -> Pong with taishen_server.exe");

    if (pingOk) {
        taishen::ipc::RenderCommand show;
        show.op = taishen::ipc::RenderOp::Show;
        show.sessionId = 99999;
        show.hostPid = GetCurrentProcessId();
        show.pinyin = "suanfa";
        show.candidates = {"算法", "算法语言"};
        show.caret = {300, 300, 302, 320};
        show.page = 0;
        show.totalPages = 1;
        Check(client.SendShow(show), "SendShow succeeded");
        Sleep(50);
        RECT moved = {320, 320, 322, 340};
        Check(client.SendMove(99999, moved), "SendMove succeeded");
        Sleep(50);
        Check(client.SendHide(99999), "SendHide succeeded");
        Check(client.SendReloadConfig(), "SendReloadConfig succeeded");
    }

    client.Disconnect();
    if (spawnedProc != nullptr) {
        TerminateProcess(spawnedProc, 0);
        WaitForSingleObject(spawnedProc, 2000);
        CloseHandle(spawnedProc);
    }

    std::printf("\nResult: %s (fails=%d)\n", g_fails == 0 ? "ALL PASS" : "FAILED", g_fails);
    return g_fails == 0 ? 0 : 1;
}
