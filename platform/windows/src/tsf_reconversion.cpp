/// Windows TSF IME — Q3.1 ITfFunctionProvider / ITfFnReconversion（已上屏中文再转换）
///
/// 覆盖 DEV-TRACKER: Q3.1

#include "tsf_text_service.h"

#include <algorithm>
#include <new>

#include "debug_log.h"
#include "ime_ids.h"

// ---------------------------------------------------------------------------
// ITfContext 编辑会话：读取与替换 ITfRange 文本
// ---------------------------------------------------------------------------

class CTextService::CEditSessionReadRange final : public ITfEditSession {
public:
    CEditSessionReadRange(ITfRange* pRange, std::wstring& outText)
        : m_cRef(1), m_pRange(pRange), m_outText(outText)
    {
        if (m_pRange != nullptr) {
            m_pRange->AddRef();
        }
    }

    ~CEditSessionReadRange()
    {
        if (m_pRange != nullptr) {
            m_pRange->Release();
        }
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfEditSession)) {
            *ppvObj = static_cast<ITfEditSession*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP DoEditSession(TfEditCookie ec) override
    {
        m_outText.clear();
        if (m_pRange == nullptr) {
            return E_INVALIDARG;
        }
        wchar_t buf[64] = {0};
        ULONG got = 0;
        const HRESULT hr = m_pRange->GetText(ec, 0, buf, 63, &got);
        if (SUCCEEDED(hr) && got > 0) {
            m_outText.assign(buf, got);
        }
        return hr;
    }

private:
    LONG m_cRef;
    ITfRange* m_pRange;
    std::wstring& m_outText;
};

class CTextService::CEditSessionReplaceRange final : public ITfEditSession {
public:
    CEditSessionReplaceRange(ITfRange* pRange, std::wstring text)
        : m_cRef(1), m_pRange(pRange), m_text(std::move(text))
    {
        if (m_pRange != nullptr) {
            m_pRange->AddRef();
        }
    }

    ~CEditSessionReplaceRange()
    {
        if (m_pRange != nullptr) {
            m_pRange->Release();
        }
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfEditSession)) {
            *ppvObj = static_cast<ITfEditSession*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP DoEditSession(TfEditCookie ec) override
    {
        if (m_pRange == nullptr) {
            return E_INVALIDARG;
        }
        const HRESULT hr = m_pRange->SetText(
            ec, 0, m_text.c_str(), static_cast<LONG>(m_text.size()));
        if (SUCCEEDED(hr)) {
            ITfContext* pCtx = nullptr;
            if (SUCCEEDED(m_pRange->GetContext(&pCtx)) && pCtx != nullptr) {
                ITfRange* pEnd = nullptr;
                if (SUCCEEDED(m_pRange->Clone(&pEnd)) && pEnd != nullptr) {
                    pEnd->Collapse(ec, TF_ANCHOR_END);
                    TF_SELECTION sel{};
                    sel.range = pEnd;
                    sel.style.ase = TF_AE_NONE;
                    sel.style.fInterimChar = FALSE;
                    pCtx->SetSelection(ec, 1, &sel);
                    pEnd->Release();
                }
                pCtx->Release();
            }
        }
        return hr;
    }

private:
    LONG m_cRef;
    ITfRange* m_pRange;
    std::wstring m_text;
};

// ---------------------------------------------------------------------------
// ITfCandidateString / IEnumTfCandidates / ITfCandidateList
// ---------------------------------------------------------------------------

namespace {

class CTsfCandidateString final : public ITfCandidateString {
public:
    CTsfCandidateString(ULONG index, std::wstring text)
        : m_cRef(1), m_index(index), m_text(std::move(text))
    {
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_ITfCandidateString)) {
            *ppvObj = static_cast<ITfCandidateString*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP GetString(BSTR* pbstr) override
    {
        if (pbstr == nullptr) return E_POINTER;
        *pbstr = SysAllocStringLen(m_text.data(), static_cast<UINT>(m_text.size()));
        return *pbstr == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP GetIndex(ULONG* pnIndex) override
    {
        if (pnIndex == nullptr) return E_POINTER;
        *pnIndex = m_index;
        return S_OK;
    }

private:
    LONG m_cRef;
    ULONG m_index;
    std::wstring m_text;
};

class CEnumTfCandidates final : public IEnumTfCandidates {
public:
    CEnumTfCandidates(std::vector<std::wstring> items, size_t cursor = 0)
        : m_cRef(1), m_items(std::move(items)), m_cursor(cursor)
    {
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IEnumTfCandidates)) {
            *ppvObj = static_cast<IEnumTfCandidates*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP Clone(IEnumTfCandidates** ppEnum) override
    {
        if (ppEnum == nullptr) return E_POINTER;
        auto* cloned = new (std::nothrow) CEnumTfCandidates(m_items, m_cursor);
        *ppEnum = cloned;
        return cloned == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP Next(ULONG ulCount,
                      ITfCandidateString** ppCand,
                      ULONG* pcFetched) override
    {
        if (ppCand == nullptr) return E_POINTER;
        ULONG fetched = 0;
        while (fetched < ulCount && m_cursor < m_items.size()) {
            auto* item = new (std::nothrow)
                CTsfCandidateString(static_cast<ULONG>(m_cursor), m_items[m_cursor]);
            if (item == nullptr) {
                for (ULONG i = 0; i < fetched; ++i) {
                    ppCand[i]->Release();
                }
                if (pcFetched != nullptr) *pcFetched = 0;
                return E_OUTOFMEMORY;
            }
            ppCand[fetched++] = item;
            ++m_cursor;
        }
        if (pcFetched != nullptr) {
            *pcFetched = fetched;
        }
        return fetched == ulCount ? S_OK : S_FALSE;
    }

    STDMETHODIMP Reset() override
    {
        m_cursor = 0;
        return S_OK;
    }

    STDMETHODIMP Skip(ULONG ulCount) override
    {
        const size_t rem = (m_cursor < m_items.size()) ? (m_items.size() - m_cursor) : 0;
        if (ulCount <= rem) {
            m_cursor += ulCount;
            return S_OK;
        }
        m_cursor = m_items.size();
        return S_FALSE;
    }

private:
    LONG m_cRef;
    std::vector<std::wstring> m_items;
    size_t m_cursor;
};

} // namespace

class CTextService::CTsfCandidateList final : public ITfCandidateList {
public:
    CTsfCandidateList(CTextService* owner,
                      ITfRange* pRange,
                      std::vector<std::wstring> items)
        : m_cRef(1), m_owner(owner), m_pRange(pRange), m_items(std::move(items))
    {
        if (m_owner != nullptr) {
            m_owner->AddRef();
        }
        if (m_pRange != nullptr) {
            m_pRange->AddRef();
        }
    }

    ~CTsfCandidateList()
    {
        if (m_pRange != nullptr) {
            m_pRange->Release();
        }
        if (m_owner != nullptr) {
            m_owner->Release();
        }
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_ITfCandidateList)) {
            *ppvObj = static_cast<ITfCandidateList*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP EnumCandidates(IEnumTfCandidates** ppEnum) override
    {
        if (ppEnum == nullptr) return E_POINTER;
        auto* en = new (std::nothrow) CEnumTfCandidates(m_items);
        *ppEnum = en;
        return en == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP GetCandidate(ULONG nIndex, ITfCandidateString** ppCand) override
    {
        if (ppCand == nullptr) return E_POINTER;
        *ppCand = nullptr;
        if (nIndex >= m_items.size()) {
            return E_INVALIDARG;
        }
        auto* item = new (std::nothrow)
            CTsfCandidateString(nIndex, m_items[nIndex]);
        *ppCand = item;
        return item == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP GetCandidateNum(ULONG* pnCnt) override
    {
        if (pnCnt == nullptr) return E_POINTER;
        *pnCnt = static_cast<ULONG>(m_items.size());
        return S_OK;
    }

    STDMETHODIMP SetResult(ULONG nIndex, TfCandidateResult imcr) override
    {
        if (nIndex >= m_items.size()) {
            return E_INVALIDARG;
        }
        if (imcr == CAND_FINALIZED && m_owner != nullptr && m_pRange != nullptr) {
            return m_owner->ReplaceRangeText(m_pRange, m_items[nIndex]);
        }
        return S_OK;
    }

private:
    LONG m_cRef;
    CTextService* m_owner;
    ITfRange* m_pRange;
    std::vector<std::wstring> m_items;
};

// ---------------------------------------------------------------------------
// CTsfReconversion (ITfFnReconversion)
// ---------------------------------------------------------------------------

class CTextService::CTsfReconversion final : public ITfFnReconversion {
public:
    explicit CTsfReconversion(CTextService* owner) : m_cRef(1), m_owner(owner)
    {
        if (m_owner != nullptr) {
            m_owner->AddRef();
        }
    }

    ~CTsfReconversion()
    {
        if (m_owner != nullptr) {
            m_owner->Release();
        }
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_ITfFunction) ||
            IsEqualIID(riid, IID_ITfFnReconversion)) {
            *ppvObj = static_cast<ITfFnReconversion*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    STDMETHODIMP GetDisplayName(BSTR* pbstrName) override
    {
        if (pbstrName == nullptr) return E_POINTER;
        *pbstrName = SysAllocString(L"泰深拼音再转换");
        return *pbstrName == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP QueryRange(ITfRange* pRange,
                            ITfRange** ppNewRange,
                            BOOL* pfConvertable) override
    {
        if (pfConvertable == nullptr) return E_POINTER;
        *pfConvertable = FALSE;
        if (ppNewRange != nullptr) {
            *ppNewRange = nullptr;
        }
        if (pRange == nullptr || m_owner == nullptr || m_owner->IsSecureMode()) {
            return E_INVALIDARG;
        }
        std::wstring text;
        if (!m_owner->ReadRangeText(pRange, text) ||
            !taishen::IsConvertibleReconvertText(text)) {
            return S_OK;
        }
        if (ppNewRange != nullptr) {
            const HRESULT hrClone = pRange->Clone(ppNewRange);
            if (FAILED(hrClone)) {
                return hrClone;
            }
        }
        *pfConvertable = TRUE;
        return S_OK;
    }

    STDMETHODIMP GetReconversion(ITfRange* pRange,
                                 ITfCandidateList** ppCandList) override
    {
        if (ppCandList == nullptr) return E_POINTER;
        *ppCandList = nullptr;
        if (pRange == nullptr || m_owner == nullptr || m_owner->IsSecureMode()) {
            return E_INVALIDARG;
        }
        std::wstring text;
        if (!m_owner->ReadRangeText(pRange, text) ||
            !taishen::IsConvertibleReconvertText(text)) {
            return E_FAIL;
        }
        std::string pinyin;
        std::vector<std::wstring> cands;
        m_owner->QueryReconvertCandidates(text, pinyin, cands);
        if (cands.empty()) {
            cands.push_back(text);
        }
        auto* list = new (std::nothrow)
            CTsfCandidateList(m_owner, pRange, std::move(cands));
        *ppCandList = list;
        return list == nullptr ? E_OUTOFMEMORY : S_OK;
    }

    STDMETHODIMP Reconvert(ITfRange* pRange) override
    {
        ITfCandidateList* pList = nullptr;
        const HRESULT hr = GetReconversion(pRange, &pList);
        if (pList != nullptr) {
            pList->Release();
        }
        return hr;
    }

private:
    LONG m_cRef;
    CTextService* m_owner;
};

// ---------------------------------------------------------------------------
// CTextService — ITfFunctionProvider & 文本范围辅助
// ---------------------------------------------------------------------------

void CTextService::InitFunctionProvider()
{
    if (m_pThreadMgr == nullptr || m_funcProviderAdvised) {
        return;
    }
    ITfSourceSingle* pSourceSingle = nullptr;
    if (SUCCEEDED(m_pThreadMgr->QueryInterface(
            IID_ITfSourceSingle, reinterpret_cast<void**>(&pSourceSingle))) &&
        pSourceSingle != nullptr) {
        const HRESULT hr = pSourceSingle->AdviseSingleSink(
            m_tid, IID_ITfFunctionProvider,
            static_cast<ITfFunctionProvider*>(this));
        pSourceSingle->Release();
        m_funcProviderAdvised = SUCCEEDED(hr);
        taishen::DebugLogHr("InitFunctionProvider: AdviseSingleSink", hr);
    }
}

void CTextService::UninitFunctionProvider()
{
    if (!m_funcProviderAdvised || m_pThreadMgr == nullptr) {
        m_funcProviderAdvised = false;
        return;
    }
    m_funcProviderAdvised = false;
    ITfSourceSingle* pSourceSingle = nullptr;
    if (SUCCEEDED(m_pThreadMgr->QueryInterface(
            IID_ITfSourceSingle, reinterpret_cast<void**>(&pSourceSingle))) &&
        pSourceSingle != nullptr) {
        pSourceSingle->UnadviseSingleSink(m_tid, IID_ITfFunctionProvider);
        pSourceSingle->Release();
    }
}

STDMETHODIMP CTextService::GetType(GUID* pguid)
{
    if (pguid == nullptr) return E_POINTER;
    *pguid = CLSID_TAISHEN_IME;
    return S_OK;
}

STDMETHODIMP CTextService::GetDescription(BSTR* pbstrDesc)
{
    if (pbstrDesc == nullptr) return E_POINTER;
    *pbstrDesc = SysAllocString(L"泰深输入法功能提供器");
    return *pbstrDesc == nullptr ? E_OUTOFMEMORY : S_OK;
}

STDMETHODIMP CTextService::GetFunction(REFGUID rguid,
                                       REFIID riid,
                                       IUnknown** ppunk)
{
    if (ppunk == nullptr) return E_POINTER;
    *ppunk = nullptr;
    if ((!IsEqualGUID(rguid, GUID_NULL) &&
         !IsEqualGUID(rguid, CLSID_TAISHEN_IME)) ||
        (!IsEqualIID(riid, IID_ITfFnReconversion) &&
         !IsEqualIID(riid, IID_IUnknown))) {
        return E_NOINTERFACE;
    }
    auto* fn = new (std::nothrow) CTsfReconversion(this);
    if (fn == nullptr) {
        return E_OUTOFMEMORY;
    }
    const HRESULT hr = fn->QueryInterface(riid, reinterpret_cast<void**>(ppunk));
    fn->Release();
    return hr;
}

bool CTextService::ReadRangeText(ITfRange* pRange, std::wstring& outText)
{
    outText.clear();
    if (pRange == nullptr) {
        return false;
    }
    ITfContext* pCtx = nullptr;
    if (FAILED(pRange->GetContext(&pCtx)) || pCtx == nullptr) {
        return false;
    }
    auto* session = new (std::nothrow) CEditSessionReadRange(pRange, outText);
    if (session == nullptr) {
        pCtx->Release();
        return false;
    }
    HRESULT hrSession = E_FAIL;
    const HRESULT hr = pCtx->RequestEditSession(
        m_tid, session, TF_ES_ASYNCDONTCARE | TF_ES_READ, &hrSession);
    session->Release();
    pCtx->Release();
    return SUCCEEDED(hr) && SUCCEEDED(hrSession);
}

HRESULT CTextService::ReplaceRangeText(ITfRange* pRange,
                                       const std::wstring& text)
{
    if (pRange == nullptr) {
        return E_INVALIDARG;
    }
    ITfContext* pCtx = nullptr;
    if (FAILED(pRange->GetContext(&pCtx)) || pCtx == nullptr) {
        return E_FAIL;
    }
    auto* session = new (std::nothrow) CEditSessionReplaceRange(pRange, text);
    if (session == nullptr) {
        pCtx->Release();
        return E_OUTOFMEMORY;
    }
    HRESULT hrSession = E_FAIL;
    const HRESULT hr = pCtx->RequestEditSession(
        m_tid, session, TF_ES_ASYNCDONTCARE | TF_ES_READWRITE, &hrSession);
    session->Release();
    pCtx->Release();
    return FAILED(hr) ? hr : hrSession;
}

bool CTextService::QueryReconvertCandidates(const std::wstring& text,
                                            std::string& outPinyin,
                                            std::vector<std::wstring>& outCands)
{
    outPinyin.clear();
    outCands.clear();
    if (IsSecureMode() || !taishen::IsConvertibleReconvertText(text)) {
        return false;
    }

    taishen::ipc::ReconvertRequest req;
    req.sessionId = m_renderSessionId;
    req.hostPid = GetCurrentProcessId();
    req.text = taishen::WideToUtf8(text);
    req.maxCount = 9;

    taishen::ipc::ReconvertResponse resp;
    if (m_renderClient.SendReconvert(req, resp, 150) && resp.ok) {
        outPinyin = resp.pinyin;
        outCands.reserve(resp.candidates.size());
        for (const auto& item : resp.candidates) {
            const std::wstring w = taishen::Utf8ToWide(item);
            if (!w.empty()) {
                outCands.push_back(w);
            }
        }
    }
    if (std::find(outCands.begin(), outCands.end(), text) == outCands.end()) {
        outCands.push_back(text);
    }
    return !outCands.empty();
}
