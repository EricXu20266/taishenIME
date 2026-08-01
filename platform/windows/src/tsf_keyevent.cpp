/// TSF 按键处理逻辑实现 — 键码 → 引擎 FFI 映射
///
/// 规则表（对应 SPEC §四）：
///   VK_A..VK_Z → engine_process_key（吞键）
///   VK_BACK    → engine_backspace（吞键）
///   VK_SPACE   → 候选数>0 时选第 0 个（吞键）
///   VK_1..VK_9 → 候选数>索引 时选该候选（吞键）
///   其他       → 透传

#include "tsf_keyevent.h"
#include "engine_bridge.h"

namespace taishen {

bool HandleKeyDown(int vk, LPARAM /*lparam*/, KeyEventResult& out) {
    // 英文字母：A-Z（0x41-0x5A）→ 引擎累积拼音
    if (vk >= 'A' && vk <= 'Z') {
        const char ch = static_cast<char>(vk + ('a' - 'A')); // 转小写
        const int count = engine_process_key(static_cast<int>(ch));
        out.eaten = true;
        out.state_changed = true;
        out.candidate_count = count;
        return true;
    }

    // 退格：删除拼音串最后一个字符
    if (vk == VK_BACK) {
        const int count = engine_backspace();
        out.eaten = true;
        out.state_changed = true;
        out.candidate_count = count;
        return true;
    }

    // 空格：选第 0 个候选（默认候选）
    if (vk == VK_SPACE) {
        const int count = engine_get_candidate_count();
        if (count > 0) {
            char buf[512] = {0};
            const int len = engine_select_candidate(0, buf, sizeof(buf));
            if (len > 0) {
                out.committed = Utf8ToWide(buf);
                out.eaten = true;
                out.state_changed = true;
                return true;
            }
        }
        // 无候选时透传空格给应用
        return false;
    }

    // 数字键 1-9：选择对应候选（索引 0-8）
    if (vk >= '1' && vk <= '9') {
        const int index = vk - '1';
        const int count = engine_get_candidate_count();
        if (count > index) {
            char buf[512] = {0};
            const int len = engine_select_candidate(index, buf, sizeof(buf));
            if (len > 0) {
                out.committed = Utf8ToWide(buf);
                out.eaten = true;
                out.state_changed = true;
                return true;
            }
        }
        return false;
    }

    // 其他键：透传
    return false;
}

std::wstring Utf8ToWide(const std::string& utf8) {
    if (utf8.empty()) {
        return std::wstring();
    }
    const int len = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                        static_cast<int>(utf8.size()), nullptr, 0);
    if (len <= 0) {
        return std::wstring();
    }
    std::wstring result(static_cast<size_t>(len), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(), static_cast<int>(utf8.size()),
                        &result[0], len);
    return result;
}

} // namespace taishen
