/// 配置读取冒烟测试 — 独立 exe
///
/// 验证 LoadConfig 能正确解析 config.ini（候选数、词库路径）。
/// 返回 0 = 通过。

#include <windows.h>
#include <cstdio>
#include <string>

#include "config_reader.h"

int wmain()
{
    // 模拟 DLL 目录（带尾分隔符）—— 使用当前 exe 所在目录
    wchar_t exePath[MAX_PATH] = {0};
    GetModuleFileNameW(nullptr, exePath, MAX_PATH);
    std::wstring dllDir(exePath);
    const size_t slash = dllDir.find_last_of(L"\\/");
    dllDir = dllDir.substr(0, slash + 1);

    // 测试 1：正常配置
    {
        // 写一个临时配置
        const std::wstring path = dllDir + L"config.ini";
        FILE* f = _wfopen(path.c_str(), L"wb");
        if (f != nullptr) {
            const char* content =
                "# 泰深输入法配置\n"
                "candidate_count=5\n"
                "dict_path=system_dict.db\n"
                "unknown_key=ignore_me\n";
            fwrite(content, 1, strlen(content), f);
            fclose(f);
        }

        const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        wprintf(L"candidate_count=%d (期望 5)\n", cfg.candidate_count);
        wprintf(L"dict_path=%ls (期望 system_dict.db)\n", cfg.dict_path.c_str());

        if (cfg.candidate_count != 5) {
            wprintf(L"FAIL: candidate_count 解析错误\n");
            return 1;
        }
        if (cfg.dict_path != L"system_dict.db") {
            wprintf(L"FAIL: dict_path 解析错误\n");
            return 1;
        }

        // 路径解析测试
        const std::wstring abs = taishen::ResolveDictPath(cfg, dllDir);
        wprintf(L"resolved=%ls (期望 %ls)\n", abs.c_str(),
                (dllDir + L"system_dict.db").c_str());
        if (abs != dllDir + L"system_dict.db") {
            wprintf(L"FAIL: 路径解析错误\n");
            return 1;
        }
    }

    // 测试 2：配置缺失 → 默认值
    {
        const std::wstring missingDir = L"C:\\test\\missing\\";
        const taishen::ImeConfig cfg = taishen::LoadConfig(missingDir);
        wprintf(L"默认 candidate_count=%d (期望 5)\n", cfg.candidate_count);
        if (cfg.candidate_count != 5 || !cfg.dict_path.empty()) {
            wprintf(L"FAIL: 默认值错误\n");
            return 1;
        }
        // 默认主题 = 深色（V0.2.4）
        const taishen::CandidateTheme def = taishen::CandidateTheme::Default();
        if (cfg.theme.bg.r != def.bg.r || cfg.theme.bg.g != def.bg.g ||
            cfg.theme.bg.b != def.bg.b) {
            wprintf(L"FAIL: 默认主题背景色错误\n");
            return 1;
        }
        wprintf(L"默认主题 OK (bg=0x%02X%02X%02X)\n",
                static_cast<int>(cfg.theme.bg.r * 255),
                static_cast<int>(cfg.theme.bg.g * 255),
                static_cast<int>(cfg.theme.bg.b * 255));
    }

    // 测试 3：主题配置解析（V0.2.4）
    {
        const std::wstring path = dllDir + L"config.ini";
        FILE* f = _wfopen(path.c_str(), L"wb");
        if (f != nullptr) {
            const char* content =
                "theme_bg=F5F5F5\n"
                "theme_text=333333\n"
                "theme_highlight=0078D4\n"
                "theme_dim=999999\n"
                "theme_bad=XYZ\n";  // 非法 HEX → 回退默认
            fwrite(content, 1, strlen(content), f);
            fclose(f);
        }
        const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        const int r = static_cast<int>(cfg.theme.bg.r * 255);
        const int g = static_cast<int>(cfg.theme.bg.g * 255);
        const int b = static_cast<int>(cfg.theme.bg.b * 255);
        wprintf(L"theme_bg=%02X%02X%02X (期望 F5F5F5)\n", r, g, b);
        if (r != 0xF5 || g != 0xF5 || b != 0xF5) {
            wprintf(L"FAIL: theme_bg 解析错误\n");
            return 1;
        }
        const int tr = static_cast<int>(cfg.theme.text.r * 255);
        const int tg = static_cast<int>(cfg.theme.text.g * 255);
        const int tb = static_cast<int>(cfg.theme.text.b * 255);
        if (tr != 0x33 || tg != 0x33 || tb != 0x33) {
            wprintf(L"FAIL: theme_text 解析错误\n");
            return 1;
        }
        wprintf(L"主题解析 OK\n");
    }

    // 测试 4：应用级配置解析（V0.2.32 app_ascii/app_cn/app_inline）
    {
        const std::wstring path = dllDir + L"config.ini";
        FILE* f = _wfopen(path.c_str(), L"wb");
        if (f != nullptr) {
            const char* content =
                "app_ascii=cod.exe, CMD.EXE\n"
                "app_cn=notepad.exe\n"
                "app_inline=firefox.exe\n"
                "app_ascii=wechat.exe\n";  // 重复 key → 追加
            fwrite(content, 1, strlen(content), f);
            fclose(f);
        }
        const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        wprintf(L"app_ascii 数=%zu (期望 3)\n", cfg.app_ascii_list.size());
        if (cfg.app_ascii_list.size() != 3 ||
            cfg.app_ascii_list[0] != L"cod.exe" ||
            cfg.app_ascii_list[1] != L"cmd.exe" ||   // 大小写归一
            cfg.app_ascii_list[2] != L"wechat.exe") {
            wprintf(L"FAIL: app_ascii 解析错误\n");
            return 1;
        }
        wprintf(L"app_cn 数=%zu (期望 1)\n", cfg.app_cn_list.size());
        if (cfg.app_cn_list.size() != 1 || cfg.app_cn_list[0] != L"notepad.exe") {
            wprintf(L"FAIL: app_cn 解析错误\n");
            return 1;
        }
        wprintf(L"app_inline 数=%zu (期望 1)\n", cfg.app_inline_list.size());
        if (cfg.app_inline_list.size() != 1 || cfg.app_inline_list[0] != L"firefox.exe") {
            wprintf(L"FAIL: app_inline 解析错误\n");
            return 1;
        }
        wprintf(L"应用级配置解析 OK\n");
    }

    wprintf(L"ALL TESTS PASSED\n");
    return 0;
}
