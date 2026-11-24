/// Windows TSF IME — Q3.1 契约补全（ITfCompartment / ITfInputScope / ActivateEx dwFlags）
///
/// 覆盖 DEV-TRACKER: Q3.1

#include "tsf_text_service.h"

#include <new>

#include "app_container.h"
#include "app_state.h"
#include "debug_log.h"

namespace {

class ScopedBoolFlag {
public:
    explicit ScopedBoolFlag(bool& target) : m_target(target), m_prev(target)
    {
        m_target = true;
    }
    ~ScopedBoolFlag() { m_target = m_prev; }

    ScopedBoolFlag(const ScopedBoolFlag&) = delete;
    ScopedBoolFlag& operator=(const ScopedBoolFlag&) = delete;

private:
    bool& m_target;
    bool m_prev;
};

bool HasPasswordWindowStyle(HWND hwnd)
{
    if (hwnd == nullptr || !IsWindow(hwnd)) {
        return false;
    }
    const LONG_PTR style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    return (style & ES_PASSWORD) != 0;
}

bool GetCompartmentInt(ITfCompartmentMgr* pMgr, REFGUID rguid, LONG& outVal)
{
    if (pMgr == nullptr) {
        return false;
    }
    ITfCompartment* pComp = nullptr;
    if (FAILED(pMgr->GetCompartment(rguid, &pComp)) || pComp == nullptr) {
        return false;
    }
    VARIANT var;
    VariantInit(&var);
    const HRESULT hr = pComp->GetValue(&var);
    pComp->Release();
    const bool ok = SUCCEEDED(hr) && var.vt == VT_I4;
    if (ok) {
        outVal = var.lVal;
    }
    VariantClear(&var);
    return ok;
}

bool SetCompartmentIntIfChanged(ITfCompartmentMgr* pMgr,
                                TfClientId tid,
                                REFGUID rguid,
                                LONG val)
{
    if (pMgr == nullptr) {
        return false;
    }
    ITfCompartment* pComp = nullptr;
    if (FAILED(pMgr->GetCompartment(rguid, &pComp)) || pComp == nullptr) {
        return false;
    }
    VARIANT cur;
    VariantInit(&cur);
    const HRESULT hrGet = pComp->GetValue(&cur);
    const bool alreadyMatch = SUCCEEDED(hrGet) && cur.vt == VT_I4 && cur.lVal == val;
    VariantClear(&cur);
    if (alreadyMatch) {
        pComp->Release();
        return true;
    }
    VARIANT next;
    VariantInit(&next);
    next.vt = VT_I4;
    next.lVal = val;
    const HRESULT hrSet = pComp->SetValue(tid, &next);
    VariantClear(&next);
    pComp->Release();
    return SUCCEEDED(hrSet);
}

bool AdviseCompartmentSink(ITfCompartmentMgr* pMgr,
                           REFGUID rguid,
                           ITfCompartmentEventSink* pSink,
                           DWORD& outCookie)
{
    outCookie = TF_INVALID_COOKIE;
    if (pMgr == nullptr || pSink == nullptr) {
        return false;
    }
    ITfCompartment* pComp = nullptr;
    if (FAILED(pMgr->GetCompartment(rguid, &pComp)) || pComp == nullptr) {
        return false;
    }
    ITfSource* pSource = nullptr;
    const HRESULT hrQi = pComp->QueryInterface(
        IID_ITfSource, reinterpret_cast<void**>(&pSource));
    pComp->Release();
    if (FAILED(hrQi) || pSource == nullptr) {
        return false;
    }
    const HRESULT hrAdv = pSource->AdviseSink(
        IID_ITfCompartmentEventSink, pSink, &outCookie);
    pSource->Release();
    if (FAILED(hrAdv)) {
        outCookie = TF_INVALID_COOKIE;
        return false;
    }
    return true;
}

void UnadviseCompartmentSink(ITfCompartmentMgr* pMgr, REFGUID rguid, DWORD& cookie)
{
    if (cookie == TF_INVALID_COOKIE) {
        return;
    }
    const DWORD saved = cookie;
    cookie = TF_INVALID_COOKIE;
    if (pMgr == nullptr) {
        return;
    }
    ITfCompartment* pComp = nullptr;
    if (FAILED(pMgr->GetCompartment(rguid, &pComp)) || pComp == nullptr) {
        return;
    }
    ITfSource* pSource = nullptr;
    if (SUCCEEDED(pComp->QueryInterface(
            IID_ITfSource, reinterpret_cast<void**>(&pSource))) &&
        pSource != nullptr) {
        pSource->UnadviseSink(saved);
        pSource->Release();
    }
    pComp->Release();
}

} // namespace

// ---------------------------------------------------------------------------
// CEditSessionInputScope — 在受控读锁内探测当前上下文的 InputScope
// ---------------------------------------------------------------------------

class CTextService::CEditSessionInputScope final : public ITfEditSession {
public:
    CEditSessionInputScope(CTextService* owner, ITfContext* pic, bool passwordStyle)
        : m_cRef(1), m_pOwner(owner), m_pic(pic), m_passwordStyle(passwordStyle)
    {
        if (m_pOwner != nullptr) {
            m_pOwner->AddRef();
        }
        if (m_pic != nullptr) {
            m_pic->AddRef();
        }
    }

    ~CEditSessionInputScope()
    {
        if (m_pic != nullptr) {
            m_pic->Release();
        }
        if (m_pOwner != nullptr) {
            m_pOwner->Release();
        }
    }

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
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
        if (ref == 0) {
            delete this;
        }
        return ref;
    }

    STDMETHODIMP DoEditSession(TfEditCookie ec) override
    {
        bool asciiRequired = m_passwordStyle;
        if (!asciiRequired && m_pic != nullptr) {
            ITfContextView* pView = nullptr;
            if (SUCCEEDED(m_pic->GetActiveView(&pView)) && pView != nullptr) {
                HWND hwnd = nullptr;
                if (SUCCEEDED(pView->GetWnd(&hwnd)) && HasPasswordWindowStyle(hwnd)) {
                    asciiRequired = true;
                }
                pView->Release();
            }
        }

        if (!asciiRequired && m_pic != nullptr) {
            ITfProperty* pProp = nullptr;
            if (SUCCEEDED(m_pic->GetProperty(taishen::kGuidPropInputScope, &pProp)) &&
                pProp != nullptr) {
                TF_SELECTION sel{};
                ULONG fetched = 0;
                if (SUCCEEDED(m_pic->GetSelection(
                        ec, TF_DEFAULT_SELECTION, 1, &sel, &fetched)) &&
                    fetched > 0 && sel.range != nullptr) {
                    VARIANT var;
                    VariantInit(&var);
                    if (SUCCEEDED(pProp->GetValue(ec, sel.range, &var)) &&
                        var.vt == VT_UNKNOWN && var.punkVal != nullptr) {
                        ITfInputScope* pScope = nullptr;
                        if (SUCCEEDED(var.punkVal->QueryInterface(
                                IID_ITfInputScope,
                                reinterpret_cast<void**>(&pScope))) &&
                            pScope != nullptr) {
                            InputScope* pScopes = nullptr;
                            UINT cScopes = 0;
                            if (SUCCEEDED(pScope->GetInputScopes(&pScopes, &cScopes)) &&
                                pScopes != nullptr) {
                                for (UINT i = 0; i < cScopes; ++i) {
                                    if (taishen::IsAsciiRequiredInputScope(pScopes[i])) {
                                        asciiRequired = true;
                                        break;
                                    }
                                }
                                CoTaskMemFree(pScopes);
                            }
                            pScope->Release();
                        }
                    }
                    VariantClear(&var);
                    sel.range->Release();
                }
                pProp->Release();
            }
        }

        if (m_pOwner != nullptr) {
            m_pOwner->OnInputScopeResolved(m_pic, asciiRequired);
        }
        return S_OK;
    }

private:
    LONG m_cRef;
    CTextService* m_pOwner;
    ITfContext* m_pic;
    bool m_passwordStyle;
};

// ---------------------------------------------------------------------------
// ITfCompartment & ActivateEx dwFlags
// ---------------------------------------------------------------------------

void CTextService::InitCompartments()
{
    if (m_pThreadMgr == nullptr) {
        return;
    }
    ITfCompartmentMgr* pMgr = nullptr;
    if (FAILED(m_pThreadMgr->QueryInterface(
            IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) ||
        pMgr == nullptr) {
        return;
    }

    AdviseCompartmentSink(pMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                          static_cast<ITfCompartmentEventSink*>(this),
                          m_dwOpenCloseSinkCookie);
    AdviseCompartmentSink(pMgr, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                          static_cast<ITfCompartmentEventSink*>(this),
                          m_dwConvModeSinkCookie);

    ScopedBoolFlag guard(m_updatingCompartments);
    if (!IsSecureMode()) {
        LONG openClose = 0;
        if (GetCompartmentInt(pMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, openClose)) {
            m_remoteState.asciiMode = (openClose == 0);
            taishen::AppStateSetAsciiRemote(m_remoteState.asciiMode);
        }
        LONG convMode = 0;
        if (GetCompartmentInt(pMgr, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                              convMode)) {
            taishen::ConversionState st{};
            st.asciiMode = m_remoteState.asciiMode;
            st.fullWidth = taishen::IsFullWidthMode();
            st.asciiPunct = m_remoteState.asciiPunct;
            st = taishen::ApplyExternalConversionMode(convMode, st);
            m_remoteState.asciiMode = st.asciiMode;
            taishen::SetFullWidthMode(st.fullWidth);
            m_remoteState.asciiPunct = st.asciiPunct;
            taishen::AppStateSetAsciiRemote(st.asciiMode);
        }
    }

    taishen::ConversionState syncSt{};
    syncSt.asciiMode = m_remoteState.asciiMode || IsSecureMode();
    syncSt.fullWidth = taishen::IsFullWidthMode();
    syncSt.asciiPunct = m_remoteState.asciiPunct;
    SetCompartmentIntIfChanged(pMgr, m_tid, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                               syncSt.asciiMode ? 0 : 1);
    SetCompartmentIntIfChanged(pMgr, m_tid,
                               GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                               taishen::ComputeConversionMode(syncSt));
    pMgr->Release();
}

void CTextService::UninitCompartments()
{
    if (m_pThreadMgr == nullptr) {
        m_dwOpenCloseSinkCookie = TF_INVALID_COOKIE;
        m_dwConvModeSinkCookie = TF_INVALID_COOKIE;
        return;
    }
    ITfCompartmentMgr* pMgr = nullptr;
    if (SUCCEEDED(m_pThreadMgr->QueryInterface(
            IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) &&
        pMgr != nullptr) {
        UnadviseCompartmentSink(pMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                                m_dwOpenCloseSinkCookie);
        UnadviseCompartmentSink(pMgr,
                                GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                                m_dwConvModeSinkCookie);
        pMgr->Release();
    } else {
        m_dwOpenCloseSinkCookie = TF_INVALID_COOKIE;
        m_dwConvModeSinkCookie = TF_INVALID_COOKIE;
    }
}

void CTextService::SyncCompartmentsFromState()
{
    if (m_pThreadMgr == nullptr || !m_fActive || m_updatingCompartments) {
        return;
    }
    ITfCompartmentMgr* pMgr = nullptr;
    if (FAILED(m_pThreadMgr->QueryInterface(
            IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) ||
        pMgr == nullptr) {
        return;
    }
    ScopedBoolFlag guard(m_updatingCompartments);
    taishen::ConversionState st{};
    st.asciiMode = m_remoteState.asciiMode || IsSecureMode();
    st.fullWidth = taishen::IsFullWidthMode();
    st.asciiPunct = m_remoteState.asciiPunct;
    SetCompartmentIntIfChanged(pMgr, m_tid, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                               st.asciiMode ? 0 : 1);
    SetCompartmentIntIfChanged(pMgr, m_tid,
                               GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                               taishen::ComputeConversionMode(st));
    pMgr->Release();
}

void CTextService::ApplyExternalAsciiMode(bool wantAscii,
                                          bool syncOpenClose,
                                          bool syncConv)
{
    if (wantAscii && (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose)) {
        if (m_pFocusContext != nullptr && m_composition.IsActive()) {
            RunCompositionOp(m_pFocusContext, CEditSessionComposition::Op::Commit, "");
        }
        SendRemoteResetCommit();
        ClearRemoteState();
        HideCandidateUi();
    }
    m_remoteState.asciiMode = wantAscii;
    taishen::AppStateSetAsciiRemote(wantAscii);

    if (m_pThreadMgr != nullptr && (syncOpenClose || syncConv)) {
        ITfCompartmentMgr* pMgr = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) &&
            pMgr != nullptr) {
            ScopedBoolFlag guard(m_updatingCompartments);
            taishen::ConversionState st{};
            st.asciiMode = m_remoteState.asciiMode || IsSecureMode();
            st.fullWidth = taishen::IsFullWidthMode();
            st.asciiPunct = m_remoteState.asciiPunct;
            if (syncOpenClose) {
                SetCompartmentIntIfChanged(pMgr, m_tid,
                                           GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
                                           st.asciiMode ? 0 : 1);
            }
            if (syncConv) {
                SetCompartmentIntIfChanged(
                    pMgr, m_tid, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                    taishen::ComputeConversionMode(st));
            }
            pMgr->Release();
        }
    }
    RefreshLangBarItem();
}

STDMETHODIMP CTextService::OnChange(REFGUID rguid)
{
    if (m_updatingCompartments || !m_fActive || m_pThreadMgr == nullptr ||
        IsSecureMode()) {
        return S_OK;
    }
    ITfCompartmentMgr* pMgr = nullptr;
    if (FAILED(m_pThreadMgr->QueryInterface(
            IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) ||
        pMgr == nullptr) {
        return S_OK;
    }

    if (IsEqualGUID(rguid, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)) {
        LONG openClose = 1;
        if (GetCompartmentInt(pMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, openClose)) {
            ApplyExternalAsciiMode(openClose == 0, false, true);
        }
    } else if (IsEqualGUID(rguid, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION)) {
        LONG convMode = 0;
        if (GetCompartmentInt(pMgr,
                              GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
                              convMode)) {
            taishen::ConversionState cur{};
            cur.asciiMode = m_remoteState.asciiMode;
            cur.fullWidth = taishen::IsFullWidthMode();
            cur.asciiPunct = m_remoteState.asciiPunct;
            const taishen::ConversionState next =
                taishen::ApplyExternalConversionMode(convMode, cur);
            taishen::SetFullWidthMode(next.fullWidth);
            m_remoteState.asciiPunct = next.asciiPunct;
            m_appCfg.ascii_punct = next.asciiPunct;
            ApplyExternalAsciiMode(next.asciiMode, true, false);
        }
    }

    pMgr->Release();
    return S_OK;
}

bool CTextService::IsContextKeyboardDisabled(ITfContext* pic) const
{
    if (pic == nullptr) {
        return false;
    }
    ITfCompartmentMgr* pMgr = nullptr;
    if (FAILED(pic->QueryInterface(
            IID_ITfCompartmentMgr, reinterpret_cast<void**>(&pMgr))) ||
        pMgr == nullptr) {
        return false;
    }
    LONG val = 0;
    const bool disabled =
        (GetCompartmentInt(pMgr, GUID_COMPARTMENT_KEYBOARD_DISABLED, val) &&
         val != 0) ||
        (GetCompartmentInt(pMgr, GUID_COMPARTMENT_EMPTYCONTEXT, val) && val != 0);
    pMgr->Release();
    return disabled;
}

void CTextService::QueryContextInputScope(ITfContext* pic)
{
    if (pic == nullptr || !m_fActive) {
        m_inputScopeAscii = false;
        m_lastInputScopeContext = nullptr;
        return;
    }
    const bool passwordStyle = HasPasswordWindowStyle(GetFocus());
    auto* session = new (std::nothrow)
        CEditSessionInputScope(this, pic, passwordStyle);
    if (session == nullptr) {
        return;
    }
    const DWORD flags = taishen::IsAppContainerProcess()
                            ? (TF_ES_ASYNC | TF_ES_READ)
                            : (TF_ES_ASYNCDONTCARE | TF_ES_READ);
    HRESULT hrSession = E_FAIL;
    const HRESULT hr = pic->RequestEditSession(m_tid, session, flags, &hrSession);
    session->Release();
    if (FAILED(hr)) {
        OnInputScopeResolved(pic, passwordStyle);
    }
}

void CTextService::OnInputScopeResolved(ITfContext* pic, bool asciiRequired)
{
    if (m_pFocusContext != nullptr && pic != nullptr && pic != m_pFocusContext) {
        return;
    }
    m_lastInputScopeContext = pic;
    const bool prev = m_inputScopeAscii;
    m_inputScopeAscii = asciiRequired;
    if (asciiRequired && (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose)) {
        if (m_pFocusContext != nullptr && m_composition.IsActive()) {
            RunCompositionOp(m_pFocusContext, CEditSessionComposition::Op::Commit, "");
        }
        SendRemoteResetCommit();
        ClearRemoteState();
        HideCandidateUi();
    }
    if (prev != asciiRequired) {
        taishen::DebugLog(std::string("InputScope: asciiRequired=") +
                          (asciiRequired ? "1" : "0"));
        RefreshLangBarItem();
    }
}

bool CTextService::IsEffectiveAsciiMode() const
{
    return m_remoteState.asciiMode || m_inputScopeAscii || IsSecureMode();
}
