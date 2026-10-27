/// TSF 语言栏按钮实现 — 输入指示器图标 + 中英切换按钮
///
/// 参考 Microsoft TSF SampleIME 的 CLangBarItemButton：
///   - GetIcon 返回图标句柄（调用方负责 DestroyIcon，因此每次新建，不缓存）
///   - GetText 返回空串，让语言栏只显示图标而不是文字
///   - 文本/提示走 GetTooltipString

#include "langbar_item.h"

#include <msctf.h>
#include <strsafe.h>

#include "ime_ids.h"
#include "resource.h"

extern HMODULE g_hModule;

// ---------------------------------------------------------------------------
// 中英按钮图标：运行时 GDI 绘制（蓝底"中" / 灰底"英"），无额外资源依赖
// ---------------------------------------------------------------------------
static HICON CreateModeIcon(bool asciiMode, int size)
{
    if (size <= 0)
    {
        size = 16;
    }

    HDC hdc = GetDC(nullptr);
    HDC memDC = CreateCompatibleDC(hdc);
    HBITMAP colorBM = CreateCompatibleBitmap(hdc, size, size);
    HBITMAP maskBM = CreateBitmap(size, size, 1, 1, nullptr);
    HGDIOBJ oldColor = SelectObject(memDC, colorBM);

    RECT rc = {0, 0, size, size};
    // 中文=品牌蓝，英文=中性灰（与微软拼音的状态区分逻辑一致）
    HBRUSH bg = CreateSolidBrush(asciiMode ? RGB(0x6B, 0x72, 0x80)
                                           : RGB(0x1E, 0x6F, 0xFF));
    FillRect(memDC, &rc, bg);
    DeleteObject(bg);

    SetBkMode(memDC, TRANSPARENT);
    SetTextColor(memDC, RGB(255, 255, 255));
    HFONT font = CreateFontW(-(size - 1), 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                             CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                             DEFAULT_PITCH, L"Microsoft YaHei");
    HGDIOBJ oldFont = SelectObject(memDC, font);
    DrawTextW(memDC, asciiMode ? L"英" : L"中", -1, &rc,
              DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(memDC, oldFont);
    DeleteObject(font);

    // 掩码全 0（不透明）
    HDC maskDC = CreateCompatibleDC(hdc);
    HGDIOBJ oldMask = SelectObject(maskDC, maskBM);
    HBRUSH maskBrush = CreateSolidBrush(RGB(0, 0, 0));
    FillRect(maskDC, &rc, maskBrush);
    DeleteObject(maskBrush);
    SelectObject(maskDC, oldMask);
    DeleteDC(maskDC);

    ICONINFO ii = {};
    ii.fIcon = TRUE;
    ii.hbmColor = colorBM;
    ii.hbmMask = maskBM;
    HICON icon = CreateIconIndirect(&ii);

    SelectObject(memDC, oldColor);
    DeleteDC(memDC);
    DeleteObject(colorBM);
    DeleteObject(maskBM);
    ReleaseDC(nullptr, hdc);
    return icon;
}

CLangBarItemButton::CLangBarItemButton(ILangBarHost* pHost, Kind kind)
    : m_cRef(1), m_pHost(pHost), m_kind(kind), m_fShown(FALSE)
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

    const bool isMode = (m_kind == Kind::Mode);
    pInfo->clsidService = CLSID_TAISHEN_IME;
    pInfo->guidItem = isMode ? GUID_LANGBARITEM_MODE : GUID_LANGBARITEM;
    // BTN_BUTTON：普通按钮；SHOWNINTRAY：允许出现在输入指示器/系统托盘语言区
    pInfo->dwStyle = TF_LBI_STYLE_BTN_BUTTON | TF_LBI_STYLE_SHOWNINTRAY;
    pInfo->ulSort = isMode ? 1 : 0;   // 标识图标在前，中英按钮在后
    StringCchCopyW(pInfo->szDescription, ARRAYSIZE(pInfo->szDescription),
                   isMode ? L"中英文切换" : L"泰深输入法");
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
    const wchar_t* tip = L"泰深输入法";
    if (m_kind == Kind::Mode)
    {
        const bool ascii = (m_pHost != nullptr) ? m_pHost->IsAsciiMode() : false;
        tip = ascii ? L"泰深输入法 - 英文模式（点击切回中文）"
                    : L"泰深输入法 - 中文模式（点击切到英文）";
    }
    *pbstrToolTip = SysAllocString(tip);
    return (*pbstrToolTip != nullptr) ? S_OK : E_OUTOFMEMORY;
}

// ---------------------------------------------------------------------------
// ITfLangBarItemButton
// ---------------------------------------------------------------------------
STDMETHODIMP CLangBarItemButton::OnClick(TfLBIClick /*click*/, POINT /*pt*/,
                                        const RECT* /*prcArea*/)
{
    if (m_kind == Kind::Mode && m_pHost != nullptr)
    {
        // 切换后由宿主统一刷新语言栏（重新挂载，系统重新调 GetIcon）
        m_pHost->OnLangBarToggleAscii();
    }
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

    const int cx = GetSystemMetrics(SM_CXSMICON);
    const int cy = GetSystemMetrics(SM_CYSMICON);

    if (m_kind == Kind::Mode)
    {
        const bool ascii = (m_pHost != nullptr) ? m_pHost->IsAsciiMode() : false;
        *phIcon = CreateModeIcon(ascii, cx > 0 ? cx : 16);
        return S_OK;
    }

    // 标识图标：资源内含 16~256 多尺寸，LoadImage 负责挑选/缩放
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
