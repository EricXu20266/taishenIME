/// 配置读取 — 实现
///
/// 对应 SPEC: docs/modules/config-system/SPEC.md
/// 格式：
///   # 注释
///   candidate_count=9
///   dict_path=system_dict.db

#include "config_reader.h"

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
        } else if (key == L"dict_path") {
            cfg.dict_path = value;
        } else if (key == L"user_dict_path") {
            // 用户词库路径（V0.2.2）；空值 = 用默认 %APPDATA%
            cfg.user_dict_path = value;
        } else if (key == L"fuzzy") {
            // 模糊音开关：1/true/on 开，0/false/off 关
            cfg.fuzzy_enabled = ParseBool(value, true);
        } else if (key == L"shuangpin") {
            // 双拼模式开关（0.1.14）
            cfg.shuangpin_mode = ParseBool(value, false);
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

} // namespace taishen
