/// Q3.1 TSF 契约补全综合自动化测试
///
/// 覆盖 5 项契约与关联路径：
///   1. 纯逻辑单测（InputScope / ConversionMode 1025↔0 / ActivateEx dwFlags / Reconvert）
///   2. 真实 taishen_ime.dll COM 实例化与 QueryInterface / GetFunction 校验
///   3. 模拟 COM TSF 宿主交互（SecureMode/COMLess、Compartment 双向同步、IS_PASSWORD、Reconversion）
///   4. Rust engine_reconvert FFI + render_ipc.h reconvert JSON 协议往返
///   5. CCandidateWindow UIA Provider（Root/Item 结构、ISelectionProvider、Select 选词）

#include <windows.h>
#include <UIAutomation.h>
#include <ctffunc.h>
#include <inputscope.h>
#include <msctf.h>

#include <cstdio>
#include <map>
#include <string>
#include <vector>

#include "candidate_window.h"
#include "engine_bridge.h"
#include "ime_ids.h"
#include "render_ipc.h"
#include "tsf_contracts.h"

namespace {

int g_fails = 0;

void Check(bool ok, const char* msg)
{
    if (!ok) {
        std::printf("[FAIL] %s\n", msg);
        ++g_fails;
    }
}

struct GuidLess {
    bool operator()(const GUID& a, const GUID& b) const
    {
        return memcmp(&a, &b, sizeof(GUID)) < 0;
    }
};

// ---------------------------------------------------------------------------
// Mock TSF COM 宿主环境
// ---------------------------------------------------------------------------

class MockInputScope final : public ITfInputScope {
public:
    explicit MockInputScope(InputScope s) : m_cRef(1), m_scope(s) {}
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfInputScope)) {
            *ppv = static_cast<ITfInputScope*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }
    STDMETHODIMP GetInputScopes(InputScope** pprg, UINT* pc) override
    {
        if (!pprg || !pc) return E_POINTER;
        auto* arr = static_cast<InputScope*>(CoTaskMemAlloc(sizeof(InputScope)));
        *arr = m_scope;
        *pprg = arr;
        *pc = 1;
        return S_OK;
    }
    STDMETHODIMP GetPhrase(BSTR**, UINT* pc) override { if (pc) *pc = 0; return E_NOTIMPL; }
    STDMETHODIMP GetRegularExpression(BSTR*) override { return E_NOTIMPL; }
    STDMETHODIMP GetSRGS(BSTR*) override { return E_NOTIMPL; }
    STDMETHODIMP GetXML(BSTR*) override { return E_NOTIMPL; }

private:
    LONG m_cRef;
    InputScope m_scope;
};

class MockRange final : public ITfRange {
public:
    MockRange(ITfContext* ctx, std::wstring text)
        : m_cRef(1), m_ctx(ctx), m_text(std::move(text))
    {
    }
    const std::wstring& Text() const { return m_text; }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfRange)) {
            *ppv = static_cast<ITfRange*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }
    STDMETHODIMP GetText(TfEditCookie, DWORD, WCHAR* pch, ULONG cchMax, ULONG* pcch) override
    {
        ULONG n = static_cast<ULONG>(m_text.size());
        if (n > cchMax) n = cchMax;
        if (pch && n > 0) wmemcpy(pch, m_text.c_str(), n);
        if (pcch) *pcch = n;
        return S_OK;
    }
    STDMETHODIMP SetText(TfEditCookie, DWORD, const WCHAR* pch, LONG cch) override
    {
        if (cch < 0 || !pch) m_text.clear();
        else m_text.assign(pch, static_cast<size_t>(cch));
        return S_OK;
    }
    STDMETHODIMP GetFormattedText(TfEditCookie, IDataObject**) override { return E_NOTIMPL; }
    STDMETHODIMP GetEmbedded(TfEditCookie, REFGUID, REFIID, IUnknown**) override { return E_NOTIMPL; }
    STDMETHODIMP InsertEmbedded(TfEditCookie, DWORD, IDataObject*) override { return E_NOTIMPL; }
    STDMETHODIMP ShiftStart(TfEditCookie, LONG, LONG* pc, const TF_HALTCOND*) override { if (pc) *pc = 0; return S_OK; }
    STDMETHODIMP ShiftEnd(TfEditCookie, LONG, LONG* pc, const TF_HALTCOND*) override { if (pc) *pc = 0; return S_OK; }
    STDMETHODIMP ShiftStartToRange(TfEditCookie, ITfRange*, TfAnchor) override { return S_OK; }
    STDMETHODIMP ShiftEndToRange(TfEditCookie, ITfRange*, TfAnchor) override { return S_OK; }
    STDMETHODIMP ShiftStartRegion(TfEditCookie, TfShiftDir, BOOL*) override { return S_OK; }
    STDMETHODIMP ShiftEndRegion(TfEditCookie, TfShiftDir, BOOL*) override { return S_OK; }
    STDMETHODIMP IsEmpty(TfEditCookie, BOOL* pf) override { if (pf) *pf = m_text.empty() ? TRUE : FALSE; return S_OK; }
    STDMETHODIMP Collapse(TfEditCookie, TfAnchor) override { return S_OK; }
    STDMETHODIMP IsEqualStart(TfEditCookie, ITfRange*, TfAnchor, BOOL* pf) override { if (pf) *pf = TRUE; return S_OK; }
    STDMETHODIMP IsEqualEnd(TfEditCookie, ITfRange*, TfAnchor, BOOL* pf) override { if (pf) *pf = TRUE; return S_OK; }
    STDMETHODIMP CompareStart(TfEditCookie, ITfRange*, TfAnchor, LONG* pl) override { if (pl) *pl = 0; return S_OK; }
    STDMETHODIMP CompareEnd(TfEditCookie, ITfRange*, TfAnchor, LONG* pl) override { if (pl) *pl = 0; return S_OK; }
    STDMETHODIMP AdjustForInsert(TfEditCookie, ULONG, BOOL* pf) override { if (pf) *pf = TRUE; return S_OK; }
    STDMETHODIMP GetGravity(TfGravity* ps, TfGravity* pe) override { if (ps) *ps = TF_GRAVITY_BACKWARD; if (pe) *pe = TF_GRAVITY_FORWARD; return S_OK; }
    STDMETHODIMP SetGravity(TfEditCookie, TfGravity, TfGravity) override { return S_OK; }
    STDMETHODIMP Clone(ITfRange** ppClone) override
    {
        if (!ppClone) return E_POINTER;
        *ppClone = new MockRange(m_ctx, m_text);
        return S_OK;
    }
    STDMETHODIMP GetContext(ITfContext** ppCtx) override
    {
        if (!ppCtx) return E_POINTER;
        *ppCtx = m_ctx;
        if (m_ctx) m_ctx->AddRef();
        return S_OK;
    }

private:
    LONG m_cRef;
    ITfContext* m_ctx;
    std::wstring m_text;
};

class MockProperty final : public ITfProperty {
public:
    MockProperty(ITfContext* ctx, InputScope scope)
        : m_cRef(1), m_ctx(ctx), m_scope(scope)
    {
    }
    void SetScope(InputScope s) { m_scope = s; }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_ITfReadOnlyProperty) ||
            IsEqualIID(riid, IID_ITfProperty)) {
            *ppv = static_cast<ITfProperty*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }
    STDMETHODIMP GetType(GUID* pg) override { if (pg) *pg = taishen::kGuidPropInputScope; return S_OK; }
    STDMETHODIMP EnumRanges(TfEditCookie, IEnumTfRanges**, ITfRange*) override { return E_NOTIMPL; }
    STDMETHODIMP GetValue(TfEditCookie, ITfRange*, VARIANT* pv) override
    {
        if (!pv) return E_POINTER;
        VariantInit(pv);
        pv->vt = VT_UNKNOWN;
        pv->punkVal = new MockInputScope(m_scope);
        return S_OK;
    }
    STDMETHODIMP GetContext(ITfContext** ppCtx) override
    {
        if (!ppCtx) return E_POINTER;
        *ppCtx = m_ctx;
        if (m_ctx) m_ctx->AddRef();
        return S_OK;
    }
    STDMETHODIMP FindRange(TfEditCookie, ITfRange*, ITfRange**, TfAnchor) override { return E_NOTIMPL; }
    STDMETHODIMP SetValueStore(TfEditCookie, ITfRange*, ITfPropertyStore*) override { return E_NOTIMPL; }
    STDMETHODIMP SetValue(TfEditCookie, ITfRange*, const VARIANT*) override { return E_NOTIMPL; }
    STDMETHODIMP Clear(TfEditCookie, ITfRange*) override { return E_NOTIMPL; }

private:
    LONG m_cRef;
    ITfContext* m_ctx;
    InputScope m_scope;
};

class MockCompartment final : public ITfCompartment, public ITfSource {
public:
    explicit MockCompartment(REFGUID guid) : m_cRef(1), m_guid(guid)
    {
        VariantInit(&m_val);
    }
    ~MockCompartment()
    {
        for (auto& kv : m_sinks) kv.second->Release();
        VariantClear(&m_val);
    }

    size_t SinkCount() const { return m_sinks.size(); }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfCompartment)) {
            *ppv = static_cast<ITfCompartment*>(this);
        } else if (IsEqualIID(riid, IID_ITfSource)) {
            *ppv = static_cast<ITfSource*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }

    STDMETHODIMP SetValue(TfClientId, const VARIANT* pv) override
    {
        if (!pv) return E_POINTER;
        VariantClear(&m_val);
        VariantCopy(&m_val, pv);
        auto copy = m_sinks;
        for (auto& kv : copy) {
            kv.second->OnChange(m_guid);
        }
        return S_OK;
    }
    STDMETHODIMP GetValue(VARIANT* pv) override
    {
        if (!pv) return E_POINTER;
        VariantInit(pv);
        return VariantCopy(pv, &m_val);
    }

    STDMETHODIMP AdviseSink(REFIID riid, IUnknown* punk, DWORD* pdwCookie) override
    {
        if (!punk || !pdwCookie) return E_POINTER;
        if (!IsEqualIID(riid, IID_ITfCompartmentEventSink)) return E_INVALIDARG;
        ITfCompartmentEventSink* sink = nullptr;
        if (FAILED(punk->QueryInterface(IID_ITfCompartmentEventSink,
                                        reinterpret_cast<void**>(&sink)))) {
            return E_NOINTERFACE;
        }
        DWORD cookie = ++m_nextCookie;
        m_sinks[cookie] = sink;
        *pdwCookie = cookie;
        return S_OK;
    }
    STDMETHODIMP UnadviseSink(DWORD dwCookie) override
    {
        auto it = m_sinks.find(dwCookie);
        if (it == m_sinks.end()) return E_INVALIDARG;
        it->second->Release();
        m_sinks.erase(it);
        return S_OK;
    }

private:
    LONG m_cRef;
    GUID m_guid;
    VARIANT m_val;
    DWORD m_nextCookie = 0;
    std::map<DWORD, ITfCompartmentEventSink*> m_sinks;
};

class MockCompartmentStore {
public:
    ~MockCompartmentStore()
    {
        for (auto& kv : m_map) kv.second->Release();
    }
    MockCompartment* Get(REFGUID rguid)
    {
        auto it = m_map.find(rguid);
        if (it == m_map.end()) {
            auto* c = new MockCompartment(rguid);
            m_map[rguid] = c;
            it = m_map.find(rguid);
        }
        it->second->AddRef();
        return it->second;
    }
    MockCompartment* Find(REFGUID rguid) const
    {
        auto it = m_map.find(rguid);
        return it == m_map.end() ? nullptr : it->second;
    }

private:
    std::map<GUID, MockCompartment*, GuidLess> m_map;
};

class MockContext final : public ITfContext, public ITfCompartmentMgr {
public:
    explicit MockContext(InputScope scope)
        : m_cRef(1), m_range(new MockRange(this, L"")),
          m_prop(new MockProperty(this, scope))
    {
    }
    ~MockContext()
    {
        m_range->Release();
        m_prop->Release();
    }
    void SetScope(InputScope s) { m_prop->SetScope(s); }
    MockCompartment* Compartment(REFGUID g) { return m_comps.Get(g); }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfContext)) {
            *ppv = static_cast<ITfContext*>(this);
        } else if (IsEqualIID(riid, IID_ITfCompartmentMgr)) {
            *ppv = static_cast<ITfCompartmentMgr*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }

    STDMETHODIMP RequestEditSession(TfClientId, ITfEditSession* pes,
                                    DWORD, HRESULT* phrSession) override
    {
        if (!pes || !phrSession) return E_POINTER;
        *phrSession = pes->DoEditSession(1);
        return S_OK;
    }
    STDMETHODIMP InWriteSession(TfClientId, BOOL* pf) override { if (pf) *pf = FALSE; return S_OK; }
    STDMETHODIMP GetSelection(TfEditCookie, ULONG, ULONG ulCount, TF_SELECTION* pSel, ULONG* pc) override
    {
        if (!pSel || !pc || ulCount == 0) return E_INVALIDARG;
        pSel[0].range = m_range;
        m_range->AddRef();
        pSel[0].style.ase = TF_AE_NONE;
        pSel[0].style.fInterimChar = FALSE;
        *pc = 1;
        return S_OK;
    }
    STDMETHODIMP SetSelection(TfEditCookie, ULONG, const TF_SELECTION*) override { return S_OK; }
    STDMETHODIMP GetStart(TfEditCookie, ITfRange** pp) override { if (!pp) return E_POINTER; *pp = m_range; m_range->AddRef(); return S_OK; }
    STDMETHODIMP GetEnd(TfEditCookie, ITfRange** pp) override { if (!pp) return E_POINTER; *pp = m_range; m_range->AddRef(); return S_OK; }
    STDMETHODIMP GetActiveView(ITfContextView** pp) override { if (pp) *pp = nullptr; return E_FAIL; }
    STDMETHODIMP EnumViews(IEnumTfContextViews**) override { return E_NOTIMPL; }
    STDMETHODIMP GetStatus(TF_STATUS* pdcs) override { if (pdcs) *pdcs = {}; return S_OK; }
    STDMETHODIMP GetProperty(REFGUID guidProp, ITfProperty** ppProp) override
    {
        if (!ppProp) return E_POINTER;
        *ppProp = nullptr;
        if (IsEqualGUID(guidProp, taishen::kGuidPropInputScope)) {
            *ppProp = m_prop;
            m_prop->AddRef();
            return S_OK;
        }
        return E_FAIL;
    }
    STDMETHODIMP GetAppProperty(REFGUID, ITfReadOnlyProperty**) override { return E_NOTIMPL; }
    STDMETHODIMP TrackProperties(const GUID**, ULONG, const GUID**, ULONG, ITfReadOnlyProperty**) override { return E_NOTIMPL; }
    STDMETHODIMP EnumProperties(IEnumTfProperties**) override { return E_NOTIMPL; }
    STDMETHODIMP GetDocumentMgr(ITfDocumentMgr**) override { return E_NOTIMPL; }
    STDMETHODIMP CreateRangeBackup(TfEditCookie, ITfRange*, ITfRangeBackup**) override { return E_NOTIMPL; }

    STDMETHODIMP GetCompartment(REFGUID rguid, ITfCompartment** ppcomp) override
    {
        if (!ppcomp) return E_POINTER;
        *ppcomp = m_comps.Get(rguid);
        return S_OK;
    }
    STDMETHODIMP ClearCompartment(TfClientId, REFGUID) override { return S_OK; }
    STDMETHODIMP EnumCompartments(IEnumGUID**) override { return E_NOTIMPL; }

private:
    LONG m_cRef;
    MockRange* m_range;
    MockProperty* m_prop;
    MockCompartmentStore m_comps;
};

class MockDocumentMgr final : public ITfDocumentMgr {
public:
    explicit MockDocumentMgr(MockContext* ctx) : m_cRef(1), m_ctx(ctx)
    {
        if (m_ctx) m_ctx->AddRef();
    }
    ~MockDocumentMgr()
    {
        if (m_ctx) m_ctx->Release();
    }
    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfDocumentMgr)) {
            *ppv = static_cast<ITfDocumentMgr*>(this);
            AddRef();
            return S_OK;
        }
        return E_NOINTERFACE;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }
    STDMETHODIMP CreateContext(TfClientId, DWORD, IUnknown*, ITfContext**, TfEditCookie*) override { return E_NOTIMPL; }
    STDMETHODIMP Push(ITfContext*) override { return S_OK; }
    STDMETHODIMP Pop(DWORD) override { return S_OK; }
    STDMETHODIMP GetTop(ITfContext** ppic) override
    {
        if (!ppic) return E_POINTER;
        *ppic = m_ctx;
        if (m_ctx) m_ctx->AddRef();
        return S_OK;
    }
    STDMETHODIMP GetBase(ITfContext** ppic) override { return GetTop(ppic); }
    STDMETHODIMP EnumContexts(IEnumTfContexts**) override { return E_NOTIMPL; }

private:
    LONG m_cRef;
    MockContext* m_ctx;
};

class MockThreadMgr final : public ITfThreadMgr,
                            public ITfCompartmentMgr,
                            public ITfSource,
                            public ITfSourceSingle,
                            public ITfKeystrokeMgr {
public:
    MockThreadMgr() : m_cRef(1), m_doc(nullptr), m_funcProv(nullptr), m_keySink(nullptr) {}
    ~MockThreadMgr()
    {
        if (m_doc) m_doc->Release();
        if (m_funcProv) m_funcProv->Release();
        if (m_keySink) m_keySink->Release();
        for (auto& kv : m_tmSinks) kv.second->Release();
    }
    void SetDocMgr(MockDocumentMgr* d)
    {
        if (m_doc) m_doc->Release();
        m_doc = d;
        if (m_doc) m_doc->AddRef();
    }
    MockCompartment* Compartment(REFGUID g) { return m_comps.Get(g); }
    MockCompartment* FindCompartment(REFGUID g) const { return m_comps.Find(g); }
    ITfFunctionProvider* AdvisedFunctionProvider() const { return m_funcProv; }
    size_t TmSinkCount() const { return m_tmSinks.size(); }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppv) override
    {
        if (!ppv) return E_POINTER;
        *ppv = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_ITfThreadMgr)) {
            *ppv = static_cast<ITfThreadMgr*>(this);
        } else if (IsEqualIID(riid, IID_ITfCompartmentMgr)) {
            *ppv = static_cast<ITfCompartmentMgr*>(this);
        } else if (IsEqualIID(riid, IID_ITfSource)) {
            *ppv = static_cast<ITfSource*>(this);
        } else if (IsEqualIID(riid, IID_ITfSourceSingle)) {
            *ppv = static_cast<ITfSourceSingle*>(this);
        } else if (IsEqualIID(riid, IID_ITfKeystrokeMgr)) {
            *ppv = static_cast<ITfKeystrokeMgr*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        ULONG r = InterlockedDecrement(&m_cRef);
        if (r == 0) delete this;
        return r;
    }

    // ITfThreadMgr
    STDMETHODIMP Activate(TfClientId* ptid) override { if (ptid) *ptid = 1; return S_OK; }
    STDMETHODIMP Deactivate() override { return S_OK; }
    STDMETHODIMP CreateDocumentMgr(ITfDocumentMgr**) override { return E_NOTIMPL; }
    STDMETHODIMP EnumDocumentMgrs(IEnumTfDocumentMgrs**) override { return E_NOTIMPL; }
    STDMETHODIMP GetFocus(ITfDocumentMgr** ppdim) override
    {
        if (!ppdim) return E_POINTER;
        *ppdim = m_doc;
        if (m_doc) m_doc->AddRef();
        return m_doc ? S_OK : S_FALSE;
    }
    STDMETHODIMP SetFocus(ITfDocumentMgr*) override { return S_OK; }
    STDMETHODIMP AssociateFocus(HWND, ITfDocumentMgr*, ITfDocumentMgr**) override { return S_OK; }
    STDMETHODIMP IsThreadFocus(BOOL* pf) override { if (pf) *pf = TRUE; return S_OK; }
    STDMETHODIMP GetFunctionProvider(REFCLSID, ITfFunctionProvider** pp) override
    {
        if (!pp) return E_POINTER;
        *pp = m_funcProv;
        if (m_funcProv) m_funcProv->AddRef();
        return m_funcProv ? S_OK : E_FAIL;
    }
    STDMETHODIMP EnumFunctionProviders(IEnumTfFunctionProviders**) override { return E_NOTIMPL; }
    STDMETHODIMP GetGlobalCompartment(ITfCompartmentMgr** pp) override { return QueryInterface(IID_ITfCompartmentMgr, reinterpret_cast<void**>(pp)); }

    // ITfCompartmentMgr
    STDMETHODIMP GetCompartment(REFGUID rguid, ITfCompartment** ppcomp) override
    {
        if (!ppcomp) return E_POINTER;
        *ppcomp = m_comps.Get(rguid);
        return S_OK;
    }
    STDMETHODIMP ClearCompartment(TfClientId, REFGUID) override { return S_OK; }
    STDMETHODIMP EnumCompartments(IEnumGUID**) override { return E_NOTIMPL; }

    // ITfSource
    STDMETHODIMP AdviseSink(REFIID riid, IUnknown* punk, DWORD* pdwCookie) override
    {
        if (!punk || !pdwCookie) return E_POINTER;
        if (IsEqualIID(riid, IID_ITfThreadMgrEventSink)) {
            punk->AddRef();
            DWORD c = ++m_nextCookie;
            m_tmSinks[c] = punk;
            *pdwCookie = c;
            return S_OK;
        }
        return E_INVALIDARG;
    }
    STDMETHODIMP UnadviseSink(DWORD dwCookie) override
    {
        auto it = m_tmSinks.find(dwCookie);
        if (it == m_tmSinks.end()) return E_INVALIDARG;
        it->second->Release();
        m_tmSinks.erase(it);
        return S_OK;
    }

    // ITfSourceSingle
    STDMETHODIMP AdviseSingleSink(TfClientId, REFIID riid, IUnknown* punk) override
    {
        if (!IsEqualIID(riid, IID_ITfFunctionProvider) || !punk) return E_INVALIDARG;
        if (m_funcProv) m_funcProv->Release();
        return punk->QueryInterface(IID_ITfFunctionProvider,
                                    reinterpret_cast<void**>(&m_funcProv));
    }
    STDMETHODIMP UnadviseSingleSink(TfClientId, REFIID riid) override
    {
        if (IsEqualIID(riid, IID_ITfFunctionProvider) && m_funcProv) {
            m_funcProv->Release();
            m_funcProv = nullptr;
        }
        return S_OK;
    }

    // ITfKeystrokeMgr
    STDMETHODIMP AdviseKeyEventSink(TfClientId, ITfKeyEventSink* pSink, BOOL) override
    {
        if (m_keySink) m_keySink->Release();
        m_keySink = pSink;
        if (m_keySink) m_keySink->AddRef();
        return S_OK;
    }
    STDMETHODIMP UnadviseKeyEventSink(TfClientId) override
    {
        if (m_keySink) { m_keySink->Release(); m_keySink = nullptr; }
        return S_OK;
    }
    STDMETHODIMP GetForeground(CLSID*) override { return E_NOTIMPL; }
    STDMETHODIMP TestKeyDown(WPARAM, LPARAM, BOOL*) override { return E_NOTIMPL; }
    STDMETHODIMP TestKeyUp(WPARAM, LPARAM, BOOL*) override { return E_NOTIMPL; }
    STDMETHODIMP KeyDown(WPARAM, LPARAM, BOOL*) override { return E_NOTIMPL; }
    STDMETHODIMP KeyUp(WPARAM, LPARAM, BOOL*) override { return E_NOTIMPL; }
    STDMETHODIMP GetPreservedKey(ITfContext*, const TF_PRESERVEDKEY*, GUID*) override { return E_NOTIMPL; }
    STDMETHODIMP IsPreservedKey(REFGUID, const TF_PRESERVEDKEY*, BOOL*) override { return E_NOTIMPL; }
    STDMETHODIMP PreserveKey(TfClientId, REFGUID, const TF_PRESERVEDKEY*, const WCHAR*, ULONG) override { return E_NOTIMPL; }
    STDMETHODIMP UnpreserveKey(REFGUID, const TF_PRESERVEDKEY*) override { return E_NOTIMPL; }
    STDMETHODIMP SetPreservedKeyDescription(REFGUID, const WCHAR*, ULONG) override { return E_NOTIMPL; }
    STDMETHODIMP GetPreservedKeyDescription(REFGUID, BSTR*) override { return E_NOTIMPL; }
    STDMETHODIMP SimulatePreservedKey(ITfContext*, REFGUID, BOOL*) override { return E_NOTIMPL; }

private:
    LONG m_cRef;
    MockDocumentMgr* m_doc;
    MockCompartmentStore m_comps;
    DWORD m_nextCookie = 0;
    std::map<DWORD, IUnknown*> m_tmSinks;
    ITfFunctionProvider* m_funcProv;
    ITfKeyEventSink* m_keySink;
};

LONG ReadCompInt(MockCompartment* comp, LONG fallback = -1)
{
    if (!comp) return fallback;
    VARIANT v;
    VariantInit(&v);
    LONG out = fallback;
    if (SUCCEEDED(comp->GetValue(&v)) && v.vt == VT_I4) {
        out = v.lVal;
    }
    VariantClear(&v);
    return out;
}

void WriteCompInt(MockCompartment* comp, LONG val)
{
    VARIANT v;
    VariantInit(&v);
    v.vt = VT_I4;
    v.lVal = val;
    comp->SetValue(99, &v);
    VariantClear(&v);
}

void TestStep1PureHelpers()
{
    Check(taishen::IsAsciiRequiredInputScope(IS_PASSWORD), "IS_PASSWORD -> ascii");
    Check(taishen::IsAsciiRequiredInputScope(IS_NUMERIC_PASSWORD), "IS_NUMERIC_PASSWORD -> ascii");
    Check(taishen::IsAsciiRequiredInputScope(IS_ALPHANUMERIC_PIN), "IS_ALPHANUMERIC_PIN -> ascii");
    Check(!taishen::IsAsciiRequiredInputScope(IS_DEFAULT), "IS_DEFAULT keeps CN");
    Check(!taishen::IsAsciiRequiredInputScope(IS_URL), "IS_URL keeps CN");
    Check(!taishen::IsAsciiRequiredInputScope(IS_SEARCH), "IS_SEARCH keeps CN");

    Check(taishen::ComputeConversionMode({ false, false, false }) == 1025, "CN default = 1025");
    Check(taishen::ComputeConversionMode({ true, false, false }) == 0, "EN default = 0");

    const auto after0 = taishen::ApplyExternalConversionMode(0, { false, false, false });
    Check(after0.asciiMode && !after0.asciiPunct, "im-select 0 preserves asciiPunct");
    const auto after1025 = taishen::ApplyExternalConversionMode(1025, after0);
    Check(!after1025.asciiMode && !after1025.asciiPunct, "im-select 1025 returns CN + punct");
    const auto noSym = taishen::ApplyExternalConversionMode(TF_CONVERSIONMODE_NATIVE, after1025);
    Check(!noSym.asciiMode && noSym.asciiPunct, "toggling SYMBOL off sets asciiPunct");

    Check(taishen::IsSecureActivateFlags(TF_TMAE_SECUREMODE), "secure flag");
    Check(taishen::IsUiElementOnlyActivateFlags(TF_TMAE_UIELEMENTENABLEDONLY), "uielement flag");
    Check(taishen::IsComlessActivateFlags(TF_TMAE_COMLESS), "comless flag");
    Check(taishen::IsConvertibleReconvertText(L"你好"), "CN convertible");
    Check(!taishen::IsConvertibleReconvertText(L"hello"), "EN not convertible");
    std::printf("STEP1 Pure Helpers OK\n");
}

void TestStep2And3ComContracts()
{
    HMODULE hDll = LoadLibraryW(L"taishen_ime.dll");
    Check(hDll != nullptr, "LoadLibraryW(taishen_ime.dll)");
    if (!hDll) return;

    using FnGetClass = HRESULT(STDAPICALLTYPE*)(REFCLSID, REFIID, LPVOID*);
    auto pGetClass = reinterpret_cast<FnGetClass>(GetProcAddress(hDll, "DllGetClassObject"));
    Check(pGetClass != nullptr, "DllGetClassObject resolved");
    if (!pGetClass) { FreeLibrary(hDll); return; }

    IClassFactory* pFactory = nullptr;
    Check(SUCCEEDED(pGetClass(CLSID_TAISHEN_IME, IID_IClassFactory,
                              reinterpret_cast<void**>(&pFactory))) && pFactory,
          "DllGetClassObject(CLSID_TAISHEN_IME)");
    if (!pFactory) { FreeLibrary(hDll); return; }

    // A) ActivateEx(TF_TMAE_SECUREMODE | TF_TMAE_COMLESS)
    {
        ITfTextInputProcessorEx* pTip = nullptr;
        Check(SUCCEEDED(pFactory->CreateInstance(nullptr, IID_ITfTextInputProcessorEx,
                                                 reinterpret_cast<void**>(&pTip))),
              "CreateInstance ITfTextInputProcessorEx");

        ITfCompartmentEventSink* pCompSink = nullptr;
        ITfFunctionProvider* pFuncProv = nullptr;
        Check(SUCCEEDED(pTip->QueryInterface(IID_ITfCompartmentEventSink,
                                             reinterpret_cast<void**>(&pCompSink))) && pCompSink,
              "QI ITfCompartmentEventSink");
        Check(SUCCEEDED(pTip->QueryInterface(IID_ITfFunctionProvider,
                                             reinterpret_cast<void**>(&pFuncProv))) && pFuncProv,
              "QI ITfFunctionProvider");
        if (pCompSink) pCompSink->Release();
        if (pFuncProv) pFuncProv->Release();

        auto* tm = new MockThreadMgr();
        auto* ctx = new MockContext(IS_DEFAULT);
        auto* doc = new MockDocumentMgr(ctx);
        tm->SetDocMgr(doc);

        Check(SUCCEEDED(pTip->ActivateEx(tm, 7, TF_TMAE_SECUREMODE | TF_TMAE_COMLESS)),
              "ActivateEx(SECUREMODE|COMLESS)");
        ITfKeyEventSink* pKey = nullptr;
        Check(SUCCEEDED(pTip->QueryInterface(IID_ITfKeyEventSink, reinterpret_cast<void**>(&pKey))),
              "QI ITfKeyEventSink");
        BOOL eaten = TRUE;
        Check(SUCCEEDED(pKey->OnTestKeyDown(ctx, 'A', 0, &eaten)) && eaten == FALSE,
              "SecureMode OnTestKeyDown('A') -> FALSE");
        pKey->Release();

        Check(SUCCEEDED(pTip->Deactivate()), "Deactivate after SecureMode");
        auto* oc = tm->FindCompartment(GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
        Check(!oc || oc->SinkCount() == 0, "OPENCLOSE sink unadvised");
        Check(tm->TmSinkCount() == 0 && tm->AdvisedFunctionProvider() == nullptr,
              "All TSF sinks cleanly unadvised");

        doc->Release();
        ctx->Release();
        tm->Release();
        pTip->Release();
    }

    // B) Compartment, InputScope, & Reconversion under normal ActivateEx(0)
    {
        ITfTextInputProcessorEx* pTip = nullptr;
        Check(SUCCEEDED(pFactory->CreateInstance(nullptr, IID_ITfTextInputProcessorEx,
                                                 reinterpret_cast<void**>(&pTip))),
              "CreateInstance #2");
        auto* tm = new MockThreadMgr();
        auto* ctx = new MockContext(IS_DEFAULT);
        auto* doc = new MockDocumentMgr(ctx);
        tm->SetDocMgr(doc);

        Check(SUCCEEDED(pTip->ActivateEx(tm, 8, 0)), "ActivateEx(0)");
        MockCompartment* oc = tm->Compartment(GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
        MockCompartment* conv = tm->Compartment(GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION);
        Check(ReadCompInt(oc) == 1, "Initial OPENCLOSE == 1");
        Check(ReadCompInt(conv) == 1025, "Initial CONVERSION == 1025");

        // External write CONVERSION=0 (im-select 0 -> English)
        WriteCompInt(conv, 0);
        Check(ReadCompInt(oc) == 0, "External CONVERSION=0 -> OPENCLOSE=0");
        ITfKeyEventSink* pKey = nullptr;
        ITfThreadMgrEventSink* pTmSink = nullptr;
        pTip->QueryInterface(IID_ITfKeyEventSink, reinterpret_cast<void**>(&pKey));
        pTip->QueryInterface(IID_ITfThreadMgrEventSink, reinterpret_cast<void**>(&pTmSink));
        BOOL eaten = TRUE;
        pKey->OnTestKeyDown(ctx, 'A', 0, &eaten);
        Check(eaten == FALSE, "English compartment OnTestKeyDown('A') -> FALSE");

        // External write CONVERSION=1025 (im-select 1025 -> Chinese)
        WriteCompInt(conv, 1025);
        Check(ReadCompInt(oc) == 1, "External CONVERSION=1025 -> OPENCLOSE=1");

        // Context GUID_COMPARTMENT_KEYBOARD_DISABLED=1
        MockCompartment* ctxDis = ctx->Compartment(GUID_COMPARTMENT_KEYBOARD_DISABLED);
        WriteCompInt(ctxDis, 1);
        eaten = TRUE;
        pKey->OnTestKeyDown(ctx, 'A', 0, &eaten);
        Check(eaten == FALSE, "Disabled context passes key through");
        ctxDis->Release();
        WriteCompInt(ctxDis = ctx->Compartment(GUID_COMPARTMENT_KEYBOARD_DISABLED), 0);
        ctxDis->Release();

        // Password InputScope on fresh context leaves global OPENCLOSE=1 untouched
        auto* pwCtx = new MockContext(IS_PASSWORD);
        auto* pwDoc = new MockDocumentMgr(pwCtx);
        pTmSink->OnSetFocus(pwDoc, doc);
        eaten = TRUE;
        pKey->OnTestKeyDown(pwCtx, 'A', 0, &eaten);
        Check(eaten == FALSE, "IS_PASSWORD context OnTestKeyDown('A') -> FALSE");
        Check(ReadCompInt(oc) == 1, "Global OPENCLOSE stays 1 during password box");
        pTmSink->OnSetFocus(doc, pwDoc);

        // ITfFnReconversion via advised ITfFunctionProvider
        ITfFunctionProvider* fp = tm->AdvisedFunctionProvider();
        Check(fp != nullptr, "Advised ITfFunctionProvider present");
        if (fp) {
            ITfFnReconversion* pReconv = nullptr;
            Check(SUCCEEDED(fp->GetFunction(GUID_NULL, IID_ITfFnReconversion,
                                            reinterpret_cast<IUnknown**>(&pReconv))) && pReconv,
                  "GetFunction(IID_ITfFnReconversion)");
            if (pReconv) {
                auto* rangeCn = new MockRange(ctx, L"你好");
                auto* rangeEn = new MockRange(ctx, L"hello");
                BOOL convCn = FALSE, convEn = TRUE;
                ITfRange* newRange = nullptr;
                Check(SUCCEEDED(pReconv->QueryRange(rangeCn, &newRange, &convCn)) && convCn == TRUE && newRange != nullptr,
                      "QueryRange CN == TRUE");
                if (newRange) newRange->Release();
                Check(SUCCEEDED(pReconv->QueryRange(rangeEn, nullptr, &convEn)) && convEn == FALSE,
                      "QueryRange EN == FALSE");

                ITfCandidateList* pCandList = nullptr;
                Check(SUCCEEDED(pReconv->GetReconversion(rangeCn, &pCandList)) && pCandList != nullptr,
                      "GetReconversion succeeds");
                if (pCandList) {
                    ULONG cnt = 0;
                    pCandList->GetCandidateNum(&cnt);
                    Check(cnt >= 1, "GetCandidateNum >= 1");
                    Check(SUCCEEDED(pCandList->SetResult(0, CAND_FINALIZED)),
                          "SetResult(0, CAND_FINALIZED)");
                    Check(!rangeCn->Text().empty(), "Range updated after reconvert SetResult");
                    pCandList->Release();
                }
                rangeCn->Release();
                rangeEn->Release();
                pReconv->Release();
            }
        }

        oc->Release();
        conv->Release();
        pwDoc->Release();
        pwCtx->Release();
        pTmSink->Release();
        pKey->Release();
        pTip->Deactivate();
        doc->Release();
        ctx->Release();
        tm->Release();
        pTip->Release();
    }

    pFactory->Release();
    FreeLibrary(hDll);
    std::printf("STEP2-3 COM Contracts OK\n");
}

void TestStep4EngineAndIpc()
{
    Check(engine_init(nullptr) == 0, "engine_init(nullptr)");
    char pyBuf[64] = {0};
    char candsBuf[1024] = {0};
    const int n = engine_reconvert(
        "\xE5\x86\x8D\xE8\xA7\x81", 9, pyBuf, sizeof(pyBuf), candsBuf, sizeof(candsBuf));
    Check(n >= 1 && std::string(pyBuf) == "zaijian" &&
              std::string(candsBuf).find("\xE5\x86\x8D\xE8\xA7\x81") != std::string::npos,
          "engine_reconvert UTF-8 再见 roundtrip");

    taishen::ipc::ReconvertRequest req;
    req.sessionId = 42;
    req.hostPid = 1234;
    req.text = "\xE4\xBD\xA0\xE5\xA5\xBD";
    req.maxCount = 5;
    taishen::ipc::ReconvertRequest decReq;
    Check(taishen::ipc::DecodeReconvertRequest(taishen::ipc::EncodeReconvertRequest(req), decReq) &&
              decReq.text == req.text && decReq.maxCount == 5,
          "IPC ReconvertRequest JSON roundtrip");

    taishen::ipc::ReconvertResponse resp;
    resp.ok = true;
    resp.pinyin = "nihao";
    resp.candidates = { "\xE4\xBD\xA0\xE5\xA5\xBD", "\xE4\xBD\xA0\xE8\xB1\xAA" };
    taishen::ipc::ReconvertResponse decResp;
    Check(taishen::ipc::DecodeReconvertResponse(taishen::ipc::EncodeReconvertResponse(resp), decResp) &&
              decResp.ok && decResp.candidates.size() == 2,
          "IPC ReconvertResponse JSON roundtrip");
    engine_destroy();
    std::printf("STEP4 Engine + IPC Reconvert OK\n");
}

void TestStep5CandidateUia()
{
    taishen::CCandidateWindow win;
    int clicked = -1;
    win.SetClickCallback([&](int idx) { clicked = idx; });
    int outW = 0, outH = 0;
    win.PrepareLayoutForTest(
        "nihao", { "\xE4\xBD\xA0\xE5\xA5\xBD", "\xE6\x8B\x9F\xE5\xA5\xBD" }, 0, 1, outW, outH);
    win.SetSelectedIndex(1);

    IRawElementProviderSimple* pRoot = nullptr;
    Check(SUCCEEDED(win.GetUiaProviderForTest(&pRoot)) && pRoot != nullptr,
          "GetUiaProviderForTest");
    if (!pRoot) return;

    VARIANT vt{};
    pRoot->GetPropertyValue(UIA_ControlTypePropertyId, &vt);
    Check(vt.vt == VT_I4 && vt.lVal == UIA_ListControlTypeId, "UIA root == ListControl");
    VariantClear(&vt);

    IUnknown* pUnkSel = nullptr;
    ISelectionProvider* pSelProv = nullptr;
    Check(SUCCEEDED(pRoot->GetPatternProvider(UIA_SelectionPatternId, &pUnkSel)) && pUnkSel,
          "Root SelectionPatternId");
    if (pUnkSel) {
        pUnkSel->QueryInterface(IID_ISelectionProvider, reinterpret_cast<void**>(&pSelProv));
        pUnkSel->Release();
    }
    if (pSelProv) {
        SAFEARRAY* sa = nullptr;
        Check(SUCCEEDED(pSelProv->GetSelection(&sa)) && sa != nullptr, "GetSelection");
        if (sa) SafeArrayDestroy(sa);
        pSelProv->Release();
    }

    IRawElementProviderFragment* pRootFrag = nullptr;
    IRawElementProviderFragment* pChild0 = nullptr;
    IRawElementProviderFragment* pChild1 = nullptr;
    pRoot->QueryInterface(IID_IRawElementProviderFragment, reinterpret_cast<void**>(&pRootFrag));
    if (pRootFrag) {
        pRootFrag->Navigate(NavigateDirection_FirstChild, &pChild0);
        if (pChild0) pChild0->Navigate(NavigateDirection_NextSibling, &pChild1);
        pRootFrag->Release();
    }
    Check(pChild0 != nullptr && pChild1 != nullptr, "Navigate FirstChild -> NextSibling");
    if (pChild1) {
        ISelectionItemProvider* pItemSel = nullptr;
        IUnknown* pUnkItem = nullptr;
        IRawElementProviderSimple* pChildSimple = nullptr;
        pChild1->QueryInterface(IID_IRawElementProviderSimple, reinterpret_cast<void**>(&pChildSimple));
        if (pChildSimple) {
            pChildSimple->GetPatternProvider(UIA_SelectionItemPatternId, &pUnkItem);
            if (pUnkItem) {
                pUnkItem->QueryInterface(IID_ISelectionItemProvider,
                                         reinterpret_cast<void**>(&pItemSel));
                pUnkItem->Release();
            }
            pChildSimple->Release();
        }
        BOOL selected = FALSE;
        Check(pItemSel != nullptr && SUCCEEDED(pItemSel->get_IsSelected(&selected)) && selected == TRUE,
              "Child 1 IsSelected == TRUE");
        if (pItemSel) {
            Check(SUCCEEDED(pItemSel->Select()) && clicked == 1, "UIA Select() fires click callback");
            pItemSel->Release();
        }
    }
    if (pChild0) pChild0->Release();
    if (pChild1) pChild1->Release();
    pRoot->Release();
    std::printf("STEP5 UIA Provider OK\n");
}

} // namespace

int wmain()
{
    CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    TestStep1PureHelpers();
    TestStep2And3ComContracts();
    TestStep4EngineAndIpc();
    TestStep5CandidateUia();
    CoUninitialize();
    if (g_fails == 0) {
        std::printf("ALL TESTS PASSED\n");
        return 0;
    }
    std::printf("FAILED: %d checks\n", g_fails);
    return 1;
}
