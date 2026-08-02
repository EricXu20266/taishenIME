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

/// 无副作用的按键预测试——只判断键位是否由输入法处理。
/// 注意：绝不调用引擎的修改性 FFI（process_key/backspace/select_candidate）。
/// TSF 中 OnTestKeyDown 会先于 OnKeyDown 调用，有副作用的处理只允许在 OnKeyDown。
bool ShouldEatKey(int vk) {
    // Ctrl+Space 中英切换
    if (vk == VK_SPACE && (GetKeyState(VK_CONTROL) & 0x8000)) {
        return true;
    }
    // 英文模式（ascii_mode=1）：字母透传给应用，不吞键（0.1.15）
    // 修复：之前 ascii 模式字母被吞后走 committed 提交，但无组合时
    // CommitComposition 不写任何文本 → 字母丢失（"无法输入"）。
    if (engine_get_ascii_mode() == 1) {
        if (vk >= 'A' && vk <= 'Z') {
            return false;
        }
    }
    // 字母键
    if (vk >= 'A' && vk <= 'Z') {
        return true;
    }
    // 退格：仅在引擎有拼音时吞（无拼音时退格交给应用——否则应用无法删除文字）
    if (vk == VK_BACK) {
        return engine_get_pinyin_str(nullptr, 0) > 0;
    }
    // 空格：候选数 > 0 时选默认候选（吞）
    if (vk == VK_SPACE) {
        return engine_get_candidate_count() > 0;
    }
    // 数字键 1-9
    if (vk >= '1' && vk <= '9') {
        return engine_get_candidate_count() > (vk - '1');
    }
    // 翻页键（0.1.13 新增）：PgUp/PgDn + 候选存在
    if (vk == VK_PRIOR || vk == VK_NEXT) {
        return engine_get_candidate_count() > 0;
    }
    // 附加翻页键（0.1.13，竞品标配）：+/= 下一页，-/逗号 上一页（候选存在时）
    if (vk == VK_OEM_PLUS || vk == VK_OEM_COMMA) {
        return engine_get_candidate_count() > 0;
    }
    if (vk == VK_OEM_MINUS || vk == VK_OEM_PERIOD) {
        return engine_get_candidate_count() > 0;
    }
    // 其他键：透传
    return false;
}

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
        // 英文模式：字母透传给应用（0.1.15）
        // 修复：之前走 committed 提交，但无组合时 CommitComposition 不写文本
        // → 字母被吞但不上屏。透传是输入法英文模式的业界标准做法。
        if (engine_get_ascii_mode() == 1) {
            return false;
        }
        // 中文模式：累积拼音
        const char ch = static_cast<char>(vk + ('a' - 'A')); // 转小写
        const int count = engine_process_key(static_cast<int>(ch));
        out.eaten = true;
        out.state_changed = true;
        out.candidate_count = count;
        return true;
    }

    // 退格：删除拼音串最后一个字符（无拼音时透传，让应用正常删除文字）
    if (vk == VK_BACK) {
        // 无拼音时不吞键——交给应用处理删除
        if (engine_get_pinyin_str(nullptr, 0) <= 0) {
            return false;
        }
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

    // 翻页键：PgUp/PgDn、+/= 下一页，-/,/./逗号 上一页（0.1.13）
    // 竞品约定：PgDn/+/./'=' 下一页，PgUp/-/,/',' 上一页
    if (vk == VK_PRIOR || vk == VK_NEXT ||
        vk == VK_OEM_PLUS || vk == VK_OEM_MINUS ||
        vk == VK_OEM_COMMA || vk == VK_OEM_PERIOD) {
        bool forward = (vk == VK_NEXT || vk == VK_OEM_PLUS || vk == VK_OEM_PERIOD);
        const int count = engine_page(forward ? 1 : -1);
        if (count > 0) {
            out.eaten = true;
            out.state_changed = true;
            out.candidate_count = count;
            return true;
        }
        // 无更多页时不吞键（如逗号/句号在无候选时应作为标点透传）
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
