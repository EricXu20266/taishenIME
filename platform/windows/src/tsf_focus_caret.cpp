/// Windows TSF IME — 焦点跟踪、UIElement 协商、光标定位与候选窗同步
///
/// 覆盖 DEV-TRACKER: V4.2

#include "tsf_text_service.h"

#include <algorithm>
#include <new>

#include "app_container.h"
#include "app_state.h"
#include "debug_log.h"
#include "dpi_util.h"

namespace {

/// 二级光标兜底：取前台线程的 Win32 caret（GetGUIThreadInfo）
bool GetCaretRectFromGuiThread(RECT* pRect)
{
    if (pRect == nullptr) {
        return false;
    }
    const HWND fg = GetForegroundWindow();
    if (fg == nullptr) {
        return false;
    }
    const DWORD tid = GetWindowThreadProcessId(fg, nullptr);
    if (tid == 0) {
        return false;
    }
    GUITHREADINFO gti{};
    gti.cbSize = sizeof(gti);
    if (!GetGUIThreadInfo(tid, &gti) || gti.hwndCaret == nullptr) {
        return false;
    }
    POINT topLeft{gti.rcCaret.left, gti.rcCaret.top};
    POINT bottomRight{gti.rcCaret.right, gti.rcCaret.bottom};
    if (!ClientToScreen(gti.hwndCaret, &topLeft) ||
        !ClientToScreen(gti.hwndCaret, &bottomRight)) {
        return false;
    }
    pRect->left = topLeft.x;
    pRect->top = topLeft.y;
    pRect->right = bottomRight.x;
    pRect->bottom = bottomRight.y;
    taishen::CaretToPhysicalPixel(gti.hwndCaret, pRect);
    return true;
}

} // namespace

// ---------------------------------------------------------------------------
// CCandidateListUIElement — TSF UILess 候选 UIElement 协商
// ---------------------------------------------------------------------------

CTextService::CCandidateListUIElement::CCandidateListUIElement(CTextService* owner)
    : m_cRef(1), m_owner(owner), m_shown(TRUE),
      m_updatedFlags(TF_CLUIE_DOCUMENTMGR | TF_CLUIE_COUNT |
                     TF_CLUIE_SELECTION | TF_CLUIE_STRING |
                     TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE)
{
}

STDMETHODIMP CTextService::CCandidateListUIElement::QueryInterface(
    REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;
    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfUIElement) ||
        IsEqualIID(riid, IID_ITfCandidateListUIElement)) {
        *ppvObj = static_cast<ITfCandidateListUIElement*>(this);
    } else {
        return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTextService::CCandidateListUIElement::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTextService::CCandidateListUIElement::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetDescription(
    BSTR* pbstrDescription)
{
    if (pbstrDescription == nullptr) {
        return E_POINTER;
    }
    *pbstrDescription = SysAllocString(L"泰深拼音候选");
    return *pbstrDescription == nullptr ? E_OUTOFMEMORY : S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetGUID(GUID* pguid)
{
    if (pguid == nullptr) {
        return E_POINTER;
    }
    static const GUID kCandidateElementGuid = {
        0xA6D5B2F1, 0x5D2C, 0x4B1E,
        {0x91, 0xC7, 0xD0, 0x7B, 0x39, 0x4E, 0x8F, 0x21}};
    *pguid = kCandidateElementGuid;
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::Show(BOOL bShow)
{
    m_shown = bShow ? TRUE : FALSE;
    if (m_owner != nullptr) {
        m_owner->OnCandidateUiElementShow(m_shown);
    }
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::IsShown(BOOL* pbShow)
{
    if (pbShow == nullptr) {
        return E_POINTER;
    }
    *pbShow = m_shown;
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetUpdatedFlags(
    DWORD* pdwFlags)
{
    if (pdwFlags == nullptr) {
        return E_POINTER;
    }
    *pdwFlags = m_updatedFlags;
    m_updatedFlags = 0;
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetDocumentMgr(
    ITfDocumentMgr** ppdim)
{
    if (ppdim == nullptr) {
        return E_POINTER;
    }
    *ppdim = nullptr;
    if (m_owner == nullptr || m_owner->m_pFocusContext == nullptr) {
        return E_FAIL;
    }
    return m_owner->m_pFocusContext->GetDocumentMgr(ppdim);
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetCount(UINT* puCount)
{
    if (puCount == nullptr) {
        return E_POINTER;
    }
    if (m_owner != nullptr) {
        InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    }
    if (m_owner == nullptr) {
        *puCount = 0;
    } else {
        *puCount = static_cast<UINT>(
            (std::max)(0, m_owner->m_remoteState.totalCandidates));
    }
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetSelection(UINT* puIndex)
{
    if (puIndex == nullptr) {
        return E_POINTER;
    }
    if (m_owner == nullptr || m_owner->m_candidates.empty()) {
        return S_FALSE;
    }
    InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    *puIndex = 0;
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetString(
    UINT uIndex, BSTR* pstr)
{
    if (pstr == nullptr) {
        return E_POINTER;
    }
    *pstr = nullptr;
    if (m_owner == nullptr) {
        return E_INVALIDARG;
    }
    InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    std::string candidate;
    const size_t pageSz = static_cast<size_t>(
        (std::max)(1, m_owner->m_remoteState.pageSize));
    const size_t offset = static_cast<size_t>(
        (std::max)(0, m_owner->m_remoteState.page)) * pageSz;
    if (uIndex >= offset && (uIndex - offset) < m_owner->m_candidates.size()) {
        candidate = m_owner->m_candidates[uIndex - offset];
    } else if (uIndex < m_owner->m_candidates.size()) {
        candidate = m_owner->m_candidates[uIndex];
    } else {
        return E_INVALIDARG;
    }
    const std::wstring value = taishen::Utf8ToWide(candidate);
    *pstr = SysAllocStringLen(value.data(), static_cast<UINT>(value.size()));
    return *pstr == nullptr ? E_OUTOFMEMORY : S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetPageIndex(
    UINT* pIndex, UINT uSize, UINT* puPageCnt)
{
    if (puPageCnt == nullptr) {
        return E_POINTER;
    }
    if (m_owner != nullptr) {
        InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    }
    const int rawPages = (m_owner == nullptr) ? 0 : m_owner->m_remoteState.totalPages;
    const UINT pageCount =
        (m_owner == nullptr) ? 0 : static_cast<UINT>((std::max)(1, rawPages));
    *puPageCnt = pageCount;
    if (pIndex == nullptr) {
        return S_OK;
    }
    if (uSize < pageCount) {
        return E_INVALIDARG;
    }
    const int rawPageSize = (m_owner == nullptr) ? 1 : m_owner->m_remoteState.pageSize;
    const UINT pageSize = (std::max)(1u, static_cast<UINT>(rawPageSize));
    for (UINT i = 0; i < pageCount; ++i) {
        pIndex[i] = i * pageSize;
    }
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::SetPageIndex(
    UINT* pIndex, UINT uPageCnt)
{
    if (uPageCnt > 0 && pIndex == nullptr) {
        return E_INVALIDARG;
    }
    return S_OK;
}

STDMETHODIMP CTextService::CCandidateListUIElement::GetCurrentPage(
    UINT* puPage)
{
    if (puPage == nullptr) {
        return E_POINTER;
    }
    if (m_owner != nullptr) {
        InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    }
    const int curPage = (m_owner == nullptr) ? 0 : m_owner->m_remoteState.page;
    *puPage = static_cast<UINT>((std::max)(0, curPage));
    return S_OK;
}

void CTextService::CCandidateListUIElement::MarkUpdated(DWORD flags)
{
    m_updatedFlags |= flags;
}

HRESULT CTextService::EnsureCandidateUiElement()
{
    if (m_candidateUiElement != nullptr && m_pUIElementMgr != nullptr) {
        return S_OK;
    }
    if (m_pThreadMgr == nullptr) {
        return E_UNEXPECTED;
    }

    ITfUIElementMgr* manager = nullptr;
    HRESULT hr = m_pThreadMgr->QueryInterface(
        IID_ITfUIElementMgr, reinterpret_cast<void**>(&manager));
    if (FAILED(hr) || manager == nullptr) {
        taishen::DebugLogHr("EnsureCandidateUiElement: QueryInterface", hr);
        return FAILED(hr) ? hr : E_NOINTERFACE;
    }

    auto* element = new (std::nothrow) CCandidateListUIElement(this);
    if (element == nullptr) {
        manager->Release();
        return E_OUTOFMEMORY;
    }

    BOOL show = TRUE;
    DWORD elementId = 0;
    InterlockedExchange(&m_uiElementDataRead, 0);
    hr = manager->BeginUIElement(
        static_cast<ITfUIElement*>(element), &show, &elementId);
    if (FAILED(hr)) {
        taishen::DebugLogHr("EnsureCandidateUiElement: BeginUIElement", hr);
        element->Release();
        manager->Release();
        return hr;
    }

    m_pUIElementMgr = manager;
    m_candidateUiElement = element;
    m_candidateUiElementId = elementId;
    m_candidateUiSuppressed = (show == FALSE);
    if (m_candidateUiSuppressed && !ShouldSelfDrawWhileSuppressed() &&
        !taishen::IsAppContainerProcess()) {
        HideCandidateUi();
    }
    taishen::DebugLog("EnsureCandidateUiElement: begun id=" +
                      std::to_string(elementId) + " show=" +
                      std::to_string(show == TRUE ? 1 : 0));
    return S_OK;
}

void CTextService::EndCandidateUiElement()
{
    if (m_pUIElementMgr != nullptr) {
        const HRESULT hr = m_pUIElementMgr->EndUIElement(m_candidateUiElementId);
        taishen::DebugLogHr("EndCandidateUiElement", hr);
        m_pUIElementMgr->Release();
        m_pUIElementMgr = nullptr;
    }
    if (m_candidateUiElement != nullptr) {
        m_candidateUiElement->Release();
        m_candidateUiElement = nullptr;
    }
    m_candidateUiElementId = 0;
    m_candidateUiSuppressed = false;
}

bool CTextService::ShouldSelfDrawWhileSuppressed()
{
    return taishen::AppStateShouldSelfDrawWhenSuppressed(
        m_appCfg, m_uiElementDataRead != 0);
}

void CTextService::OnCandidateUiElementShow(BOOL show)
{
    m_candidateUiSuppressed = (show == FALSE);
    if (m_candidateUiSuppressed && !ShouldSelfDrawWhileSuppressed() &&
        !taishen::IsAppContainerProcess()) {
        HideCandidateUi();
    }
}

// ---------------------------------------------------------------------------
// ITfThreadMgrEventSink — 焦点切换与上下文管理
// ---------------------------------------------------------------------------

STDMETHODIMP CTextService::OnInitDocumentMgr(ITfDocumentMgr* /*pdim*/)
{
    return S_OK;
}

STDMETHODIMP CTextService::OnUninitDocumentMgr(ITfDocumentMgr* /*pdim*/)
{
    return S_OK;
}

STDMETHODIMP CTextService::OnSetFocus(ITfDocumentMgr* pdimFocus,
                                      ITfDocumentMgr* /*pdimPrevFocus*/)
{
    {
        static std::atomic<ULONGLONG> s_lastFocusProbeTick{0};
        if (TaishenShouldEmitRateLimitedLog(s_lastFocusProbeTick, 30000)) {
            taishen::ForceLog(
                "[FOCUSPROBE] OnSetFocus pid=" + std::to_string(GetCurrentProcessId()) +
                " tid=" + std::to_string(GetCurrentThreadId()) +
                " hasFocus=" + std::string(pdimFocus != nullptr ? "1" : "0") +
                " active=" + std::string(m_fActive ? "1" : "0") +
                " remote=1");
        }
    }
    if (!taishen::IsAppContainerProcess()) {
        ReloadConfigIfChanged();
    }
    m_hasLastCaretRect = false;
    ++m_caretSeq;
    if (m_pFocusContext != nullptr) {
        m_pFocusContext->Release();
        m_pFocusContext = nullptr;
    }
    if (pdimFocus != nullptr) {
        pdimFocus->GetTop(&m_pFocusContext);
    } else {
        HideCandidateUi();
    }
    taishen::AppStateApplyRemote(m_appCfg, m_remoteState.asciiMode);
    return S_OK;
}

STDMETHODIMP CTextService::OnPushContext(ITfContext* /*pic*/)
{
    return S_OK;
}

STDMETHODIMP CTextService::OnPopContext(ITfContext* /*pic*/)
{
    return S_OK;
}

// ---------------------------------------------------------------------------
// 光标获取与候选窗口显示/隐藏
// ---------------------------------------------------------------------------

void CTextService::HideCandidateUi()
{
    m_renderClient.SendHide(m_renderSessionId);
}

void CTextService::OnCaretRectResolved(uint64_t seq, const RECT& rect)
{
    if (seq != m_caretSeq) {
        return;
    }
    m_lastCaretRect = rect;
    m_hasLastCaretRect = true;
    m_lastResolvedCaretSeq = seq;

    if (!m_candidates.empty() && !m_lastDisplayPinyin.empty()) {
        m_renderClient.SendMove(m_renderSessionId, rect);
    }
}

HRESULT CTextService::GetCaretRectFromContext(ITfContext* pic, RECT* pRect)
{
    if (pic == nullptr || pRect == nullptr) {
        return E_POINTER;
    }

    const uint64_t seq = ++m_caretSeq;
    CEditSessionGetCaret* pSession =
        new (std::nothrow) CEditSessionGetCaret(this, pic, seq);
    if (pSession == nullptr) {
        return E_OUTOFMEMORY;
    }

    const DWORD flags = taishen::IsAppContainerProcess()
                            ? (TF_ES_ASYNC | TF_ES_READ)
                            : (TF_ES_ASYNCDONTCARE | TF_ES_READ);
    HRESULT hrSession = E_FAIL;
    const HRESULT hr = pic->RequestEditSession(m_tid, pSession, flags, &hrSession);
    pSession->Release();
    if (FAILED(hr)) {
        return hr;
    }
    if (m_lastResolvedCaretSeq == seq && m_hasLastCaretRect) {
        *pRect = m_lastCaretRect;
        return S_OK;
    }
    return hrSession;
}

void CTextService::UpdateCandidateWindow(ITfContext* caretContext)
{
    if (m_pinyin.empty() && m_candidates.empty()) {
        EndCandidateUiElement();
        HideCandidateUi();
        return;
    }
    if (m_candidates.empty()) {
        EndCandidateUiElement();
        HideCandidateUi();
        return;
    }

    const HRESULT uiElementHr = EnsureCandidateUiElement();
    if (FAILED(uiElementHr)) {
        taishen::DebugLogHr("UpdateCandidateWindow: EnsureCandidateUiElement", uiElementHr);
    } else if (m_pUIElementMgr != nullptr) {
        const HRESULT updateHr = m_pUIElementMgr->UpdateUIElement(m_candidateUiElementId);
        taishen::DebugLogHr("UpdateCandidateWindow: UpdateUIElement", updateHr);
    }

    const bool isContainer = taishen::IsAppContainerProcess();
    if (m_candidateUiSuppressed && !ShouldSelfDrawWhileSuppressed()) {
        if (!isContainer) {
            HideCandidateUi();
            return;
        }
        taishen::ForceLog("UpdateCandidateWindow(AC): suppressed=1 read=" +
                          std::to_string(m_uiElementDataRead != 0 ? 1 : 0) +
                          " bypass suppress for remote render");
    }

    std::string displayPinyin = m_pinyin;
    const bool inCompose = m_remoteState.inCompose;
    if (inCompose) {
        displayPinyin = BuildCompositionText(m_remoteState.syllableDisplay);
    }
    {
        std::string preview;
        for (size_t i = 0; i < m_candidates.size() && i < 3; ++i) {
            if (i) {
                preview += " / ";
            }
            preview += m_candidates[i];
        }
        taishen::DebugLog("UpdateCandidateWindow: pinyin=" + m_pinyin +
                          " cands=" + std::to_string(m_candidates.size()) +
                          " first3=" + preview);
    }

    RECT caretRect = {0, 0, 0, 0};
    bool gotCaret = false;
    ITfContext* caretCtx =
        (caretContext != nullptr) ? caretContext : m_pFocusContext;
    if (caretCtx != nullptr) {
        if (GetCaretRectFromContext(caretCtx, &caretRect) == S_OK) {
            gotCaret = true;
        }
    }
    if (!gotCaret && m_hasLastCaretRect) {
        caretRect = m_lastCaretRect;
        gotCaret = true;
    }
    if (!gotCaret && GetCaretRectFromGuiThread(&caretRect)) {
        gotCaret = true;
    }
    if (!gotCaret) {
        POINT pt = {};
        GetCursorPos(&pt);
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.bottom = pt.y;
        caretRect.right = pt.x;
    }
    taishen::DebugLog("UpdateCandidateWindow: gotCaret=" +
                      std::string(gotCaret ? "T" : "F") + " remote=1 caret=(" +
                      std::to_string(caretRect.left) + "," +
                      std::to_string(caretRect.top) + ")");

    const int page = m_remoteState.page;
    const int totalPages = m_remoteState.totalPages;
    const bool inlinePreedit =
        taishen::AppStateInlinePreeditEnabled(m_appCfg) &&
        taishen::g_inlineCompositionReadable;
    const int cursorPos = inCompose ? -1 : m_remoteState.cursor;

    m_lastDisplayPinyin = displayPinyin;
    m_lastPage = page;
    m_lastTotalPages = totalPages;

    taishen::ipc::RenderCommand cmd;
    cmd.op = taishen::ipc::RenderOp::Show;
    cmd.sessionId = m_renderSessionId;
    cmd.hostPid = GetCurrentProcessId();
    cmd.pinyin = displayPinyin;
    cmd.candidates = m_candidates;
    cmd.caret = caretRect;
    cmd.page = page;
    cmd.totalPages = totalPages;
    cmd.selectedIndex = 0;
    cmd.cursorPos = cursorPos;
    cmd.inlinePreedit = inlinePreedit;
    cmd.multiRow = m_multiRowExpanded;
    const bool sent = m_renderClient.SendShow(cmd);
    if (isContainer) {
        taishen::ForceLog(
            "UpdateCandidateWindow(AC): suppressed=" +
            std::to_string(m_candidateUiSuppressed ? 1 : 0) +
            " read=" + std::to_string(m_uiElementDataRead != 0 ? 1 : 0) +
            " gotCaret=" + std::to_string(gotCaret ? 1 : 0) +
            " caret=(" + std::to_string(caretRect.left) + "," +
            std::to_string(caretRect.top) + "," +
            std::to_string(caretRect.right) + "," +
            std::to_string(caretRect.bottom) + ")" +
            " cands=" + std::to_string(m_candidates.size()) +
            " sendShow=" + (sent ? "1" : "0"));
        return;
    }
    if (!sent) {
        static std::atomic<ULONGLONG> s_lastRenderFallbackLogTick{0};
        if (TaishenShouldEmitRateLimitedLog(s_lastRenderFallbackLogTick, 30000)) {
            taishen::ForceLog("UpdateCandidateWindow: remote SendShow failed");
        }
    }
}
