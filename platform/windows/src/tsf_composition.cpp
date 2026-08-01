/// TSF 组合管理 — 实现
///
/// 对应 SPEC: docs/modules/composition/SPEC.md
/// 组合流程：
///   StartComposition → ITfContextComposition::StartComposition(光标位置, sink)
///   UpdateComposition → 组合 range 的 SetText(拼音串)
///   CommitComposition → 组合 range 的 SetText(汉字) + EndComposition

#include "tsf_composition.h"

namespace taishen {

// UTF-8 → 宽字符串
std::wstring CTsfComposition::Utf8ToWide(const std::string& utf8)
{
    if (utf8.empty()) {
        return std::wstring();
    }
    const int len = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                        static_cast<int>(utf8.size()),
                                        nullptr, 0);
    if (len <= 0) {
        return std::wstring();
    }
    std::wstring result(static_cast<size_t>(len), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                        static_cast<int>(utf8.size()), &result[0], len);
    return result;
}

// ---------------------------------------------------------------------------
// 构造/析构
// ---------------------------------------------------------------------------
CTsfComposition::CTsfComposition()
    : m_cRef(1), m_pComposition(nullptr)
{
}

CTsfComposition::~CTsfComposition()
{
}

// ---------------------------------------------------------------------------
// IUnknown
// ---------------------------------------------------------------------------
STDMETHODIMP CTsfComposition::QueryInterface(REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;
    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfCompositionSink)) {
        *ppvObj = static_cast<ITfCompositionSink*>(this);
    } else {
        return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTsfComposition::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTsfComposition::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

// ---------------------------------------------------------------------------
// ITfCompositionSink — 组合被外部结束（应用撤销等）
// ---------------------------------------------------------------------------
STDMETHODIMP CTsfComposition::OnCompositionTerminated(
    TfEditCookie /*ec*/, ITfComposition* /*pComposition*/)
{
    // 应用主动结束组合（如用户撤销、焦点切换）——清理内部状态
    m_pComposition = nullptr;
    return S_OK;
}

// ---------------------------------------------------------------------------
// 组合操作
// ---------------------------------------------------------------------------

/// 启动组合：在光标位置创建组合，文本为当前拼音串
HRESULT CTsfComposition::StartComposition(TfEditCookie ec, ITfContext* pic,
                                          const std::string& pinyin)
{
    if (pic == nullptr) {
        return E_POINTER;
    }

    // 已有组合则先结束（防御：异常状态恢复）
    if (m_pComposition != nullptr) {
        m_pComposition->EndComposition(ec);
        m_pComposition = nullptr;
    }

    // 1. 获取光标位置（当前选区起点）
    TF_SELECTION selection = {};
    ULONG fetched = 0;
    HRESULT hr = pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1,
                                   &selection, &fetched);
    if (FAILED(hr) || fetched == 0 || selection.range == nullptr) {
        return E_FAIL;
    }

    // 2. 创建组合
    ITfContextComposition* pContextComp = nullptr;
    hr = pic->QueryInterface(IID_ITfContextComposition,
                             reinterpret_cast<void**>(&pContextComp));
    if (SUCCEEDED(hr) && pContextComp != nullptr) {
        // 组合覆盖光标位置（空选区 = 插入点）
        hr = pContextComp->StartComposition(ec, selection.range,
                                            this, &m_pComposition);
        pContextComp->Release();
        if (SUCCEEDED(hr) && m_pComposition != nullptr) {
            // 3. 写入拼音文本（range 自动扩展覆盖插入文本）
            const std::wstring text = Utf8ToWide(pinyin);
            hr = selection.range->SetText(ec, 0, text.c_str(),
                                          static_cast<LONG>(text.size()));
        }
    }

    if (selection.range != nullptr) {
        selection.range->Release();
    }
    return hr;
}

/// 更新组合文本：用新拼音串替换组合内文本
HRESULT CTsfComposition::UpdateComposition(TfEditCookie ec, ITfContext* pic,
                                           const std::string& pinyin)
{
    if (m_pComposition == nullptr) {
        // 组合不存在则重新启动（防御）
        return StartComposition(ec, pic, pinyin);
    }
    return ReplaceCompositionText(ec, pic, Utf8ToWide(pinyin));
}

/// 提交组合：替换为上屏文本并结束组合
HRESULT CTsfComposition::CommitComposition(TfEditCookie ec, ITfContext* pic,
                                           const std::string& text)
{
    if (m_pComposition == nullptr) {
        return S_OK; // 无组合，无事可做
    }

    // 1. 替换组合文本为上屏内容
    HRESULT hr = ReplaceCompositionText(ec, pic, Utf8ToWide(text));

    // 2. 结束组合（上屏）
    m_pComposition->EndComposition(ec);
    m_pComposition = nullptr;
    return hr;
}

/// 替换组合 range 内文本
HRESULT CTsfComposition::ReplaceCompositionText(TfEditCookie ec,
                                                ITfContext* pic,
                                                const std::wstring& text)
{
    if (pic == nullptr || m_pComposition == nullptr) {
        return E_POINTER;
    }

    // 获取组合的当前范围
    ITfRange* pRange = nullptr;
    HRESULT hr = m_pComposition->GetRange(&pRange);
    if (FAILED(hr) || pRange == nullptr) {
        return hr;
    }

    // 替换为指定文本
    hr = pRange->SetText(ec, 0, text.c_str(), static_cast<LONG>(text.size()));

    // 将插入点移到文本末尾（组合终点跟随）
    if (SUCCEEDED(hr)) {
        pRange->Collapse(ec, TF_ANCHOR_END);
        m_pComposition->ShiftEnd(ec, pRange);
        // 起点保持，终点已对齐
        pRange->Release();
        pRange = nullptr;
    }

    if (pRange != nullptr) {
        pRange->Release();
    }
    return hr;
}

} // namespace taishen
