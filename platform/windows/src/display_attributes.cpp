/// 组合串分段显示属性 — 实现

#include "display_attributes.h"
#include "debug_log.h"

#include <new>

namespace taishen {

// 泰深自定义显示属性 GUID（固定值，注册/应用两端必须一致）
const GUID GUID_TAISHEN_ATTR_CONFIRMED =
    {0x8f3a1c01, 0x7b2e, 0x4c55,
     {0x9a, 0x11, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70}};
const GUID GUID_TAISHEN_ATTR_PENDING =
    {0x8f3a1c02, 0x7b2e, 0x4c55,
     {0x9a, 0x11, 0x2b, 0x3c, 0x4d, 0x5e, 0x6f, 0x70}};

// ---------------------------------------------------------------------------
// CDisplayAttributeInfo — 单个属性的描述对象
// ---------------------------------------------------------------------------
class CDisplayAttributeInfo final : public ITfDisplayAttributeInfo
{
public:
    explicit CDisplayAttributeInfo(const GUID& guid) : m_cRef(1), m_guid(guid) {}

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_ITfDisplayAttributeInfo)) {
            *ppvObj = static_cast<ITfDisplayAttributeInfo*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override
    {
        return InterlockedIncrement(&m_cRef);
    }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) {
            delete this;
        }
        return ref;
    }

    // ITfDisplayAttributeInfo
    STDMETHODIMP GetGUID(GUID* pguid) override
    {
        if (pguid == nullptr) {
            return E_POINTER;
        }
        *pguid = m_guid;
        return S_OK;
    }

    STDMETHODIMP GetDescription(BSTR* pbstrDesc) override
    {
        if (pbstrDesc == nullptr) {
            return E_POINTER;
        }
        const bool confirmed = IsEqualGUID(m_guid, GUID_TAISHEN_ATTR_CONFIRMED);
        *pbstrDesc = SysAllocString(confirmed ? L"泰深已选文字" : L"泰深待选拼音");
        return *pbstrDesc != nullptr ? S_OK : E_OUTOFMEMORY;
    }

    STDMETHODIMP GetAttributeInfo(TF_DISPLAYATTRIBUTE* pAttr) override
    {
        if (pAttr == nullptr) {
            return E_POINTER;
        }
        ZeroMemory(pAttr, sizeof(*pAttr));
        // 文字/背景/下划线颜色都不指定，交给宿主主题
        pAttr->crText.type = TF_CT_NONE;
        pAttr->crBk.type = TF_CT_NONE;
        pAttr->crLine.type = TF_CT_NONE;
        if (IsEqualGUID(m_guid, GUID_TAISHEN_ATTR_CONFIRMED)) {
            // 已选汉字：和普通文本一样，不额外标记
            pAttr->lsStyle = TF_LS_NONE;
            pAttr->bAttr = TF_ATTR_TARGET_CONVERTED;
        } else {
            // 待选拼音：虚线下划线（与系统输入法一致）
            pAttr->lsStyle = TF_LS_DOT;
            pAttr->bAttr = TF_ATTR_INPUT;
        }
        return S_OK;
    }

    STDMETHODIMP SetAttributeInfo(const TF_DISPLAYATTRIBUTE* /*pAttr*/) override
    {
        // 样式由本实现内置，不接受外部修改
        return S_OK;
    }

    STDMETHODIMP Reset() override
    {
        return S_OK;
    }

private:
    ~CDisplayAttributeInfo() = default;

    LONG m_cRef;
    GUID m_guid;
};

// ---------------------------------------------------------------------------
// CDisplayAttributeEnum — 属性枚举器
// ---------------------------------------------------------------------------
class CDisplayAttributeEnum final : public IEnumTfDisplayAttributeInfo
{
public:
    CDisplayAttributeEnum() : m_cRef(1), m_index(0) {}

    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) ||
            IsEqualIID(riid, IID_IEnumTfDisplayAttributeInfo)) {
            *ppvObj = static_cast<IEnumTfDisplayAttributeInfo*>(this);
        } else {
            return E_NOINTERFACE;
        }
        AddRef();
        return S_OK;
    }
    STDMETHODIMP_(ULONG) AddRef() override
    {
        return InterlockedIncrement(&m_cRef);
    }
    STDMETHODIMP_(ULONG) Release() override
    {
        const ULONG ref = InterlockedDecrement(&m_cRef);
        if (ref == 0) {
            delete this;
        }
        return ref;
    }

    STDMETHODIMP Next(ULONG ulCount, ITfDisplayAttributeInfo** rgInfo,
                      ULONG* pcFetched) override
    {
        if (rgInfo == nullptr) {
            return E_POINTER;
        }
        ULONG fetched = 0;
        while (fetched < ulCount && m_index < 2) {
            const GUID& guid = (m_index == 0) ? GUID_TAISHEN_ATTR_CONFIRMED
                                              : GUID_TAISHEN_ATTR_PENDING;
            ITfDisplayAttributeInfo* info = CreateDisplayAttributeInfo(guid);
            if (info == nullptr) {
                break;
            }
            rgInfo[fetched++] = info;
            ++m_index;
        }
        if (pcFetched != nullptr) {
            *pcFetched = fetched;
        }
        return (fetched == ulCount) ? S_OK : S_FALSE;
    }

    STDMETHODIMP Skip(ULONG ulCount) override
    {
        m_index += ulCount;
        if (m_index > 2) {
            m_index = 2;
        }
        return m_index < 2 ? S_OK : S_FALSE;
    }

    STDMETHODIMP Reset() override
    {
        m_index = 0;
        return S_OK;
    }

    STDMETHODIMP Clone(IEnumTfDisplayAttributeInfo** ppEnum) override
    {
        if (ppEnum == nullptr) {
            return E_POINTER;
        }
        auto* copy = new (std::nothrow) CDisplayAttributeEnum();
        if (copy == nullptr) {
            *ppEnum = nullptr;
            return E_OUTOFMEMORY;
        }
        copy->m_index = m_index;
        *ppEnum = copy;
        return S_OK;
    }

private:
    ~CDisplayAttributeEnum() = default;

    LONG m_cRef;
    ULONG m_index;
};

// ---------------------------------------------------------------------------
// 工厂
// ---------------------------------------------------------------------------
ITfDisplayAttributeInfo* CreateDisplayAttributeInfo(REFGUID guid)
{
    if (!IsEqualGUID(guid, GUID_TAISHEN_ATTR_CONFIRMED) &&
        !IsEqualGUID(guid, GUID_TAISHEN_ATTR_PENDING)) {
        return nullptr;
    }
    return new (std::nothrow) CDisplayAttributeInfo(guid);
}

IEnumTfDisplayAttributeInfo* CreateDisplayAttributeEnum()
{
    return new (std::nothrow) CDisplayAttributeEnum();
}

// ---------------------------------------------------------------------------
// 应用分段属性
// ---------------------------------------------------------------------------
HRESULT ApplyCompositionDisplayAttributes(TfEditCookie ec, ITfContext* pic,
                                          ITfRange* pRange,
                                          LONG convertedChars,
                                          LONG totalChars)
{
    if (pic == nullptr || pRange == nullptr) {
        return E_POINTER;
    }
    LONG cchTotal = totalChars;

    ITfProperty* pProp = nullptr;
    HRESULT hr = pic->GetProperty(GUID_PROP_ATTRIBUTE, &pProp);
    if (FAILED(hr) || pProp == nullptr) {
        return hr;
    }

    ITfCategoryMgr* pCat = nullptr;
    hr = CoCreateInstance(CLSID_TF_CategoryMgr, nullptr, CLSCTX_INPROC_SERVER,
                          IID_ITfCategoryMgr, reinterpret_cast<void**>(&pCat));
    TfGuidAtom atomConfirmed = TF_INVALID_GUIDATOM;
    TfGuidAtom atomPending = TF_INVALID_GUIDATOM;
    if (SUCCEEDED(hr) && pCat != nullptr) {
        pCat->RegisterGUID(GUID_TAISHEN_ATTR_CONFIRMED, &atomConfirmed);
        pCat->RegisterGUID(GUID_TAISHEN_ATTR_PENDING, &atomPending);
        pCat->Release();
    }
    if (atomConfirmed == TF_INVALID_GUIDATOM || atomPending == TF_INVALID_GUIDATOM) {
        pProp->Release();
        return E_FAIL;
    }

    if (cchTotal <= 0) {
        pProp->Release();
        return S_OK;
    }
    if (convertedChars < 0) {
        convertedChars = 0;
    }
    if (convertedChars > cchTotal) {
        convertedChars = cchTotal;
    }

    auto setValue = [&](ITfRange* range, TfGuidAtom atom) -> HRESULT {
        VARIANT var;
        VariantInit(&var);
        var.vt = VT_I4;
        var.lVal = static_cast<LONG>(atom);
        const HRESULT setHr = pProp->SetValue(ec, range, &var);
        VariantClear(&var);
        return setHr;
    };

    HRESULT finalHr = S_OK;
    LONG shifted = 0;

    // 1) 待选拼音（后半段）：先整段标 pending，再用 confirmed 覆盖前缀——
    //    这样即使前缀切分失败，整段至少还是「待选」样式，不会变成无属性。
    ITfRange* pPending = nullptr;
    if (SUCCEEDED(pRange->Clone(&pPending)) && pPending != nullptr) {
        setValue(pPending, atomPending);
        pPending->Release();
    }

    if (convertedChars > 0) {
        ITfRange* pConfirmed = nullptr;
        if (SUCCEEDED(pRange->Clone(&pConfirmed)) && pConfirmed != nullptr) {
            pConfirmed->Collapse(ec, TF_ANCHOR_START);
            if (SUCCEEDED(pConfirmed->ShiftEnd(ec, convertedChars, &shifted, nullptr)) &&
                shifted > 0) {
                finalHr = setValue(pConfirmed, atomConfirmed);
            }
            pConfirmed->Release();
        }
    }

    pProp->Release();
    return finalHr;
}

} // namespace taishen
