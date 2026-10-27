/// TSF 语言栏按钮实现 — 输入指示器图标
///
/// 参考 Microsoft TSF SampleIME 的 CLangBarItemButton：
///   - GetIcon 从 DLL 自身的 ICON 资源加载（资源 IDI_APP_ICON）
///   - GetText 返回空串，让语言栏只显示图标而不是文字
///   - 文本/提示走 GetTooltipString

#include "langbar_item.h"

#include <msctf.h>
#include <strsafe.h>

#include "ime_ids.h"
#include "resource.h"

extern HMODULE g_hModule;

CLangBarItemButton::CLangBarItemButton(CTextService* pService)
    : m_cRef(1), m_pService(pService), m_fShown(FALSE)
{
}

CLangBarItemButton::~CLangBarItemButton() = default;

// ---------------------------------------------------------------------------
// IUnknown
// ---------------------------------------------------------------------------
STDMETHODIMP CLangBarItemButton::QueryInterface(REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr)
    {
        return E_INVALIDARG;
    }
    *ppvObj = nullptr;

    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfLangBarItem))
    {
        *ppvObj = static_cast<ITfLangBarItem*>(this);
    }
    else if (IsEqualIID(riid, IID_ITfLangBarItemButton))
    {
        *ppvObj = static_cast<ITfLangBarItemButton*>(this);
    }
    else
    {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CLangBarItemButton::AddRef()
{
    return static_cast<ULONG>(InterlockedIncrement(&m_cRef));
}

STDMETHODIMP_(ULONG) CLangBarItemButton::Release()
{
    const LONG cRef = InterlockedDecrement(&m_cRef);
    if (cRef == 0)
    {
        delete this;
    }
    return static_cast<ULONG>(cRef);
}

// ---------------------------------------------------------------------------
// ITfLangBarItem
// ---------------------------------------------------------------------------
STDMETHODIMP CLangBarItemButton::GetInfo(TF_LANGBARITEMINFO* pInfo)
{
    if (pInfo == nullptr)
    {
        return E_INVALIDARG;
    }

    pInfo->clsidService = CLSID_TAISHEN_IME;
    pInfo->guidItem = GUID_LANGBARITEM;
    // BTN_BUTTON：普通按钮；SHOWNINTRAY：允许出现在输入指示器/系统托盘语言区
    pInfo->dwStyle = TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY;
    pInfo->ulSort = 0;
    StringCchCopyW(pInfo->szDescription, ARRAYSIZE(pInfo->szDescription),
                   L"泰深输入法");
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::GetStatus(DWORD* pdwStatus)
{
    if (pdwStatus == nullptr)
    {
        return E_INVALIDARG;
    }
    *pdwStatus = 0;
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::Show(BOOL fShow)
{
    m_fShown = fShow;
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::GetTooltipString(BSTR* pbstrToolTip)
{
    if (pbstrToolTip == nullptr)
    {
        return E_INVALIDARG;
    }
    *pbstrToolTip = SysAllocString(L"泰深输入法");
    return (*pbstrToolTip != nullptr) ? S_OK : E_OUTOFMEMORY;
}

// ---------------------------------------------------------------------------
// ITfLangBarItemButton
// ---------------------------------------------------------------------------
STDMETHODIMP CLangBarItemButton::OnClick(TfLBIClick /*click*/, POINT /*pt*/,
                                        const RECT* /*prcArea*/)
{
    // 中英切换由工具栏/快捷键驱动，语言栏图标仅作展示与切换入口
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::InitMenu(ITfMenu* /*pMenu*/)
{
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::OnMenuSelect(UINT /*wID*/)
{
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::GetIcon(HICON* phIcon)
{
    if (phIcon == nullptr)
    {
        return E_INVALIDARG;
    }
    *phIcon = nullptr;

    // 语言栏按小图标尺寸取图；资源内含 16~256 多尺寸，LoadImage 负责挑选/缩放
    const int cx = GetSystemMetrics(SM_CXSMICON);
    const int cy = GetSystemMetrics(SM_CYSMICON);
    HICON hIcon = static_cast<HICON>(
        LoadImageW(g_hModule, MAKEINTRESOURCEW(IDI_APP_ICON), IMAGE_ICON,
                   cx, cy, LR_DEFAULTCOLOR));
    if (hIcon == nullptr)
    {
        hIcon = static_cast<HICON>(
            LoadImageW(g_hModule, MAKEINTRESOURCEW(IDI_APP_ICON), IMAGE_ICON,
                       16, 16, LR_DEFAULTCOLOR));
    }
    *phIcon = hIcon;
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::GetText(BSTR* pbstrText)
{
    if (pbstrText == nullptr)
    {
        return E_INVALIDARG;
    }
    // 空串：语言栏只用图标，不额外显示文字
    *pbstrText = SysAllocString(L"");
    return (*pbstrText != nullptr) ? S_OK : E_OUTOFMEMORY;
}

