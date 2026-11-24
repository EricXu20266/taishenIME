/// Windows TSF IME — 编辑会话、组合区读写与光标坐标采集
///
/// 覆盖 DEV-TRACKER: V4.2

#include "tsf_text_service.h"

#include <new>

#include "app_container.h"
#include "debug_log.h"
#include "dpi_util.h"

namespace {

/// 将原始拼音光标偏移映射为音节分隔串（如 ni'hao）的宽字符位置；末尾或无需定位返回 -1
int MapRawCursorToDisplayChars(const std::string& displayUtf8, int rawCursor)
{
    if (rawCursor < 0) {
        return -1;
    }
    const std::wstring wide = taishen::Utf8ToWide(displayUtf8);
    int rawSeen = 0;
    int widePos = 0;
    const int totalWide = static_cast<int>(wide.size());
    while (widePos < totalWide && rawSeen < rawCursor) {
        if (wide[widePos] != L'\'') {
            ++rawSeen;
        }
        ++widePos;
    }
    if (rawCursor > 0 && widePos < totalWide && wide[widePos] == L'\'') {
        ++widePos;
    }
    return widePos;
}

} // namespace

// ---------------------------------------------------------------------------
// CEditSessionComposition
// ---------------------------------------------------------------------------

CTextService::CEditSessionComposition::CEditSessionComposition(
    CTextService* owner, ITfContext* pic, Op op, const std::string& text,
    taishen::CTsfComposition* comp, int caretOffset, LONG convertedChars,
    bool forceStart, int cursorChars)
    : m_cRef(1), m_pOwner(owner), m_pic(pic), m_op(op), m_text(text),
      m_pComp(comp), m_caretOffset(caretOffset),
      m_convertedChars(convertedChars), m_forceStart(forceStart),
      m_cursorChars(cursorChars)
{
    if (m_pOwner != nullptr) {
        m_pOwner->AddRef();
    }
    if (m_pic != nullptr) {
        m_pic->AddRef();
    }
}

CTextService::CEditSessionComposition::~CEditSessionComposition()
{
    if (m_pic != nullptr) {
        m_pic->Release();
    }
    if (m_pOwner != nullptr) {
        m_pOwner->Release();
    }
}

STDMETHODIMP CTextService::CEditSessionComposition::QueryInterface(
    REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;
    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfEditSession)) {
        *ppvObj = static_cast<ITfEditSession*>(this);
    } else {
        return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTextService::CEditSessionComposition::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTextService::CEditSessionComposition::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

STDMETHODIMP CTextService::CEditSessionComposition::DoEditSession(TfEditCookie ec)
{
    if (m_pComp == nullptr || m_pic == nullptr) {
        return E_POINTER;
    }
    switch (m_op) {
    case Op::Start:
        // 异步排队下前一条 Start 可能已生效，非 forceStart 时无缝转 Update 防闪烁重建
        if (!m_forceStart && m_pComp->IsActive()) {
            return m_pComp->UpdateComposition(ec, m_pic, m_text,
                                              m_convertedChars, m_cursorChars);
        }
        return m_pComp->StartComposition(ec, m_pic, m_text);
    case Op::Update:
        return m_pComp->UpdateComposition(ec, m_pic, m_text,
                                          m_convertedChars, m_cursorChars);
    case Op::Commit:
        return m_pComp->CommitComposition(ec, m_pic, m_text, m_caretOffset);
    }
    return E_FAIL;
}

// ---------------------------------------------------------------------------
// CEditSessionGetCaret
// ---------------------------------------------------------------------------

CTextService::CEditSessionGetCaret::CEditSessionGetCaret(
    CTextService* owner, ITfContext* pic, uint64_t seq)
    : m_cRef(1), m_pOwner(owner), m_pic(pic), m_seq(seq)
{
    if (m_pOwner != nullptr) {
        m_pOwner->AddRef();
    }
    if (m_pic != nullptr) {
        m_pic->AddRef();
    }
}

CTextService::CEditSessionGetCaret::~CEditSessionGetCaret()
{
    if (m_pic != nullptr) {
        m_pic->Release();
    }
    if (m_pOwner != nullptr) {
        m_pOwner->Release();
    }
}

STDMETHODIMP CTextService::CEditSessionGetCaret::QueryInterface(
    REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;
    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfEditSession)) {
        *ppvObj = static_cast<ITfEditSession*>(this);
    } else {
        return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTextService::CEditSessionGetCaret::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTextService::CEditSessionGetCaret::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

STDMETHODIMP CTextService::CEditSessionGetCaret::DoEditSession(TfEditCookie ec)
{
    HRESULT hr = E_FAIL;
    if (m_pic != nullptr && m_pOwner != nullptr) {
        TF_SELECTION selection = {};
        ULONG fetched = 0;
        hr = m_pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1, &selection, &fetched);
        if (SUCCEEDED(hr) && fetched > 0 && selection.range != nullptr) {
            ITfContextView* pView = nullptr;
            hr = m_pic->GetActiveView(&pView);
            if (SUCCEEDED(hr) && pView != nullptr) {
                RECT rc = {0, 0, 0, 0};
                BOOL clipped = FALSE;
                hr = pView->GetTextExt(ec, selection.range, &rc, &clipped);
                HWND hostWnd = nullptr;
                pView->GetWnd(&hostWnd);
                pView->Release();
                if (SUCCEEDED(hr)) {
                    if (rc.left != 0 || rc.top != 0 || rc.right != 0 || rc.bottom != 0) {
                        if (hostWnd != nullptr) {
                            taishen::CaretToPhysicalPixel(hostWnd, &rc);
                        }
                        m_pOwner->OnCaretRectResolved(m_seq, rc);
                    }
                }
            }
            selection.range->Release();
        }
    }
    return hr;
}

// ---------------------------------------------------------------------------
// 组合串拼装与编辑会话调度
// ---------------------------------------------------------------------------

std::string CTextService::BuildCompositionText(const std::string& pending) const
{
    return m_remoteState.inCompose ? (m_remoteState.composePrefix + pending) : pending;
}

void CTextService::SyncComposition(ITfContext* pic, const std::string& pending,
                                   bool forceStart)
{
    if (pic == nullptr) {
        return;
    }
    const bool inCompose = m_remoteState.inCompose;
    std::string tail = pending;
    if (tail.empty()) {
        tail = m_remoteState.syllableDisplay;
    }
    const std::string text = BuildCompositionText(tail);
    const CEditSessionComposition::Op op =
        (!forceStart && m_composition.IsActive())
            ? CEditSessionComposition::Op::Update
            : CEditSessionComposition::Op::Start;
    LONG convertedChars = 0;
    int cursorChars = -1;
    if (inCompose) {
        const std::string& prefix = m_remoteState.composePrefix;
        if (!prefix.empty()) {
            convertedChars = static_cast<LONG>(MultiByteToWideChar(
                CP_UTF8, 0, prefix.c_str(), static_cast<int>(prefix.size()),
                nullptr, 0));
        }
    } else {
        const int rawCursor = m_remoteState.cursor;
        const int rawLen = static_cast<int>(m_remoteState.rawPinyin.size());
        if (rawCursor >= 0 && rawCursor < rawLen) {
            cursorChars = MapRawCursorToDisplayChars(text, rawCursor);
        }
    }
    taishen::DebugLog(
        std::string("SyncComposition: compose=") + (inCompose ? "Y" : "N") +
        " op=" + (op == CEditSessionComposition::Op::Start ? "Start" : "Update") +
        " converted=" + std::to_string(convertedChars) +
        " cursorChars=" + std::to_string(cursorChars) +
        " text=" + text);
    RunCompositionOp(pic, op, text, -1, convertedChars, forceStart, cursorChars);
}

HRESULT CTextService::RunCompositionOp(ITfContext* pic,
                                       CEditSessionComposition::Op op,
                                       const std::string& text,
                                       int caretOffset,
                                       LONG convertedChars,
                                       bool forceStart,
                                       int cursorChars)
{
    if (pic == nullptr) {
        return E_POINTER;
    }

    CEditSessionComposition* pSession =
        new (std::nothrow) CEditSessionComposition(
            this, pic, op, text, &m_composition, caretOffset, convertedChars,
            forceStart, cursorChars);
    if (pSession == nullptr) {
        return E_OUTOFMEMORY;
    }

    const DWORD flags = taishen::IsAppContainerProcess()
                            ? (TF_ES_ASYNC | TF_ES_READWRITE)
                            : (TF_ES_ASYNCDONTCARE | TF_ES_READWRITE);
    HRESULT hrSession = E_FAIL;
    const HRESULT hr =
        pic->RequestEditSession(m_tid, pSession, flags, &hrSession);
    pSession->Release();
    taishen::DebugLogHr("RunCompositionOp: RequestEditSession", hr);
    taishen::DebugLogHr("RunCompositionOp: session", hrSession);
    if (FAILED(hr)) {
        return hr;
    }
    return hrSession;
}
