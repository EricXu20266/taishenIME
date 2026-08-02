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
        wprintf(L"默认 candidate_count=%d (期望 9)\n", cfg.candidate_count);
        if (cfg.candidate_count != 9 || !cfg.dict_path.empty()) {
            wprintf(L"FAIL: 默认值错误\n");
            return 1;
        }
    }

    wprintf(L"ALL TESTS PASSED\n");
    return 0;
}
