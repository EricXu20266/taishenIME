#pragma once

#include <algorithm>
#include <string>
#include <utility>
#include <vector>

#include "engine_bridge.h"

namespace taishen {

/// 读取 Rust FFI 返回的 UTF-8 文本。
///
/// FFI 返回值是包含 NUL 的所需容量；调用方不能把它直接当作已写入长度，
/// 否则长文本会造成栈缓冲区越界读取。首次查询容量后再精确读取，并允许
/// 引擎在两次调用之间产生更长结果时有限重试。
template <typename ReadFn>
bool ReadEngineUtf8(ReadFn&& read, std::string& out)
{
    constexpr int kMaxTextBytes = 1024 * 1024;
    out.clear();

    int required = read(nullptr, 0);
    for (int attempt = 0; attempt < 3; ++attempt) {
        if (required <= 0 || required > kMaxTextBytes) {
            return false;
        }

        std::vector<char> buffer(static_cast<size_t>(required), '\0');
        const int reported = read(buffer.data(), required);
        if (reported <= 0) {
            return false;
        }
        if (reported > required) {
            required = reported;
            continue;
        }

        const auto nul = std::find(buffer.begin(), buffer.end(), '\0');
        if (nul == buffer.end()) {
            return false;
        }
        out.assign(buffer.data(), static_cast<size_t>(nul - buffer.begin()));
        return true;
    }

    return false;
}

inline bool ReadEngineCandidate(int index, std::string& out)
{
    return ReadEngineUtf8(
        [index](char* buffer, int capacity) {
            return engine_get_candidate(index, buffer, capacity);
        },
        out);
}

/// 按全局索引读取候选（跨页）。宿主通过 TSF 候选 UIElement 取词时用这个，
/// 索引范围是全量候选，不是当前页。
inline bool ReadEngineCandidateAt(int index, std::string& out)
{
    return ReadEngineUtf8(
        [index](char* buffer, int capacity) {
            return engine_get_candidate_at(index, buffer, capacity);
        },
        out);
}

/// 读取「有副作用的 FFI」返回值。
///
/// 这类接口（选候选 / 组词回退 / 以词定字）每调用一次就推进引擎状态，
/// **不能**用 ReadEngineUtf8——它先调一次查容量、再调一次读正文，动作会执行两次。
/// 实测症状：组词时按一次数字键，引擎前进两个音节直接退出组词，
/// 用户表现为「选了首字就再也选不动后面的字」。
/// 这里给固定缓冲、只调一次；512 字节上限对词条足够（超长会被截断而非崩）。
template <typename ActionFn>
bool ReadEngineActionUtf8(ActionFn&& action, std::string& out)
{
    out.clear();
    char buffer[512] = {0};
    const int required = action(buffer, static_cast<int>(sizeof(buffer) - 1));
    if (required <= 1) {
        return false;
    }
    buffer[sizeof(buffer) - 1] = '\0';
    out.assign(buffer);
    return true;
}

/// 组词撤销：回退上一个已选词（可能多字）。
inline bool ReadEngineComposeUndo(std::string& out)
{
    return ReadEngineActionUtf8(
        [](char* buffer, int capacity) {
            return engine_compose_undo(buffer, capacity);
        },
        out);
}

/// 组词已选前缀：已选词汉字串（引擎是唯一状态源）。
inline bool ReadEngineComposePrefix(std::string& out)
{
    return ReadEngineUtf8(
        [](char* buffer, int capacity) {
            return engine_compose_prefix(buffer, capacity);
        },
        out);
}

inline bool ReadEngineSelectedCandidate(int index, std::string& out)
{
    return ReadEngineActionUtf8(
        [index](char* buffer, int capacity) {
            return engine_select_candidate(index, buffer, capacity);
        },
        out);
}

inline bool ReadEnginePinyin(std::string& out)
{
    return ReadEngineUtf8(
        [](char* buffer, int capacity) {
            return engine_get_pinyin_str(buffer, capacity);
        },
        out);
}

inline bool ReadEngineSyllableDisplay(std::string& out)
{
    return ReadEngineUtf8(
        [](char* buffer, int capacity) {
            return engine_syllable_display(buffer, capacity);
        },
        out);
}

inline bool ReadEngineTakenChar(bool first, std::string& out)
{
    return ReadEngineActionUtf8(
        [first](char* buffer, int capacity) {
            return engine_take_char(first ? 1 : 0, buffer, capacity);
        },
        out);
}

} // namespace taishen
