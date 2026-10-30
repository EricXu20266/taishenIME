/// TSF 语言栏按钮实现 — 输入指示器图标 + 中英切换按钮
///
/// 参考 Microsoft TSF SampleIME 的 CLangBarItemButton：
///   - GetIcon 返回图标句柄（调用方负责 DestroyIcon，因此每次新建，不缓存）
///   - GetText 返回空串，让语言栏只显示图标而不是文字
///   - 文本/提示走 GetTooltipString

#include "langbar_item.h"

#include <ctffunc.h>   // GUID_LBI_INPUTMODE（系统预定义中/英语言栏项）
#include <msctf.h>
#include <ocidl.h>
#include <strsafe.h>

#include "banner_window.h"
#include "engine_bridge.h"
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
    : m_cRef(1), m_pHost(pHost), m_kind(kind), m_fShown(FALSE),
      m_pSink(nullptr), m_dwSinkCookie(0)
{
}

CLangBarItemButton::~CLangBarItemButton()
{
    if (m_pSink != nullptr)
    {
        m_pSink->Release();
        m_pSink = nullptr;
    }
}

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
    else if (IsEqualIID(riid, IID_ITfSource))
    {
        // 语言栏在 AddItem 内部会 QI ITfSource 并 AdviseSink(IID_ITfLangBarItemSink)。
        // 缺这条 → AddItem 返回 0x80004005(E_FAIL)。
        *ppvObj = static_cast<ITfSource*>(this);
    }
    else
    {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

// ---------------------------------------------------------------------------
// ITfSource — 语言栏的事件 sink（AddItem 的硬性前提）
// ---------------------------------------------------------------------------
// IConnectionPoint 错误码（SDK 头里分散，直接给字面量避免头依赖）
static constexpr HRESULT kConnectECannotConnect =
    static_cast<HRESULT>(0x80040200L);   // CONNECT_E_CANNOTCONNECT
static constexpr HRESULT kConnectEAdviselimit =
    static_cast<HRESULT>(0x80040201L);   // CONNECT_E_ADVISELIMIT

STDMETHODIMP CLangBarItemButton::AdviseSink(REFIID riid, IUnknown* punk,
                                            DWORD* pdwCookie)
{
    if (punk == nullptr || pdwCookie == nullptr)
    {
        return E_INVALIDARG;
    }
    if (!IsEqualIID(riid, IID_ITfLangBarItemSink))
    {
        return kConnectECannotConnect;
    }
    if (m_pSink != nullptr)
    {
        return kConnectEAdviselimit;
    }
    if (FAILED(punk->QueryInterface(IID_ITfLangBarItemSink,
                                    reinterpret_cast<void**>(&m_pSink))))
    {
        m_pSink = nullptr;
        return E_NOINTERFACE;
    }
    m_dwSinkCookie = 1;
    *pdwCookie = m_dwSinkCookie;
    return S_OK;
}

STDMETHODIMP CLangBarItemButton::UnadviseSink(DWORD dwCookie)
{
    if (dwCookie != m_dwSinkCookie || m_pSink == nullptr)
    {
        return E_INVALIDARG;
    }
    m_pSink->Release();
    m_pSink = nullptr;
    m_dwSinkCookie = 0;
    return S_OK;
}

void CLangBarItemButton::NotifyUpdate()
{
    if (m_pSink != nullptr)
    {
        m_pSink->OnUpdate(TF_LBI_ICON | TF_LBI_TEXT | TF_LBI_STATUS);
    }
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
    // 中英按钮用系统预定义项 GUID_LBI_INPUTMODE：Win11 会把它显示在任务栏
    // 输入指示器区域（品牌图标左边），与微软拼音/青简同款。自定义 GUID 只有
    // 打开桌面语言栏时才会显示，任务栏上看不到。
    pInfo->guidItem = isMode ? GUID_LBI_INPUTMODE : GUID_LANGBARITEM;
    // 注意：不加 TF_LBI_STYLE_SHOWNINTRAY——加入该标志后系统走另一条显示路径，
    // 任务栏输入指示器不渲染（青简同样只用 BTN_BUTTON）。
    pInfo->dwStyle = TF_LBI_STYLE_BTN_BUTTON;
    pInfo->ulSort = isMode ? 0 : 1;   // 中英按钮在前，标识图标在后
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
// ---------------------------------------------------------------------------
// 右键快速菜单（对齐微软拼音：左键切中英，右键出快速指令）
// ---------------------------------------------------------------------------
static void ShowQuickMenu(ILangBarHost* host, POINT pt)
{
    const int ascii = engine_get_ascii_mode();
    const int trad = engine_get_traditional();
    const int shuangpin = engine_get_shuangpin();
    const int punct = engine_get_ascii_punct();
    const bool toolbarOn = taishen::CBannerWindow::Instance().IsEnabled();

    HMENU menu = CreatePopupMenu();
    if (menu == nullptr) {
        return;
    }
    AppendMenuW(menu, MF_STRING, 1, ascii ? L"切换到中文模式" : L"切换到英文模式");
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING | (trad ? MF_CHECKED : 0), 2, L"简繁转换");
    AppendMenuW(menu, MF_STRING | (shuangpin ? MF_CHECKED : 0), 3, L"双拼输入");
    AppendMenuW(menu, MF_STRING | (punct ? MF_CHECKED : 0), 4, L"英文标点");
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING | (toolbarOn ? MF_CHECKED : 0), 5, L"显示工具栏");

    HWND owner = GetForegroundWindow();
    if (owner != nullptr) {
        SetForegroundWindow(owner);  // 菜单需前台窗口才能正常收发点击，否则瞬间关闭
    }
    const int cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTBUTTON,
                                   pt.x, pt.y, 0, owner, nullptr);
    DestroyMenu(menu);
    if (owner != nullptr) {
        PostMessageW(owner, WM_NULL, 0, 0);  // 菜单关闭后归还前台（TrackPopupMenu 惯例）
    }

    switch (cmd) {
    case 1:
        if (host != nullptr) {
            host->OnLangBarToggleAscii();
        }
        break;
    case 2:
        engine_set_traditional(trad ? 0 : 1);
        if (host != nullptr) {
            host->OnLangBarNeedRefresh();
        }
        break;
    case 3:
        engine_set_shuangpin(shuangpin ? 0 : 1);
        if (host != nullptr) {
            host->OnLangBarNeedRefresh();
        }
        break;
    case 4:
        engine_set_ascii_punct(punct ? 0 : 1);
        break;
    case 5:
        if (host != nullptr) {
            host->OnLangBarToggleToolbar();
        }
        break;
    default:
        break;
    }
}

STDMETHODIMP CLangBarItemButton::OnClick(TfLBIClick click, POINT pt,
                                        const RECT* /*prcArea*/)
{
    if (m_kind == Kind::Mode && m_pHost != nullptr)
    {
        if (click == TF_LBI_CLK_RIGHT)
        {
            ShowQuickMenu(m_pHost, pt);
        }
        else
        {
            // 切换后由宿主统一刷新语言栏（重新挂载，系统重新调 GetIcon）
            m_pHost->OnLangBarToggleAscii();
        }
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
    // 中英按钮：与青简/微软一致，文本也给「中」/「英」，
    // 输入指示器在只取文本不取图标的渲染路径下仍能显示状态。
    if (m_kind == Kind::Mode)
    {
        const bool ascii = (m_pHost != nullptr) ? m_pHost->IsAsciiMode() : false;
        *pbstrText = SysAllocString(ascii ? L"英" : L"中");
    }
    else
    {
        *pbstrText = SysAllocString(L"泰");
    }
    return (*pbstrText != nullptr) ? S_OK : E_OUTOFMEMORY;
}
