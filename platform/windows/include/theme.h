/// 系统主题检测与应用策略（V0.2.20 深色模式跟随系统）
///
/// 检测：HKCU\...\Themes\Personalize\AppsUseLightTheme（0=深色，1=浅色）
/// 策略：用户显式配置 theme_* → 固定用户主题；否则跟随系统默认主题。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include "config_reader.h"

namespace taishen {

/// 检测系统应用模式：0=深色，1=浅色，-1=未知（注册表读取失败）
int GetSystemAppTheme();

/// 系统浅色主题默认值（与 CandidateTheme::Default 深色镜像）
CandidateTheme LightTheme();

/// 应用主题策略（候选窗/横幅共用）：
/// @param out       输出最终主题
/// @param cfg       配置（含 theme 与 userThemeExplicit）
/// @return 是否跟随了系统主题（true=系统决定，false=用户显式配置）
bool ApplyThemeWithSystem(CandidateTheme& out, const ImeConfig& cfg);

} // namespace taishen
