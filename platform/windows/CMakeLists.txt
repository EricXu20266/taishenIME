cmake_minimum_required(VERSION 3.20)
project(taishen_ime_tsf LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Rust 引擎静态库路径（第一期静态链接）
set(ENGINE_LIB_DIR "${CMAKE_SOURCE_DIR}/../../engine/target/release")
set(ENGINE_INCLUDE_DIR "${CMAKE_SOURCE_DIR}/include")

# IME DLL 源文件
set(SOURCES
    src/dllmain.cpp
    src/tsf_module.cpp
    src/engine_bridge.cpp
)

# 编译为 DLL
add_library(taishen_ime SHARED ${SOURCES})

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
)

# 输出目录
set_target_properties(taishen_ime PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
    ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_SOURCE_DIR}/out"
)
