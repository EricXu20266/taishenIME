/// TSF 按键处理逻辑 — 键码 → 引擎 FFI 的映射与决策
///
/// 独立于 COM 层，纯逻辑，便于单元测试。
/// 返回 true 表示吞掉该键（不再透传给应用），false 表示透传。

#pragma once

#include <windows.h>
#include <string>

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
};

/// 处理一次按键（虚拟键码 + lParam），填充结果。
/// @param vk      虚拟键码（VK_*）
/// @param lparam  TSF OnKeyDown 传入的 lParam（当前仅用于功能判断，可扩展）
/// @param out     处理结果
/// @return        true 吞键 / false 透传（与 out.eaten 一致）
bool HandleKeyDown(int vk, LPARAM lparam, KeyEventResult& out);

/// 将 UTF-8 字节串转换为宽字符串（候选窗口显示用）
std::wstring Utf8ToWide(const std::string& utf8);

} // namespace taishen
