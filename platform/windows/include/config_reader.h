/// 配置读取 — 声明
///
/// 对应 SPEC: docs/modules/config-system/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.8 基础配置系统
///
/// 读取 DLL 同目录 config.ini（key=value 格式），解析候选数与词库路径。
/// 解析失败或文件缺失 → 全部回退默认值。

#pragma once

#include <windows.h>
#include <string>

namespace taishen {

/// 输入法配置
struct ImeConfig {
    /// 候选词数量上限（默认 9）
    int candidate_count = 9;
    /// 系统词库路径（相对 DLL 目录或绝对路径；空 = 内置词库）
    std::wstring dict_path;
    /// 用户词库路径（V0.2.2，默认 %APPDATA%/taishen-ime/user_dict.db）
    std::wstring user_dict_path;
    /// 模糊音开关（RIME 拼写变体，默认开，0.1.14）
    bool fuzzy_enabled = true;
    /// 双拼模式（RIME 微软双拼方案，默认关，0.1.14）
    bool shuangpin_mode = false;
    /// 智能纠错开关（键盘相邻键容错，默认开，0.2.10）
    bool correction_enabled = true;
    /// 中英混输开关（中文模式候选末尾英文候选，默认开，0.2.8）
    bool mix_mode_enabled = true;
    /// 简繁转换开关（候选输出转繁体，默认关，0.2.11）
    bool traditional_enabled = false;
    /// 快捷短语开关（简码→短语，默认开，0.2.12）
    bool phrase_enabled = true;
    /// 自定义短语文件路径（空 = 仅内置，0.2.12）
    std::wstring phrase_path;
};

/// 读取 DLL 同目录 config.ini。
/// @param dllDir DLL 所在目录（带尾分隔符），用于定位 config.ini 与解析相对词库路径
/// @return 解析后的配置（缺失项用默认值）
ImeConfig LoadConfig(const std::wstring& dllDir);

/// 将配置中的词库路径解析为绝对路径。
/// 相对路径以 dllDir 为基准；空路径返回空（= 内置词库）。
std::wstring ResolveDictPath(const ImeConfig& cfg, const std::wstring& dllDir);

/// 解析用户词库路径（V0.2.2）。
/// 配置为空 → 默认 %APPDATA%/taishen-ime/user_dict.db；
/// 相对路径以 dllDir 为基准；绝对路径直接使用。
std::wstring ResolveUserDictPath(const ImeConfig& cfg, const std::wstring& dllDir);

} // namespace taishen
