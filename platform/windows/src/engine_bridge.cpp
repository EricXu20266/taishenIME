/// Windows TSF 平台层 — 引擎桥接实现
///
/// 负责加载 Rust 引擎 DLL 并调用其 C FFI 接口。
/// 第一期 MVP：直接静态链接 Rust staticlib。

#include "engine_bridge.h"

// 第一期 MVP 直接链接 Rust staticlib 中的符号
// 后续版本可改为动态加载 DLL（LoadLibrary）

extern "C" {
    // 这些符号由 Rust engine 编译为 staticlib 时导出
    // 链接时 CMake 会自动找到 libtaishen_engine.a
}

// 如果采用动态链接方式，在此封装 LoadLibrary/GetProcAddress
// 第一期先静态链接，保持简单
