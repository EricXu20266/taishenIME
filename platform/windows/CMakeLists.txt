cmake_minimum_required(VERSION 3.20)
project(taishen_ime_tsf LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# 源码为 UTF-8（含中文注释/字符串），强制 MSVC 按 UTF-8 解析
add_compile_options(/utf-8)

# 目标 Windows 10（GetDpiForWindow 等 API 需要 _WIN32_WINNT >= 0x0A00）
add_compile_definitions(_WIN32_WINNT=0x0A00 WINVER=0x0A00)

# rc.exe 编译 settings.rc 时定位 include/resource.h
if(MSVC)
    set(CMAKE_RC_FLAGS "${CMAKE_RC_FLAGS} /I${CMAKE_CURRENT_SOURCE_DIR}/include")
endif()

# Rust 引擎静态库路径（第一期静态链接）
set(ENGINE_LIB_DIR "${CMAKE_SOURCE_DIR}/../../engine/target/release")
set(ENGINE_INCLUDE_DIR "${CMAKE_SOURCE_DIR}/include")

# IME DLL 源文件
set(SOURCES
    src/dllmain.cpp
    src/tsf_module.cpp
    src/tsf_keyevent.cpp
    src/tsf_composition.cpp
    src/candidate_window.cpp
    src/banner_window.cpp
    src/config_reader.cpp
    src/settings_dialog.cpp
    src/theme.cpp
    src/engine_bridge.cpp
    src/app_state.cpp
    src/debug_log.cpp
    src/ui_theme.cpp
    src/ui_render.cpp
    src/ui_control.cpp
    src/ui_layout.cpp
    src/ui_window.cpp
    settings.rc
)

# 编译为 DLL
add_library(taishen_ime SHARED ${SOURCES})

# COM 导出函数通过 .def 文件导出（微软 TSF 示例标准做法）
target_sources(taishen_ime PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/taishen_ime.def)

target_include_directories(taishen_ime PRIVATE ${ENGINE_INCLUDE_DIR})

# 链接 Rust 引擎静态库
target_link_directories(taishen_ime PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(taishen_ime PRIVATE taishen_engine)

# Windows 特有链接
target_link_libraries(taishen_ime PRIVATE
    advapi32
    ole32
    oleaut32
    user32
    gdi32
    comctl32
    ws2_32
    ntdll
    bcrypt
    userenv
    d2d1
    dwrite
    dwmapi
)

# 输出目录
set_target_properties(taishen_ime PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
)

# ─── 冒烟测试（0.1.12 集成测试配套）───

# 链接 Rust 引擎静态库的测试（模拟 TSF 平台层调用 FFI）
set(ENGINE_SRC_FOR_TESTS
    src/tsf_keyevent.cpp
    src/engine_bridge.cpp
    src/config_reader.cpp
    src/app_state.cpp
    src/debug_log.cpp
)

# TSF COM 加载测试（验证 DLL 可被系统按 CLSID 实例化，防 0xc0000409 崩溃回归）
add_executable(test_tsf_load test/test_tsf_load.cpp)
target_include_directories(test_tsf_load PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_tsf_load PRIVATE taishen_ime ole32)

# 中英文切换冒烟测试
add_executable(test_ascii_mode test/test_ascii_mode.cpp ${ENGINE_SRC_FOR_TESTS})
target_include_directories(test_ascii_mode PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_ascii_mode PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_ascii_mode PRIVATE
    taishen_engine
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

# 配置读取冒烟测试
add_executable(test_config_reader test/test_config_reader.cpp src/config_reader.cpp)
target_include_directories(test_config_reader PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_config_reader PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_config_reader PRIVATE
    taishen_engine
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

# 状态横幅冒烟测试（0.1.26，V0.2.15）
add_executable(test_banner_window test/test_banner_window.cpp src/banner_window.cpp src/theme.cpp src/settings_dialog.cpp src/config_reader.cpp src/app_state.cpp src/tsf_keyevent.cpp src/debug_log.cpp)
target_include_directories(test_banner_window PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_banner_window PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_banner_window PRIVATE
    taishen_engine
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv shell32 comdlg32
)

# 设置对话框预览（独立 exe：弹窗验证 UI，不依赖 TSF 注册）
add_executable(test_settings_dialog test/test_settings_dialog.cpp src/settings_dialog.cpp src/config_reader.cpp src/debug_log.cpp settings.rc)
target_include_directories(test_settings_dialog PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_settings_dialog PRIVATE
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv shell32 comdlg32
)

# 窗体系统底座冒烟测试（0.3.0：主题/布局/命中/窗口生命周期/渲染）
add_executable(test_ui_framework test/test_ui_framework.cpp
    src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp src/ui_layout.cpp src/ui_window.cpp)
target_include_directories(test_ui_framework PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_ui_framework PRIVATE
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

foreach(t test_tsf_load test_ascii_mode test_config_reader test_banner_window test_settings_dialog test_ui_framework)
    set_target_properties(${t} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )
endforeach()
