/// taishen_server 服务端再转换（Q3.1 `ITfFnReconversion`）IPC 处理器声明

#pragma once

#include <windows.h>

#include <cstdint>
#include <string>

namespace taishen {

/// 处理命名管道 `"reconvert"` 请求：校验 hostPid → 调 `engine_reconvert` → 回包。
/// 返回 false 表示连接损坏应断开，true 表示已按协议回包（无论是否有候选）。
bool HandleServerReconvert(HANDLE hPipe,
                           const std::string& payload,
                           ULONG clientPid,
                           uint32_t connVersion,
                           DWORD writeTimeoutMs);

} // namespace taishen
