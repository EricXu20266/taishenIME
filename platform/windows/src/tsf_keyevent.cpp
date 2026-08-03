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
#include "app_state.h"

#include <utility> // std::move

namespace taishen {

/// P1-2 数字分隔符状态：最近提交以数字结尾 → , . 直通半角（tsf_module 提交后更新）
bool g_lastCommitEndsWithDigit = false;

/// P2-4 小键盘归一（对标 rime KP_0-9 等键绑定）：小键盘键 → 主键盘等价键。
int NormalizeKeypad(int vk)
{
    if (vk >= VK_NUMPAD0 && vk <= VK_NUMPAD9) {
        return vk - VK_NUMPAD0 + '0';
    }
    if (vk == VK_DECIMAL) {
        return VK_OEM_PERIOD;
    }
    if (vk == VK_ADD) {
        return VK_OEM_PLUS;
    }
    if (vk == VK_SUBTRACT) {
        return VK_OEM_MINUS;
    }
    if (vk == VK_MULTIPLY) {
        return '8'; // 近似主键盘 *（Shift+8）
    }
    if (vk == VK_DIVIDE) {
        return VK_OEM_2;
    }
    if (vk == VK_SEPARATOR) {
        return VK_OEM_COMMA;
    }
    return vk;
}

/// 中文模式标点复选候选表（0.2.28，对标 rime full_shape 多映射）。
/// 返回非空 = 该键在当前 Shift 状态下应弹出复选候选（如 《〈«‹）。
static std::vector<std::wstring> MapPunctCandidates(int vk, bool shift) {
    switch (vk) {
    case VK_OEM_COMMA:  if (shift) return {L"《", L"〈", L"«", L"‹"}; break;
    case VK_OEM_PERIOD: if (shift) return {L"》", L"〉", L"»", L"›"}; break;
    case VK_OEM_4:      if (shift) return {L"「", L"『", L"〖", L"｛"}; break;
    case VK_OEM_6:      if (shift) return {L"」", L"』", L"〗", L"｝"}; break;
    default: break;
    }
    return {};
}

/// 中文模式标点映射表（0.2.27 对齐 rime-ice half_shape）。
/// 返回非空 = 该键在当前 Shift 状态下应上屏（吞键）。
/// 对齐 half_shape：`@#%&*+-=/|~` 保留半角，`_`→破折号，`\`→顿号，`$`→¥。
/// 数字键仅在 Shift 时映射，无 Shift 数字透传（候选选择不受影响）。
static const wchar_t* MapFullWidthPunct(int vk, bool shift) {
    switch (vk) {
    case VK_OEM_COMMA:  return shift ? nullptr : L"，";  // Shift 由复选接管
    case VK_OEM_PERIOD: return shift ? nullptr : L"。";  // Shift 由复选接管
    case VK_OEM_2:      return shift ? L"？" : L"/";    // '/' '?'（半角 /）
    case VK_OEM_1:      return shift ? L"：" : L"；";   // ';' ':'
    case VK_OEM_4:      return shift ? nullptr : L"【";  // Shift 由复选接管
    case VK_OEM_6:      return shift ? nullptr : L"】";  // Shift 由复选接管
    case VK_OEM_5:      return shift ? L"|" : L"、";    // '\' '|'（| 半角）
    case VK_OEM_3:      return shift ? L"~" : L"·";    // '`' '~'（· U+00B7）
    case VK_OEM_MINUS:  return shift ? L"——" : L"-";   // '-' '_'（- 半角，_ 破折号）
    case VK_OEM_PLUS:   return shift ? L"+" : L"=";     // '=' '+'（半角）
    case '1': return shift ? L"！" : nullptr;
    case '2': return shift ? L"@" : nullptr;   // @ 半角
    case '3': return shift ? L"#" : nullptr;   // # 半角
    case '4': return shift ? L"¥" : nullptr;   // ¥ U+00A5（half_shape '$':'¥'）
    case '5': return shift ? L"%" : nullptr;   // % 半角
    case '6': return shift ? L"……" : nullptr;  // '^' → 省略号
    case '7': return shift ? L"&" : nullptr;   // & 半角
    case '8': return shift ? L"*" : nullptr;   // * 半角
    case '9': return shift ? L"（" : nullptr;
    case '0': return shift ? L"）" : nullptr;
    default:   return nullptr;
    }
}

/// 无副作用的按键预测试——只判断键位是否由输入法处理。
/// 注意：绝不调用引擎的修改性 FFI（process_key/backspace/select_candidate）。
/// TSF 中 OnTestKeyDown 会先于 OnKeyDown 调用，有副作用的处理只允许在 OnKeyDown。
bool ShouldEatKey(int vk) {
    // Shift 键放行（0.2.26 fix）：TSF 传递的 Shift 虚拟键是 VK_SHIFT(16)，
    // 必须显式放行让 OnTestKeyDown 返回 TRUE，否则 OnKeyDown/OnKeyUp 不达，
    // Shift tap 切换中英完全失效。放行后 OnKeyDown 返回 FALSE → 键仍透传应用。
    if (vk == VK_SHIFT || vk == VK_LSHIFT || vk == VK_RSHIFT) {
        return true;
    }
    // Ctrl+Space 中英切换
    if (vk == VK_SPACE && (GetKeyState(VK_CONTROL) & 0x8000)) {
        return true;
    }
    // P0-2 运行时开关快捷键（对标 rime Control+Shift+3/4）：Ctrl+Shift+数字
    if (vk >= '1' && vk <= '9' && (GetKeyState(VK_CONTROL) & 0x8000) &&
        (GetKeyState(VK_SHIFT) & 0x8000)) {
        return true;
    }
    // P1-4 特殊模式吞键：计算器/数字大写/Unicode 模式激活时，
    // 数字/小数点/运算符进引擎累积（否则透传给应用，模式永远无法输入）
    {
        const int mode = engine_input_mode();
        if (mode == 1 || mode == 2 || mode == 3) {
            if (vk >= '0' && vk <= '9') {
                return true;
            }
            // 数字大写 R 模式：小数点
            if (mode == 2 && vk == VK_OEM_PERIOD &&
                !(GetKeyState(VK_SHIFT) & 0x8000)) {
                return true;
            }
            // 计算器 c 模式：运算符 + - * / ( ) % ^ . 与小键盘
            if (mode == 1) {
                if (vk == VK_OEM_PLUS || vk == VK_OEM_MINUS ||
                    vk == VK_OEM_2 || vk == VK_OEM_PERIOD ||
                    vk == VK_OEM_7 || vk == VK_OEM_8 || vk == VK_OEM_5 ||
                    vk == '6' && (GetKeyState(VK_SHIFT) & 0x8000) ||
                    vk == '8' && (GetKeyState(VK_SHIFT) & 0x8000) ||
                    vk == VK_NUMPAD0 || vk == VK_NUMPAD1 || vk == VK_NUMPAD2 ||
                    vk == VK_NUMPAD3 || vk == VK_NUMPAD4 || vk == VK_NUMPAD5 ||
                    vk == VK_NUMPAD6 || vk == VK_NUMPAD7 || vk == VK_NUMPAD8 ||
                    vk == VK_NUMPAD9 || vk == VK_DECIMAL || vk == VK_ADD ||
                    vk == VK_SUBTRACT || vk == VK_MULTIPLY || vk == VK_DIVIDE) {
                    return true;
                }
            }
        }
    }
    // 中文模式：无候选时标点键全角化（0.3.x 修复：之前透传 → 英文标点）
    // 需与 HandleKeyDown 的标点处理保持同步（OnTestKeyDown 决定是否放行到 OnKeyDown）
    // P0-2：ascii_punct=1 时标点透传英文（中英标点独立开关）
    if (engine_get_ascii_mode() == 0 && engine_get_ascii_punct() == 0 &&
        engine_get_candidate_count() == 0) {
        const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        // P1-2 数字分隔符（对标 rime digit_separators）：最近提交以数字结尾 →
        // , . 直通半角（不吞键），如日期候选 2026-08-03 后按 . 出半角
        if ((vk == VK_OEM_COMMA || vk == VK_OEM_PERIOD) && !shift &&
            g_lastCommitEndsWithDigit) {
            return false;
        }
        if (MapFullWidthPunct(vk, shift) != nullptr ||
            !MapPunctCandidates(vk, shift).empty() ||
            vk == VK_OEM_7) {  // VK_OEM_7: 配对引号（0.2.28）
            return true;
        }
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
    // 注意：engine_get_pinyin_str 返回 len+1（空串=1），所以"有拼音"是 >1
    if (vk == VK_BACK) {
        return engine_get_pinyin_str(nullptr, 0) > 1;
    }
    // P2-1 编辑键：Ctrl+BackSpace 删音节 / Tab 移光标 / Ctrl+Delete 删候选
    if (vk == VK_BACK && (GetKeyState(VK_CONTROL) & 0x8000) &&
        engine_get_pinyin_str(nullptr, 0) > 1) {
        return true;
    }
    if (vk == VK_TAB && engine_get_pinyin_str(nullptr, 0) > 1) {
        return true;
    }
    if (vk == VK_DELETE && (GetKeyState(VK_CONTROL) & 0x8000) &&
        engine_get_candidate_count() > 0) {
        return true;
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
    // 多行展开/收起（0.2.14）：↓ 展开 / ↑ 收起，候选存在时吞键
    if (vk == VK_DOWN || vk == VK_UP) {
        return engine_get_candidate_count() > 0;
    }
    // Esc：多行展开时收起（吞键），否则透传（应用取消）
    if (vk == VK_ESCAPE) {
        return false;  // 展开状态由候选窗口管理，Esc 由引擎 reset 处理
    }
    // 附加翻页键（0.1.13，竞品标配）：+/= 下一页，-/逗号 上一页（候选存在时）
    if (vk == VK_OEM_PLUS || vk == VK_OEM_COMMA) {
        return engine_get_candidate_count() > 0;
    }
    if (vk == VK_OEM_MINUS || vk == VK_OEM_PERIOD) {
        return engine_get_candidate_count() > 0;
    }
    // 以词定字（0.2.24）：[ 取首字 / ] 取末字，候选存在时吞键
    if (vk == VK_OEM_4 || vk == VK_OEM_6) {
        return engine_get_candidate_count() > 0;
    }
    // 其他键：透传
    return false;
}

bool HandleKeyDown(int vk, LPARAM /*lparam*/, KeyEventResult& out) {
    // Ctrl+Space：切换中英文模式（V0.2.33 走 per-app 记忆，更新当前进程状态）
    if (vk == VK_SPACE && (GetKeyState(VK_CONTROL) & 0x8000)) {
        const int cur = engine_get_ascii_mode();
        taishen::AppStateSetAscii(cur ? false : true);
        out.eaten = true;
        out.state_changed = true; // 触发候选窗口刷新（模式变化）
        return true;
    }

    // P1-4 特殊输入模式（计算器 c / 数字大写 R / Unicode U）：
    // 数字与运算符进引擎累积（平台层透传会截断模式输入，计算器此前从未接通）
    {
        const int mode = engine_input_mode();
        if (mode == 1 || mode == 2 || mode == 3) {
            const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
            // 数字 0-9（calc 模式 Shift 出 * ( )）
            if (vk >= '0' && vk <= '9') {
                char ch = static_cast<char>(vk);
                if (shift && vk == '8') { ch = '*'; }
                else if (shift && vk == '9') { ch = '('; }
                else if (shift && vk == '0') { ch = ')'; }
                else if (shift) { return false; } // !@# 等不进引擎
                const int count = engine_process_key(ch);
                out.eaten = true;
                out.state_changed = true;
                out.candidate_count = count;
                return true;
            }
            // 运算符与小数点（calc/number 模式）
            if (mode == 1 || mode == 2) {
                char ch = 0;
                if (vk == VK_OEM_PERIOD && !shift) { ch = '.'; }
                else if (vk == VK_OEM_PLUS && !shift) { ch = '+'; }
                else if (vk == VK_OEM_MINUS && !shift) { ch = '-'; }
                else if (vk == VK_OEM_2 && !shift) { ch = '/'; }
                else if (vk == VK_OEM_5 && !shift) { ch = '%'; }
                else if (vk == '6' && shift) { ch = '^'; }
                if (ch != 0) {
                    const int count = engine_process_key(ch);
                    out.eaten = true;
                    out.state_changed = true;
                    out.candidate_count = count;
                    return true;
                }
            }
        }
    }

    // 中文模式：无候选时标点键全角化（0.3.x 修复：之前透传 → 英文标点）
    // 放置于引擎逻辑之前——有候选时（翻页/选词/以词定字）不进入此分支
    // P0-2：ascii_punct=1 时标点透传英文（中英标点独立开关）
    if (engine_get_ascii_mode() == 0 && engine_get_ascii_punct() == 0 &&
        engine_get_candidate_count() == 0) {
        const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        // P1-2 数字分隔符：最近提交以数字结尾 → , . 直通半角（不吞键）
        if ((vk == VK_OEM_COMMA || vk == VK_OEM_PERIOD) && !shift &&
            g_lastCommitEndsWithDigit) {
            return false;
        }
        // 有未提交拼音 → 先丢弃（与 Enter 行为一致），避免拼音残留 + 英文标点混排
        const bool hasPinyin = engine_get_pinyin_str(nullptr, 0) > 1;
        // 复选标点（0.2.28）：< > [ ] 的 Shift 变体 → 候选列表（如 《〈«‹）
        auto cands = MapPunctCandidates(vk, shift);
        if (!cands.empty()) {
            if (hasPinyin) { engine_reset(); }
            out.punct_candidates = std::move(cands);
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
        // 配对引号（0.2.28）：' 单引号（‘’）/" 双引号（“”），开闭交替
        if (vk == VK_OEM_7) {
            if (hasPinyin) { engine_reset(); }
            out.punct_quote = shift ? 2 : 1;
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
        // 单值标点
        const wchar_t* punct = MapFullWidthPunct(vk, shift);
        if (punct != nullptr) {
            if (hasPinyin) { engine_reset(); }
            out.committed = punct;
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
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

    // P2-1 Ctrl+BackSpace：删除一个音节（对标 rime back_syllable）
    if (vk == VK_BACK && (GetKeyState(VK_CONTROL) & 0x8000)) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            const int count = engine_backspace_syllable();
            out.eaten = true;
            out.state_changed = true;
            out.candidate_count = count;
            return true;
        }
        return false;
    }

    // P2-1 Tab/Shift+Tab：移动光标到相邻音节边界（对标 rime Tab/Shift+Tab）
    if (vk == VK_TAB) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            const bool shiftDown = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
            engine_move_cursor(shiftDown ? -1 : 1);
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
        return false;
    }

    // P2-1 Ctrl+Delete：删除当前页首个候选（从用户词库移除，对标 rime delete_candidate）
    if (vk == VK_DELETE && (GetKeyState(VK_CONTROL) & 0x8000)) {
        if (engine_get_candidate_count() > 0) {
            const int count = engine_delete_candidate(0);
            out.eaten = true;
            out.state_changed = true;
            out.candidate_count = count;
            return true;
        }
        return false;
    }

    // 退格：删除拼音串最后一个字符（无拼音时透传，让应用正常删除文字）
    if (vk == VK_BACK) {
        // 无拼音时不吞键——交给应用处理删除
        // 注意：engine_get_pinyin_str 返回 len+1（空串=1），无拼音是 <=1
        if (engine_get_pinyin_str(nullptr, 0) <= 1) {
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

    // 多行展开/收起（0.2.14）：↓ 展开 / ↑ 收起（候选存在时）
    if (vk == VK_DOWN || vk == VK_UP) {
        if (engine_get_candidate_count() > 0) {
            out.eaten = true;
            out.state_changed = true;
            // ↓ 展开多行，↑ 收起（请求由平台层应用）
            out.multirow_requested = (vk == VK_DOWN);
            return true;
        }
        return false;
    }

    // 以词定字（0.2.24）：[ 取当前候选首字，] 取末字
    if (vk == VK_OEM_4 || vk == VK_OEM_6) {
        if (engine_get_candidate_count() > 0) {
            const bool first = (vk == VK_OEM_4); // [ = 首字
            char buf[64] = {0};
            const int len = engine_take_char(first ? 1 : 0, buf, sizeof(buf));
            if (len > 0) {
                out.committed = Utf8ToWide(buf);
                out.eaten = true;
                out.state_changed = true;
                return true;
            }
        }
        // 无候选时透传方括号给应用
        return false;
    }

    // 回车（0.1.26）：清空未提交拼音（不上屏），然后透传给应用
    // 修复：之前拼音挂着时按 Enter，候选窗口残留且应用收到的 Enter
    // 与残留输入状态混乱（泰深聊天框表现为"回车只换行不发送"）。
    // 微软拼音同款行为：Enter 丢弃未提交拼音，应用正常收到 Enter。
    if (vk == VK_RETURN) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            engine_reset();
            out.state_changed = true;  // 关闭候选窗口
        }
        return false;  // 透传：发送/换行由应用决定
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
