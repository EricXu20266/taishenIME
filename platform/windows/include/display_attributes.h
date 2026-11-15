/// 组合串分段显示属性（V0.6.x）
///
/// 组合串文本 = 已选前缀（组词已确认的汉字）+ 待选后缀（剩余拼音）。
/// 两者在文档里必须能区分，否则用户看不出「哪些字已经定了」。
/// 微软拼音 / QQ 拼音走的就是这条路：一个组合串 + TSF DisplayAttribute 分段着色。
///
/// 这里定义两个属性：
///   Confirmed —— 已选汉字：正常颜色、无下划线
///   Pending   —— 剩余拼音：虚线下划线
///
/// 注册方式（DllRegisterServer 已声明类别）：CLSID 同时作为
/// ITfDisplayAttributeProvider，TSF 通过 CoCreateInstance + QI 获取。

#pragma once

#include <windows.h>
#include <msctf.h>

namespace taishen {

/// 已选文字（组词前缀）
extern const GUID GUID_TAISHEN_ATTR_CONFIRMED;
/// 待选拼音（尚未选择的部分）
extern const GUID GUID_TAISHEN_ATTR_PENDING;

/// 按 GUID 创建单个属性描述对象；GUID 不匹配返回 nullptr。
/// 调用方负责 Release。
ITfDisplayAttributeInfo* CreateDisplayAttributeInfo(REFGUID guid);

/// 创建属性枚举器（包含上面两个属性）。调用方负责 Release。
IEnumTfDisplayAttributeInfo* CreateDisplayAttributeEnum();

/// 把分段属性应用到组合 range。
/// @param ec             读写编辑会话 cookie
/// @param pic            目标上下文
/// @param pRange         覆盖整段组合文本的 range
/// @param convertedChars 已选前缀的**宽字符数**（0 表示整段都是待选拼音）
/// @param totalChars     整段组合文本的宽字符数
HRESULT ApplyCompositionDisplayAttributes(TfEditCookie ec, ITfContext* pic,
                                          ITfRange* pRange,
                                          LONG convertedChars,
                                          LONG totalChars);

} // namespace taishen
