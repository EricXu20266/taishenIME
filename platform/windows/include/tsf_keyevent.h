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
    /// 当前拼音串（调试/候选窗口用）
    std::string pinyin;
    /// 当前候选词数
    int candidate_count = 0;
    /// 多行展开请求（V0.2.14）：true 展开 / false 收起 / 无变化不置位
    bool multirow_requested = false;
    /// 标点复选候选列表（0.2.28）：非空 = 中文模式复选标点（如 《〈«‹），
    /// 由平台层显示候选窗，数字键/空格选择后上屏
    std::vector<std::wstring> punct_candidates;
    /// 配对引号（0.2.28）：1=单引号(' → ‘’) 2=双引号(" → “”)，0=无。
    /// 平台层按开闭状态交替上屏
    int punct_quote = 0;
};

/// 判断是否应吞掉该键（无副作用，供 OnTestKeyDown 预测试使用）。
/// 只做键位与状态的只读判断，绝不修改引擎状态。
/// @param vk      虚拟键码（VK_*）
/// @return        true 输入法会处理该键（应吞） / false 透传给应用
bool ShouldEatKey(int vk);

/// 处理一次按键（虚拟键码 + lParam），填充结果。
/// 有副作用——只在 OnKeyDown 中调用，绝不能在 OnTestKeyDown 中调用。
/// @param vk      虚拟键码（VK_*）
/// @param lparam  TSF OnKeyDown 传入的 lParam（当前仅用于功能判断，可扩展）
/// @param out     处理结果
/// @return        true 吞键 / false 透传（与 out.eaten 一致）
bool HandleKeyDown(int vk, LPARAM lparam, KeyEventResult& out);

/// 将 UTF-8 字节串转换为宽字符串（候选窗口显示用）
std::wstring Utf8ToWide(const std::string& utf8);

/// 将宽字符串转换为 UTF-8（TSF 文本提交用）
std::string WideToUtf8(const std::wstring& wide);

/// P1-2 数字分隔符状态：最近一次 IME 提交以数字结尾 → , . 直通半角。
/// 定义于 tsf_keyevent.cpp，提交文本后由 tsf_module 更新。
extern bool g_lastCommitEndsWithDigit;

} // namespace taishen
