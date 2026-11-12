/// per-app 状态记忆（V0.2.33，对标 rime weasel app_options）
///
/// 每个进程独立记忆中英状态：焦点切换时应用该进程的记忆状态，
/// 用户手动切换时更新当前前台进程的记忆。
/// 引擎保持全局单例（FFI 零改动），状态决策完全在本模块完成。

#pragma once

#include <string>

struct ImeConfig;

namespace taishen {

/// AppStateApply 的结果
struct AppStateResult {
    /// 当前前台进程名（小写，空 = 无法获取）
    std::wstring proc;
    /// 命中 app_inline_list（该进程强制行内预编辑，不受全局开关影响）
    bool inline_hit = false;
    /// 命中 app_vim_list（该进程启用 vim 模式：Esc/Ctrl+C/Ctrl+[ 切英文并透传）
    bool vim_hit = false;
    /// ascii 状态本次是否发生变化（true = 引擎 set_ascii_mode 被调用）
    bool ascii_changed = false;
};

/// 应用前台进程的初始/记忆状态到引擎。
/// 首次进入进程时按配置定初始状态（app_ascii → 英文；app_cn → 中文；
/// 未配置 → 继承当前引擎状态，保持无配置时行为不变），随后记忆用户手动切换。
/// 同时计算 inline 覆盖供候选窗口使用。
AppStateResult AppStateApply(const ImeConfig& cfg);

/// 设置引擎 ascii 模式并更新当前前台进程的记忆。
/// 所有手动切换入口（Shift / 托盘 / 工具栏）统一走这里，保证记忆同步。
void AppStateSetAscii(bool ascii);

/// 查询当前前台进程是否命中 app_vim（供 OnTestKeyDown/OnKeyUp 的 vim_mode 判断）。
/// 内部 GetForegroundProcessName + 查 cfg.app_vim_list，无副作用（不写引擎状态）。
bool AppStateIsVimForeground(const ImeConfig& cfg);

/// 宿主拒绝 TSF 候选 UI（BeginUIElement show=FALSE）时，是否改由输入法自绘候选窗。
///
/// 背景：show=FALSE 只表示宿主不让系统画 UIElement，有两种完全相反的含义——
///   1) 宿主自己会通过 ITfCandidateListUIElement 读数据自绘（微信/部分现代应用）；
///   2) 宿主根本不画，也不读数据（魔兽世界等游戏，经 msctfime 桥接）。
/// 判据优先级：app_self_draw（强制画）> app_no_self_draw（禁画）>
///             自动（宿主没读候选数据 = 它不会画 → 我们兜底）。
///
/// @param cfg 当前配置
/// @param hostReadData 宿主是否调用过候选数据接口（GetCount/GetString 等）
bool AppStateShouldSelfDrawWhenSuppressed(const ImeConfig& cfg, bool hostReadData);

}  // namespace taishen
