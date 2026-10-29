/// TSF 按键处理逻辑 — 键码 → 引擎 FFI 的映射与决策
///
/// 独立于 COM 层，纯逻辑，便于单元测试。
/// 返回 true 表示吞掉该键（不再透传给应用），false 表示透传。

#pragma once

#include <windows.h>
#include <string>
#include <vector>

namespace taishen {

/// 按键处理结果
struct KeyEventResult {
    /// 是否吞掉按键
    bool eaten = false;
    /// 是否产生了新的拼音/候选状态（供候选窗口刷新）
    bool state_changed = false;
    /// 按键后提交的文本（选词时非空，0.1.7 才真正上屏）
    std::wstring committed;
    /// 光标定位偏移（V0.4.x 配对符号成对上屏）：提交后光标应停在第 N 个
    /// 字符后（N=开符号宽度）。-1 = 不定位（光标留在文本末尾，默认）。
    int caret_offset = -1;
    /// 当前拼音串（调试/候选窗口用）
    std::string pinyin;
    /// 当前候选词数
    int candidate_count = 0;
    /// 多行展开请求（V0.2.14）：true 展开（↓）——仅请求，由平台层应用
    bool multirow_requested = false;
    /// 多行收起请求（V0.3.x）：true 收起（↑）/ 拼音变化自动复位——
    /// 与 multirow_requested 分离，修复"↑ 收不起 + 输入新词仍维持多行"
    bool multirow_collapse = false;
    /// 配对引号（0.2.28）：1=单引号(' → ‘’) 2=双引号(" → “”)，0=无。
    /// 平台层按开闭状态交替上屏（V0.4.x 成对开启时直接成对输出）
    int punct_quote = 0;
};

/// 判断是否应吞掉该键（无副作用，供 OnTestKeyDown 预测试使用）。
/// 只做键位与状态的只读判断，绝不修改引擎状态。
/// @param vk              虚拟键码（VK_*）——已由 NormalizeKeypad 归一为等价键
/// @param vimPassthrough  vim_mode 进程（V0.2.36）：true 时 Esc/Ctrl+C/Ctrl+[ 强制透传
///                        （vim 需要收到这些键退出插入模式）
/// @param rawVk           原始虚拟键码（未归一），默认 0 = 未知。用于区分小键盘
///                        符号键（+ - * / . ,），避免被当成主键盘翻页/选词语义
/// @return                true 输入法会处理该键（应吞） / false 透传给应用
bool ShouldEatKey(int vk, bool vimPassthrough = false, int rawVk = 0);

/// V0.2.36 vim_mode 键判定：Esc / Ctrl+C / Ctrl+[（vim 中等价 Esc）。
/// 只读判断（读 GetKeyState），无副作用。
bool IsVimModeKey(int vk);

/// 处理一次按键（虚拟键码 + lParam），填充结果。
/// 有副作用——只在 OnKeyDown 中调用，绝不能在 OnTestKeyDown 中调用。
/// @param vk      虚拟键码（VK_*）——已由 NormalizeKeypad 归一为等价键
/// @param lparam  TSF OnKeyDown 传入的 lParam（当前仅用于功能判断，可扩展）
/// @param out     处理结果
/// @param rawVk   原始虚拟键码（未归一），默认 0 = 未知。小键盘 + / * 需借它
///                补回 Shift 语义（否则 + 会变 =、* 会被当成数字 8）
/// @return        true 吞键 / false 透传（与 out.eaten 一致）
bool HandleKeyDown(int vk, LPARAM lparam, KeyEventResult& out, int rawVk = 0);

/// 将 UTF-8 字节串转换为宽字符串（候选窗口显示用）
std::wstring Utf8ToWide(const std::string& utf8);

/// 将宽字符串转换为 UTF-8（TSF 文本提交用）
std::string WideToUtf8(const std::wstring& wide);

/// P2-4 小键盘归一（对标 rime KP_0-9 等键绑定）：小键盘键 → 主键盘等价键。
/// KP_0-9 → '0'-'9'、KP_Decimal → '.'、KP_+ - * / → 主键盘运算符。
/// 归一后候选选择/计算器/数字大写/Unicode 模式自动支持小键盘。
int NormalizeKeypad(int vk);

/// 是否小键盘键（VK_NUMPAD0-9 / VK_MULTIPLY / VK_ADD / VK_SUBTRACT /
/// VK_DECIMAL / VK_DIVIDE / VK_SEPARATOR）。
/// 小键盘符号不应参与主键盘的候选选择与翻页语义（+/− 翻页、1-9 选词）。
bool IsKeypadKey(int rawVk);

/// 小键盘键的「Shift 等价」判定：VK_ADD(+) / VK_MULTIPLY(*) 在主键盘上
/// 需要 Shift 才产出同一字符，归一化成 OEM_PLUS / '8' 后必须补回该语义，
/// 否则小键盘 + 会被当成 '='（原 bug）、小键盘 * 会被当成数字 8。
bool KeypadShiftEquivalent(int rawVk);

/// P1-2 数字分隔符状态：最近一次 IME 提交以数字结尾 → , . 直通半角。
/// 定义于 tsf_keyevent.cpp，提交文本后由 tsf_module 更新。
extern bool g_lastCommitEndsWithDigit;

/// V0.4.x 配对符号成对上屏开关（由 tsf_module 从 config.ini 同步）。
/// true=开符号成对输出+光标居中；false=单符号输出。
void SetPairPunctEnabled(bool enabled);
bool IsPairPunctEnabled();

/// 键位绑定运行时状态（0.1.49）：由 tsf_module 在 ApplyConfig 时从
/// config.ini 注入，tsf_keyevent 只读不改。
struct KeyBindingState {
    bool toggleAsciiCtrlSpace = true;   // Ctrl+空格 切中英
    bool toggleAsciiShift = true;       // 单按 Shift 切中英
    int toggleFullwidth = 1;            // 0=无 1=Shift+空格 切全半角
    int togglePunct = 1;                // 0=无 1=Ctrl+. 切中英标点
    int toggleTraditionalKey = 'F';     // 0=禁用，否则 Ctrl+Shift+<字母>
    bool pageCommaPeriod = false;       // 逗号/句号 翻页
    bool pageMinusEqual = false;        // 主键盘 +/− 翻页
    bool pageBrackets = false;          // 左/右方括号 翻页
    bool pageNumpad = false;            // 小键盘 +/− 翻页
};

/// 注入键位绑定（线程内全局，进程内共享）
void SetKeyBindings(const KeyBindingState& st);
const KeyBindingState& GetKeyBindings();

/// 全/半角状态（0.1.49）：false=半角（默认），true=全角
void SetFullWidthMode(bool on);
bool IsFullWidthMode();

/// 提交文本全角化（全角模式下调用）：ASCII 字母/数字/符号 → 全角。
/// 中文与其他非 ASCII 字符原样保留。
std::wstring ToFullWidth(const std::wstring& text);

/// V0.4.x 配对符号扩展（复选标点/引号共用）：若 sel 是开符号且开关开启，
/// 输出 sel+闭符号 并给出光标偏移；否则原样输出、光标不动。
/// @param sel          选中的符号（宽字符）
/// @param committed    输出：最终提交文本
/// @param caretOffset  输出：光标偏移（-1=不定位）
void ExpandPairPunct(const std::wstring& sel, std::wstring& committed,
                     int& caretOffset);

} // namespace taishen
