/// Diagnostic log - troubleshooting TSF activation/key handling (0.1.15)
/// Output: %LOCALAPPDATA%\TaishenIME\ime_debug.log (append)
/// NOTE: keep this file pure-ASCII - the old MSVC (cl 19.0, no /utf-8)
/// parses sources as GBK; CJK chars in comments can corrupt following
/// identifiers. Other sources keep CJK comments (they happen to survive),
/// but this header must stay ASCII-safe.
#pragma once

#include <string>

namespace taishen {

/// Append one log line (timestamp + pid/tid prefix)
void DebugLog(const std::string& msg);

/// Append one log line with HRESULT hex suffix
void DebugLogHr(const std::string& msg, long hr);

/// V0.4.x: Enable/disable diagnostic logging (called from ApplyConfig)
void SetDebugLogEnabled(bool enabled);

/// 强制写日志（不受诊断开关控制）——崩溃/致命路径必须记录
void ForceLog(const std::string& msg);

} // namespace taishen
