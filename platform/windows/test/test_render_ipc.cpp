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
#include <atomic>
#include <cstdio>
#include <string>
#include <thread>

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

static HANDLE OpenRawPipeWithWait(const wchar_t* pipeName)
{
    for (int attempt = 0; attempt < 10; ++attempt) {
        HANDLE h = CreateFileW(
            pipeName,
            GENERIC_READ | GENERIC_WRITE,
            0, nullptr, OPEN_EXISTING, 0, nullptr);
        if (h != INVALID_HANDLE_VALUE) {
            return h;
        }
        const DWORD err = GetLastError();
        if (err == ERROR_PIPE_BUSY || err == ERROR_FILE_NOT_FOUND) {
            WaitNamedPipeW(pipeName, 200);
            Sleep(10);
            continue;
        }
        break;
    }
    return INVALID_HANDLE_VALUE;
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
        src.cursorPos = 3;
        src.inlinePreedit = false;
        src.multiRow = true;

        const std::string encoded = taishen::ipc::EncodeShowCommand(src);
        taishen::ipc::RenderCommand dst;
        const bool ok = taishen::ipc::DecodeCommand(encoded, dst);
        Check(ok, "V3 EncodeShowCommand -> DecodeCommand round-trip ok");
        Check(dst.version == taishen::ipc::kRenderIpcVersion, "version == 3");
        Check(dst.op == taishen::ipc::RenderOp::Show, "op == Show");
        Check(dst.sessionId == src.sessionId, "sessionId exact match");
        Check(dst.hostPid == src.hostPid, "hostPid exact match");
        Check(dst.pinyin == src.pinyin, "pinyin exact match");
        Check(dst.candidates == src.candidates, "candidates UTF-8 & escaping match");
        Check(dst.caret.left == 100 && dst.caret.top == 200 &&
              dst.caret.right == 102 && dst.caret.bottom == 224,
              "caret rect match");
        Check(dst.page == 1 && dst.totalPages == 4 && dst.selectedIndex == 2 &&
              dst.cursorPos == 3,
              "page/total/sel/cursor match");
        Check(!dst.inlinePreedit && dst.multiRow, "flags match");

        // 不含 cursor 的旧帧默认解码为 -1（向下兼容）
        taishen::ipc::RenderCommand noCursorDst;
        Check(taishen::ipc::DecodeCommand(
                  "{\"v\":2,\"op\":\"show\",\"sid\":1,\"pid\":2,\"pinyin\":\"ni\",\"cands\":[\"你\"],\"caret\":[0,0,1,1],\"page\":0,\"total\":1,\"sel\":0}",
                  noCursorDst) &&
              noCursorDst.cursorPos == -1,
              "Legacy show without cursor defaults cursorPos to -1");
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
        ksrc.flags = taishen::ipc::kFlagHasTraditional |
                     taishen::ipc::kFlagTraditional |
                     taishen::ipc::kFlagHasShuangpin;

        const std::string kjson = taishen::ipc::EncodeKeyInput(ksrc);
        taishen::ipc::KeyInputRequest kdst;
        Check(taishen::ipc::DecodeKeyInput(kjson, kdst), "EncodeKeyInput -> DecodeKeyInput ok");
        Check(kdst.seq == 42 && kdst.sessionId == ksrc.sessionId && kdst.hostPid == 7788 &&
              kdst.vk == 'N' && kdst.rawVk == VK_NUMPAD1 &&
              kdst.modifiers == ksrc.modifiers && kdst.asciiMode && kdst.fresh &&
              kdst.flags == ksrc.flags,
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
        csrc.cursor = 2;
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
              cdst.cursor == 2 &&
              cdst.inCompose && cdst.composePrefix == "泰" && cdst.semicolonActive,
              "CandidateListResponse metadata & cursor match");

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

        const std::string pongJson =
            taishen::ipc::EncodePongReply(true, 123, "b3", true, 2, true, false);
        taishen::ipc::PongInfo pongDst;
        Check(taishen::ipc::DecodePong(pongJson, pongDst) && pongDst.ok &&
              pongDst.dictReady && pongDst.entries == 123 && pongDst.uiAccess &&
              pongDst.windowBand == 2 && pongDst.traditional && !pongDst.shuangpin,
              "V3 EncodePongReply -> DecodePong round-trip ok");

        // V2 向下兼容编解码校验（无 flags 字段、v=2 回包）
        const std::string v2PongJson =
            taishen::ipc::EncodePongReply(true, 123, "b2", true, 2, false, false, 2);
        taishen::ipc::PongInfo v2PongDst;
        Check(taishen::ipc::DecodePong(v2PongJson, v2PongDst) &&
              v2PongDst.version == 2 && v2PongDst.ok && v2PongDst.dictReady,
              "V2 EncodePongReply -> DecodePong round-trip ok");

        taishen::ipc::KeyInputRequest v2KeyDst;
        Check(taishen::ipc::DecodeKeyInput(
                  "{\"v\":2,\"op\":\"key_input\",\"seq\":1,\"sid\":99,\"pid\":42,"
                  "\"vk\":78,\"rawVk\":78,\"modifiers\":0,\"asciiMode\":0,\"fresh\":1}",
                  v2KeyDst) &&
              v2KeyDst.version == 2 && v2KeyDst.flags == 0 && v2KeyDst.vk == 78,
              "V2 DecodeKeyInput without flags succeeds with flags=0");

        const std::string v2CandJson = taishen::ipc::EncodeCandidateList(csrc, 2);
        taishen::ipc::CandidateListResponse v2CandDst;
        Check(taishen::ipc::DecodeCandidateList(v2CandJson, v2CandDst) &&
              v2CandDst.version == 2 && v2CandDst.pinyin == "nihao" &&
              v2CandDst.candidates.size() == 2,
              "V2 EncodeCandidateList -> DecodeCandidateList round-trip ok");

        // F10：数字越界校验
        taishen::ipc::KeyInputRequest badKey;
        Check(!taishen::ipc::DecodeKeyInput(
                  "{\"v\":3,\"op\":\"key_input\",\"seq\":99999999999999999999999}", badKey),
              "DecodeKeyInput rejects uint64 overflow");
        Check(!taishen::ipc::DecodeKeyInput(
                  "{\"v\":3,\"op\":\"key_input\",\"vk\":4294967296}", badKey),
              "DecodeKeyInput rejects uint32 overflow");
        taishen::ipc::CandidateListResponse badCand;
        Check(!taishen::ipc::DecodeCandidateList(
                  "{\"v\":3,\"op\":\"candidate_list\",\"candidates\":[{\"text\":\"a\",\"tier\":256}]}",
                  badCand),
              "DecodeCandidateList rejects uint8 tier overflow");
        Check(!taishen::ipc::DecodeCandidateList(
                  "{\"v\":3,\"op\":\"candidate_list\",\"candidates\":[{\"text\":\"a\",\"score\":2147483648}]}",
                  badCand),
              "DecodeCandidateList rejects int32 score overflow");
        taishen::ipc::RenderCommand badCmd;
        Check(!taishen::ipc::DecodeCommand(
                  "{\"v\":3,\"op\":\"move\",\"sid\":1,\"caret\":[0,0,0,2147483648]}",
                  badCmd),
              "DecodeCommand rejects int32 caret overflow");

        // V1.2 open_settings / toggle_voice / ack 编解码往返
        {
            const std::string osJson = taishen::ipc::EncodeOpenSettingsCommand(6, 9527);
            taishen::ipc::RenderCommand osCmd;
            Check(taishen::ipc::DecodeCommand(osJson, osCmd) &&
                  osCmd.op == taishen::ipc::RenderOp::OpenSettings &&
                  osCmd.hostPid == 9527 && osCmd.tabIndex == 6,
                  "EncodeOpenSettingsCommand -> DecodeCommand round-trip ok");

            const std::string tvJson = taishen::ipc::EncodeToggleVoiceCommand(9527);
            taishen::ipc::RenderCommand tvCmd;
            Check(taishen::ipc::DecodeCommand(tvJson, tvCmd) &&
                  tvCmd.op == taishen::ipc::RenderOp::ToggleVoice &&
                  tvCmd.hostPid == 9527,
                  "EncodeToggleVoiceCommand -> DecodeCommand round-trip ok");

            const std::string ackJson = taishen::ipc::EncodeAckReply(true, true);
            taishen::ipc::AckReply ackDst;
            Check(taishen::ipc::DecodeAck(ackJson, ackDst) &&
                  ackDst.version == taishen::ipc::kRenderIpcVersion &&
                  ackDst.ok && ackDst.listening,
                  "EncodeAckReply -> DecodeAck round-trip ok");
        }
    }

    // 2. SDDL 合法性（不含 AU，含 OW + AC + LPAC + LW 标签在当前 Windows 上有效）
    {
        const std::wstring sddl(taishen::ipc::kRenderPipeSddl);
        Check(sddl.find(L";;;AU)") == std::wstring::npos,
              "kRenderPipeSddl excludes broad AU grant");
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
    Check(pingOk, "Named pipe V3 Ping -> Pong with taishen_server.exe");
    Check(dictReady, "Server dictionary ready");
    std::printf("[INFO] Server pong: v=%u dictReady=%d entries=%d uiAccess=%d band=%u trad=%d sp=%d build=%s\n",
                client.LastPongInfo().version,
                client.LastPongInfo().dictReady ? 1 : 0,
                client.LastPongInfo().entries,
                client.LastPongInfo().uiAccess ? 1 : 0,
                client.LastPongInfo().windowBand,
                client.LastPongInfo().traditional ? 1 : 0,
                client.LastPongInfo().shuangpin ? 1 : 0,
                client.LastPongInfo().build.c_str());

    if (pingOk) {
        // 3a. 协议版本不一致校验（发 V1 ping/show 帧必须被拒绝并返回 error）
        HANDLE rawPipe = OpenRawPipeWithWait(taishen::ipc::kRenderPipeName);
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

        // F4 #4: 非 ping 操作（show）版本不匹配也返回 version_mismatch error 帧
        {
            HANDLE showVerPipe = OpenRawPipeWithWait(taishen::ipc::kRenderPipeName);
            if (showVerPipe != INVALID_HANDLE_VALUE) {
                const bool wrote = taishen::ipc::WritePipeFrame(
                    showVerPipe, "{\"v\":1,\"op\":\"show\",\"sid\":1}");
                std::string errReply;
                const bool read = wrote && taishen::ipc::ReadPipeFrame(showVerPipe, errReply);
                taishen::ipc::PongInfo errInfo;
                const bool decoded = read && taishen::ipc::DecodePong(errReply, errInfo);
                Check(decoded && !errInfo.ok && errInfo.errorCode == "version_mismatch" &&
                      errInfo.expectedVersion == taishen::ipc::kRenderIpcVersion &&
                      errInfo.gotVersion == 1,
                      "Server rejects V1 show op with version_mismatch error");
                CloseHandle(showVerPipe);
            } else {
                Check(false, "Raw pipe connect for V1 show error test");
            }
        }

        // F5: 伪造 hostPid 必须被服务端直接拒绝断开
        {
            HANDLE spoofPipe = OpenRawPipeWithWait(taishen::ipc::kRenderPipeName);
            Check(spoofPipe != INVALID_HANDLE_VALUE, "Raw pipe connect for spoofed pid test");
            if (spoofPipe != INVALID_HANDLE_VALUE) {
                taishen::ipc::KeyInputRequest spoofReq;
                spoofReq.seq = 1;
                spoofReq.sessionId = 12345;
                spoofReq.hostPid = GetCurrentProcessId() + 1;
                spoofReq.vk = 'N';
                spoofReq.rawVk = 'N';
                spoofReq.fresh = true;
                const bool wrote = taishen::ipc::WritePipeFrame(
                    spoofPipe, taishen::ipc::EncodeKeyInput(spoofReq));
                std::string spoofReply;
                const bool gotReply = wrote && taishen::ipc::ReadPipeFrame(spoofPipe, spoofReply);
                Check(!gotReply, "Server rejects key_input with spoofed hostPid");
                CloseHandle(spoofPipe);
            }
        }

        // 确保 spoof 测试后客户端拥有干净健康的连接
        client.Disconnect();
        Check(client.Ping(), "Client reconnect after spoofed-pid test");

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
        Sleep(50);

        // 3f. F4：远程引擎简繁与双拼模式实时切换（并在结束前恢复简体全拼）
        {
            const uint32_t kClearFlags =
                taishen::ipc::kFlagHasTraditional | taishen::ipc::kFlagHasShuangpin;
            auto sendWord = [&](const char* keys, uint32_t flags) -> bool {
                commit.pinyin = "";
                commit.word = "";
                client.SendCommit(commit);
                for (int i = 0; keys[i] != '\0'; ++i) {
                    taishen::ipc::KeyInputRequest req;
                    req.seq = seq++;
                    req.sessionId = sid;
                    req.hostPid = GetCurrentProcessId();
                    req.vk = static_cast<uint32_t>(keys[i]);
                    req.rawVk = req.vk;
                    req.asciiMode = false;
                    req.fresh = (i == 0);
                    req.flags = flags;
                    if (!client.SendKeyInput(req, resp, 50)) {
                        return false;
                    }
                }
                return true;
            };

            const bool tradOk = sendWord(
                "CESHI", kClearFlags | taishen::ipc::kFlagTraditional);
            Check(tradOk && !resp.candidates.empty() && resp.candidates[0].text == "測試",
                  "F4: Remote traditional toggle CESHI -> 測試");

            const bool simpOk = sendWord("CESHI", kClearFlags);
            Check(simpOk && !resp.candidates.empty() && resp.candidates[0].text == "测试",
                  "F4: Remote simplified restore CESHI -> 测试");

            const bool spOk = sendWord(
                "VS", kClearFlags | taishen::ipc::kFlagShuangpin);
            bool hasZhong = false;
            for (const auto& c : resp.candidates) {
                if (c.text == "中") {
                    hasZhong = true;
                    break;
                }
            }
            Check(spOk && resp.eaten && resp.pinyin == "vs" && hasZhong,
                  "F4: Remote shuangpin toggle VS -> vs/中");

            // 恢复全拼简体并清会话
            sendWord("NI", kClearFlags);
            commit.pinyin = "";
            commit.word = "";
            client.SendCommit(commit);
        }

        // 3g. 终端用户免重启升级：V3 server 必须按连接向下兼容旧 V2 客户端
        {
            HANDLE v2Pipe = OpenRawPipeWithWait(taishen::ipc::kRenderPipeName);
            Check(v2Pipe != INVALID_HANDLE_VALUE, "Raw pipe connect for V2 compat test");
            if (v2Pipe != INVALID_HANDLE_VALUE) {
                const DWORD curPid = GetCurrentProcessId();
                const uint64_t v2Sid = (static_cast<uint64_t>(curPid) << 32) | 2002ULL;

                const std::string v2Ping =
                    "{\"v\":2,\"op\":\"ping\",\"sid\":0,\"pid\":" +
                    std::to_string(curPid) + "}";
                std::string v2PongStr;
                taishen::ipc::PongInfo v2Pong;
                const bool pingOkV2 =
                    taishen::ipc::WritePipeFrame(v2Pipe, v2Ping) &&
                    taishen::ipc::ReadPipeFrame(v2Pipe, v2PongStr) &&
                    taishen::ipc::DecodePong(v2PongStr, v2Pong);
                Check(pingOkV2 && v2Pong.ok && v2Pong.version == 2 && v2Pong.dictReady,
                      "V3 server accepts V2 ping and replies with v=2 pong");

                auto sendV2Key = [&](uint64_t keySeq, uint32_t vkCode, bool fresh,
                                     taishen::ipc::CandidateListResponse& outCand) -> bool {
                    const std::string reqJson =
                        "{\"v\":2,\"op\":\"key_input\",\"seq\":" + std::to_string(keySeq) +
                        ",\"sid\":" + std::to_string(v2Sid) +
                        ",\"pid\":" + std::to_string(curPid) +
                        ",\"vk\":" + std::to_string(vkCode) +
                        ",\"rawVk\":" + std::to_string(vkCode) +
                        ",\"modifiers\":0,\"asciiMode\":0,\"fresh\":" +
                        (fresh ? "1" : "0") + "}";
                    std::string replyJson;
                    return taishen::ipc::WritePipeFrame(v2Pipe, reqJson) &&
                           taishen::ipc::ReadPipeFrame(v2Pipe, replyJson) &&
                           taishen::ipc::DecodeCandidateList(replyJson, outCand);
                };

                taishen::ipc::CandidateListResponse v2Resp1, v2Resp2;
                const bool k1 = sendV2Key(1, 'N', true, v2Resp1);
                const bool k2 = k1 && sendV2Key(2, 'I', false, v2Resp2);
                Check(k2 && v2Resp2.version == 2 && v2Resp2.eaten &&
                      v2Resp2.pinyin == "ni" && !v2Resp2.candidates.empty(),
                      "V2 legacy key_input without flags receives v=2 candidates");

                const std::string v2Show =
                    "{\"v\":2,\"op\":\"show\",\"sid\":" + std::to_string(v2Sid) +
                    ",\"pid\":" + std::to_string(curPid) +
                    ",\"pinyin\":\"ni\",\"cands\":[\"你\"],\"caret\":[300,300,302,320],"
                    "\"page\":0,\"total\":1,\"sel\":0,\"inline\":1,\"multi\":0}";
                const std::string v2Move =
                    "{\"v\":2,\"op\":\"move\",\"sid\":" + std::to_string(v2Sid) +
                    ",\"caret\":[310,310,312,330]}";
                const std::string v2Hide =
                    "{\"v\":2,\"op\":\"hide\",\"sid\":" + std::to_string(v2Sid) + "}";
                const std::string v2Commit =
                    "{\"v\":2,\"op\":\"commit\",\"sid\":" + std::to_string(v2Sid) +
                    ",\"pid\":" + std::to_string(curPid) +
                    ",\"pinyin\":\"\",\"word\":\"\",\"index\":-1}";
                const bool renderWritesOk =
                    taishen::ipc::WritePipeFrame(v2Pipe, v2Show) &&
                    taishen::ipc::WritePipeFrame(v2Pipe, v2Move) &&
                    taishen::ipc::WritePipeFrame(v2Pipe, v2Hide) &&
                    taishen::ipc::WritePipeFrame(v2Pipe, v2Commit);
                Check(renderWritesOk, "V2 show/move/hide/commit writes succeed on V3 server");

                const std::string v3SwitchPing =
                    "{\"v\":3,\"op\":\"ping\",\"sid\":0,\"pid\":" +
                    std::to_string(curPid) + "}";
                std::string switchReply;
                taishen::ipc::PongInfo switchErr;
                const bool switchRejected =
                    taishen::ipc::WritePipeFrame(v2Pipe, v3SwitchPing) &&
                    taishen::ipc::ReadPipeFrame(v2Pipe, switchReply) &&
                    taishen::ipc::DecodePong(switchReply, switchErr) &&
                    !switchErr.ok && switchErr.errorCode == "version_mismatch";
                Check(switchRejected,
                      "Server rejects mid-connection version switch on V2 connection");

                CloseHandle(v2Pipe);
            }
        }
    }

    client.Disconnect();
    if (spawnedProc != nullptr) {
        TerminateProcess(spawnedProc, 0);
        WaitForSingleObject(spawnedProc, 2000);
        CloseHandle(spawnedProc);
    }

    // 4. 客户端协议失配持久化标记、30s 限频重连与成功自清验证（独立模拟管道）
    {
        const std::wstring mockPipeName =
            L"\\\\.\\pipe\\taishen-ime-test-mismatch-" +
            std::to_wstring(GetCurrentProcessId());
        std::atomic<bool> stop{false};
        std::atomic<bool> replyValid{false};
        std::atomic<bool> serverReady{false};
        std::atomic<int> acceptCount{0};

        std::thread mockServer([&]() {
            while (!stop.load()) {
                HANDLE hPipe = CreateNamedPipeW(
                    mockPipeName.c_str(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    4,
                    taishen::ipc::kMaxFrameBytes,
                    taishen::ipc::kMaxFrameBytes,
                    0,
                    nullptr);
                if (hPipe == INVALID_HANDLE_VALUE) {
                    Sleep(10);
                    continue;
                }
                serverReady.store(true);

                HANDLE hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
                if (hEvent == nullptr) {
                    CloseHandle(hPipe);
                    break;
                }
                OVERLAPPED ov{};
                ov.hEvent = hEvent;
                bool connected = false;
                if (ConnectNamedPipe(hPipe, &ov)) {
                    connected = true;
                } else {
                    const DWORD err = GetLastError();
                    if (err == ERROR_PIPE_CONNECTED) {
                        connected = true;
                    } else if (err == ERROR_IO_PENDING) {
                        while (!stop.load()) {
                            const DWORD w = WaitForSingleObject(hEvent, 20);
                            if (w == WAIT_OBJECT_0) {
                                DWORD dummy = 0;
                                connected = GetOverlappedResult(hPipe, &ov, &dummy, FALSE) != FALSE;
                                break;
                            }
                            if (w != WAIT_TIMEOUT) {
                                break;
                            }
                        }
                        if (!connected) {
                            if (CancelIoEx(hPipe, &ov) || GetLastError() == ERROR_NOT_FOUND) {
                                DWORD dummy = 0;
                                GetOverlappedResult(hPipe, &ov, &dummy, TRUE);
                            }
                        }
                    }
                }
                CloseHandle(hEvent);

                if (!connected || stop.load()) {
                    CloseHandle(hPipe);
                    continue;
                }

                acceptCount.fetch_add(1);
                std::string reqFrame;
                if (!replyValid.load()) {
                    if (taishen::ipc::ReadPipeFrameTimed(hPipe, reqFrame, 500)) {
                        taishen::ipc::WritePipeFrameTimed(
                            hPipe,
                            "{\"v\":4,\"op\":\"error\",\"ok\":0,\"code\":\"version_mismatch\","
                            "\"expected\":4,\"got\":3}",
                            500);
                        FlushFileBuffers(hPipe);
                    }
                } else {
                    while (!stop.load() &&
                           taishen::ipc::ReadPipeFrameTimed(hPipe, reqFrame, 500)) {
                        uint32_t ver = 0;
                        std::string op;
                        std::string replyFrame;
                        if (taishen::ipc::PeekEnvelope(reqFrame, ver, op) &&
                            (op == "open_settings" || op == "toggle_voice")) {
                            replyFrame = taishen::ipc::EncodeAckReply(true, op == "toggle_voice");
                        } else {
                            replyFrame = taishen::ipc::EncodePongReply(true, 100, "mock-v3");
                        }
                        taishen::ipc::WritePipeFrameTimed(hPipe, replyFrame, 500);
                    }
                }
                DisconnectNamedPipe(hPipe);
                CloseHandle(hPipe);
            }
        });

        for (int i = 0; i < 100 && !serverReady.load(); ++i) {
            Sleep(10);
        }
        WaitNamedPipeW(mockPipeName.c_str(), 500);

        taishen::CRenderIpcClient mismatchClient(mockPipeName);
        Check(!mismatchClient.IsProtocolIncompatible(),
              "Mismatch client starts compatible");

        taishen::ipc::KeyInputRequest keyReq;
        keyReq.seq = 1;
        keyReq.sessionId = 777;
        keyReq.hostPid = GetCurrentProcessId();
        keyReq.vk = 'N';
        keyReq.rawVk = 'N';
        keyReq.fresh = true;
        taishen::ipc::CandidateListResponse keyResp;
        Check(!mismatchClient.SendKeyInput(keyReq, keyResp, 50),
              "SendKeyInput fails on v4 version_mismatch reply");
        Check(mismatchClient.IsProtocolIncompatible(),
              "IsProtocolIncompatible persists true after handshake mismatch");
        Check(acceptCount.load() == 1,
              "Mock server accepted exactly 1 connection on mismatch");

        keyReq.seq = 2;
        Check(!mismatchClient.SendKeyInput(keyReq, keyResp, 50),
              "Second SendKeyInput returns false immediately during mismatch window");
        Check(!mismatchClient.TryReconnectIfMismatchDue(),
              "TryReconnectIfMismatchDue throttles within 30s window");
        Check(acceptCount.load() == 1,
              "Zero reconnects/writes occurred during 30s mismatch window");

        replyValid.store(true);
        for (int w = 0; w < 20 && !WaitNamedPipeW(mockPipeName.c_str(), 50); ++w) {
            Sleep(10);
        }
        Check(mismatchClient.Ping(),
              "Explicit Ping recovers when mock server upgrades to v3");
        Check(!mismatchClient.IsProtocolIncompatible(),
              "IsProtocolIncompatible clears after valid v3 handshake");

        // V1.2 客户端 SendOpenSettings / SendToggleVoice 命名管道端到端验证
        Check(mismatchClient.SendOpenSettings(6, 200),
              "SendOpenSettings IPC ack round-trip ok");
        bool voiceListening = false;
        Check(mismatchClient.SendToggleVoice(&voiceListening, 200) && voiceListening,
              "SendToggleVoice IPC ack round-trip ok");

        mismatchClient.Disconnect();
        stop.store(true);
        mockServer.join();
    }

    // 5. V2.1 指数短探测重连（50 -> 150 -> 500 ms）与延迟就绪快速恢复验证
    {
        const std::wstring missingPipeName =
            L"\\\\.\\pipe\\taishen-ime-test-missing-" +
            std::to_wstring(GetCurrentProcessId());
        taishen::CRenderIpcClient backoffClient(missingPipeName);
        taishen::ipc::KeyInputRequest req;
        req.seq = 1;
        req.sessionId = 888;
        req.hostPid = GetCurrentProcessId();
        req.vk = 'N';
        req.rawVk = 'N';
        req.fresh = true;
        taishen::ipc::CandidateListResponse resp;

        // 5a: 连续失败按 50ms -> 150ms -> 500ms 步进，而非 5s 黑洞
        Check(!backoffClient.SendKeyInput(req, resp, 45) &&
              backoffClient.ConsecutiveFailCount() == 1 &&
              backoffClient.IsInHardBackoff(),
              "V2.1 fail #1 enters ~50ms backoff");
        Check(!backoffClient.SendKeyInput(req, resp, 45) &&
              backoffClient.ConsecutiveFailCount() == 1,
              "V2.1 call inside 50ms backoff skips IPC cleanly");
        Sleep(75);
        Check(!backoffClient.IsInHardBackoff(),
              "V2.1 fail #1 ~50ms backoff expires before 75ms (no 5s blackhole)");

        req.seq = 2;
        Check(!backoffClient.SendKeyInput(req, resp, 45) &&
              backoffClient.ConsecutiveFailCount() == 2 &&
              backoffClient.IsInHardBackoff(),
              "V2.1 fail #2 enters ~150ms backoff");
        Sleep(70);
        Check(backoffClient.IsInHardBackoff(),
              "V2.1 fail #2 stays in 150ms backoff at 70ms");
        Sleep(125);
        Check(!backoffClient.IsInHardBackoff(),
              "V2.1 fail #2 ~150ms backoff expires");

        req.seq = 3;
        Check(!backoffClient.SendKeyInput(req, resp, 45) &&
              backoffClient.ConsecutiveFailCount() == 3 &&
              backoffClient.IsInHardBackoff(),
              "V2.1 fail #3 enters ~500ms backoff");
        Sleep(200);
        Check(backoffClient.IsInHardBackoff(),
              "V2.1 fail #3 stays in 500ms backoff at 200ms");

        // 5b: server 延迟 80ms 就绪时，纯走 SendKeyInput 在 1s 内自动恢复并清零计数
        const std::wstring recoverPipeName =
            L"\\\\.\\pipe\\taishen-ime-test-recover-" +
            std::to_wstring(GetCurrentProcessId());
        std::atomic<bool> stopRecover{false};
        std::thread delayedServer([&]() {
            Sleep(80);
            while (!stopRecover.load()) {
                HANDLE hPipe = CreateNamedPipeW(
                    recoverPipeName.c_str(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    2,
                    taishen::ipc::kMaxFrameBytes,
                    taishen::ipc::kMaxFrameBytes,
                    0,
                    nullptr);
                if (hPipe == INVALID_HANDLE_VALUE) {
                    Sleep(10);
                    continue;
                }
                HANDLE hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
                if (hEvent == nullptr) {
                    CloseHandle(hPipe);
                    break;
                }
                OVERLAPPED ov{};
                ov.hEvent = hEvent;
                bool connected = false;
                if (ConnectNamedPipe(hPipe, &ov)) {
                    connected = true;
                } else {
                    const DWORD err = GetLastError();
                    if (err == ERROR_PIPE_CONNECTED) {
                        connected = true;
                    } else if (err == ERROR_IO_PENDING) {
                        while (!stopRecover.load()) {
                            const DWORD w = WaitForSingleObject(hEvent, 20);
                            if (w == WAIT_OBJECT_0) {
                                DWORD dummy = 0;
                                connected = GetOverlappedResult(hPipe, &ov, &dummy, FALSE) != FALSE;
                                break;
                            }
                            if (w != WAIT_TIMEOUT) {
                                break;
                            }
                        }
                        if (!connected) {
                            if (CancelIoEx(hPipe, &ov) || GetLastError() == ERROR_NOT_FOUND) {
                                DWORD dummy = 0;
                                GetOverlappedResult(hPipe, &ov, &dummy, TRUE);
                            }
                        }
                    }
                }
                CloseHandle(hEvent);

                if (connected && !stopRecover.load()) {
                    std::string frame;
                    while (!stopRecover.load() &&
                           taishen::ipc::ReadPipeFrameTimed(hPipe, frame, 200)) {
                        uint32_t ver = 0;
                        std::string op;
                        if (!taishen::ipc::PeekEnvelope(frame, ver, op)) {
                            break;
                        }
                        if (op == "ping") {
                            taishen::ipc::WritePipeFrameTimed(
                                hPipe,
                                taishen::ipc::EncodePongReply(true, 100, "mock-v2-recover"),
                                200);
                        } else if (op == "key_input") {
                            taishen::ipc::KeyInputRequest inReq;
                            if (taishen::ipc::DecodeKeyInput(frame, inReq)) {
                                taishen::ipc::CandidateListResponse outCand;
                                outCand.seq = inReq.seq;
                                outCand.sessionId = inReq.sessionId;
                                outCand.eaten = true;
                                outCand.pinyin = "ni";
                                outCand.preedit = "ni";
                                outCand.candidates = {{"你", 1, 1, 100, true}};
                                outCand.pageSize = 5;
                                taishen::ipc::WritePipeFrameTimed(
                                    hPipe, taishen::ipc::EncodeCandidateList(outCand), 200);
                            }
                        }
                    }
                }
                DisconnectNamedPipe(hPipe);
                CloseHandle(hPipe);
            }
        });

        LARGE_INTEGER freq{};
        QueryPerformanceFrequency(&freq);
        LARGE_INTEGER t0{}, tNow{};
        QueryPerformanceCounter(&t0);

        taishen::CRenderIpcClient recoverClient(recoverPipeName);
        taishen::ipc::KeyInputRequest rReq;
        rReq.seq = 1;
        rReq.sessionId = 999;
        rReq.hostPid = GetCurrentProcessId();
        rReq.vk = 'N';
        rReq.rawVk = 'N';
        rReq.fresh = true;
        taishen::ipc::CandidateListResponse rResp;

        const bool firstFail = !recoverClient.SendKeyInput(rReq, rResp, 45);
        Check(firstFail && recoverClient.ConsecutiveFailCount() == 1,
              "V2.1 initial SendKeyInput before server ready fails into 50ms probe");

        bool recovered = false;
        double recoverMs = 0.0;
        for (int iter = 0; iter < 50; ++iter) {
            Sleep(20);
            rReq.seq = static_cast<uint64_t>(iter + 2);
            if (recoverClient.SendKeyInput(rReq, rResp, 50)) {
                QueryPerformanceCounter(&tNow);
                recoverMs = QpcToMs(t0, tNow, freq);
                recovered = true;
                break;
            }
        }
        std::printf("[INFO] V2.1 delayed-ready recovery elapsed=%.2fms pinyin=%s cands=%zu\n",
                    recoverMs, rResp.pinyin.c_str(), rResp.candidates.size());
        Check(recovered && recoverMs < 1000.0 && rResp.eaten && rResp.pinyin == "ni" &&
              !rResp.candidates.empty() && rResp.candidates[0].text == "你" &&
              recoverClient.ConsecutiveFailCount() == 0 && !recoverClient.IsInHardBackoff(),
              "V2.1 recovers cleanly within < 1000ms and resets backoff state");

        recoverClient.Disconnect();
        stopRecover.store(true);
        delayedServer.join();
    }

    std::printf("\nResult: %s (fails=%d)\n", g_fails == 0 ? "ALL PASS" : "FAILED", g_fails);
    return g_fails == 0 ? 0 : 1;
}
