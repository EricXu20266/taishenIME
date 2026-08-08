/// 自研窗体系统 — 主题 token（V0.3.0）
///
/// 对应 SPEC: docs/modules/ui-framework/SPEC.md §3.2
/// 深浅两套主题 + 跟随系统（AppsUseLightTheme），所有控件绘制只消费 UITheme，
/// 不感知具体色值。几何 token（圆角/间距/字号）与颜色 token 同结构。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include <string>

namespace taishen {

/// 主题 token 集
struct UITheme {
    // ── 颜色 ──
    D2D1_COLOR_F bg;         ///< 窗口背景
    D2D1_COLOR_F cardBg;     ///< 卡片背景（内容面板/分组）
    D2D1_COLOR_F text;       ///< 主文字
    D2D1_COLOR_F textDim;    ///< 次要文字（说明/占位）
    D2D1_COLOR_F accent;     ///< 强调色（选中项/主按钮）
    D2D1_COLOR_F accentText; ///< 强调色上的文字
    D2D1_COLOR_F border;     ///< 边框/分隔线
    D2D1_COLOR_F hoverBg;    ///< 悬停背景
    D2D1_COLOR_F pressedBg;  ///< 按下背景
    D2D1_COLOR_F checkmark;  ///< 复选框勾/选中标记
    // ── toggle 开关（V0.3.6）──
    D2D1_COLOR_F switchTrackOn;  ///< 开关开启轨道（= accent）
    D2D1_COLOR_F switchTrackOff; ///< 开关关闭轨道（浅灰）
    D2D1_COLOR_F switchThumb;    ///< 滑块（白）
    // ── 几何 ──
    float cornerRadius;      ///< 控件圆角
    float cardRadius;        ///< 卡片圆角
    int   padding;           ///< 页面内边距
    int   gap;               ///< 控件间距
    // ── 字号 ──
    float fontSize;          ///< 正文
    float fontSizeTitle;     ///< 分组标题
};

/// 深色主题（参考候选窗深色 0xF22E2E2E 背景族）
UITheme UIThemeDark();

/// 浅色主题（Windows 11 风格：白底 + 浅灰卡片 + 蓝强调）
UITheme UIThemeLight();

/// 检测系统应用模式：0=深色，1=浅色，-1=未知（注册表读取失败）
/// 读取 HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme
int UIThemeSystemMode();

/// 当前主题：跟随系统模式（深色系统→深色主题，浅色→浅色）
UITheme UIThemeCurrent();

} // namespace taishen
