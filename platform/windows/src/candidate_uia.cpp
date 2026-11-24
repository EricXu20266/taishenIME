/// Direct2D 候选窗口 UI Automation (UIA) 提供器实现（Q3.1）
///
/// 覆盖：
///   - Root：IRawElementProviderSimple + IRawElementProviderFragment + IRawElementProviderFragmentRoot + ISelectionProvider
///   - Item：IRawElementProviderSimple + IRawElementProviderFragment + ISelectionItemProvider
///   - 动态绑定 uiautomationcore.dll API（UiaReturnRawElementProvider / UiaClientsAreListening 等）

#include "candidate_uia.h"

#include <atomic>
#include <new>

namespace taishen {

namespace {

struct UiaCoreApi {
    using FnReturnRaw = LRESULT(WINAPI*)(HWND, WPARAM, LPARAM, IRawElementProviderSimple*);
    using FnListening = BOOL(WINAPI*)();
    using FnRaiseEvent = HRESULT(WINAPI*)(IRawElementProviderSimple*, EVENTID);
    using FnRaiseStruct = HRESULT(WINAPI*)(IRawElementProviderSimple*,
                                           enum StructureChangeType,
                                           int*,
                                           int);
    using FnDisconnect = HRESULT(WINAPI*)(IRawElementProviderSimple*);
    using FnHostFromHwnd = HRESULT(WINAPI*)(HWND, IRawElementProviderSimple**);

    FnReturnRaw returnRaw = nullptr;
    FnListening clientsListening = nullptr;
    FnRaiseEvent raiseEvent = nullptr;
    FnRaiseStruct raiseStruct = nullptr;
    FnDisconnect disconnect = nullptr;
    FnHostFromHwnd hostFromHwnd = nullptr;
};

template <typename Fn>
void LoadProc(HMODULE mod, const char* name, Fn& out)
{
    out = reinterpret_cast<Fn>(GetProcAddress(mod, name));
}

const UiaCoreApi& GetUiaApi(bool allowLoad)
{
    static UiaCoreApi api{};
    static std::atomic<bool> initialized{false};
    if (!initialized.load(std::memory_order_acquire)) {
        HMODULE mod = GetModuleHandleW(L"uiautomationcore.dll");
        if (mod == nullptr && allowLoad) {
            mod = LoadLibraryW(L"uiautomationcore.dll");
        }
        if (mod != nullptr) {
            LoadProc(mod, "UiaReturnRawElementProvider", api.returnRaw);
            LoadProc(mod, "UiaClientsAreListening", api.clientsListening);
            LoadProc(mod, "UiaRaiseAutomationEvent", api.raiseEvent);
            LoadProc(mod, "UiaRaiseStructureChangedEvent", api.raiseStruct);
            LoadProc(mod, "UiaDisconnectProvider", api.disconnect);
            LoadProc(mod, "UiaHostProviderFromHwnd", api.hostFromHwnd);
            initialized.store(true, std::memory_order_release);
        }
    }
    return api;
}

UiaRect RectToUiaRect(const RECT& rc)
{
    UiaRect out{};
    out.left = static_cast<double>(rc.left);
    out.top = static_cast<double>(rc.top);
    out.width = static_cast<double>(rc.right > rc.left ? rc.right - rc.left : 0);
    out.height = static_cast<double>(rc.bottom > rc.top ? rc.bottom - rc.top : 0);
    return out;
}

} // namespace

class CandidateItemUiaProvider;

class CandidateWindowUiaProvider final : public IRawElementProviderSimple,
                                         public IRawElementProviderFragment,
                                         public IRawElementProviderFragmentRoot,
                                         public ISelectionProvider {
public:
    explicit CandidateWindowUiaProvider(ICandidateUiaHost* host)
        : m_cRef(1), m_host(host)
    {
    }

    void Detach()
    {
        ICandidateUiaHost* prev = m_host;
        m_host = nullptr;
        if (prev != nullptr) {
            const auto& api = GetUiaApi(false);
            if (api.disconnect != nullptr) {
                api.disconnect(static_cast<IRawElementProviderSimple*>(this));
            }
        }
    }

    ICandidateUiaHost* Host() const { return m_host; }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IRawElementProviderSimple)) {
            *ppvObj = static_cast<IRawElementProviderSimple*>(this);
        } else if (IsEqualIID(riid, IID_IRawElementProviderFragment)) {
            *ppvObj = static_cast<IRawElementProviderFragment*>(this);
        } else if (IsEqualIID(riid, IID_IRawElementProviderFragmentRoot)) {
            *ppvObj = static_cast<IRawElementProviderFragmentRoot*>(this);
        } else if (IsEqualIID(riid, IID_ISelectionProvider)) {
            *ppvObj = static_cast<ISelectionProvider*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    // IRawElementProviderSimple
    STDMETHODIMP get_ProviderOptions(enum ProviderOptions* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = static_cast<enum ProviderOptions>(
            ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading);
        return S_OK;
    }

    STDMETHODIMP GetPatternProvider(PATTERNID patternId, IUnknown** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        if (patternId == UIA_SelectionPatternId) {
            *pRetVal = static_cast<ISelectionProvider*>(this);
            AddRef();
        }
        return S_OK;
    }

    STDMETHODIMP GetPropertyValue(PROPERTYID propertyId, VARIANT* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        VariantInit(pRetVal);
        switch (propertyId) {
        case UIA_ControlTypePropertyId:
            pRetVal->vt = VT_I4;
            pRetVal->lVal = UIA_ListControlTypeId;
            break;
        case UIA_NamePropertyId: {
            std::wstring name = L"泰深输入法候选窗口";
            if (m_host != nullptr) {
                const std::wstring py = m_host->UiaPinyin();
                if (!py.empty()) {
                    name += L" " + py;
                }
            }
            pRetVal->vt = VT_BSTR;
            pRetVal->bstrVal = SysAllocString(name.c_str());
            break;
        }
        case UIA_AutomationIdPropertyId:
            pRetVal->vt = VT_BSTR;
            pRetVal->bstrVal = SysAllocString(L"TaishenCandidateWindow");
            break;
        case UIA_ClassNamePropertyId:
            pRetVal->vt = VT_BSTR;
            pRetVal->bstrVal = SysAllocString(L"TaishenCandidateWindow");
            break;
        case UIA_IsKeyboardFocusablePropertyId:
            pRetVal->vt = VT_BOOL;
            pRetVal->boolVal = VARIANT_FALSE;
            break;
        case UIA_IsControlElementPropertyId:
        case UIA_IsContentElementPropertyId:
            pRetVal->vt = VT_BOOL;
            pRetVal->boolVal = VARIANT_TRUE;
            break;
        default:
            break;
        }
        return S_OK;
    }

    STDMETHODIMP get_HostRawElementProvider(IRawElementProviderSimple** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        if (m_host == nullptr) return S_OK;
        const HWND hwnd = m_host->UiaHwnd();
        if (hwnd == nullptr) return S_OK;
        const auto& api = GetUiaApi(true);
        if (api.hostFromHwnd != nullptr) {
            return api.hostFromHwnd(hwnd, pRetVal);
        }
        return S_OK;
    }

    // IRawElementProviderFragment
    STDMETHODIMP Navigate(enum NavigateDirection direction,
                          IRawElementProviderFragment** pRetVal) override;

    STDMETHODIMP GetRuntimeId(SAFEARRAY** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        return S_OK;
    }

    STDMETHODIMP get_BoundingRectangle(struct UiaRect* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = UiaRect{};
        if (m_host == nullptr) return S_OK;
        const HWND hwnd = m_host->UiaHwnd();
        if (hwnd != nullptr && IsWindow(hwnd)) {
            RECT rc{};
            if (GetWindowRect(hwnd, &rc)) {
                *pRetVal = RectToUiaRect(rc);
            }
        }
        return S_OK;
    }

    STDMETHODIMP GetEmbeddedFragmentRoots(SAFEARRAY** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        return S_OK;
    }

    STDMETHODIMP SetFocus() override { return S_OK; }

    STDMETHODIMP get_FragmentRoot(IRawElementProviderFragmentRoot** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = static_cast<IRawElementProviderFragmentRoot*>(this);
        AddRef();
        return S_OK;
    }

    // IRawElementProviderFragmentRoot
    STDMETHODIMP ElementProviderFromPoint(
        double x, double y, IRawElementProviderFragment** pRetVal) override;

    STDMETHODIMP GetFocus(IRawElementProviderFragment** pRetVal) override;

    // ISelectionProvider
    STDMETHODIMP GetSelection(SAFEARRAY** pRetVal) override;

    STDMETHODIMP get_CanSelectMultiple(BOOL* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = FALSE;
        return S_OK;
    }

    STDMETHODIMP get_IsSelectionRequired(BOOL* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = (m_host != nullptr && m_host->UiaCandidateCount() > 0) ? TRUE : FALSE;
        return S_OK;
    }

    IRawElementProviderFragment* CreateItemFragment(size_t index);

private:
    LONG m_cRef;
    ICandidateUiaHost* m_host;
};

class CandidateItemUiaProvider final : public IRawElementProviderSimple,
                                       public IRawElementProviderFragment,
                                       public ISelectionItemProvider {
public:
    CandidateItemUiaProvider(CandidateWindowUiaProvider* root, size_t index)
        : m_cRef(1), m_root(root), m_index(index)
    {
        if (m_root != nullptr) {
            m_root->AddRef();
        }
    }

    ~CandidateItemUiaProvider()
    {
        if (m_root != nullptr) {
            m_root->Release();
        }
    }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) return E_POINTER;
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IRawElementProviderSimple)) {
            *ppvObj = static_cast<IRawElementProviderSimple*>(this);
        } else if (IsEqualIID(riid, IID_IRawElementProviderFragment)) {
            *ppvObj = static_cast<IRawElementProviderFragment*>(this);
        } else if (IsEqualIID(riid, IID_ISelectionItemProvider)) {
            *ppvObj = static_cast<ISelectionItemProvider*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override { return InterlockedIncrement(&m_cRef); }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) delete this;
        return ref;
    }

    // IRawElementProviderSimple
    STDMETHODIMP get_ProviderOptions(enum ProviderOptions* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = static_cast<enum ProviderOptions>(
            ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading);
        return S_OK;
    }

    STDMETHODIMP GetPatternProvider(PATTERNID patternId, IUnknown** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        if (patternId == UIA_SelectionItemPatternId) {
            *pRetVal = static_cast<ISelectionItemProvider*>(this);
            AddRef();
        }
        return S_OK;
    }

    STDMETHODIMP GetPropertyValue(PROPERTYID propertyId, VARIANT* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        VariantInit(pRetVal);
        ICandidateUiaHost* host = (m_root != nullptr) ? m_root->Host() : nullptr;
        switch (propertyId) {
        case UIA_ControlTypePropertyId:
            pRetVal->vt = VT_I4;
            pRetVal->lVal = UIA_ListItemControlTypeId;
            break;
        case UIA_NamePropertyId: {
            const std::wstring txt =
                (host != nullptr && m_index < host->UiaCandidateCount())
                    ? host->UiaCandidateText(m_index)
                    : L"";
            pRetVal->vt = VT_BSTR;
            pRetVal->bstrVal = SysAllocString(txt.c_str());
            break;
        }
        case UIA_AutomationIdPropertyId: {
            const std::wstring id = L"TaishenCandidateItem_" + std::to_wstring(m_index);
            pRetVal->vt = VT_BSTR;
            pRetVal->bstrVal = SysAllocString(id.c_str());
            break;
        }
        case UIA_ItemStatusPropertyId:
            if (host != nullptr && host->UiaTotalPages() > 0) {
                const std::wstring status =
                    L"第 " + std::to_wstring(host->UiaPage() + 1) + L"/" +
                    std::to_wstring(host->UiaTotalPages()) + L" 页";
                pRetVal->vt = VT_BSTR;
                pRetVal->bstrVal = SysAllocString(status.c_str());
            }
            break;
        case UIA_IsKeyboardFocusablePropertyId:
            pRetVal->vt = VT_BOOL;
            pRetVal->boolVal = VARIANT_FALSE;
            break;
        case UIA_IsControlElementPropertyId:
        case UIA_IsContentElementPropertyId:
            pRetVal->vt = VT_BOOL;
            pRetVal->boolVal = VARIANT_TRUE;
            break;
        default:
            break;
        }
        return S_OK;
    }

    STDMETHODIMP get_HostRawElementProvider(IRawElementProviderSimple** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        return S_OK;
    }

    // IRawElementProviderFragment
    STDMETHODIMP Navigate(enum NavigateDirection direction,
                          IRawElementProviderFragment** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        if (m_root == nullptr) return S_OK;
        ICandidateUiaHost* host = m_root->Host();
        if (host == nullptr) return S_OK;
        const size_t count = host->UiaCandidateCount();
        switch (direction) {
        case NavigateDirection_Parent:
            *pRetVal = static_cast<IRawElementProviderFragment*>(m_root);
            m_root->AddRef();
            break;
        case NavigateDirection_NextSibling:
            if (m_index + 1 < count) {
                *pRetVal = m_root->CreateItemFragment(m_index + 1);
            }
            break;
        case NavigateDirection_PreviousSibling:
            if (m_index > 0 && count > 0) {
                *pRetVal = m_root->CreateItemFragment(
                    m_index - 1 < count ? m_index - 1 : count - 1);
            }
            break;
        default:
            break;
        }
        return S_OK;
    }

    STDMETHODIMP GetRuntimeId(SAFEARRAY** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        SAFEARRAY* sa = SafeArrayCreateVector(VT_I4, 0, 2);
        if (sa == nullptr) return E_OUTOFMEMORY;
        LONG idx0 = 0;
        LONG idx1 = 1;
        int val0 = UiaAppendRuntimeId;
        int val1 = static_cast<int>(m_index + 1);
        SafeArrayPutElement(sa, &idx0, &val0);
        SafeArrayPutElement(sa, &idx1, &val1);
        *pRetVal = sa;
        return S_OK;
    }

    STDMETHODIMP get_BoundingRectangle(struct UiaRect* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = UiaRect{};
        ICandidateUiaHost* host = (m_root != nullptr) ? m_root->Host() : nullptr;
        if (host != nullptr && m_index < host->UiaCandidateCount()) {
            *pRetVal = RectToUiaRect(host->UiaItemScreenRect(m_index));
        }
        return S_OK;
    }

    STDMETHODIMP GetEmbeddedFragmentRoots(SAFEARRAY** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = nullptr;
        return S_OK;
    }

    STDMETHODIMP SetFocus() override { return S_OK; }

    STDMETHODIMP get_FragmentRoot(IRawElementProviderFragmentRoot** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = static_cast<IRawElementProviderFragmentRoot*>(m_root);
        if (*pRetVal != nullptr) (*pRetVal)->AddRef();
        return S_OK;
    }

    // ISelectionItemProvider
    STDMETHODIMP Select() override
    {
        ICandidateUiaHost* host = (m_root != nullptr) ? m_root->Host() : nullptr;
        if (host == nullptr || m_index >= host->UiaCandidateCount()) {
            return E_FAIL;
        }
        host->UiaSelectCandidate(static_cast<int>(m_index));
        return S_OK;
    }

    STDMETHODIMP AddToSelection() override { return Select(); }
    STDMETHODIMP RemoveFromSelection() override { return S_OK; }

    STDMETHODIMP get_IsSelected(BOOL* pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        ICandidateUiaHost* host = (m_root != nullptr) ? m_root->Host() : nullptr;
        *pRetVal = (host != nullptr && m_index < host->UiaCandidateCount() &&
                    host->UiaSelectedIndex() == static_cast<int>(m_index))
                       ? TRUE
                       : FALSE;
        return S_OK;
    }

    STDMETHODIMP get_SelectionContainer(IRawElementProviderSimple** pRetVal) override
    {
        if (pRetVal == nullptr) return E_POINTER;
        *pRetVal = static_cast<IRawElementProviderSimple*>(m_root);
        if (*pRetVal != nullptr) (*pRetVal)->AddRef();
        return S_OK;
    }

private:
    LONG m_cRef;
    CandidateWindowUiaProvider* m_root;
    size_t m_index;
};

IRawElementProviderFragment* CandidateWindowUiaProvider::CreateItemFragment(size_t index)
{
    return new (std::nothrow) CandidateItemUiaProvider(this, index);
}

STDMETHODIMP CandidateWindowUiaProvider::Navigate(enum NavigateDirection direction,
                                                  IRawElementProviderFragment** pRetVal)
{
    if (pRetVal == nullptr) return E_POINTER;
    *pRetVal = nullptr;
    if (m_host == nullptr) return S_OK;
    const size_t count = m_host->UiaCandidateCount();
    if (count == 0) return S_OK;
    if (direction == NavigateDirection_FirstChild) {
        *pRetVal = CreateItemFragment(0);
    } else if (direction == NavigateDirection_LastChild) {
        *pRetVal = CreateItemFragment(count - 1);
    }
    return S_OK;
}

STDMETHODIMP CandidateWindowUiaProvider::ElementProviderFromPoint(
    double x, double y, IRawElementProviderFragment** pRetVal)
{
    if (pRetVal == nullptr) return E_POINTER;
    *pRetVal = nullptr;
    if (m_host == nullptr) return S_OK;
    const int hit = m_host->UiaHitTestScreenPoint(
        static_cast<int>(x), static_cast<int>(y));
    if (hit >= 0 && static_cast<size_t>(hit) < m_host->UiaCandidateCount()) {
        *pRetVal = CreateItemFragment(static_cast<size_t>(hit));
    }
    return S_OK;
}

STDMETHODIMP CandidateWindowUiaProvider::GetFocus(
    IRawElementProviderFragment** pRetVal)
{
    if (pRetVal == nullptr) return E_POINTER;
    *pRetVal = nullptr;
    if (m_host == nullptr) return S_OK;
    const size_t count = m_host->UiaCandidateCount();
    const int sel = m_host->UiaSelectedIndex();
    if (sel >= 0 && static_cast<size_t>(sel) < count) {
        *pRetVal = CreateItemFragment(static_cast<size_t>(sel));
    }
    return S_OK;
}

STDMETHODIMP CandidateWindowUiaProvider::GetSelection(SAFEARRAY** pRetVal)
{
    if (pRetVal == nullptr) return E_POINTER;
    *pRetVal = nullptr;
    if (m_host == nullptr) return S_OK;
    const size_t count = m_host->UiaCandidateCount();
    const int sel = m_host->UiaSelectedIndex();
    if (sel < 0 || static_cast<size_t>(sel) >= count) {
        *pRetVal = SafeArrayCreateVector(VT_UNKNOWN, 0, 0);
        return *pRetVal == nullptr ? E_OUTOFMEMORY : S_OK;
    }
    SAFEARRAY* sa = SafeArrayCreateVector(VT_UNKNOWN, 0, 1);
    if (sa == nullptr) return E_OUTOFMEMORY;
    IRawElementProviderFragment* frag = CreateItemFragment(static_cast<size_t>(sel));
    if (frag == nullptr) {
        SafeArrayDestroy(sa);
        return E_OUTOFMEMORY;
    }
    IUnknown* punk = static_cast<IRawElementProviderSimple*>(
        static_cast<CandidateItemUiaProvider*>(frag));
    LONG idx = 0;
    SafeArrayPutElement(sa, &idx, punk);
    frag->Release();
    *pRetVal = sa;
    return S_OK;
}

// ---------------------------------------------------------------------------
// 导出辅助接口
// ---------------------------------------------------------------------------

CandidateWindowUiaProvider* CreateCandidateWindowUiaProvider(ICandidateUiaHost* host)
{
    return new (std::nothrow) CandidateWindowUiaProvider(host);
}

void DetachCandidateWindowUiaProvider(CandidateWindowUiaProvider* provider)
{
    if (provider != nullptr) {
        provider->Detach();
    }
}

void ReleaseCandidateWindowUiaProvider(CandidateWindowUiaProvider* provider)
{
    if (provider != nullptr) {
        provider->Release();
    }
}

LRESULT HandleCandidateUiaWmGetObject(CandidateWindowUiaProvider* provider,
                                      HWND hwnd,
                                      WPARAM wParam,
                                      LPARAM lParam)
{
    if (provider == nullptr || hwnd == nullptr ||
        static_cast<LONG>(lParam) != UiaRootObjectId) {
        return 0;
    }
    const auto& api = GetUiaApi(true);
    if (api.returnRaw == nullptr) {
        return 0;
    }
    return api.returnRaw(hwnd, wParam, lParam,
                         static_cast<IRawElementProviderSimple*>(provider));
}

HRESULT QueryCandidateWindowUiaSimple(CandidateWindowUiaProvider* provider,
                                      IRawElementProviderSimple** ppOut)
{
    if (ppOut == nullptr) return E_POINTER;
    *ppOut = nullptr;
    if (provider == nullptr) return E_UNEXPECTED;
    return provider->QueryInterface(IID_IRawElementProviderSimple,
                                    reinterpret_cast<void**>(ppOut));
}

void NotifyCandidateUiaUpdated(CandidateWindowUiaProvider* provider)
{
    if (provider == nullptr) return;
    const auto& api = GetUiaApi(false);
    if (api.clientsListening == nullptr || !api.clientsListening()) {
        return;
    }
    if (api.raiseStruct != nullptr) {
        api.raiseStruct(static_cast<IRawElementProviderSimple*>(provider),
                        StructureChangeType_ChildrenInvalidated, nullptr, 0);
    }
    if (api.raiseEvent != nullptr) {
        api.raiseEvent(static_cast<IRawElementProviderSimple*>(provider),
                       UIA_LayoutInvalidatedEventId);
    }
}

void NotifyCandidateUiaSelectionChanged(CandidateWindowUiaProvider* provider)
{
    if (provider == nullptr) return;
    const auto& api = GetUiaApi(false);
    if (api.clientsListening == nullptr || !api.clientsListening() ||
        api.raiseEvent == nullptr) {
        return;
    }
    ICandidateUiaHost* host = provider->Host();
    if (host == nullptr) return;
    const int sel = host->UiaSelectedIndex();
    if (sel < 0 || static_cast<size_t>(sel) >= host->UiaCandidateCount()) {
        return;
    }
    IRawElementProviderFragment* frag =
        provider->CreateItemFragment(static_cast<size_t>(sel));
    if (frag != nullptr) {
        api.raiseEvent(
            static_cast<IRawElementProviderSimple*>(
                static_cast<CandidateItemUiaProvider*>(frag)),
            UIA_SelectionItem_ElementSelectedEventId);
        frag->Release();
    }
}

} // namespace taishen
