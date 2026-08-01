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
    // Ctrl+Space：切换中英文模式
    if (vk == VK_SPACE && (GetKeyState(VK_CONTROL) & 0x8000)) {
        const int cur = engine_get_ascii_mode();
        engine_set_ascii_mode(cur ? 0 : 1);
        out.eaten = true;
        out.state_changed = true; // 触发候选窗口刷新（模式变化）
        return true;
    }

    // 英文字母：A-Z（0x41-0x5A）
    if (vk >= 'A' && vk <= 'Z') {
        // 英文模式：字母直接上屏（走 committed 通道，复用 TSF 组合提交）
        if (engine_get_ascii_mode() == 1) {
            const wchar_t ch = static_cast<wchar_t>(vk + ('a' - 'A'));
            out.eaten = true;
            out.committed = std::wstring(1, ch);
            out.state_changed = false;
            return true;
        }
        // 中文模式：累积拼音
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

std::string WideToUtf8(const std::wstring& wide) {
    if (wide.empty()) {
        return std::string();
    }
    const int len = WideCharToMultiByte(CP_UTF8, 0, wide.c_str(),
                                        static_cast<int>(wide.size()),
                                        nullptr, 0, nullptr, nullptr);
    if (len <= 0) {
        return std::string();
    }
    std::string result(static_cast<size_t>(len), '\0');
    WideCharToMultiByte(CP_UTF8, 0, wide.c_str(),
                        static_cast<int>(wide.size()), &result[0], len,
                        nullptr, nullptr);
    return result;
}

} // namespace taishen
