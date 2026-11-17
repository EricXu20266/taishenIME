/// AppContainer 运行环境检测（阶段 1：资源 ACL + 运行期降级）
///
/// TSF 文本服务 DLL 会被加载进 AppContainer 进程（任务栏搜索、商店应用）。
/// 容器内 %APPDATA% / %LOCALAPPDATA% 被重定向到包内虚拟目录，安装目录下的
/// 词库与配置文件默认不可读，用户词库不可写。这些失败是静默的（engine_init
/// 恒返回 0），所以必须在平台层先判断环境，再决定降级哪些功能。
///
/// 进程属性在生命周期内不变，检测一次（magic static 线程安全）。
#pragma once

#include <windows.h>

namespace taishen {

inline bool IsAppContainerProcess()
{
    static const bool s_isContainer = []() -> bool {
        BOOL isContainer = FALSE;
        DWORD returned = 0;
        if (!GetTokenInformation(GetCurrentProcessToken(), TokenIsAppContainer,
                                 &isContainer, sizeof(isContainer), &returned)) {
            // 查询失败按非容器处理：宁走正常路径，也不因检测失败误禁用功能
            return false;
        }
        return isContainer != FALSE;
    }();
    return s_isContainer;
}

} // namespace taishen
