/// TSF 组件与语言栏项 GUID — 唯一定义源
///
/// 三个 GUID 在 tsf_module.cpp（注册/激活）与 langbar_item.cpp（语言栏按钮）
/// 之间共享，避免同一 CLSID 出现两份手工维护的字面量。

#pragma once

#include <windows.h>

// CLSID_TAISHEN_IME: Text Service 组件标识
// {7D77E4AA-276E-4582-B952-94B6EFAADA28}
static const CLSID CLSID_TAISHEN_IME = {
    0x7D77E4AA, 0x276E, 0x4582,
    {0xB9, 0x52, 0x94, 0xB6, 0xEF, 0xAA, 0xDA, 0x28}};

// GUID_LANGPROFILE: 语言配置文件标识（中文 zh-CN 0x0804）
// {882067BC-3F50-4904-ADC9-282C8F0CF91E}
static const GUID GUID_LANGPROFILE = {
    0x882067BC, 0x3F50, 0x4904,
    {0xAD, 0xC9, 0x28, 0x2C, 0x8F, 0x0C, 0xF9, 0x1E}};

// GUID_LANGBARITEM: 语言栏按钮项标识（输入指示器图标宿主）
// {5B36A0B4-0C11-4B77-9E0A-D3F2C41A8B77}
static const GUID GUID_LANGBARITEM = {
    0x5B36A0B4, 0x0C11, 0x4B77,
    {0x9E, 0x0A, 0xD3, 0xF2, 0xC4, 0x1A, 0x8B, 0x77}};

// GUID_LANGBARITEM_MODE: 中英切换按钮项标识
// {C7E24A6D-5F19-4E8B-9D3A-6B2F8C1E4A90}
static const GUID GUID_LANGBARITEM_MODE = {
    0xC7E24A6D, 0x5F19, 0x4E8B,
    {0x9D, 0x3A, 0x6B, 0x2F, 0x8C, 0x1E, 0x4A, 0x90}};
