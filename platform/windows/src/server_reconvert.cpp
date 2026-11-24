/// taishen_server 服务端再转换（Q3.1 `ITfFnReconversion`）IPC 处理器实现

#include "server_reconvert.h"

#include <vector>

#include "debug_log.h"
#include "engine_bridge.h"
#include "render_ipc.h"

namespace taishen {

bool HandleServerReconvert(HANDLE hPipe,
                           const std::string& payload,
                           ULONG clientPid,
                           uint32_t connVersion,
                           DWORD writeTimeoutMs)
{
    ipc::ReconvertRequest req;
    if (!ipc::DecodeReconvertRequest(payload, req)) {
        ForceLog("taishen_server: invalid reconvert payload clientPid=" +
                 std::to_string(clientPid));
        return false;
    }
    if (req.hostPid != 0 && req.hostPid != clientPid) {
        ForceLog("taishen_server: hostPid mismatch on reconvert claimed=" +
                 std::to_string(req.hostPid) + " actual=" + std::to_string(clientPid));
        return false;
    }

    ipc::ReconvertResponse resp;
    if (engine_dict_ready() == 1 && !req.text.empty()) {
        char pinyinBuf[128] = {0};
        char candsBuf[4096] = {0};
        const int count = engine_reconvert(
            req.text.c_str(), req.maxCount,
            pinyinBuf, static_cast<int>(sizeof(pinyinBuf)),
            candsBuf, static_cast<int>(sizeof(candsBuf)));
        if (count > 0) {
            resp.ok = true;
            resp.pinyin = pinyinBuf;
            const std::string raw(candsBuf);
            size_t start = 0;
            while (start <= raw.size()) {
                const size_t nl = raw.find('\n', start);
                const std::string item = (nl == std::string::npos)
                                             ? raw.substr(start)
                                             : raw.substr(start, nl - start);
                if (!item.empty()) {
                    resp.candidates.push_back(item);
                }
                if (nl == std::string::npos) {
                    break;
                }
                start = nl + 1;
            }
        }
    }

    return ipc::WritePipeFrameTimed(
        hPipe, ipc::EncodeReconvertResponse(resp, connVersion), writeTimeoutMs);
}

} // namespace taishen
