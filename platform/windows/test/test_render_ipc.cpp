/// V4 D2/D3 进程外渲染与引擎 IPC 冒烟测试（test_render_ipc / test_render_ipc_x86）
///
/// 验证：
///   1. render_ipc.h V2 JSON 编解码往返（RenderCommand / KeyInput / CandidateList / Commit / Pong）；
///   2. kRenderPipeSddl 含 AC (S-1-15-2-1) + LPAC (S-1-15-2-2) + 低完整性标签 (LW) 且 Win32 解析通过；
///   3. 实际连同目录 `taishen_server.exe`：
///      - V2 握手与词库就绪（371,718 条）
///      - V1 协议版本拒绝校验
///      - `nihao` 逐键 KeyInput -> CandidateList（含 tier/reasons/score 与往返延迟）
///      - 14 键长句 `zhongguorenmin` 往返延迟（每键 < 50ms）
///      - 异步 Commit + Show -> Move -> Hide -> ReloadConfig 渲染全链路

#include <windows.h>
#include <sddl.h>

#include <algorithm>
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

static double QpcToMs(LARGE_INTEGER start, LARGE_INTEGER end, LARGE_INTEGER freq)
{
    return static_cast<double>(end.QuadPart - start.QuadPart) * 1000.0 /
           static_cast<double>(freq.QuadPart);
}

int main()
{
    std::printf("[INFO] test_render_ipc ptr_bits=%zu\n", sizeof(void*) * 8);

    // 1. V2 JSON 编解码往返
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
        Check(ok, "V2 EncodeShowCommand -> DecodeCommand round-trip ok");
        Check(dst.version == taishen::ipc::kRenderIpcVersion, "version == 2");
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

    {
        taishen::ipc::KeyInputRequest ksrc;
        ksrc.seq = 42;
        ksrc.sessionId = 0xAABBCCDDEEFF0011ULL;
        ksrc.hostPid = 7788;
        ksrc.vk = 'N';
        ksrc.rawVk = VK_NUMPAD1;
        ksrc.modifiers = taishen::ipc::kModShift | taishen::ipc::kModCapsLock;
        ksrc.asciiMode = true;
        ksrc.fresh = true;

        const std::string kjson = taishen::ipc::EncodeKeyInput(ksrc);
        taishen::ipc::KeyInputRequest kdst;
        Check(taishen::ipc::DecodeKeyInput(kjson, kdst), "EncodeKeyInput -> DecodeKeyInput ok");
        Check(kdst.seq == 42 && kdst.sessionId == ksrc.sessionId && kdst.hostPid == 7788 &&
              kdst.vk == 'N' && kdst.rawVk == VK_NUMPAD1 &&
              kdst.modifiers == ksrc.modifiers && kdst.asciiMode && kdst.fresh,
              "KeyInputRequest fields exact match");

        taishen::ipc::CandidateListResponse csrc;
        csrc.seq = 42;
        csrc.sessionId = ksrc.sessionId;
        csrc.eaten = true;
        csrc.pinyin = "nihao";
        csrc.preedit = "nihao";
        csrc.syllable = "ni'hao";
        csrc.candidates = {
            {"你好", 1, 29, -120, true},
            {"拟好", 1, 1, 0, false},
        };
        csrc.page = 0;
        csrc.totalPages = 3;
        csrc.totalCandidates = 15;
        csrc.pageSize = 5;
        csrc.selectedIndex = 0;
        csrc.inCompose = true;
        csrc.composePrefix = "泰";
        csrc.committed = "";
        csrc.inputMode = 0;
        csrc.symbolPrefix = false;
        csrc.semicolonActive = true;

        const std::string cjson = taishen::ipc::EncodeCandidateList(csrc);
        taishen::ipc::CandidateListResponse cdst;
        Check(taishen::ipc::DecodeCandidateList(cjson, cdst),
              "EncodeCandidateList -> DecodeCandidateList ok");
        Check(cdst.candidates.size() == 2 &&
              cdst.candidates[0].text == "你好" &&
              cdst.candidates[0].tier == 1 &&
              cdst.candidates[0].reasons == 29 &&
              cdst.candidates[0].hasScore &&
              cdst.candidates[0].score == -120 &&
              cdst.candidates[1].text == "拟好" &&
              !cdst.candidates[1].hasScore,
              "CandidateItem tier/reasons/nullable score match");
        Check(cdst.eaten && cdst.pinyin == "nihao" && cdst.syllable == "ni'hao" &&
              cdst.inCompose && cdst.composePrefix == "泰" && cdst.semicolonActive,
              "CandidateListResponse metadata match");

        taishen::ipc::CommitRequest cmsrc;
        cmsrc.sessionId = ksrc.sessionId;
        cmsrc.hostPid = 7788;
        cmsrc.pinyin = "nihao";
        cmsrc.word = "你好";
        cmsrc.index = 0;
        const std::string cmjson = taishen::ipc::EncodeCommit(cmsrc);
        taishen::ipc::CommitRequest cmdst;
        Check(taishen::ipc::DecodeCommit(cmjson, cmdst) &&
              cmdst.sessionId == cmsrc.sessionId && cmdst.pinyin == "nihao" &&
              cmdst.word == "你好" && cmdst.index == 0,
              "EncodeCommit -> DecodeCommit ok");
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
            Sleep(150);
        }
    }

    taishen::CRenderIpcClient client;
    bool pingOk = false;
    bool dictReady = false;
    for (int i = 0; i < 80; ++i) {
        if (client.Ping()) {
            pingOk = true;
            if (client.LastPongInfo().dictReady) {
                dictReady = true;
                break;
            }
        }
        Sleep(50);
    }
    Check(pingOk, "Named pipe V2 Ping -> Pong with taishen_server.exe");
    Check(dictReady, "Server dictionary ready");
    std::printf("[INFO] Server pong: v=%u dictReady=%d entries=%d build=%s\n",
                client.LastPongInfo().version,
                client.LastPongInfo().dictReady ? 1 : 0,
                client.LastPongInfo().entries,
                client.LastPongInfo().build.c_str());

    if (pingOk) {
        // 3a. 协议版本不一致校验（发 V1 frame 必须被拒绝并返回 error）
        HANDLE rawPipe = CreateFileW(
            taishen::ipc::kRenderPipeName,
            GENERIC_READ | GENERIC_WRITE,
            0, nullptr, OPEN_EXISTING, 0, nullptr);
        Check(rawPipe != INVALID_HANDLE_VALUE, "Raw pipe connect for version-mismatch test");
        if (rawPipe != INVALID_HANDLE_VALUE) {
            const bool wrote = taishen::ipc::WritePipeFrame(rawPipe, "{\"v\":1,\"op\":\"ping\"}");
            std::string errReply;
            const bool read = wrote && taishen::ipc::ReadPipeFrame(rawPipe, errReply);
            taishen::ipc::PongInfo errInfo;
            const bool decoded = read && taishen::ipc::DecodePong(errReply, errInfo);
            Check(decoded && !errInfo.ok && errInfo.errorCode == "version_mismatch" &&
                  errInfo.expectedVersion == taishen::ipc::kRenderIpcVersion &&
                  errInfo.gotVersion == 1,
                  "Server rejects V1 handshake with version_mismatch error");
            CloseHandle(rawPipe);
        }

        // 3b. nihao KeyInput -> CandidateList 往返与延迟实测
        LARGE_INTEGER freq{};
        QueryPerformanceFrequency(&freq);
        const uint64_t sid = (static_cast<uint64_t>(GetCurrentProcessId()) << 32) | 1001ULL;
        const char* shortKeys = "NIHAO";
        taishen::ipc::CandidateListResponse resp;
        double shortTotalMs = 0.0;
        double shortMaxMs = 0.0;
        uint64_t seq = 1;
        for (int i = 0; shortKeys[i] != '\0'; ++i) {
            taishen::ipc::KeyInputRequest req;
            req.seq = seq++;
            req.sessionId = sid;
            req.hostPid = GetCurrentProcessId();
            req.vk = static_cast<uint32_t>(shortKeys[i]);
            req.rawVk = req.vk;
            req.asciiMode = false;
            req.fresh = (i == 0);
            LARGE_INTEGER t1{}, t2{};
            QueryPerformanceCounter(&t1);
            const bool ok = client.SendKeyInput(req, resp, 50);
            QueryPerformanceCounter(&t2);
            const double ms = QpcToMs(t1, t2, freq);
            shortTotalMs += ms;
            shortMaxMs = (std::max)(shortMaxMs, ms);
            if (!ok) {
                Check(false, "SendKeyInput nihao key failed");
                break;
            }
        }
        std::printf("[INFO] nihao 5-key IPC: total=%.2fms maxKey=%.2fms pinyin=%s cands=%zu\n",
                    shortTotalMs, shortMaxMs, resp.pinyin.c_str(), resp.candidates.size());
        for (size_t i = 0; i < resp.candidates.size(); ++i) {
            const auto& c = resp.candidates[i];
            std::printf("  cand[%zu]: %s (tier=%u reasons=%u hasScore=%d score=%d)\n",
                        i, c.text.c_str(), c.tier, c.reasons,
                        c.hasScore ? 1 : 0, c.score);
        }
        Check(resp.eaten && resp.pinyin == "nihao" && !resp.candidates.empty() &&
              resp.candidates[0].text == "你好",
              "KeyInput(nihao) top candidate == 你好");
        Check(shortMaxMs < 50.0, "nihao max per-key RTT < 50ms");

        // 3c. 异步 Commit（走活跃候选索引学习，无返回）
        taishen::ipc::CommitRequest commit;
        commit.sessionId = sid;
        commit.hostPid = GetCurrentProcessId();
        commit.pinyin = "nihao";
        commit.word = "你好";
        commit.index = 0;
        Check(client.SendCommit(commit), "SendCommit nihao->你好 succeeded");

        // 3d. 14 键长句 zhongguorenmin 延迟实测（首轮冷态 + 次轮热态）
        const char* longKeys = "ZHONGGUORENMIN";
        for (int round = 0; round < 2; ++round) {
            double longTotalMs = 0.0;
            double longMaxMs = 0.0;
            for (int i = 0; longKeys[i] != '\0'; ++i) {
                taishen::ipc::KeyInputRequest req;
                req.seq = seq++;
                req.sessionId = sid;
                req.hostPid = GetCurrentProcessId();
                req.vk = static_cast<uint32_t>(longKeys[i]);
                req.rawVk = req.vk;
                req.asciiMode = false;
                req.fresh = (i == 0);
                LARGE_INTEGER t1{}, t2{};
                QueryPerformanceCounter(&t1);
                const bool ok = client.SendKeyInput(req, resp, 50);
                QueryPerformanceCounter(&t2);
                const double ms = QpcToMs(t1, t2, freq);
                longTotalMs += ms;
                longMaxMs = (std::max)(longMaxMs, ms);
                if (!ok) {
                    Check(false, "SendKeyInput zhongguorenmin key failed");
                    break;
                }
            }
            std::printf("[INFO] zhongguorenmin 14-key IPC (round %d %s): total=%.2fms maxKey=%.2fms top1=%s\n",
                        round + 1, round == 0 ? "cold" : "warm",
                        longTotalMs, longMaxMs,
                        resp.candidates.empty() ? "(none)" : resp.candidates[0].text.c_str());
            Check(resp.pinyin == "zhongguorenmin" && !resp.candidates.empty() && longMaxMs < 50.0,
                  round == 0 ? "zhongguorenmin cold round per-key < 50ms"
                             : "zhongguorenmin warm round per-key < 50ms");
            if (round == 1) {
                Check(longTotalMs < 50.0, "zhongguorenmin warm 14-key total < 50ms");
            }

            commit.pinyin = "zhongguorenmin";
            commit.word = "";
            client.SendCommit(commit);
        }

        // 3e. 渲染帧往返
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
