/// 配置读取 — 声明
///
/// 对应 SPEC: docs/modules/config-system/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.8 基础配置系统
///
/// 读取 DLL 同目录 config.ini（key=value 格式），解析候选数与词库路径。
/// 解析失败或文件缺失 → 全部回退默认值。

#pragma once

#include <windows.h>
#include <d2d1.h>
#include <string>
#include <vector>

namespace taishen {

/// 候选窗口主题（P0-1 视觉升级，对标 weasel preset_color_schemes）：
/// 10 项配色：背景/正文/序号/注释/边框/选中背景/选中文字/选中序号/页码/标记
struct CandidateTheme {
    D2D1_COLOR_F bg;             // 背景
    D2D1_COLOR_F text;           // 候选词正文
    D2D1_COLOR_F label;          // 序号标签
    D2D1_COLOR_F comment;        // 注释（预留）
    D2D1_COLOR_F border;         // 边框
    D2D1_COLOR_F highlight_bg;   // 选中候选背景（原 highlight）
    D2D1_COLOR_F highlight_text; // 选中候选文字
    D2D1_COLOR_F highlight_label;// 选中候选序号
    D2D1_COLOR_F dim;            // 页码/次要文字
    D2D1_COLOR_F mark;           // 选中标记（悬停/标记色）

    /// 默认构造 = 深色主题
    /// ⚠️ 修复 0.1.24 死递归：旧实现 `*this = Default()` 与
    /// `Default() { CandidateTheme t; }` 互相调用 → 无限递归 → 栈溢出
    /// （宿主程序切换输入法即崩 0xc000041d / 0xc00000fd）。
    /// 现在默认构造直接初始化成员，Default() 返回默认实例，无递归。
    CandidateTheme()
        : bg(D2D1::ColorF(0x1E1E1E, 1.0f)),
          text(D2D1::ColorF(0xE8E8E8, 1.0f)),
          label(D2D1::ColorF(0x9A9A9A, 1.0f)),
          comment(D2D1::ColorF(0x808080, 1.0f)),
          border(D2D1::ColorF(0x333333, 1.0f)),
          highlight_bg(D2D1::ColorF(0x4C8DFF, 1.0f)),
          highlight_text(D2D1::ColorF(0xFFFFFF, 1.0f)),
          highlight_label(D2D1::ColorF(0xFFFFFF, 1.0f)),
          dim(D2D1::ColorF(0x9A9A9A, 1.0f)),
          mark(D2D1::ColorF(0x3A3A3A, 1.0f)) {}

    /// 默认深色主题（与 0.1.6 初始配色一致）
    static CandidateTheme Default() { return CandidateTheme(); }
};

/// 输入法配置
struct ImeConfig {
    /// 候选词数量上限（默认 5）
    int candidate_count = 5;
    /// 候选排序模式（P0-2，默认 0）：0=默认 1=单字优先 2=长词优先
    int sort_mode = 0;
    /// 上下文联想开关（P1-1，默认关）：1=开 0=关
    bool context_assoc = false;
    /// 专业词库分类文件路径（对标微软/搜狗分类词库，默认空=停用）
    std::wstring domain_dicts;
    /// 系统词库路径（相对 DLL 目录或绝对路径；空 = 内置词库）
    std::wstring dict_path;
    /// 用户词库路径（V0.2.2，默认 %APPDATA%/taishen-ime/user_dict.db）
    std::wstring user_dict_path;
    /// 模糊音开关（RIME 拼写变体，默认开，0.1.14）
    bool fuzzy_enabled = true;
    /// 双拼模式（RIME 微软双拼方案，默认关，0.1.14）
    bool shuangpin_mode = false;
    /// 双拼方案（P2-7，默认 mspy）：mspy/flypy/sogou/zrm/ziguang/jiajia
    std::string shuangpin_scheme = "mspy";
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
    /// 候选窗口主题（V0.2.4，默认深色）
    CandidateTheme theme;
    /// 主题模式（0.2.34：0=跟随系统 1=深色 2=浅色）——显式持久化，
    /// 不再靠"config 存在 theme_* 键"推断（否则跟随系统保存后读回变显式深色）
    int theme_mode = 0;
    /// 用户是否显式配置了任一 theme_* 键（V0.2.20，true=固定用户主题不跟随系统）
    bool userThemeExplicit = false;
    /// 候选窗字体名（V0.2.21，默认 Microsoft YaHei）
    std::wstring font_face = L"Microsoft YaHei";
    /// 候选窗正文字号（V0.2.21，px，默认 16，范围 12-32）
    float font_size = 16.0f;
    /// 行内预编辑（V0.2.18，默认开）：拼音写在组合（光标处），候选窗不重复画拼音
    bool inline_preedit = true;
    /// 中英标点开关（P0-2，默认关）：false=中文标点全角化，true=英文标点透传
    bool ascii_punct = false;
    /// 配对符号成对上屏（V0.4.x，默认开）：《（）【】「」等开符号上屏时
    /// 自动补闭符号并光标居中（对齐微软/搜狗/微信）；false=单符号上屏
    bool pair_punct = true;
    /// 诊断日志开关（V0.4.x，默认关）：开发调试用，终端用户无需开启
    bool debug_log = false;
    /// Emoji 开关（P2-5，默认开）：候选命中映射时追加 emoji 候选
    bool emoji_enabled = false;
    /// 应用级英文模式（P2-6 → V0.2.32 语义修正，对标 rime weasel app_options）：
    /// 进程名列表（小写，如 cmd.exe/cod.exe），这些程序**首次进入时**默认英文模式
    /// （非强制：用户手动切换后保持，不被打回）
    std::vector<std::wstring> app_ascii_list;
    /// 应用级默认中文（V0.2.32）：进程名列表，这些程序首次进入时默认中文
    /// （覆盖全局默认，用于 app_ascii 与全局默认不一致的场景）
    std::vector<std::wstring> app_cn_list;
    /// 应用级强制行内预编辑（V0.2.32，对标 weasel firefox inline_preedit bug 规避）：
    /// 进程名列表，命中则强制行内预编辑（不受全局 inline_preedit 开关影响）
    std::vector<std::wstring> app_inline_list;
    /// 应用级 vim 模式（V0.2.36，对标 weasel app_options vim_mode）：
    /// 进程名列表，命中时 Esc / <C-c> / <C-[> 切换到 ascii 状态并透传按键
    /// （vim 需要收到这些键；典型场景 nvim-qt.exe）
    std::vector<std::wstring> app_vim_list;
    /// 候选标签格式（P0-1，对标 weasel label_format）：%d = 数字，%s = 数字文本
    /// 如 "%d." → "1."、"①"（数字变体）、"%s、" → "1、"
    std::wstring label_format = L"%d.";
    /// 窗口圆角半径（P0-1，对标 weasel corner_radius；V0.3.6 默认 4 → 8）
    float corner_radius = 8.0f;
    /// 选中高亮块圆角半径（P0-1，对标 weasel round_corner；V0.3.6 默认 3 → 6）
    float hilite_corner_radius = 6.0f;
    /// 窗口内边距（P0-1，对标 weasel margin；V0.3.6 默认 8 → 10）
    int padding = 10;
    /// 候选间距（P0-1，对标 weasel candidate_spacing；V0.3.6 默认 14 → 12）
    int candidate_spacing = 12;
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

/// 将配置写回 DLL 同目录 config.ini（覆盖写，保留注释头）。
/// 保存后 tsf_module 的 2s 轮询热加载自动检测 mtime → ApplyConfig。
/// @param dllDir DLL 所在目录（带尾分隔符）
/// @param cfg 要写入的配置（全部键显式写出，含默认值）
/// @return true 成功 / false 文件打开失败
bool SaveConfig(const std::wstring& dllDir, const ImeConfig& cfg);

} // namespace taishen
