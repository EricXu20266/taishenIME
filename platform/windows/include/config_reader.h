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
};

/// 读取 DLL 同目录 config.ini。
/// @param dllDir DLL 所在目录（带尾分隔符），用于定位 config.ini 与解析相对词库路径
/// @return 解析后的配置（缺失项用默认值）
ImeConfig LoadConfig(const std::wstring& dllDir);

/// 将配置中的词库路径解析为绝对路径。
/// 相对路径以 dllDir 为基准；空路径返回空（= 内置词库）。
std::wstring ResolveDictPath(const ImeConfig& cfg, const std::wstring& dllDir);

} // namespace taishen
