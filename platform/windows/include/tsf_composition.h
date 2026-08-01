/// TSF 组合管理 — 声明
///
/// 对应 SPEC: docs/modules/composition/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.7 选词上屏（候选→TSF 文本提交）
///
/// 负责在目标应用文档中创建/更新/提交 TSF 组合：
///   - StartComposition: 首个拼音字母输入时创建组合
///   - UpdateComposition: 后续拼音字母/退格时更新组合文本
///   - CommitComposition: 选词/ESC 时提交（上屏汉字）

#pragma once

#include <windows.h>
#include <msctf.h>
#include <string>

namespace taishen {

/// TSF 组合管理类
class CTsfComposition : public ITfCompositionSink
{
public:
    CTsfComposition();
    virtual ~CTsfComposition();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // ITfCompositionSink — 组合被外部结束（应用撤销等）时回调
    STDMETHODIMP OnCompositionTerminated(TfEditCookie ec,
                                         ITfComposition* pComposition) override;

    /// 在编辑会话中启动组合（首个拼音字母输入时调用）
    /// @param ec      编辑会话 cookie（由 RequestEditSession 提供）
    /// @param pic     目标上下文
    /// @param pinyin  当前拼音串（UTF-8）
    HRESULT StartComposition(TfEditCookie ec, ITfContext* pic,
                             const std::string& pinyin);

    /// 在编辑会话中更新组合文本（后续拼音字母/退格时调用）
    HRESULT UpdateComposition(TfEditCookie ec, ITfContext* pic,
                              const std::string& pinyin);

    /// 在编辑会话中提交组合（选词/ESC 时调用）
    /// @param text 要上屏的文本（UTF-8，选词时为汉字，ESC 时为空=撤销）
    HRESULT CommitComposition(TfEditCookie ec, ITfContext* pic,
                              const std::string& text);

    /// 组合是否活跃
    bool IsActive() const { return m_pComposition != nullptr; }

    /// 强制结束组合（Deactivate 等清理场景）
    void Reset() { m_pComposition = nullptr; }

private:
    // UTF-8 → 宽字符串
    static std::wstring Utf8ToWide(const std::string& utf8);

    // 在编辑会话内将组合文本替换为指定内容
    HRESULT ReplaceCompositionText(TfEditCookie ec, ITfContext* pic,
                                   const std::wstring& text);

    // IUnknown 引用计数
    LONG m_cRef;
    // 当前组合（nullptr = 无组合）
    ITfComposition* m_pComposition;
};

} // namespace taishen
