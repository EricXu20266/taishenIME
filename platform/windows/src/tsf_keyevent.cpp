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

/// V0.4.x 配对符号表：开符号 → 闭符号（成对上屏 + 光标居中）。
/// 覆盖中文语境常用的全角配对：括号/书名号/方括号/花括号/引号。
/// 半角符号不在此列——成对仅作用于全角（中文模式标点全角化后自然命中）。
static const wchar_t kPairOpenTable[] = {
    L'（', L'《', L'【', L'｛', L'「', L'『', L'〖', L'〈', L'“', L'‘',
};
static const wchar_t kPairCloseTable[] = {
    L'）', L'》', L'】', L'｝', L'」', L'』', L'〗', L'〉', L'”', L'’',
};
static const size_t kPairCount = sizeof(kPairOpenTable) / sizeof(kPairOpenTable[0]);

/// 查询开符号对应的闭符号。非开符号返回 0。
static wchar_t PairCloseFor(wchar_t open)
{
    for (size_t i = 0; i < kPairCount; ++i) {
        if (kPairOpenTable[i] == open) {
            return kPairCloseTable[i];
        }
    }
    return 0;
}

/// 当前开关状态（V0.4.x）：由 tsf_module 从 config.ini 同步。
/// 0=关（单符号上屏） / 1=开（成对+光标居中），默认开。
static int g_pairPunctEnabled = 1;
void SetPairPunctEnabled(bool enabled) { g_pairPunctEnabled = enabled ? 1 : 0; }
bool IsPairPunctEnabled() { return g_pairPunctEnabled != 0; }

/// V0.4.x 配对符号扩展（复选标点/引号共用）：
/// sel 为开符号且开关开启 → committed = 开+闭，caretOffset = 开符号宽；
/// 否则 committed = sel，caretOffset = -1。
void ExpandPairPunct(const std::wstring& sel, std::wstring& committed,
                     int& caretOffset)
{
    committed = sel;
    caretOffset = -1;
    if (!IsPairPunctEnabled() || sel.empty()) {
        return;
    }
    const wchar_t close = PairCloseFor(sel[0]);
    if (close != 0) {
        committed += close;
        caretOffset = 1; // 开符号后（sel[0] 占 1 个 wchar）
    }
}

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

/// 中文模式标点映射表（0.2.27 对齐 rime-ice half_shape；V0.4.x 移除复选，
/// Shift+< > { } 直接上屏书名号/大括号，与引号/括号同一逻辑）。
/// 返回非空 = 该键在当前 Shift 状态下应上屏（吞键）。
/// 对齐 half_shape：`@#%&*+-=/|~` 保留半角，`_`→破折号，`\`→顿号，`$`→¥。
/// 数字键仅在 Shift 时映射，无 Shift 数字透传（候选选择不受影响）。
static const wchar_t* MapFullWidthPunct(int vk, bool shift) {
    switch (vk) {
    case VK_OEM_COMMA:  return shift ? L"《" : L"，";  // , <（< → 书名号开）
    case VK_OEM_PERIOD: return shift ? L"》" : L"。";  // . >（> → 书名号闭）
    case VK_OEM_2:      return shift ? L"？" : L"/";    // '/' '?'（半角 /）
    case VK_OEM_1:      return shift ? L"：" : L"；";   // ';' ':'
    case VK_OEM_4:      return shift ? L"｛" : L"【";   // [ {（{ → 大括号开）
    case VK_OEM_6:      return shift ? L"｝" : L"】";   // ] }（} → 大括号闭）
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

/// V0.2.36 vim_mode 键判定：Esc / Ctrl+C / Ctrl+[（vim 中等价 Esc）。
/// 只读判断（读 GetKeyState），无副作用。
bool IsVimModeKey(int vk)
{
    if (vk == VK_ESCAPE) {
        return true;
    }
    const bool ctrl = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
    // VK_OEM_4 = '['（美式键盘）
    return ctrl && (vk == 'C' || vk == VK_OEM_4);
}

/// 无副作用的按键预测试——只判断键位是否由输入法处理。
/// 注意：绝不调用引擎的修改性 FFI（process_key/backspace/select_candidate）。
/// TSF 中 OnTestKeyDown 会先于 OnKeyDown 调用，有副作用的处理只允许在 OnKeyDown。
bool ShouldEatKey(int vk, bool vimPassthrough) {
    // V0.2.36 vim_mode：vim 键强制透传（vim 需要收到 Esc/Ctrl+C/Ctrl+[ 退出插入模式）。
    // 透传后 OnKeyDown 不执行，切英文动作由 OnKeyUp 完成。
    if (vimPassthrough && IsVimModeKey(vk)) {
        return false;
    }
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
    // 中文模式：标点键全角化（V0.3.x：无论有无候选都处理——
    // 有候选时 HandleKeyDown 先上屏默认候选再打标点，微软拼音行为）
    // 需与 HandleKeyDown 的标点处理保持同步（OnTestKeyDown 决定是否放行到 OnKeyDown）
    // P0-2：ascii_punct=1 时标点透传英文（中英标点独立开关）
    if (engine_get_ascii_mode() == 0 && engine_get_ascii_punct() == 0) {
        const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        // P1-2 数字分隔符（对标 rime digit_separators）：最近提交以数字结尾 →
        // , . 直通半角（不吞键），如日期候选 2026-08-03 后按 . 出半角
        if ((vk == VK_OEM_COMMA || vk == VK_OEM_PERIOD) && !shift &&
            g_lastCommitEndsWithDigit) {
            return false;
        }
        if (MapFullWidthPunct(vk, shift) != nullptr ||
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
    // 字母键：Ctrl/Alt 组合键放行（Ctrl+C/V/R 等系统快捷键透传应用，问题 2）
    // Shift+字母：中文模式吞键（大写上屏由 HandleKeyDown 处理，问题 10）
    if (vk >= 'A' && vk <= 'Z') {
        const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
        if (ctrlDown || altDown) {
            return false; // 系统快捷键透传（Ctrl+C 复制 / Ctrl+V 粘贴 / Ctrl+R 刷新）
        }
        return true;
    }
    // 退格：仅在引擎有拼音时吞（无拼音时退格交给应用——否则应用无法删除文字）
    // 注意：engine_get_pinyin_str 返回 len+1（空串=1），所以"有拼音"是 >1
    if (vk == VK_BACK) {
        return engine_get_pinyin_str(nullptr, 0) > 1;
    }
    // 回车（V0.4.3 double 修复）：有拼音串时吞键——拼音串由 HandleKeyDown
    // 提交上屏。此前 OnTestKeyDown 对回车总透传（ShouldEatKey 无此分支），
    // Scintilla（Notepad++）等应用收到 Enter 时提交自身 IME 组合状态 →
    // TSF 提交一次 + 应用再提交一次 = womwom 二次上屏。无拼音时透传回车。
    if (vk == VK_RETURN) {
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
    // 附加翻页键（0.1.13，竞品标配）：+/= 下一页，- 上一页（候选存在时）
    // V0.3.x：移除逗号/句号翻页——让位给中文标点（问题 2：中文下打不出标点）
    if (vk == VK_OEM_PLUS || vk == VK_OEM_MINUS) {
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

    // 中文模式：标点键全角化（V0.3.x：无论有无候选都处理。
    // 有候选时先上屏默认候选再打标点——微软拼音行为，如 zhong + ，→ "中，"）
    // 放置于引擎逻辑之前——有候选时（翻页/选词/以词定字）不进入此分支
    // P0-2：ascii_punct=1 时标点透传英文（中英标点独立开关）
    if (engine_get_ascii_mode() == 0 && engine_get_ascii_punct() == 0) {
        const bool shift = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        // P1-2 数字分隔符：最近提交以数字结尾 → , . 直通半角（不吞键）
        if ((vk == VK_OEM_COMMA || vk == VK_OEM_PERIOD) && !shift &&
            g_lastCommitEndsWithDigit) {
            return false;
        }
        // 配对引号（0.2.28）：' 单引号（‘’）/" 双引号（“”），开闭交替
        if (vk == VK_OEM_7) {
            if (engine_get_candidate_count() > 0) {
                char buf[512] = {0};
                const int len = engine_select_candidate(0, buf, sizeof(buf));
                if (len > 0) {
                    out.committed = Utf8ToWide(buf);
                }
            } else if (engine_get_pinyin_str(nullptr, 0) > 1) {
                engine_reset();
            }
            out.punct_quote = shift ? 2 : 1;
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
        // 单值标点：有候选 → 先上屏默认候选，再接标点
        const wchar_t* punct = MapFullWidthPunct(vk, shift);
        if (punct != nullptr) {
            std::wstring commit;
            if (engine_get_candidate_count() > 0) {
                char buf[512] = {0};
                const int len = engine_select_candidate(0, buf, sizeof(buf));
                if (len > 0) {
                    commit += Utf8ToWide(buf);
                }
            } else if (engine_get_pinyin_str(nullptr, 0) > 1) {
                engine_reset(); // 丢弃未完成拼音（与 Enter 行为一致）
            }
            // V0.4.x 配对符号成对上屏：开符号补闭符号 + 光标居中偏移。
            // 仅当开关开启且为全角开符号；有候选先上屏时偏移在候选之后，
            // 因此 caret_offset 需在提交文本的绝对偏移（候选宽度 + 开符号宽）。
            if (IsPairPunctEnabled()) {
                const wchar_t close = PairCloseFor(punct[0]);
                if (close != 0) {
                    const size_t before = commit.size();
                    commit += punct;
                    commit += close;
                    out.caret_offset = static_cast<int>(before + wcslen(punct));
                } else {
                    commit += punct;
                }
            } else {
                commit += punct;
            }
            out.committed = commit;
            out.eaten = true;
            out.state_changed = true;
            return true;
        }
    }

    // 英文字母：A-Z（0x41-0x5A）
    if (vk >= 'A' && vk <= 'Z') {
        const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
        const bool shiftDown = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        // Ctrl/Alt 组合键：透传应用（Ctrl+C 复制 / Ctrl+V 粘贴 / Ctrl+R 刷新，问题 2）
        if (ctrlDown || altDown) {
            return false;
        }
        // 英文模式：字母透传给应用（0.1.15）
        // 修复：之前走 committed 提交，但无组合时 CommitComposition 不写文本
        // → 字母被吞但不上屏。透传是输入法英文模式的业界标准做法。
        if (engine_get_ascii_mode() == 1) {
            return false;
        }
        // 中文模式 + Shift：输出大写字母（问题 10，微软拼音行为）。
        // 有候选 → 先上屏默认候选（zhong + Shift+Z → "中Z"）；无候选但有拼音 → 丢弃。
        if (shiftDown) {
            std::wstring commit;
            const int count = engine_get_candidate_count();
            if (count > 0) {
                char buf[512] = {0};
                const int len = engine_select_candidate(0, buf, sizeof(buf));
                if (len > 0) {
                    commit += Utf8ToWide(buf);
                }
            } else if (engine_get_pinyin_str(nullptr, 0) > 1) {
                engine_reset(); // 丢弃未完成拼音
            }
            commit += static_cast<wchar_t>(vk); // 大写字母（vk 即大写）
            out.committed = commit;
            out.eaten = true;
            out.state_changed = true;
            out.multirow_collapse = true; // 提交大写 → 复位多行
            return true;
        }
        // 中文模式：累积拼音
        const char ch = static_cast<char>(vk + ('a' - 'A')); // 转小写
        const int count = engine_process_key(static_cast<int>(ch));
        out.eaten = true;
        out.state_changed = true;
        out.candidate_count = count;
        out.multirow_collapse = true; // 新拼音 → 复位多行（问题：输入新词仍维持多列）
        return true;
    }

    // P2-1 Ctrl+BackSpace：删除一个音节（对标 rime back_syllable）
    if (vk == VK_BACK && (GetKeyState(VK_CONTROL) & 0x8000)) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            const int count = engine_backspace_syllable();
            out.eaten = true;
            out.state_changed = true;
            out.candidate_count = count;
            out.multirow_collapse = true; // 拼音变化 → 复位多行
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
            out.multirow_collapse = true; // 候选列表变化 → 复位多行
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
        out.multirow_collapse = true; // 拼音变化 → 复位多行
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

    // v 前缀数字别名（0.2.32，对标 QQ v1-v9）：v 前缀（拼音串恰为 v）时数字键
    // 送进引擎 → v1/v2... 触发数字分类符号查询，而非选择热门符号候选。
    // 符号分类候选出现后（v1/vbd 模式）数字键恢复正常选词。
    if (vk >= '1' && vk <= '9' && engine_is_symbol_prefix() == 1) {
        const int count = engine_process_key(vk);
        out.eaten = true;
        out.state_changed = true;
        out.candidate_count = count;
        return true;
    }

    // 数字键 1-9：选择对应候选（索引 0-8）
    if (vk >= '1' && vk <= '9') {
        const int index = vk - '1';
        const int count = engine_get_candidate_count();
        if (count > index) {
            char buf[512] = {0};
            const int len = engine_select_candidate(index, buf, sizeof(buf));
            // V0.5 组词模式：中间音节选字无文本提交（len=0），但必须吞键并
            // 刷新候选（候选切到下一音节单字）——否则数字键透传给应用。
            if (len > 0 || engine_in_compose() == 1) {
                out.committed = Utf8ToWide(buf);
                out.eaten = true;
                out.state_changed = true;
                return true;
            }
        }
        return false;
    }

    // 翻页键：PgUp/PgDn、+/= 下一页，- 上一页（0.1.13）
    // V0.3.x：移除逗号/句号翻页——让位给中文标点（问题 2）
    if (vk == VK_PRIOR || vk == VK_NEXT ||
        vk == VK_OEM_PLUS || vk == VK_OEM_MINUS) {
        bool forward = (vk == VK_NEXT || vk == VK_OEM_PLUS);
        const int count = engine_page(forward ? 1 : -1);
        if (count > 0) {
            out.eaten = true;
            out.state_changed = true;
            out.candidate_count = count;
            return true;
        }
        // 无更多页时不吞键
        return false;
    }

    // 多行展开/收起（0.2.14）：↓ 展开 / ↑ 收起（候选存在时）
    if (vk == VK_DOWN || vk == VK_UP) {
        if (engine_get_candidate_count() > 0) {
            out.eaten = true;
            out.state_changed = true;
            // ↓ 展开多行，↑ 收起（请求由平台层应用）
            out.multirow_requested = (vk == VK_DOWN);
            out.multirow_collapse = (vk == VK_UP);
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

    // 回车（V0.4.3 修正语义）：
    // 中文模式下有拼音串 → 不选词，拼音串作为英文提交上屏，关闭候选窗。
    // 无拼音 → 透传回车给应用。吞回车（不额外发送/换行）。
    if (vk == VK_RETURN) {
        if (engine_get_pinyin_str(nullptr, 0) > 1) {
            char buf[128] = {0};
            const int len = engine_get_pinyin_str(buf, sizeof(buf));
            engine_reset();
            out.state_changed = true;
            out.eaten = true;  // 吞回车
            if (len > 0 && buf[0] != '\0') {
                out.committed = Utf8ToWide(buf);
            }
            return true;
        }
        return false;  // 无拼音：透传回车给应用
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
