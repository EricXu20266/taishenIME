/// TSF 契约纯逻辑辅助（Q3.1，无状态/易单测）
///
/// 覆盖：
///   - ITfInputScope 密码/PIN/数字控件自动切英文判据
///   - ITfCompartment（OPENCLOSE / INPUTMODE_CONVERSION）双向映射
///   - ActivateEx dwFlags 语义位判定
///   - ITfFnReconversion 选区文本可转换性判定

#pragma once

#include <windows.h>
#include <inputscope.h>
#include <msctf.h>

#include <string>

namespace taishen {

/// GUID_PROP_INPUTSCOPE（<inputscope.h>:200，内联定义避免跨库 GUID 符号缺失）
inline constexpr GUID kGuidPropInputScope = {
    0x1713dd5a, 0x68e7, 0x4a5b, {0x9a, 0xf6, 0x59, 0x2a, 0x59, 0x5c, 0x77, 0x8d}};

/// InputScope 是否要求强制切英文直通（密码框、PIN 框、纯数字/邮箱字段）。
/// 显式保留 IS_DEFAULT / IS_URL / IS_SEARCH / IS_CHINESE_FULLWIDTH 可中文输入（浏览器地址栏/搜索框可用）。
inline bool IsAsciiRequiredInputScope(InputScope scope)
{
    switch (scope) {
    case IS_PASSWORD:
    case IS_NUMERIC_PASSWORD:
    case IS_ALPHANUMERIC_PIN:
    case IS_ALPHANUMERIC_PIN_SET:
    case IS_ALPHANUMERIC_HALFWIDTH:
    case IS_EMAIL_SMTPEMAILADDRESS:
    case IS_DIGITS:
    case IS_NUMBER:
        return true;
    default:
        return false;
    }
}

struct ConversionState {
    bool asciiMode = false;
    bool fullWidth = false;
    bool asciiPunct = false;
};

/// 将内部状态投影为 Windows TSF `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION` 位掩码
inline LONG ComputeConversionMode(const ConversionState& st)
{
    if (st.asciiMode) {
        return st.fullWidth ? TF_CONVERSIONMODE_FULLSHAPE
                            : TF_CONVERSIONMODE_ALPHANUMERIC;
    }
    LONG mode = TF_CONVERSIONMODE_NATIVE;
    if (st.fullWidth) {
        mode |= TF_CONVERSIONMODE_FULLSHAPE;
    }
    if (!st.asciiPunct) {
        mode |= TF_CONVERSIONMODE_SYMBOL;
    }
    return mode;
}

/// 解析外部写入 `GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION` 的位掩码（适配 im-select 等工具）。
/// 若仅切换中/英文（NATIVE 位变化），保留用户原有的中文标点习惯；
/// 若持续处于中文态内调整 SYMBOL 位，同步更新 `asciiPunct`。
inline ConversionState ApplyExternalConversionMode(LONG convMode,
                                                   const ConversionState& cur)
{
    ConversionState next = cur;
    next.asciiMode = ((convMode & TF_CONVERSIONMODE_NATIVE) == 0);
    next.fullWidth = ((convMode & TF_CONVERSIONMODE_FULLSHAPE) != 0);
    if (!cur.asciiMode && !next.asciiMode) {
        next.asciiPunct = ((convMode & TF_CONVERSIONMODE_SYMBOL) == 0);
    }
    return next;
}

/// ActivateEx dwFlags 语义判定
inline bool IsNoActivateTipFlags(DWORD dwFlags)
{
    return (dwFlags & TF_TMAE_NOACTIVATETIP) != 0;
}

inline bool IsSecureActivateFlags(DWORD dwFlags)
{
    return (dwFlags & TF_TMAE_SECUREMODE) != 0;
}

inline bool IsUiElementOnlyActivateFlags(DWORD dwFlags)
{
    return (dwFlags & TF_TMAE_UIELEMENTENABLEDONLY) != 0;
}

inline bool IsComlessActivateFlags(DWORD dwFlags)
{
    return (dwFlags & TF_TMAE_COMLESS) != 0;
}

inline bool IsConsoleActivateFlags(DWORD dwFlags)
{
    return (dwFlags & TF_TMAE_CONSOLE) != 0;
}

/// 文本是否适合走 ITfFnReconversion（1–16 UTF-16 码元且包含至少一个中日韩统一表意文字）
inline bool IsConvertibleReconvertText(const std::wstring& text)
{
    if (text.empty() || text.size() > 16) {
        return false;
    }
    for (wchar_t ch : text) {
        if ((ch >= 0x4E00 && ch <= 0x9FFF) || (ch >= 0x3400 && ch <= 0x4DBF)) {
            return true;
        }
    }
    return false;
}

} // namespace taishen
