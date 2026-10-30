#pragma once

/// 0.1.68：把配置应用到当前进程的引擎单例 + 配置变更通知。
/// TSF 的 CTextService::ApplyConfig 与设置页保存后共用——
/// 设置页保存不再只写文件等 2s 热加载，而是立即生效、工具栏随即同步。
///
/// 变更通知走函数指针回调而不是直接调 CBannerWindow：test_settings_dialog
/// 这个测试目标不链接 banner_window.cpp，直接引用会链接失败。

#include "config_reader.h"
#include "engine_bridge.h"

namespace taishen {

using ConfigChangedHook = void (*)();

/// 回调槽（函数内静态，C++11 起跨 TU 唯一，不依赖 C++17 inline 变量）
inline ConfigChangedHook& ConfigChangedHookRef()
{
    static ConfigChangedHook hook = nullptr;
    return hook;
}

/// 由 TSF 模块注册为「刷新工具栏」；未注册时为空操作（如独立设置测试程序）
inline void SetConfigChangedHook(ConfigChangedHook hook)
{
    ConfigChangedHookRef() = hook;
}

inline void NotifyConfigChanged()
{
    const ConfigChangedHook hook = ConfigChangedHookRef();
    if (hook != nullptr) {
        hook();
    }
}

/// 只含引擎级开关（不含键位绑定 / 候选窗主题 / app_options）。
/// 这些引擎 setter 都是幂等的，重复调用无害。
inline void ApplyConfigToEngine(const ImeConfig& cfg)
{
    engine_set_candidate_count(cfg.candidate_count);
    engine_set_sort_mode(cfg.sort_mode);
    engine_set_context_assoc(cfg.context_assoc ? 1 : 0);
    engine_set_fuzzy(cfg.fuzzy_enabled ? 1 : 0);
    engine_set_shuangpin(cfg.shuangpin_mode ? 1 : 0);
    engine_set_shuangpin_scheme(cfg.shuangpin_scheme.c_str());
    engine_set_shuangpin_full_preedit(cfg.shuangpin_full_preedit ? 1 : 0);
    engine_set_correction(cfg.correction_enabled ? 1 : 0);
    engine_set_mix_mode(cfg.mix_mode_enabled ? 1 : 0);
    engine_set_traditional(cfg.traditional_enabled ? 1 : 0);
    engine_set_ascii_punct(cfg.ascii_punct ? 1 : 0);
    engine_set_emoji(cfg.emoji_enabled ? 1 : 0);
}

} // namespace taishen
