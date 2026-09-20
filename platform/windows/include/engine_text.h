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

inline bool ReadEngineSelectedCandidate(int index, std::string& out)
{
    return ReadEngineUtf8(
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
    return ReadEngineUtf8(
        [first](char* buffer, int capacity) {
            return engine_take_char(first ? 1 : 0, buffer, capacity);
        },
        out);
}

} // namespace taishen
