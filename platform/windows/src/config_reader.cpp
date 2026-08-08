/// 配置读取 — 实现
///
/// 对应 SPEC: docs/modules/config-system/SPEC.md
/// 格式：
///   # 注释
///   candidate_count=9
///   dict_path=system_dict.db

#include "config_reader.h"

#include <algorithm>
#include <cwctype>
#include <fstream>
#include <functional>
#include <sstream>
#include <shlobj.h>

namespace taishen {

/// 去除字符串首尾空白
static std::wstring Trim(const std::wstring& s)
{
    size_t start = 0;
    while (start < s.size() && (s[start] == L' ' || s[start] == L'\t')) {
        ++start;
    }
    size_t end = s.size();
    while (end > start && (s[end - 1] == L' ' || s[end - 1] == L'\t' ||
                           s[end - 1] == L'\r' || s[end - 1] == L'\n')) {
        --end;
    }
    return s.substr(start, end - start);
}

/// 解析布尔配置值（1/true/on/yes → true，0/false/off/no → false，非法回退默认）
static bool ParseBool(const std::wstring& value, bool defaultValue)
{
    if (value == L"1" || value == L"true" || value == L"on" ||
        value == L"yes" || value == L"True" || value == L"ON") {
        return true;
    }
    if (value == L"0" || value == L"false" || value == L"off" ||
        value == L"no" || value == L"False" || value == L"OFF") {
        return false;
    }
    return defaultValue;
}

/// 解析 HEX 颜色 "RRGGBB"（如 2E2E2E）→ D2D1_COLOR_F（alpha=1）
/// 非法/空 → 返回 false（调用方回退默认）
static bool ParseHexColor(const std::wstring& value, D2D1_COLOR_F& out)
{
    if (value.size() != 6) {
        return false;
    }
    auto hexVal = [](wchar_t c) -> int {
        if (c >= L'0' && c <= L'9') return c - L'0';
        if (c >= L'a' && c <= L'f') return c - L'a' + 10;
        if (c >= L'A' && c <= L'F') return c - L'A' + 10;
        return -1;
    };
    int r = 0, g = 0, b = 0;
    for (int i = 0; i < 6; ++i) {
        const int v = hexVal(value[i]);
        if (v < 0) {
            return false;
        }
        if (i < 2)      { r = r * 16 + v; }
        else if (i < 4) { g = g * 16 + v; }
        else            { b = b * 16 + v; }
    }
    out.r = r / 255.0f;
    out.g = g / 255.0f;
    out.b = b / 255.0f;
    out.a = 1.0f;
    return true;
}

/// 逐行读取配置文件（UTF-8 兼容：文件可能为 UTF-8 或 ANSI）
static void ReadConfigFile(const std::wstring& path,
                           std::function<void(const std::wstring&)> onLine)
{
    // 用标准 ifstream 读字节流，按 UTF-8 解码
    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) {
        return;
    }
    std::string line;
    while (std::getline(file, line)) {
        // 转为宽字符串
        std::wstring wline;
        const int len = MultiByteToWideChar(CP_UTF8, 0, line.c_str(),
                                            static_cast<int>(line.size()),
                                            nullptr, 0);
        if (len > 0) {
            wline.resize(static_cast<size_t>(len));
            MultiByteToWideChar(CP_UTF8, 0, line.c_str(),
                                static_cast<int>(line.size()), &wline[0], len);
        }
        onLine(wline);
    }
}

/// D2D1_COLOR_F → "RRGGBB"（每通道 0-255 取整）
static std::string ColorToHex(const D2D1_COLOR_F& c)
{
    char buf[8] = {0};
    snprintf(buf, sizeof(buf), "%02X%02X%02X",
             static_cast<int>(c.r * 255.0f + 0.5f),
             static_cast<int>(c.g * 255.0f + 0.5f),
             static_cast<int>(c.b * 255.0f + 0.5f));
    return buf;
}

/// 将宽字符串转 UTF-8（SaveConfig 写盘用）
static std::string WToUtf8(const std::wstring& w)
{
    if (w.empty()) {
        return std::string();
    }
    const int len = WideCharToMultiByte(CP_UTF8, 0, w.c_str(),
                                        static_cast<int>(w.size()),
                                        nullptr, 0, nullptr, nullptr);
    if (len <= 0) {
        return std::string();
    }
    std::string s(static_cast<size_t>(len), '\0');
    WideCharToMultiByte(CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
                        &s[0], len, nullptr, nullptr);
    return s;
}

/// 布尔转 "1"/"0"
static const char* BoolToStr(bool b) { return b ? "1" : "0"; }

ImeConfig LoadConfig(const std::wstring& dllDir)
{
    ImeConfig cfg;

    const std::wstring path = dllDir + L"config.ini";
    ReadConfigFile(path, [&cfg](const std::wstring& wline) {
        const std::wstring line = Trim(wline);
        if (line.empty() || line[0] == L'#') {
            return; // 空行/注释
        }

        const size_t eq = line.find(L'=');
        if (eq == std::wstring::npos) {
            return; // 无 =，忽略
        }

        const std::wstring key = Trim(line.substr(0, eq));
        const std::wstring value = Trim(line.substr(eq + 1));
        if (key.empty()) {
            return;
        }

        if (key == L"candidate_count") {
            // 非数字值忽略（保持默认）
            try {
                const int n = std::stoi(value);
                if (n >= 1 && n <= 20) {
                    cfg.candidate_count = n;
                }
            } catch (...) {
                // 忽略非法值
            }
        } else if (key == L"sort_mode") {
            // 候选排序模式（P0-2）：0=默认 1=单字优先 2=长词优先
            try {
                const int n = std::stoi(value);
                if (n >= 0 && n <= 2) {
                    cfg.sort_mode = n;
                }
            } catch (...) {
                // 忽略非法值
            }
        } else if (key == L"context_assoc") {
            // 上下文联想开关（P1-1）：1/true/on 开，0/false/off 关
            cfg.context_assoc = ParseBool(value, false);
        } else if (key == L"domain_dicts") {
            // 专业词库分类文件（逗号分隔多个；空 = 停用）
            cfg.domain_dicts = value;
        } else if (key == L"dict_path") {
            cfg.dict_path = value;
        } else if (key == L"user_dict_path") {
            // 用户词库路径（V0.2.2）；空值 = 用默认 %APPDATA%
            cfg.user_dict_path = value;
        } else if (key == L"fuzzy") {
            // 模糊音开关：1/true/on 开，0/false/off 关
            cfg.fuzzy_enabled = ParseBool(value, true);
        } else if (key == L"correction") {
            // 智能纠错开关（0.2.10）：1/true/on 开，0/false/off 关
            cfg.correction_enabled = ParseBool(value, true);
        } else if (key == L"mix_mode") {
            // 中英混输开关（0.2.8）：1/true/on 开，0/false/off 关
            cfg.mix_mode_enabled = ParseBool(value, true);
        } else if (key == L"traditional") {
            // 简繁转换开关（0.2.11）：1/true/on 开，0/false/off 关
            cfg.traditional_enabled = ParseBool(value, false);
        } else if (key == L"phrase") {
            // 快捷短语开关（0.2.12）：1/true/on 开，0/false/off 关
            cfg.phrase_enabled = ParseBool(value, true);
        } else if (key == L"phrase_path") {
            // 自定义短语文件（0.2.12，每行 code=text）
            cfg.phrase_path = value;
        } else if (key == L"theme_bg") {
            // 候选窗口主题：背景（V0.2.4，HEX RRGGBB）
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.bg = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_text") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.text = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_highlight") {
            // 兼容旧键名 → 选中候选背景（P0-1 改名 highlight→highlight_bg）
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.highlight_bg = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_dim") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.dim = c; cfg.userThemeExplicit = true; }
        } else if (key == L"shuangpin") {
            // 双拼模式开关（0.1.14）
            cfg.shuangpin_mode = ParseBool(value, false);
        } else if (key == L"shuangpin_scheme") {
            // 双拼方案（P2-7）：mspy/flypy/sogou/zrm/ziguang/jiajia
            std::string utf8;
            const int len = WideCharToMultiByte(CP_UTF8, 0, value.c_str(),
                                                static_cast<int>(value.size()),
                                                nullptr, 0, nullptr, nullptr);
            if (len > 0) {
                utf8.resize(static_cast<size_t>(len));
                WideCharToMultiByte(CP_UTF8, 0, value.c_str(),
                                    static_cast<int>(value.size()),
                                    &utf8[0], len, nullptr, nullptr);
            }
            if (!utf8.empty()) {
                cfg.shuangpin_scheme = utf8;
            }
        } else if (key == L"font_face") {
            // 候选窗字体名（V0.2.21）；空/非法忽略（保持默认）
            if (!value.empty()) {
                cfg.font_face = value;
            }
        } else if (key == L"font_size") {
            // 候选窗正文字号（V0.2.21，px，12-32，非法回退默认 16）
            try {
                const int n = std::stoi(value);
                if (n >= 12 && n <= 32) {
                    cfg.font_size = static_cast<float>(n);
                }
            } catch (...) {
                // 忽略非法值
            }
        } else if (key == L"inline_preedit") {
            // 行内预编辑（V0.2.18）：1=拼音写组合（候选窗不重复），0=候选窗画拼音行
            cfg.inline_preedit = ParseBool(value, true);
        } else if (key == L"ascii_punct") {
            // 中英标点开关（P0-2）：1=英文标点透传，0=中文标点全角化
            cfg.ascii_punct = ParseBool(value, false);
        } else if (key == L"emoji") {
            // Emoji 开关（P2-5）：1=开，0=关
            cfg.emoji_enabled = ParseBool(value, false);
        } else if (key == L"app_ascii") {
            // 应用级默认英文（P2-6 → V0.2.32 语义修正）：逗号分隔进程名（cod.exe,cmd.exe）
            // 首次进入该程序时初始英文；用户手动切换后保持（不再每次按键强制）
            std::wstringstream ss(value);
            std::wstring item;
            while (std::getline(ss, item, L',')) {
                item = Trim(item);
                std::transform(item.begin(), item.end(), item.begin(), ::towlower);
                if (!item.empty()) {
                    cfg.app_ascii_list.push_back(item);
                }
            }
        } else if (key == L"app_cn") {
            // 应用级默认中文（V0.2.32）：逗号分隔进程名（notepad.exe）
            std::wstringstream ss(value);
            std::wstring item;
            while (std::getline(ss, item, L',')) {
                item = Trim(item);
                std::transform(item.begin(), item.end(), item.begin(), ::towlower);
                if (!item.empty()) {
                    cfg.app_cn_list.push_back(item);
                }
            }
        } else if (key == L"app_inline") {
            // 应用级强制行内预编辑（V0.2.32，对标 weasel firefox inline_preedit）：逗号分隔进程名
            std::wstringstream ss(value);
            std::wstring item;
            while (std::getline(ss, item, L',')) {
                item = Trim(item);
                std::transform(item.begin(), item.end(), item.begin(), ::towlower);
                if (!item.empty()) {
                    cfg.app_inline_list.push_back(item);
                }
            }
        } else if (key == L"app_vim") {
            // 应用级 vim 模式（V0.2.36，对标 weasel app_options vim_mode）：逗号分隔进程名
            std::wstringstream ss(value);
            std::wstring item;
            while (std::getline(ss, item, L',')) {
                item = Trim(item);
                std::transform(item.begin(), item.end(), item.begin(), ::towlower);
                if (!item.empty()) {
                    cfg.app_vim_list.push_back(item);
                }
            }
        } else if (key == L"label_format") {
            // 候选标签格式（P0-1）：%d 数字 / %s 文本，如 "%d." "①" "%s、"
            if (!value.empty()) {
                cfg.label_format = value;
            }
        } else if (key == L"corner_radius") {
            // 窗口圆角（P0-1，1-16，非法回退 4）
            try {
                const int n = std::stoi(value);
                if (n >= 1 && n <= 16) { cfg.corner_radius = static_cast<float>(n); }
            } catch (...) {}
        } else if (key == L"hilite_corner_radius") {
            // 高亮块圆角（P0-1，1-16，非法回退 3）
            try {
                const int n = std::stoi(value);
                if (n >= 1 && n <= 16) { cfg.hilite_corner_radius = static_cast<float>(n); }
            } catch (...) {}
        } else if (key == L"padding") {
            // 窗口内边距（P0-1，0-20，非法回退 8）
            try {
                const int n = std::stoi(value);
                if (n >= 0 && n <= 20) { cfg.padding = n; }
            } catch (...) {}
        } else if (key == L"candidate_spacing") {
            // 候选间距（P0-1，0-40，非法回退 14）
            try {
                const int n = std::stoi(value);
                if (n >= 0 && n <= 40) { cfg.candidate_spacing = n; }
            } catch (...) {}
        } else if (key == L"theme_label") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.label = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_comment") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.comment = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_border") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.border = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_highlight_text") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.highlight_text = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_highlight_label") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.highlight_label = c; cfg.userThemeExplicit = true; }
        } else if (key == L"theme_mark") {
            D2D1_COLOR_F c;
            if (ParseHexColor(value, c)) { cfg.theme.mark = c; cfg.userThemeExplicit = true; }
        }
        // 未知 key 忽略（向前兼容）
    });

    return cfg;
}

std::wstring ResolveDictPath(const ImeConfig& cfg, const std::wstring& dllDir)
{
    if (cfg.dict_path.empty()) {
        return std::wstring(); // 内置词库
    }
    // 已是绝对路径（含盘符）则直接使用
    if (cfg.dict_path.size() >= 2 &&
        ((cfg.dict_path[0] >= L'A' && cfg.dict_path[0] <= L'Z') ||
         (cfg.dict_path[0] >= L'a' && cfg.dict_path[0] <= L'z')) &&
        cfg.dict_path[1] == L':') {
        return cfg.dict_path;
    }
    // 相对路径：以 DLL 目录为基准
    return dllDir + cfg.dict_path;
}

std::wstring ResolveUserDictPath(const ImeConfig& cfg, const std::wstring& dllDir)
{
    // 配置为空 → 默认 %APPDATA%/taishen-ime/user_dict.db
    if (cfg.user_dict_path.empty()) {
        wchar_t buf[MAX_PATH] = {0};
        if (SHGetFolderPathW(nullptr, CSIDL_APPDATA, nullptr, 0, buf) == S_OK) {
            std::wstring path = buf;
            path += L"\\taishen-ime";
            // 确保目录存在（引擎打开失败会静默降级，但主动创建更稳）
            CreateDirectoryW(path.c_str(), nullptr);
            return path + L"\\user_dict.db";
        }
        return std::wstring();
    }
    // 已是绝对路径（含盘符）则直接使用
    if (cfg.user_dict_path.size() >= 2 &&
        ((cfg.user_dict_path[0] >= L'A' && cfg.user_dict_path[0] <= L'Z') ||
         (cfg.user_dict_path[0] >= L'a' && cfg.user_dict_path[0] <= L'z')) &&
        cfg.user_dict_path[1] == L':') {
        return cfg.user_dict_path;
    }
    // 相对路径：以 DLL 目录为基准
    return dllDir + cfg.user_dict_path;
}

bool SaveConfig(const std::wstring& dllDir, const ImeConfig& cfg)
{
    const std::wstring path = dllDir + L"config.ini";
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    if (!file.is_open()) {
        return false;
    }
    auto line = [&file](const std::string& s) { file << s << "\n"; };

    line("# 泰深输入法配置（由设置窗口生成，可直接编辑）");
    line("# 修改保存后 2 秒内自动生效");
    line("");
    line("# 候选词数量上限（1-20，默认 9）");
    line("candidate_count=" + std::to_string(cfg.candidate_count));
    line("# 候选排序模式（0=默认 1=单字优先 2=长词优先）");
    line("sort_mode=" + std::to_string(cfg.sort_mode));
    line("# 上下文联想（前文搭配词前置，1=开 0=关）");
    line("context_assoc=" + std::string(BoolToStr(cfg.context_assoc)));
    line("# 专业词库分类（逗号分隔文件路径，空=停用）");
    line("domain_dicts=" + WToUtf8(cfg.domain_dicts));
    line("");
    line("# 系统词库路径（相对 DLL 目录或绝对路径；留空 = 内置词库）");
    line("dict_path=" + WToUtf8(cfg.dict_path));
    line("# 用户词库路径（留空 = 默认 %APPDATA%/taishen-ime/user_dict.db）");
    line("user_dict_path=" + WToUtf8(cfg.user_dict_path));
    line("");
    line("# 模糊音开关（1=开，0=关）");
    line("fuzzy=" + std::string(BoolToStr(cfg.fuzzy_enabled)));
    line("# 智能纠错开关");
    line("correction=" + std::string(BoolToStr(cfg.correction_enabled)));
    line("# 中英混输开关");
    line("mix_mode=" + std::string(BoolToStr(cfg.mix_mode_enabled)));
    line("# 简繁转换开关");
    line("traditional=" + std::string(BoolToStr(cfg.traditional_enabled)));
    line("# 快捷短语开关");
    line("phrase=" + std::string(BoolToStr(cfg.phrase_enabled)));
    line("# 自定义短语文件路径（留空 = 仅内置）");
    line("phrase_path=" + WToUtf8(cfg.phrase_path));
    line("# 双拼模式开关");
    line("shuangpin=" + std::string(BoolToStr(cfg.shuangpin_mode)));
    line("# 双拼方案（mspy/flypy/sogou/zrm/ziguang/jiajia）");
    line("shuangpin_scheme=" + cfg.shuangpin_scheme);
    line("# 候选窗字体名");
    line("font_face=" + WToUtf8(cfg.font_face));
    line("# 候选窗正文字号（px，12-32）");
    line("font_size=" + std::to_string(static_cast<int>(cfg.font_size)));
    line("# 行内预编辑开关");
    line("inline_preedit=" + std::string(BoolToStr(cfg.inline_preedit)));
    line("# 英文标点透传开关（1=英文标点，0=中文全角）");
    line("ascii_punct=" + std::string(BoolToStr(cfg.ascii_punct)));
    line("# Emoji 候选开关");
    line("emoji=" + std::string(BoolToStr(cfg.emoji_enabled)));
    line("# 应用级默认英文：逗号分隔进程名（小写，如 cod.exe,cmd.exe），首次进入该程序时初始英文");
    {
        std::string joined;
        for (size_t i = 0; i < cfg.app_ascii_list.size(); ++i) {
            if (i > 0) {
                joined += ",";
            }
            joined += WToUtf8(cfg.app_ascii_list[i]);
        }
        line("app_ascii=" + joined);
    }
    line("# 应用级默认中文：逗号分隔进程名（小写，如 notepad.exe）");
    {
        std::string joined;
        for (size_t i = 0; i < cfg.app_cn_list.size(); ++i) {
            if (i > 0) {
                joined += ",";
            }
            joined += WToUtf8(cfg.app_cn_list[i]);
        }
        line("app_cn=" + joined);
    }
    line("# 应用级强制行内预编辑：逗号分隔进程名（小写，如 firefox.exe）");
    {
        std::string joined;
        for (size_t i = 0; i < cfg.app_inline_list.size(); ++i) {
            if (i > 0) {
                joined += ",";
            }
            joined += WToUtf8(cfg.app_inline_list[i]);
        }
        line("app_inline=" + joined);
    }
    line("# 应用级 vim 模式：逗号分隔进程名（小写，如 nvim-qt.exe），Esc/Ctrl+C/Ctrl+[ 切英文并透传");
    {
        std::string joined;
        for (size_t i = 0; i < cfg.app_vim_list.size(); ++i) {
            if (i > 0) {
                joined += ",";
            }
            joined += WToUtf8(cfg.app_vim_list[i]);
        }
        line("app_vim=" + joined);
    }
    line("# 候选标签格式（%d = 数字，%s = 数字文本）");
    line("label_format=" + WToUtf8(cfg.label_format));
    line("# 候选窗口主题（HEX RRGGBB）");
    line("theme_bg=" + ColorToHex(cfg.theme.bg));
    line("theme_text=" + ColorToHex(cfg.theme.text));
    line("theme_label=" + ColorToHex(cfg.theme.label));
    line("theme_comment=" + ColorToHex(cfg.theme.comment));
    line("theme_border=" + ColorToHex(cfg.theme.border));
    line("theme_highlight=" + ColorToHex(cfg.theme.highlight_bg));
    line("theme_highlight_text=" + ColorToHex(cfg.theme.highlight_text));
    line("theme_highlight_label=" + ColorToHex(cfg.theme.highlight_label));
    line("theme_dim=" + ColorToHex(cfg.theme.dim));
    line("theme_mark=" + ColorToHex(cfg.theme.mark));
    line("# 窗口圆角（1-16）");
    line("corner_radius=" + std::to_string(static_cast<int>(cfg.corner_radius)));
    line("# 高亮块圆角（1-16）");
    line("hilite_corner_radius=" + std::to_string(static_cast<int>(cfg.hilite_corner_radius)));
    line("# 窗口内边距（0-20）");
    line("padding=" + std::to_string(cfg.padding));
    line("# 候选间距（0-40）");
    line("candidate_spacing=" + std::to_string(cfg.candidate_spacing));

    return true;
}

} // namespace taishen
