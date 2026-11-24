cmake_minimum_required(VERSION 3.20)

# 静态链接 MSVC CRT（/MT）—— 必须在 project() 之前启用 CMP0091，
# 否则 CMAKE_MSVC_RUNTIME_LIBRARY 变量被忽略。
#
# 为什么必须静态（2026-11-17 QQ 崩溃事故）：
#   宿主进程可能自带旧版 MSVCP140.dll（QQ 9.9.16 在 resources\app 下放了
#   14.29.30154），Windows 加载器按 basename 复用「已加载模块」，
#   本 DLL 用 14.5x 编译的代码会被强行跑在旧版 C++ 标准库上 → 崩溃在
#   MSVCP140.dll。静态 CRT 让本 DLL 不导入 MSVCP140/VCRUNTIME140，
#   从根上消除进程内 CRT 版本冲突（微信/网页/游戏不崩是因为它们没带旧版）。
#   Rust 侧同步配置见仓库根 .cargo/config.toml（crt-static）。
if(POLICY CMP0091)
    cmake_policy(SET CMP0091 NEW)
endif()

project(taishen_ime_tsf LANGUAGES CXX)

set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# 源码为 UTF-8（含中文注释/字符串），强制 MSVC 按 UTF-8 解析
add_compile_options(/utf-8 /EHa)

# 目标 Windows 10（GetDpiForWindow 等 API 需要 _WIN32_WINNT >= 0x0A00）
add_compile_definitions(_WIN32_WINNT=0x0A00 WINVER=0x0A00)

# rc.exe 编译 settings.rc 时定位 include/resource.h
if(MSVC)
    set(CMAKE_RC_FLAGS "${CMAKE_RC_FLAGS} /I${CMAKE_CURRENT_SOURCE_DIR}/include")
    add_compile_options(/guard:cf /Zi)
    add_link_options(/GUARD:CF /DEBUG /OPT:REF /OPT:ICF)
    if(CMAKE_SIZEOF_VOID_P EQUAL 8)
        add_link_options(/CETCOMPAT)
    endif()
endif()

# Rust 引擎静态库路径（第一期静态链接）
# 按目标架构选库：32 位宿主进程只能加载 32 位 TSF DLL，
# 因此 32 位构建必须链接 i686 目标产出的静态库。
if(CMAKE_SIZEOF_VOID_P EQUAL 4)
    set(ENGINE_TARGET_SUBDIR "i686-pc-windows-msvc/release")
else()
    set(ENGINE_TARGET_SUBDIR "release")
endif()
set(ENGINE_LIB_DIR "${CMAKE_SOURCE_DIR}/../../engine/target/${ENGINE_TARGET_SUBDIR}")
set(ENGINE_INCLUDE_DIR "${CMAKE_SOURCE_DIR}/include")

# 防呆：静态库必须不早于 engine/src 下任一库源码（排除 src/bin 独立 bin target）。
# C++ 与 Rust 的 FFI 契约同步演进（如 engine_text.h 的两阶段容量协议），
# 旧静态库 + 新 C++ 会让选词静默失效（查询阶段消费候选，读取阶段拿到空）。
file(GLOB_RECURSE ENGINE_RUST_SOURCES
    "${CMAKE_SOURCE_DIR}/../../engine/src/*.rs")
list(FILTER ENGINE_RUST_SOURCES EXCLUDE REGEX "/src/bin/")
set(ENGINE_LIB_FILE "${ENGINE_LIB_DIR}/taishen_engine.lib")
if(EXISTS "${ENGINE_LIB_FILE}")
    file(TIMESTAMP "${ENGINE_LIB_FILE}" _engine_lib_ts "%Y%m%d%H%M%S")
    foreach(_rust_src ${ENGINE_RUST_SOURCES})
        if(EXISTS "${_rust_src}")
            file(TIMESTAMP "${_rust_src}" _rust_src_ts "%Y%m%d%H%M%S")
            if(_rust_src_ts GREATER _engine_lib_ts)
                message(FATAL_ERROR
                    "Rust 引擎静态库早于源码：${_rust_src}\n"
                    "先执行 cargo build --release --manifest-path engine/Cargo.toml")
            endif()
        endif()
    endforeach()
endif()

# ─── 版本资源（签名服务要求 ProductName / ProductVersion 存在且每次构建一致）───
# 版本号唯一来源：engine/Cargo.toml 的 version 字段（package.ps1 用同一来源）
file(STRINGS "${CMAKE_SOURCE_DIR}/../../engine/Cargo.toml" TAISHEN_VER_LINE
     REGEX "^version[ \t]*=")
if(TAISHEN_VER_LINE MATCHES "\"([0-9]+)\\.([0-9]+)\\.([0-9]+)\"")
    set(TAISHEN_VER_MAJOR "${CMAKE_MATCH_1}")
    set(TAISHEN_VER_MINOR "${CMAKE_MATCH_2}")
    set(TAISHEN_VER_PATCH "${CMAKE_MATCH_3}")
else()
    message(WARNING "无法从 engine/Cargo.toml 解析版本号，版本资源回落 0.0.0")
    set(TAISHEN_VER_MAJOR "0")
    set(TAISHEN_VER_MINOR "0")
    set(TAISHEN_VER_PATCH "0")
endif()
set(TAISHEN_VERSION_STR "${TAISHEN_VER_MAJOR}.${TAISHEN_VER_MINOR}.${TAISHEN_VER_PATCH}")
set(TAISHEN_VERSION_DOTTED "${TAISHEN_VERSION_STR}.0")
set(TAISHEN_VERSION_COMMA "${TAISHEN_VER_MAJOR},${TAISHEN_VER_MINOR},${TAISHEN_VER_PATCH},0")

if(CMAKE_SIZEOF_VOID_P EQUAL 4)
    set(_taishen_dll_suffix "_x86")
else()
    set(_taishen_dll_suffix "")
endif()

# IMM32 兼容层版本资源
set(TAISHEN_FILETYPE "0x2L")
set(TAISHEN_FILE_DESC "Taishen IME IMM32 Compatibility Layer")
set(TAISHEN_INTERNAL_NAME "taishen_ime_imm32")
set(TAISHEN_ORIGINAL_FILENAME "taishen_ime_imm32${_taishen_dll_suffix}.ime")
configure_file("${CMAKE_CURRENT_SOURCE_DIR}/res/taishen_version.rc.in"
               "${CMAKE_CURRENT_BINARY_DIR}/taishen_version_imm32.rc" @ONLY)

# 候选窗服务版本资源
set(TAISHEN_FILETYPE "0x1L")
set(TAISHEN_FILE_DESC "Taishen IME Candidate Window Service")
set(TAISHEN_INTERNAL_NAME "taishen_server")
set(TAISHEN_ORIGINAL_FILENAME "taishen_server.exe")
configure_file("${CMAKE_CURRENT_SOURCE_DIR}/res/taishen_version.rc.in"
               "${CMAKE_CURRENT_BINARY_DIR}/taishen_version_server.rc" @ONLY)

# taishen_ime.dll 的版本变量（供下方 taishen_ime.rc.in 的 configure_file 使用）
set(TAISHEN_FILETYPE "0x2L")
set(TAISHEN_FILE_DESC "Taishen IME TSF Text Service")
set(TAISHEN_INTERNAL_NAME "taishen_ime")
set(TAISHEN_ORIGINAL_FILENAME "taishen_ime${_taishen_dll_suffix}.dll")

# DLL 图标资源：语言栏 / 输入指示器图标（IDI_APP_ICON）
# 图标路径以绝对路径注入 .rc，规避 rc.exe 相对路径解析差异
set(TAISHEN_ICON_FILE "${CMAKE_CURRENT_SOURCE_DIR}/res/taishen.ico")
set(TAISHEN_ZH_ICON "${CMAKE_CURRENT_SOURCE_DIR}/res/taishen_zh.ico")
set(TAISHEN_EN_ICON "${CMAKE_CURRENT_SOURCE_DIR}/res/taishen_en.ico")
configure_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/res/taishen_ime.rc.in"
    "${CMAKE_CURRENT_BINARY_DIR}/taishen_ime.rc"
    @ONLY)
# 图标本身也是 .rc 的依赖：configure_file 只跟踪 .rc.in，.ico 变更不会触发
# 重新编译资源，会让 DLL 留着旧图标
set_source_files_properties("${CMAKE_CURRENT_BINARY_DIR}/taishen_ime.rc"
    PROPERTIES OBJECT_DEPENDS
    "${TAISHEN_ICON_FILE};${TAISHEN_ZH_ICON};${TAISHEN_EN_ICON}")

# IME DLL 源文件（V4.2：tsf_module 拆分为 5 模块，继续保持无引擎/D2D/UI/语音链接）
set(SOURCES
    src/dllmain.cpp
    src/tsf_text_service.cpp
    src/tsf_key_sink.cpp
    src/tsf_edit_session.cpp
    src/tsf_langbar.cpp
    src/tsf_focus_caret.cpp
    src/tsf_keyevent.cpp
    src/tsf_composition.cpp
    src/display_attributes.cpp
    src/config_reader.cpp
    src/app_state.cpp
    src/debug_log.cpp
    src/dpi_util.cpp
    src/langbar_item.cpp
    src/ipc_render_client.cpp
    # DLL 图标资源（语言栏/输入指示器 IDI_APP_ICON）
    ${CMAKE_CURRENT_BINARY_DIR}/taishen_ime.rc
)

# 编译为 DLL
add_library(taishen_ime SHARED ${SOURCES})

# COM 导出函数通过 .def 文件导出（微软 TSF 示例标准做法）
target_sources(taishen_ime PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/taishen_ime.def)

target_include_directories(taishen_ime PRIVATE ${ENGINE_INCLUDE_DIR})
target_compile_definitions(taishen_ime PRIVATE TAISHEN_THIN_DLL=1)

# Windows 特有链接（V3.1 已摘除 taishen_engine / d2d1 / dwrite / dwmapi / ws2_32）
target_link_libraries(taishen_ime PRIVATE
    advapi32
    shell32
    ole32
    oleaut32
    user32
    gdi32
    comctl32
)

# 输出目录
set_target_properties(taishen_ime PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
)

# ─── IMM32 兼容层 DLL（V0.6：老游戏/老应用适配；V3.2 迁入 Server IPC 并同步摘链接）───
# 独立 DLL（仿 weasel.ime），注册为 Keyboard Layouts IMM32 IME。
# 自身只保留 ImeProcessKey / ImeToAsciiEx / ImeConversionList / 组合串上屏，引擎与渲染走 taishen_server.exe。

set(IMM32_SOURCES
    src/imm32/imm32_ime.cpp
    src/config_reader.cpp
    src/app_state.cpp
    src/tsf_keyevent.cpp
    src/debug_log.cpp
    src/dpi_util.cpp
    src/ipc_render_client.cpp
)

add_library(taishen_ime_imm32 SHARED ${IMM32_SOURCES})

# IMM32 IME 导出函数通过 .def 文件导出
target_sources(taishen_ime_imm32 PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/taishen_ime_imm32.def)

# src/ 下的头文件（debug_log.h 等）按引号 include 直接查找
target_include_directories(taishen_ime_imm32 PRIVATE
    ${ENGINE_INCLUDE_DIR}
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)
target_compile_definitions(taishen_ime_imm32 PRIVATE TAISHEN_THIN_DLL=1)

target_link_libraries(taishen_ime_imm32 PRIVATE
    imm32
    advapi32
    shell32
    ole32
    oleaut32
    user32
    gdi32
)

# 输出 .ime（IMM32 IME 标准扩展名）
set_target_properties(taishen_ime_imm32 PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    PREFIX ""
    SUFFIX ".ime"
)

# ─── 冒烟测试与仅 64 位服务端目标（0.1.12 集成测试配套，避免 32 位构建覆盖 64 位测试）───
if(CMAKE_SIZEOF_VOID_P EQUAL 8)

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
# debug_log.cpp：config_reader 在 AppContainer 降级路径里记日志（阶段 1）
add_executable(test_config_reader test/test_config_reader.cpp src/config_reader.cpp src/debug_log.cpp)
target_include_directories(test_config_reader PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_config_reader PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_config_reader PRIVATE
    taishen_engine
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

set(SETTINGS_UI_SOURCES
    src/settings_window.cpp
    src/settings_fuzzy.cpp
    src/settings_usability.cpp
    src/settings_dict_tab.cpp
    src/process_picker.cpp
)

# 状态横幅冒烟测试（0.1.26，V0.2.15；V0.3.3 迁移后依赖窗体系统）
add_executable(test_banner_window test/test_banner_window.cpp src/banner_window.cpp src/theme.cpp ${SETTINGS_UI_SOURCES} src/config_reader.cpp src/app_state.cpp src/tsf_keyevent.cpp src/debug_log.cpp src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp src/ui_layout.cpp src/ui_window.cpp src/ui_label.cpp src/ui_button.cpp src/ui_checkbox.cpp src/ui_edit.cpp src/ui_combobox.cpp src/ui_colorpicker.cpp src/audio_capture.cpp src/voice_manager.cpp)
target_include_directories(test_banner_window PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_banner_window PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_banner_window PRIVATE
    taishen_engine
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv shell32 comdlg32 d2d1 dwrite crypt32 avrt
)

# 设置窗体预览（独立 exe：弹窗验证 UI，不依赖 TSF 注册；V0.3.4 自绘窗体）
add_executable(test_settings_dialog test/test_settings_dialog.cpp ${SETTINGS_UI_SOURCES} src/config_reader.cpp src/debug_log.cpp src/theme.cpp src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp src/ui_layout.cpp src/ui_window.cpp src/ui_label.cpp src/ui_button.cpp src/ui_checkbox.cpp src/ui_edit.cpp src/ui_combobox.cpp src/ui_colorpicker.cpp src/audio_capture.cpp src/voice_manager.cpp)
target_include_directories(test_settings_dialog PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_directories(test_settings_dialog PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_settings_dialog PRIVATE
    taishen_engine
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv shell32 comdlg32 crypt32 avrt
)

# IMM32 兼容层加载冒烟测试（0.6.x：导出符号 + 输入链验证，虚拟 HIMC）
add_executable(test_imm32_load test/test_imm32_load.cpp)
target_include_directories(test_imm32_load PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_imm32_load PRIVATE imm32 ole32 user32 advapi32)

# V0.5 语音输入冒烟测试（WAV 编码 + VAD 状态机 + 泰深检测 + 候选注入 + config 往返）
add_executable(test_voice test/test_voice.cpp
    src/audio_capture.cpp src/voice_manager.cpp src/config_reader.cpp src/debug_log.cpp)
target_include_directories(test_voice PRIVATE ${ENGINE_INCLUDE_DIR} ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_directories(test_voice PRIVATE ${ENGINE_LIB_DIR})
target_link_libraries(test_voice PRIVATE
    taishen_engine
    advapi32 ole32 oleaut32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv d2d1 dwrite shell32 avrt
)

# 窗体系统底座冒烟测试（0.3.0：主题/布局/命中/窗口生命周期/渲染）
add_executable(test_ui_framework test/test_ui_framework.cpp
    src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp src/ui_layout.cpp src/ui_window.cpp)
target_include_directories(test_ui_framework PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_ui_framework PRIVATE
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

# 控件库冒烟测试（0.3.1：Label/Button/CheckBox/Edit/ComboBox/Tab/ColorSwatch/ScrollBar）
add_executable(test_ui_controls test/test_ui_controls.cpp
    src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp src/ui_layout.cpp src/ui_window.cpp
    src/ui_label.cpp src/ui_button.cpp src/ui_checkbox.cpp src/ui_edit.cpp src/ui_combobox.cpp
    src/ui_tab.cpp src/ui_colorpicker.cpp src/ui_scrollbar.cpp)
target_include_directories(test_ui_controls PRIVATE ${ENGINE_INCLUDE_DIR})
target_link_libraries(test_ui_controls PRIVATE
    d2d1 dwrite
    advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv
)

# ─── V4 D2/D3 进程外引擎与渲染服务（taishen_server.exe，仅 64 位构建一份）───
# 内嵌 Rust 引擎静态库（启动加载词库、处理 KeyInput/Commit）+ 候选窗渲染，供所有 32/64 位及 AppContainer 宿主共用。
    add_executable(taishen_server WIN32
        src/taishen_server.cpp
        src/candidate_window.cpp
        ${SETTINGS_UI_SOURCES}
        src/banner_window.cpp
        src/audio_capture.cpp
        src/voice_manager.cpp
        src/app_state.cpp
        src/config_reader.cpp
        src/debug_log.cpp
        src/dpi_util.cpp
        src/theme.cpp
        src/ui_theme.cpp
        src/ui_render.cpp
        src/ui_control.cpp
        src/ui_layout.cpp
        src/ui_window.cpp
        src/ui_label.cpp
        src/ui_button.cpp
        src/ui_checkbox.cpp
        src/ui_edit.cpp
        src/ui_combobox.cpp
        src/ui_tab.cpp
        src/ui_colorpicker.cpp
        src/ui_scrollbar.cpp
    )
    target_include_directories(taishen_server PRIVATE ${ENGINE_INCLUDE_DIR})
    target_link_directories(taishen_server PRIVATE ${ENGINE_LIB_DIR})
    target_link_libraries(taishen_server PRIVATE
        taishen_engine
        d2d1 dwrite dwmapi
        advapi32 shell32 crypt32 ole32 oleaut32 user32 gdi32 comctl32 comdlg32 avrt
        ws2_32 ntdll bcrypt userenv
    )
    set_target_properties(taishen_server PROPERTIES
        LINK_DEPENDS "${ENGINE_LIB_FILE}"
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )

    # V4 D2/D3 进程外渲染与引擎 IPC 冒烟测试
    add_executable(test_render_ipc test/test_render_ipc.cpp
        src/ipc_render_client.cpp src/debug_log.cpp)
    target_include_directories(test_render_ipc PRIVATE ${ENGINE_INCLUDE_DIR})
    target_link_libraries(test_render_ipc PRIVATE advapi32 shell32 user32)
    set_target_properties(test_render_ipc PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )

    # Q2.2 无头易用性冒烟测试（竖排候选几何命中 / 进程拾取器 / 用户词库 FFI）
    add_executable(test_q22_usability test/test_q22_usability.cpp
        src/candidate_window.cpp src/config_reader.cpp src/process_picker.cpp src/debug_log.cpp
        src/theme.cpp src/ui_theme.cpp src/ui_render.cpp src/ui_control.cpp
        src/ui_layout.cpp src/ui_window.cpp)
    target_include_directories(test_q22_usability PRIVATE ${ENGINE_INCLUDE_DIR})
    target_link_directories(test_q22_usability PRIVATE ${ENGINE_LIB_DIR})
    target_link_libraries(test_q22_usability PRIVATE
        taishen_engine
        d2d1 dwrite
        advapi32 ole32 user32 gdi32 comctl32 ws2_32 ntdll bcrypt userenv shell32
    )

foreach(t test_tsf_load test_ascii_mode test_config_reader test_banner_window test_settings_dialog test_ui_framework test_ui_controls test_imm32_load test_voice test_q22_usability)
    set_target_properties(${t} PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )
endforeach()

endif()

# 32 位构建：产文件名加 _x86 后缀，与 64 位产物并存。
# 32 位宿主进程（战网等）的 TSF 从 WOW6432Node 读注册表、加载 32 位 InprocServer，
# 两个位数必须各有一份，否则 32 位应用里输入法根本不出现。
if(CMAKE_SIZEOF_VOID_P EQUAL 4)
    set_target_properties(taishen_ime PROPERTIES OUTPUT_NAME "taishen_ime_x86")
    set_target_properties(taishen_ime_imm32 PROPERTIES OUTPUT_NAME "taishen_ime_imm32_x86")

    # 32 位客户端连 64 位 taishen_server.exe 跨位数 IPC 冒烟测试
    add_executable(test_render_ipc_x86 test/test_render_ipc.cpp
        src/ipc_render_client.cpp src/debug_log.cpp)
    target_include_directories(test_render_ipc_x86 PRIVATE ${ENGINE_INCLUDE_DIR})
    target_link_libraries(test_render_ipc_x86 PRIVATE advapi32 shell32 user32)
    set_target_properties(test_render_ipc_x86 PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )

    # 32 位 IMM32 兼容层加载冒烟测试（验证 taishen_ime_imm32_x86.ime 跨位数 IPC 输入链）
    add_executable(test_imm32_load_x86 test/test_imm32_load.cpp)
    target_include_directories(test_imm32_load_x86 PRIVATE ${ENGINE_INCLUDE_DIR})
    target_link_libraries(test_imm32_load_x86 PRIVATE imm32 ole32 user32 advapi32)
    set_target_properties(test_imm32_load_x86 PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    )
endif()

# ─── 版本资源附加（IMM32 兼容层与候选窗服务）───
# taishen_ime 的版本块写在 taishen_ime.rc.in 里（与图标共用一份 rc），
# 这两个目标没有图标资源，单独挂 configure_file 生成的版本资源。
# 缺少 VERSIONINFO 会导致 SignPath 等签名服务的元数据校验失败。
target_sources(taishen_ime_imm32 PRIVATE
    "${CMAKE_CURRENT_BINARY_DIR}/taishen_version_imm32.rc")
if(TARGET taishen_server)
    target_sources(taishen_server PRIVATE
        "${CMAKE_CURRENT_BINARY_DIR}/taishen_version_server.rc")
endif()
