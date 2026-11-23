/// V4 D2/D3 进程外渲染与引擎 IPC 协议（DLL <-> taishen_server.exe）
///
/// 传输：命名管道 `\\.\pipe\taishen-ime-render`
/// 帧格式：4 字节小端长度前缀（<= 64KB）+ UTF-8 JSON 载荷
/// 权限：服务端动态生成 DACL（当前用户 SID + OW + AC + LPAC + SY + BA），
///       SACL 含低完整性标签 (LW)，确保任务栏搜索与商店应用 AppContainer 可直连。
/// 零外部依赖（仅 Win32 + C++17 标准库），DLL 与 server 共用；全部整数使用定宽类型，
/// 天然兼容 32 位 DLL 连 64 位 server。

#pragma once

#include <windows.h>

#include <cstdint>
#include <cstring>
#include <limits>
#include <string>
#include <vector>

namespace taishen {
namespace ipc {

constexpr uint32_t kMinSupportedIpcVersion = 2;
constexpr uint32_t kRenderIpcVersion = 3;
constexpr uint32_t kMaxFrameBytes = 64 * 1024;

inline constexpr bool IsSupportedIpcVersion(uint32_t ver)
{
    return ver >= kMinSupportedIpcVersion && ver <= kRenderIpcVersion;
}

inline constexpr const wchar_t* kRenderPipeName = L"\\\\.\\pipe\\taishen-ime-render";
inline constexpr const wchar_t* kServerMutexName = L"Local\\TaishenIMEServerSingleInstance";

/// KeyInputRequest.modifiers 位掩码
constexpr uint32_t kModShift = 1u << 0;
constexpr uint32_t kModCtrl = 1u << 1;
constexpr uint32_t kModAlt = 1u << 2;
constexpr uint32_t kModCapsLock = 1u << 3;

/// KeyInputRequest.flags 位掩码（显式置位控制，避免未知默认状态覆盖服务端配置）
constexpr uint32_t kFlagTraditional = 1u << 0;
constexpr uint32_t kFlagShuangpin = 1u << 1;
constexpr uint32_t kFlagHasTraditional = 1u << 2;
constexpr uint32_t kFlagHasShuangpin = 1u << 3;

/// Rust CandidateMeta.reasons 位标镜像：前缀词（选中时触发动态组词）
constexpr uint32_t kReasonPrefixWord = 1u << 14;

/// 命名管道静态兜底 SDDL（服务端运行时以当前用户 SID 构造 DACL，不开放 AU）：
/// - SY / BA：完全控制
/// - OW：所有者读写
/// - AC (S-1-15-2-1) & LPAC (S-1-15-2-2)：AppContainer / 任务栏搜索 / 商店应用读写
/// - S:(ML;;NW;;;LW)：低完整性强制标签，允许低完整性容器进程打开管道
inline constexpr const wchar_t* kRenderPipeSddl =
    L"D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;OW)"
    L"(A;;GRGW;;;AC)(A;;GRGW;;;S-1-15-2-2)S:(ML;;NW;;;LW)";

enum class RenderOp : uint8_t {
    Unknown = 0,
    Show,
    Move,
    Hide,
    ReloadConfig,
    Ping,
    KeyInput,
    CandidateList,
    Commit,
    Error,
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

struct CandidateItem {
    std::string text;
    uint8_t tier = 0;
    uint32_t reasons = 0;
    int32_t score = 0;
    bool hasScore = false;
};

struct KeyInputRequest {
    uint32_t version = kRenderIpcVersion;
    uint64_t seq = 0;
    uint64_t sessionId = 0;
    uint32_t hostPid = 0;
    uint32_t vk = 0;
    uint32_t rawVk = 0;
    uint32_t modifiers = 0;
    bool asciiMode = false;
    bool fresh = false;
    uint32_t flags = 0;
};

struct CandidateListResponse {
    uint32_t version = kRenderIpcVersion;
    uint64_t seq = 0;
    uint64_t sessionId = 0;
    bool eaten = false;
    std::string pinyin;
    std::string preedit;
    std::string syllable;
    std::vector<CandidateItem> candidates;
    int32_t page = 0;
    int32_t totalPages = 0;
    int32_t totalCandidates = 0;
    int32_t pageSize = 5;
    int32_t selectedIndex = 0;
    bool inCompose = false;
    std::string composePrefix;
    std::string committed;
    int32_t inputMode = 0;
    bool symbolPrefix = false;
    bool semicolonActive = false;
};

struct CommitRequest {
    uint32_t version = kRenderIpcVersion;
    uint64_t sessionId = 0;
    uint32_t hostPid = 0;
    std::string pinyin;
    std::string word;
    int32_t index = -1;
};

struct PongInfo {
    uint32_t version = 0;
    bool ok = false;
    bool dictReady = false;
    int32_t entries = 0;
    std::string build;
    std::string errorCode;
    uint32_t expectedVersion = 0;
    uint32_t gotVersion = 0;
    bool uiAccess = false;
    uint32_t windowBand = 0;
    bool traditional = false;
    bool shuangpin = false;
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
    j += "{\"v\":";
    j += std::to_string(kRenderIpcVersion);
    j += ",\"op\":\"show\",\"sid\":";
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
    std::string j = "{\"v\":";
    j += std::to_string(kRenderIpcVersion);
    j += ",\"op\":\"move\",\"sid\":";
    j += std::to_string(sessionId);
    j += ",\"caret\":[";
    j += std::to_string(caret.left) + "," + std::to_string(caret.top) + "," +
         std::to_string(caret.right) + "," + std::to_string(caret.bottom);
    j += "]}";
    return j;
}

inline std::string EncodeHideCommand(uint64_t sessionId)
{
    return "{\"v\":" + std::to_string(kRenderIpcVersion) +
           ",\"op\":\"hide\",\"sid\":" + std::to_string(sessionId) + "}";
}

inline std::string EncodeReloadCommand()
{
    return "{\"v\":" + std::to_string(kRenderIpcVersion) + ",\"op\":\"reload\"}";
}

inline std::string EncodePingCommand(uint64_t sessionId = 0, uint32_t hostPid = 0)
{
    std::string j = "{\"v\":";
    j += std::to_string(kRenderIpcVersion);
    j += ",\"op\":\"ping\",\"sid\":";
    j += std::to_string(sessionId);
    j += ",\"pid\":";
    j += std::to_string(hostPid);
    j += "}";
    return j;
}

inline std::string EncodePongReply(bool dictReady, int32_t entries, const std::string& build,
                                   bool uiAccess = false, uint32_t windowBand = 0,
                                   bool traditional = false, bool shuangpin = false,
                                   uint32_t protocolVersion = kRenderIpcVersion)
{
    std::string j;
    j.reserve(192 + build.size());
    j += "{\"v\":";
    j += std::to_string(protocolVersion);
    j += ",\"op\":\"pong\",\"ok\":1,\"dictReady\":";
    j += (dictReady ? "1" : "0");
    j += ",\"entries\":";
    j += std::to_string(entries);
    j += ",\"build\":";
    AppendJsonEscaped(j, build);
    j += ",\"uiAccess\":";
    j += (uiAccess ? "1" : "0");
    j += ",\"band\":";
    j += std::to_string(windowBand);
    j += ",\"traditional\":";
    j += (traditional ? "1" : "0");
    j += ",\"shuangpin\":";
    j += (shuangpin ? "1" : "0");
    j += "}";
    return j;
}

inline std::string EncodeVersionErrorReply(uint32_t gotVersion)
{
    return "{\"v\":" + std::to_string(kRenderIpcVersion) +
           ",\"op\":\"error\",\"ok\":0,\"code\":\"version_mismatch\",\"expected\":" +
           std::to_string(kRenderIpcVersion) + ",\"got\":" + std::to_string(gotVersion) + "}";
}

inline std::string EncodeKeyInput(const KeyInputRequest& req)
{
    std::string j;
    j.reserve(224);
    j += "{\"v\":";
    j += std::to_string(kRenderIpcVersion);
    j += ",\"op\":\"key_input\",\"seq\":";
    j += std::to_string(req.seq);
    j += ",\"sid\":";
    j += std::to_string(req.sessionId);
    j += ",\"pid\":";
    j += std::to_string(req.hostPid);
    j += ",\"vk\":";
    j += std::to_string(req.vk);
    j += ",\"rawVk\":";
    j += std::to_string(req.rawVk);
    j += ",\"modifiers\":";
    j += std::to_string(req.modifiers);
    j += ",\"asciiMode\":";
    j += (req.asciiMode ? "1" : "0");
    j += ",\"fresh\":";
    j += (req.fresh ? "1" : "0");
    j += ",\"flags\":";
    j += std::to_string(req.flags);
    j += "}";
    return j;
}

inline std::string EncodeCandidateList(const CandidateListResponse& resp,
                                       uint32_t protocolVersion = kRenderIpcVersion)
{
    std::string j;
    j.reserve(384 + resp.pinyin.size() + resp.preedit.size() + resp.syllable.size() +
              resp.candidates.size() * 48);
    j += "{\"v\":";
    j += std::to_string(protocolVersion);
    j += ",\"op\":\"candidate_list\",\"seq\":";
    j += std::to_string(resp.seq);
    j += ",\"sid\":";
    j += std::to_string(resp.sessionId);
    j += ",\"eaten\":";
    j += (resp.eaten ? "1" : "0");
    j += ",\"pinyin\":";
    AppendJsonEscaped(j, resp.pinyin);
    j += ",\"preedit\":";
    AppendJsonEscaped(j, resp.preedit);
    j += ",\"syllable\":";
    AppendJsonEscaped(j, resp.syllable);
    j += ",\"candidates\":[";
    for (size_t i = 0; i < resp.candidates.size(); ++i) {
        if (i > 0) {
            j.push_back(',');
        }
        const CandidateItem& c = resp.candidates[i];
        j += "{\"text\":";
        AppendJsonEscaped(j, c.text);
        j += ",\"tier\":";
        j += std::to_string(c.tier);
        j += ",\"reasons\":";
        j += std::to_string(c.reasons);
        j += ",\"score\":";
        if (c.hasScore) {
            j += std::to_string(c.score);
        } else {
            j += "null";
        }
        j += "}";
    }
    j += "],\"page\":";
    j += std::to_string(resp.page);
    j += ",\"totalPages\":";
    j += std::to_string(resp.totalPages);
    j += ",\"totalCandidates\":";
    j += std::to_string(resp.totalCandidates);
    j += ",\"pageSize\":";
    j += std::to_string(resp.pageSize);
    j += ",\"selectedIndex\":";
    j += std::to_string(resp.selectedIndex);
    j += ",\"inCompose\":";
    j += (resp.inCompose ? "1" : "0");
    j += ",\"composePrefix\":";
    AppendJsonEscaped(j, resp.composePrefix);
    j += ",\"committed\":";
    AppendJsonEscaped(j, resp.committed);
    j += ",\"inputMode\":";
    j += std::to_string(resp.inputMode);
    j += ",\"symbolPrefix\":";
    j += (resp.symbolPrefix ? "1" : "0");
    j += ",\"semicolonActive\":";
    j += (resp.semicolonActive ? "1" : "0");
    j += "}";
    return j;
}

inline std::string EncodeCommit(const CommitRequest& req)
{
    std::string j;
    j.reserve(160 + req.pinyin.size() + req.word.size());
    j += "{\"v\":";
    j += std::to_string(kRenderIpcVersion);
    j += ",\"op\":\"commit\",\"sid\":";
    j += std::to_string(req.sessionId);
    j += ",\"pid\":";
    j += std::to_string(req.hostPid);
    j += ",\"pinyin\":";
    AppendJsonEscaped(j, req.pinyin);
    j += ",\"word\":";
    AppendJsonEscaped(j, req.word);
    j += ",\"index\":";
    j += std::to_string(req.index);
    j += "}";
    return j;
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

inline void AppendUtf8Codepoint(std::string& out, unsigned int cp)
{
    if (cp < 0x80) {
        out.push_back(static_cast<char>(cp));
    } else if (cp < 0x800) {
        out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else {
        out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
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
                    AppendUtf8Codepoint(out, code);
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
    const uint64_t limit = neg
        ? (static_cast<uint64_t>((std::numeric_limits<int64_t>::max)()) + 1ULL)
        : static_cast<uint64_t>((std::numeric_limits<int64_t>::max)());
    uint64_t val = 0;
    int digits = 0;
    constexpr int kMaxNumberDigits = 32;
    while (pos < s.size() && s[pos] >= '0' && s[pos] <= '9') {
        if (++digits > kMaxNumberDigits) {
            return false;
        }
        const uint64_t digit = static_cast<uint64_t>(s[pos++] - '0');
        if (val > (limit - digit) / 10ULL) {
            return false;
        }
        val = val * 10ULL + digit;
    }
    if (neg) {
        if (val == limit) {
            out = (std::numeric_limits<int64_t>::min)();
        } else {
            out = -static_cast<int64_t>(val);
        }
    } else {
        out = static_cast<int64_t>(val);
    }
    return true;
}

inline bool ParseUint64(const std::string& s, size_t& pos, uint64_t& out)
{
    SkipWs(s, pos);
    if (pos >= s.size() || s[pos] < '0' || s[pos] > '9') {
        return false;
    }
    constexpr uint64_t kLimit = (std::numeric_limits<uint64_t>::max)();
    uint64_t val = 0;
    int digits = 0;
    constexpr int kMaxNumberDigits = 32;
    while (pos < s.size() && s[pos] >= '0' && s[pos] <= '9') {
        if (++digits > kMaxNumberDigits) {
            return false;
        }
        const uint64_t digit = static_cast<uint64_t>(s[pos++] - '0');
        if (val > (kLimit - digit) / 10ULL) {
            return false;
        }
        val = val * 10ULL + digit;
    }
    out = val;
    return true;
}

inline bool ParseUint32(const std::string& s, size_t& pos, uint32_t& out)
{
    uint64_t v = 0;
    if (!ParseUint64(s, pos, v) ||
        v > static_cast<uint64_t>((std::numeric_limits<uint32_t>::max)())) {
        return false;
    }
    out = static_cast<uint32_t>(v);
    return true;
}

inline bool ParseUint8(const std::string& s, size_t& pos, uint8_t& out)
{
    uint64_t v = 0;
    if (!ParseUint64(s, pos, v) ||
        v > static_cast<uint64_t>((std::numeric_limits<uint8_t>::max)())) {
        return false;
    }
    out = static_cast<uint8_t>(v);
    return true;
}

inline bool ParseInt32(const std::string& s, size_t& pos, int32_t& out)
{
    int64_t v = 0;
    if (!ParseInt64(s, pos, v) ||
        v < static_cast<int64_t>((std::numeric_limits<int32_t>::min)()) ||
        v > static_cast<int64_t>((std::numeric_limits<int32_t>::max)())) {
        return false;
    }
    out = static_cast<int32_t>(v);
    return true;
}

inline bool ParseBoolOrInt(const std::string& s, size_t& pos, bool& out)
{
    SkipWs(s, pos);
    if (pos + 4 <= s.size() && s.compare(pos, 4, "true") == 0) {
        pos += 4;
        out = true;
        return true;
    }
    if (pos + 5 <= s.size() && s.compare(pos, 5, "false") == 0) {
        pos += 5;
        out = false;
        return true;
    }
    int64_t v = 0;
    if (!ParseInt64(s, pos, v)) {
        return false;
    }
    out = (v != 0);
    return true;
}

inline bool ParseNullableInt32(const std::string& s, size_t& pos, int32_t& out, bool& hasValue)
{
    SkipWs(s, pos);
    if (pos + 4 <= s.size() && s.compare(pos, 4, "null") == 0) {
        pos += 4;
        out = 0;
        hasValue = false;
        return true;
    }
    int32_t v = 0;
    if (!ParseInt32(s, pos, v)) {
        return false;
    }
    out = v;
    hasValue = true;
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

/// 通用顶层 JSON 对象解析循环：回调签名 `bool onField(const std::string& key, size_t& pos)`
template <typename OnField>
inline bool ParseJsonObject(const std::string& json, size_t& pos, OnField&& onField)
{
    SkipWs(json, pos);
    if (pos >= json.size() || json[pos] != '{') {
        return false;
    }
    ++pos;

    bool closed = false;
    while (pos < json.size()) {
        SkipWs(json, pos);
        if (pos < json.size() && json[pos] == '}') {
            ++pos;
            closed = true;
            break;
        }
        std::string key;
        if (!ParseString(json, pos, key)) {
            return false;
        }
        SkipWs(json, pos);
        if (pos >= json.size() || json[pos] != ':') {
            return false;
        }
        ++pos;
        if (!onField(key, pos)) {
            return false;
        }
        SkipWs(json, pos);
        if (pos < json.size() && json[pos] == ',') {
            ++pos;
        }
    }
    return closed;
}

inline bool ParseCandidateItem(const std::string& json, size_t& pos, CandidateItem& item)
{
    item = CandidateItem{};
    bool gotText = false;
    const bool ok = ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "text") {
            gotText = ParseString(json, p, item.text);
            return gotText;
        }
        if (key == "tier") {
            return ParseUint8(json, p, item.tier);
        }
        if (key == "reasons") {
            return ParseUint32(json, p, item.reasons);
        }
        if (key == "score") {
            return ParseNullableInt32(json, p, item.score, item.hasScore);
        }
        return SkipValue(json, p);
    });
    return ok && gotText;
}

} // namespace detail

/// 仅扫描顶层版本与操作名（用于握手版本校验与消息路由）
inline bool PeekEnvelope(const std::string& json, uint32_t& outVersion, std::string& outOp)
{
    outVersion = 0;
    outOp.clear();
    size_t pos = 0;
    const bool ok = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, outVersion);
        }
        if (key == "op") {
            return detail::ParseString(json, p, outOp);
        }
        return detail::SkipValue(json, p);
    });
    return ok && outVersion > 0 && !outOp.empty();
}

inline bool DecodeCommand(const std::string& json, RenderCommand& out)
{
    out = RenderCommand{};
    out.version = 0;
    size_t pos = 0;
    const bool ok = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, out.version);
        }
        if (key == "op") {
            std::string op;
            if (!detail::ParseString(json, p, op)) return false;
            if (op == "show") out.op = RenderOp::Show;
            else if (op == "move") out.op = RenderOp::Move;
            else if (op == "hide") out.op = RenderOp::Hide;
            else if (op == "reload") out.op = RenderOp::ReloadConfig;
            else if (op == "ping") out.op = RenderOp::Ping;
            return true;
        }
        if (key == "sid") {
            return detail::ParseUint64(json, p, out.sessionId);
        }
        if (key == "pid") {
            return detail::ParseUint32(json, p, out.hostPid);
        }
        if (key == "pinyin") {
            return detail::ParseString(json, p, out.pinyin);
        }
        if (key == "cands") {
            detail::SkipWs(json, p);
            if (p >= json.size() || json[p] != '[') return false;
            ++p;
            out.candidates.clear();
            while (p < json.size()) {
                detail::SkipWs(json, p);
                if (p < json.size() && json[p] == ']') {
                    ++p;
                    return true;
                }
                std::string cand;
                if (!detail::ParseString(json, p, cand)) return false;
                out.candidates.push_back(std::move(cand));
                detail::SkipWs(json, p);
                if (p < json.size() && json[p] == ',') ++p;
            }
            return false;
        }
        if (key == "caret") {
            detail::SkipWs(json, p);
            if (p >= json.size() || json[p] != '[') return false;
            ++p;
            int32_t vals[4] = {0, 0, 0, 0};
            for (int i = 0; i < 4; ++i) {
                if (!detail::ParseInt32(json, p, vals[i])) return false;
                detail::SkipWs(json, p);
                if (i < 3 && p < json.size() && json[p] == ',') ++p;
            }
            detail::SkipWs(json, p);
            if (p >= json.size() || json[p] != ']') return false;
            ++p;
            out.caret.left = static_cast<LONG>(vals[0]);
            out.caret.top = static_cast<LONG>(vals[1]);
            out.caret.right = static_cast<LONG>(vals[2]);
            out.caret.bottom = static_cast<LONG>(vals[3]);
            return true;
        }
        if (key == "page") {
            int32_t v = 0;
            if (!detail::ParseInt32(json, p, v)) return false;
            out.page = static_cast<int>(v);
            return true;
        }
        if (key == "total") {
            int32_t v = 0;
            if (!detail::ParseInt32(json, p, v)) return false;
            out.totalPages = static_cast<int>(v);
            return true;
        }
        if (key == "sel") {
            int32_t v = 0;
            if (!detail::ParseInt32(json, p, v)) return false;
            out.selectedIndex = static_cast<int>(v);
            return true;
        }
        if (key == "inline") {
            return detail::ParseBoolOrInt(json, p, out.inlinePreedit);
        }
        if (key == "multi") {
            return detail::ParseBoolOrInt(json, p, out.multiRow);
        }
        return detail::SkipValue(json, p);
    });
    return ok && IsSupportedIpcVersion(out.version) && out.op != RenderOp::Unknown;
}

inline bool DecodePong(const std::string& json, PongInfo& out)
{
    out = PongInfo{};
    std::string op;
    size_t pos = 0;
    const bool parsed = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, out.version);
        }
        if (key == "op") {
            return detail::ParseString(json, p, op);
        }
        if (key == "ok") {
            return detail::ParseBoolOrInt(json, p, out.ok);
        }
        if (key == "dictReady") {
            return detail::ParseBoolOrInt(json, p, out.dictReady);
        }
        if (key == "entries") {
            return detail::ParseInt32(json, p, out.entries);
        }
        if (key == "build") {
            return detail::ParseString(json, p, out.build);
        }
        if (key == "uiAccess") {
            return detail::ParseBoolOrInt(json, p, out.uiAccess);
        }
        if (key == "band") {
            return detail::ParseUint32(json, p, out.windowBand);
        }
        if (key == "traditional") {
            return detail::ParseBoolOrInt(json, p, out.traditional);
        }
        if (key == "shuangpin") {
            return detail::ParseBoolOrInt(json, p, out.shuangpin);
        }
        if (key == "code") {
            return detail::ParseString(json, p, out.errorCode);
        }
        if (key == "expected") {
            return detail::ParseUint32(json, p, out.expectedVersion);
        }
        if (key == "got") {
            return detail::ParseUint32(json, p, out.gotVersion);
        }
        return detail::SkipValue(json, p);
    });
    return parsed && out.version > 0 && (op == "pong" || op == "error");
}

inline bool DecodeKeyInput(const std::string& json, KeyInputRequest& out)
{
    out = KeyInputRequest{};
    out.version = 0;
    std::string op;
    size_t pos = 0;
    const bool parsed = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, out.version);
        }
        if (key == "op") return detail::ParseString(json, p, op);
        if (key == "seq") return detail::ParseUint64(json, p, out.seq);
        if (key == "sid") return detail::ParseUint64(json, p, out.sessionId);
        if (key == "pid") return detail::ParseUint32(json, p, out.hostPid);
        if (key == "vk") return detail::ParseUint32(json, p, out.vk);
        if (key == "rawVk") return detail::ParseUint32(json, p, out.rawVk);
        if (key == "modifiers") return detail::ParseUint32(json, p, out.modifiers);
        if (key == "asciiMode") return detail::ParseBoolOrInt(json, p, out.asciiMode);
        if (key == "fresh") return detail::ParseBoolOrInt(json, p, out.fresh);
        if (key == "flags") return detail::ParseUint32(json, p, out.flags);
        return detail::SkipValue(json, p);
    });
    return parsed && IsSupportedIpcVersion(out.version) && op == "key_input";
}

inline bool DecodeCandidateList(const std::string& json, CandidateListResponse& out)
{
    out = CandidateListResponse{};
    out.version = 0;
    std::string op;
    size_t pos = 0;
    const bool parsed = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, out.version);
        }
        if (key == "op") return detail::ParseString(json, p, op);
        if (key == "seq") return detail::ParseUint64(json, p, out.seq);
        if (key == "sid") return detail::ParseUint64(json, p, out.sessionId);
        if (key == "eaten") return detail::ParseBoolOrInt(json, p, out.eaten);
        if (key == "pinyin") return detail::ParseString(json, p, out.pinyin);
        if (key == "preedit") return detail::ParseString(json, p, out.preedit);
        if (key == "syllable") return detail::ParseString(json, p, out.syllable);
        if (key == "candidates") {
            detail::SkipWs(json, p);
            if (p >= json.size() || json[p] != '[') return false;
            ++p;
            out.candidates.clear();
            while (p < json.size()) {
                detail::SkipWs(json, p);
                if (p < json.size() && json[p] == ']') {
                    ++p;
                    return true;
                }
                CandidateItem item;
                if (!detail::ParseCandidateItem(json, p, item)) return false;
                out.candidates.push_back(std::move(item));
                detail::SkipWs(json, p);
                if (p < json.size() && json[p] == ',') ++p;
            }
            return false;
        }
        if (key == "page") return detail::ParseInt32(json, p, out.page);
        if (key == "totalPages") return detail::ParseInt32(json, p, out.totalPages);
        if (key == "totalCandidates") return detail::ParseInt32(json, p, out.totalCandidates);
        if (key == "pageSize") return detail::ParseInt32(json, p, out.pageSize);
        if (key == "selectedIndex") return detail::ParseInt32(json, p, out.selectedIndex);
        if (key == "inCompose") return detail::ParseBoolOrInt(json, p, out.inCompose);
        if (key == "composePrefix") return detail::ParseString(json, p, out.composePrefix);
        if (key == "committed") return detail::ParseString(json, p, out.committed);
        if (key == "inputMode") return detail::ParseInt32(json, p, out.inputMode);
        if (key == "symbolPrefix") return detail::ParseBoolOrInt(json, p, out.symbolPrefix);
        if (key == "semicolonActive") return detail::ParseBoolOrInt(json, p, out.semicolonActive);
        return detail::SkipValue(json, p);
    });
    return parsed && IsSupportedIpcVersion(out.version) && op == "candidate_list";
}

inline bool DecodeCommit(const std::string& json, CommitRequest& out)
{
    out = CommitRequest{};
    out.version = 0;
    std::string op;
    size_t pos = 0;
    const bool parsed = detail::ParseJsonObject(json, pos, [&](const std::string& key, size_t& p) {
        if (key == "v") {
            return detail::ParseUint32(json, p, out.version);
        }
        if (key == "op") return detail::ParseString(json, p, op);
        if (key == "sid") return detail::ParseUint64(json, p, out.sessionId);
        if (key == "pid") return detail::ParseUint32(json, p, out.hostPid);
        if (key == "pinyin") return detail::ParseString(json, p, out.pinyin);
        if (key == "word") return detail::ParseString(json, p, out.word);
        if (key == "index") return detail::ParseInt32(json, p, out.index);
        return detail::SkipValue(json, p);
    });
    return parsed && IsSupportedIpcVersion(out.version) && op == "commit";
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

// ---------------------------------------------------------------------------
// 重叠 IO + 统一截止时间保护（供 FILE_FLAG_OVERLAPPED 打开的客户端句柄使用）
// ---------------------------------------------------------------------------

inline DWORD RemainingBudgetMs(ULONGLONG deadlineTick)
{
    const ULONGLONG now = GetTickCount64();
    if (now >= deadlineTick) {
        return 0;
    }
    const ULONGLONG diff = deadlineTick - now;
    return diff > 0xFFFFFFFFULL ? 0xFFFFFFFFu : static_cast<DWORD>(diff);
}

/// 等待重叠 IO 完成；超时则取消并收尾 GetOverlappedResult，防止栈上缓冲悬空
inline bool WaitOverlapped(HANDLE hPipe, OVERLAPPED& ov, DWORD timeoutMs, DWORD* pTransferred)
{
    const DWORD wait = WaitForSingleObject(ov.hEvent, timeoutMs);
    if (wait != WAIT_OBJECT_0) {
        if (CancelIoEx(hPipe, &ov) || GetLastError() == ERROR_NOT_FOUND) {
            DWORD dummy = 0;
            GetOverlappedResult(hPipe, &ov, &dummy, TRUE);
        }
        return false;
    }
    DWORD transferred = 0;
    if (!GetOverlappedResult(hPipe, &ov, &transferred, FALSE)) {
        return false;
    }
    if (pTransferred != nullptr) {
        *pTransferred = transferred;
    }
    return true;
}

/// 发送长度前缀帧（重叠 IO + 超时）
inline bool WritePipeFrameTimed(HANDLE hPipe, const std::string& payload, DWORD timeoutMs)
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

    HANDLE hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (hEvent == nullptr) {
        return false;
    }
    OVERLAPPED ov{};
    ov.hEvent = hEvent;

    DWORD transferred = 0;
    bool ok = false;
    const BOOL issued = WriteFile(hPipe, buf.data(), static_cast<DWORD>(buf.size()),
                                  nullptr, &ov);
    if (issued != FALSE) {
        transferred = static_cast<DWORD>(ov.InternalHigh);
        ok = true;
    } else if (GetLastError() == ERROR_IO_PENDING) {
        ok = WaitOverlapped(hPipe, ov, timeoutMs, &transferred);
    }
    CloseHandle(hEvent);
    return ok && transferred == buf.size();
}

/// 读取精确字节数（重叠 IO + 绝对截止时刻）
inline bool ReadPipeExactUntil(HANDLE hPipe, void* dst, DWORD bytes, ULONGLONG deadlineTick)
{
    BYTE* ptr = static_cast<BYTE*>(dst);
    DWORD remain = bytes;
    while (remain > 0) {
        const DWORD waitMs = RemainingBudgetMs(deadlineTick);
        HANDLE hEvent = CreateEventW(nullptr, TRUE, FALSE, nullptr);
        if (hEvent == nullptr) {
            return false;
        }
        OVERLAPPED ov{};
        ov.hEvent = hEvent;

        DWORD got = 0;
        bool ok = false;
        const BOOL issued = ReadFile(hPipe, ptr, remain, nullptr, &ov);
        if (issued != FALSE) {
            got = static_cast<DWORD>(ov.InternalHigh);
            ok = true;
        } else if (GetLastError() == ERROR_IO_PENDING) {
            ok = WaitOverlapped(hPipe, ov, waitMs, &got);
        }
        CloseHandle(hEvent);
        if (!ok || got == 0) {
            return false;
        }
        ptr += got;
        remain -= got;
    }
    return true;
}

/// 读取精确字节数（重叠 IO + 相对超时）
inline bool ReadPipeExactTimed(HANDLE hPipe, void* dst, DWORD bytes, DWORD timeoutMs)
{
    return ReadPipeExactUntil(hPipe, dst, bytes, GetTickCount64() + timeoutMs);
}

/// 读取长度前缀帧（重叠 IO + 统一总预算，杜绝头与体超时叠加）
inline bool ReadPipeFrameTimed(HANDLE hPipe, std::string& outPayload, DWORD timeoutMs)
{
    const ULONGLONG deadline = GetTickCount64() + timeoutMs;
    uint32_t len = 0;
    if (!ReadPipeExactUntil(hPipe, &len, 4, deadline)) {
        return false;
    }
    if (len == 0 || len > kMaxFrameBytes) {
        return false;
    }
    outPayload.resize(len);
    return ReadPipeExactUntil(hPipe, &outPayload[0], len, deadline);
}

} // namespace ipc
} // namespace taishen
