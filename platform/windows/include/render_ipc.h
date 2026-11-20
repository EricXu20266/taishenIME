/// V4 D2 候选窗进程外渲染 IPC 协议（DLL <-> taishen_server.exe）
///
/// 传输：命名管道 `\\.\pipe\taishen-ime-render`
/// 帧格式：4 字节小端长度前缀（<= 64KB）+ UTF-8 JSON 载荷
/// 权限：DACL 含 ALL APPLICATION PACKAGES (S-1-15-2-1) 与 LPAC (S-1-15-2-2)，
///       SACL 含低完整性标签 (LW)，确保任务栏搜索与商店应用 AppContainer 可直连。
/// 零外部依赖（仅 Win32 + C++17 标准库），DLL 与 server 共用。

#pragma once

#include <windows.h>

#include <cstdint>
#include <cstring>
#include <string>
#include <vector>

namespace taishen {
namespace ipc {

constexpr uint32_t kRenderIpcVersion = 1;
constexpr uint32_t kMaxFrameBytes = 64 * 1024;

inline constexpr const wchar_t* kRenderPipeName = L"\\\\.\\pipe\\taishen-ime-render";
inline constexpr const wchar_t* kServerMutexName = L"Local\\TaishenIMEServerSingleInstance";

/// 命名管道 SDDL：
/// - SY / BA：完全控制
/// - OW / AU：当前登录用户读写
/// - AC (S-1-15-2-1) & LPAC (S-1-15-2-2)：AppContainer / 任务栏搜索 / 商店应用读写
/// - S:(ML;;NW;;;LW)：低完整性强制标签，允许低完整性容器进程打开管道
inline constexpr const wchar_t* kRenderPipeSddl =
    L"D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;OW)(A;;GRGW;;;AU)"
    L"(A;;GRGW;;;AC)(A;;GRGW;;;S-1-15-2-2)S:(ML;;NW;;;LW)";

enum class RenderOp : uint8_t {
    Unknown = 0,
    Show,
    Move,
    Hide,
    ReloadConfig,
    Ping,
};

struct RenderCommand {
    uint32_t version = kRenderIpcVersion;
    RenderOp op = RenderOp::Unknown;
    uint64_t sessionId = 0;
    uint32_t hostPid = 0;
    std::string pinyin;
    std::vector<std::string> candidates;
    RECT caret = {0, 0, 0, 0};
    int page = 0;
    int totalPages = 0;
    int selectedIndex = 0;
    bool inlinePreedit = true;
    bool multiRow = false;
};

inline void AppendJsonEscaped(std::string& out, const std::string& s)
{
    out.push_back('"');
    for (unsigned char c : s) {
        switch (c) {
        case '"':  out += "\\\""; break;
        case '\\': out += "\\\\"; break;
        case '\b': out += "\\b"; break;
        case '\f': out += "\\f"; break;
        case '\n': out += "\\n"; break;
        case '\r': out += "\\r"; break;
        case '\t': out += "\\t"; break;
        default:
            if (c < 0x20) {
                char buf[8];
                static const char kHex[] = "0123456789abcdef";
                buf[0] = '\\'; buf[1] = 'u'; buf[2] = '0'; buf[3] = '0';
                buf[4] = kHex[(c >> 4) & 0xF];
                buf[5] = kHex[c & 0xF];
                buf[6] = '\0';
                out += buf;
            } else {
                out.push_back(static_cast<char>(c));
            }
            break;
        }
    }
    out.push_back('"');
}

inline std::string EncodeShowCommand(const RenderCommand& cmd)
{
    std::string j;
    j.reserve(256 + cmd.pinyin.size() + cmd.candidates.size() * 24);
    j += "{\"v\":1,\"op\":\"show\",\"sid\":";
    j += std::to_string(cmd.sessionId);
    j += ",\"pid\":";
    j += std::to_string(cmd.hostPid);
    j += ",\"pinyin\":";
    AppendJsonEscaped(j, cmd.pinyin);
    j += ",\"cands\":[";
    for (size_t i = 0; i < cmd.candidates.size(); ++i) {
        if (i > 0) {
            j.push_back(',');
        }
        AppendJsonEscaped(j, cmd.candidates[i]);
    }
    j += "],\"caret\":[";
    j += std::to_string(cmd.caret.left) + "," + std::to_string(cmd.caret.top) + "," +
         std::to_string(cmd.caret.right) + "," + std::to_string(cmd.caret.bottom);
    j += "],\"page\":";
    j += std::to_string(cmd.page);
    j += ",\"total\":";
    j += std::to_string(cmd.totalPages);
    j += ",\"sel\":";
    j += std::to_string(cmd.selectedIndex);
    j += ",\"inline\":";
    j += (cmd.inlinePreedit ? "1" : "0");
    j += ",\"multi\":";
    j += (cmd.multiRow ? "1" : "0");
    j += "}";
    return j;
}

inline std::string EncodeMoveCommand(uint64_t sessionId, const RECT& caret)
{
    std::string j = "{\"v\":1,\"op\":\"move\",\"sid\":";
    j += std::to_string(sessionId);
    j += ",\"caret\":[";
    j += std::to_string(caret.left) + "," + std::to_string(caret.top) + "," +
         std::to_string(caret.right) + "," + std::to_string(caret.bottom);
    j += "]}";
    return j;
}

inline std::string EncodeHideCommand(uint64_t sessionId)
{
    return "{\"v\":1,\"op\":\"hide\",\"sid\":" + std::to_string(sessionId) + "}";
}

inline std::string EncodeReloadCommand()
{
    return "{\"v\":1,\"op\":\"reload\"}";
}

inline std::string EncodePingCommand()
{
    return "{\"v\":1,\"op\":\"ping\"}";
}

// ---------------------------------------------------------------------------
// 轻量 JSON 解码器（仅服务本协议固定 schema，避免引入外部依赖）
// ---------------------------------------------------------------------------
namespace detail {

inline void SkipWs(const std::string& s, size_t& pos)
{
    while (pos < s.size() && (s[pos] == ' ' || s[pos] == '\t' || s[pos] == '\r' || s[pos] == '\n')) {
        ++pos;
    }
}

inline bool ParseString(const std::string& s, size_t& pos, std::string& out)
{
    SkipWs(s, pos);
    if (pos >= s.size() || s[pos] != '"') {
        return false;
    }
    ++pos;
    out.clear();
    while (pos < s.size()) {
        char c = s[pos++];
        if (c == '"') {
            return true;
        }
        if (c == '\\' && pos < s.size()) {
            char esc = s[pos++];
            switch (esc) {
            case '"':  out.push_back('"'); break;
            case '\\': out.push_back('\\'); break;
            case '/':  out.push_back('/'); break;
            case 'b':  out.push_back('\b'); break;
            case 'f':  out.push_back('\f'); break;
            case 'n':  out.push_back('\n'); break;
            case 'r':  out.push_back('\r'); break;
            case 't':  out.push_back('\t'); break;
            case 'u':
                if (pos + 4 <= s.size()) {
                    unsigned int code = 0;
                    for (int i = 0; i < 4; ++i) {
                        char h = s[pos++];
                        code <<= 4;
                        if (h >= '0' && h <= '9') code |= static_cast<unsigned int>(h - '0');
                        else if (h >= 'a' && h <= 'f') code |= static_cast<unsigned int>(h - 'a' + 10);
                        else if (h >= 'A' && h <= 'F') code |= static_cast<unsigned int>(h - 'A' + 10);
                    }
                    if (code < 0x80) {
                        out.push_back(static_cast<char>(code));
                    }
                }
                break;
            default:
                out.push_back(esc);
                break;
            }
        } else {
            out.push_back(c);
        }
    }
    return false;
}

inline bool ParseInt64(const std::string& s, size_t& pos, int64_t& out)
{
    SkipWs(s, pos);
    if (pos >= s.size()) {
        return false;
    }
    bool neg = false;
    if (s[pos] == '-') {
        neg = true;
        ++pos;
    }
    if (pos >= s.size() || s[pos] < '0' || s[pos] > '9') {
        return false;
    }
    uint64_t val = 0;
    while (pos < s.size() && s[pos] >= '0' && s[pos] <= '9') {
        val = val * 10 + static_cast<uint64_t>(s[pos++] - '0');
    }
    out = neg ? -static_cast<int64_t>(val) : static_cast<int64_t>(val);
    return true;
}

inline bool ParseUint64(const std::string& s, size_t& pos, uint64_t& out)
{
    SkipWs(s, pos);
    if (pos >= s.size() || s[pos] < '0' || s[pos] > '9') {
        return false;
    }
    uint64_t val = 0;
    while (pos < s.size() && s[pos] >= '0' && s[pos] <= '9') {
        val = val * 10 + static_cast<uint64_t>(s[pos++] - '0');
    }
    out = val;
    return true;
}

inline bool SkipValue(const std::string& s, size_t& pos)
{
    SkipWs(s, pos);
    if (pos >= s.size()) return false;
    char c = s[pos];
    if (c == '"') {
        std::string dummy;
        return ParseString(s, pos, dummy);
    }
    if (c == '[' || c == '{') {
        char open = c;
        char close = (c == '[') ? ']' : '}';
        int depth = 0;
        while (pos < s.size()) {
            if (s[pos] == '"') {
                std::string dummy;
                if (!ParseString(s, pos, dummy)) return false;
                continue;
            }
            if (s[pos] == open) ++depth;
            else if (s[pos] == close) {
                --depth;
                if (depth == 0) { ++pos; return true; }
            }
            ++pos;
        }
        return false;
    }
    while (pos < s.size() && s[pos] != ',' && s[pos] != '}' && s[pos] != ']') {
        ++pos;
    }
    return true;
}

} // namespace detail

inline bool DecodeCommand(const std::string& json, RenderCommand& out)
{
    out = RenderCommand{};
    size_t pos = 0;
    detail::SkipWs(json, pos);
    if (pos >= json.size() || json[pos] != '{') {
        return false;
    }
    ++pos;

    while (pos < json.size()) {
        detail::SkipWs(json, pos);
        if (pos < json.size() && json[pos] == '}') {
            ++pos;
            break;
        }
        std::string key;
        if (!detail::ParseString(json, pos, key)) {
            return false;
        }
        detail::SkipWs(json, pos);
        if (pos >= json.size() || json[pos] != ':') {
            return false;
        }
        ++pos;

        if (key == "v") {
            uint64_t v = 0;
            if (!detail::ParseUint64(json, pos, v)) return false;
            out.version = static_cast<uint32_t>(v);
        } else if (key == "op") {
            std::string op;
            if (!detail::ParseString(json, pos, op)) return false;
            if (op == "show") out.op = RenderOp::Show;
            else if (op == "move") out.op = RenderOp::Move;
            else if (op == "hide") out.op = RenderOp::Hide;
            else if (op == "reload") out.op = RenderOp::ReloadConfig;
            else if (op == "ping") out.op = RenderOp::Ping;
        } else if (key == "sid") {
            if (!detail::ParseUint64(json, pos, out.sessionId)) return false;
        } else if (key == "pid") {
            uint64_t pid = 0;
            if (!detail::ParseUint64(json, pos, pid)) return false;
            out.hostPid = static_cast<uint32_t>(pid);
        } else if (key == "pinyin") {
            if (!detail::ParseString(json, pos, out.pinyin)) return false;
        } else if (key == "cands") {
            detail::SkipWs(json, pos);
            if (pos >= json.size() || json[pos] != '[') return false;
            ++pos;
            out.candidates.clear();
            while (pos < json.size()) {
                detail::SkipWs(json, pos);
                if (pos < json.size() && json[pos] == ']') {
                    ++pos;
                    break;
                }
                std::string cand;
                if (!detail::ParseString(json, pos, cand)) return false;
                out.candidates.push_back(std::move(cand));
                detail::SkipWs(json, pos);
                if (pos < json.size() && json[pos] == ',') ++pos;
            }
        } else if (key == "caret") {
            detail::SkipWs(json, pos);
            if (pos >= json.size() || json[pos] != '[') return false;
            ++pos;
            int64_t vals[4] = {0, 0, 0, 0};
            for (int i = 0; i < 4; ++i) {
                if (!detail::ParseInt64(json, pos, vals[i])) return false;
                detail::SkipWs(json, pos);
                if (i < 3 && pos < json.size() && json[pos] == ',') ++pos;
            }
            detail::SkipWs(json, pos);
            if (pos >= json.size() || json[pos] != ']') return false;
            ++pos;
            out.caret.left = static_cast<LONG>(vals[0]);
            out.caret.top = static_cast<LONG>(vals[1]);
            out.caret.right = static_cast<LONG>(vals[2]);
            out.caret.bottom = static_cast<LONG>(vals[3]);
        } else if (key == "page") {
            int64_t v = 0;
            if (!detail::ParseInt64(json, pos, v)) return false;
            out.page = static_cast<int>(v);
        } else if (key == "total") {
            int64_t v = 0;
            if (!detail::ParseInt64(json, pos, v)) return false;
            out.totalPages = static_cast<int>(v);
        } else if (key == "sel") {
            int64_t v = 0;
            if (!detail::ParseInt64(json, pos, v)) return false;
            out.selectedIndex = static_cast<int>(v);
        } else if (key == "inline") {
            int64_t v = 0;
            if (!detail::ParseInt64(json, pos, v)) return false;
            out.inlinePreedit = (v != 0);
        } else if (key == "multi") {
            int64_t v = 0;
            if (!detail::ParseInt64(json, pos, v)) return false;
            out.multiRow = (v != 0);
        } else {
            if (!detail::SkipValue(json, pos)) return false;
        }

        detail::SkipWs(json, pos);
        if (pos < json.size() && json[pos] == ',') {
            ++pos;
        }
    }

    return out.version == kRenderIpcVersion && out.op != RenderOp::Unknown;
}

/// 发送长度前缀帧（4B LE + payload）
inline bool WritePipeFrame(HANDLE hPipe, const std::string& payload)
{
    if (hPipe == INVALID_HANDLE_VALUE || payload.size() > kMaxFrameBytes) {
        return false;
    }
    const uint32_t len = static_cast<uint32_t>(payload.size());
    std::string buf;
    buf.resize(4 + payload.size());
    std::memcpy(&buf[0], &len, 4);
    if (!payload.empty()) {
        std::memcpy(&buf[4], payload.data(), payload.size());
    }
    DWORD written = 0;
    const BOOL ok = WriteFile(hPipe, buf.data(), static_cast<DWORD>(buf.size()), &written, nullptr);
    return ok != FALSE && written == buf.size();
}

/// 读取精确字节数
inline bool ReadPipeExact(HANDLE hPipe, void* dst, DWORD bytes)
{
    BYTE* ptr = static_cast<BYTE*>(dst);
    DWORD remain = bytes;
    while (remain > 0) {
        DWORD got = 0;
        if (!ReadFile(hPipe, ptr, remain, &got, nullptr) || got == 0) {
            return false;
        }
        ptr += got;
        remain -= got;
    }
    return true;
}

/// 读取长度前缀帧
inline bool ReadPipeFrame(HANDLE hPipe, std::string& outPayload)
{
    uint32_t len = 0;
    if (!ReadPipeExact(hPipe, &len, 4)) {
        return false;
    }
    if (len == 0 || len > kMaxFrameBytes) {
        return false;
    }
    outPayload.resize(len);
    return ReadPipeExact(hPipe, &outPayload[0], len);
}

} // namespace ipc
} // namespace taishen
