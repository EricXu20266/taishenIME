/// TSF 组合管理 — 实现
///
/// 对应 SPEC: docs/modules/composition/SPEC.md
/// 组合流程：
///   StartComposition → ITfContextComposition::StartComposition(光标位置, sink)
///   UpdateComposition → 组合 range 的 SetText(拼音串)
///   CommitComposition → 组合 range 的 SetText(汉字) + EndComposition
///
/// V0.4.x 配对符号光标定位：SendInput VK_LEFT
///   Scintilla 的 TSF 实现在编辑会话后锁死所有 range 位移
///   （ShiftEnd/ShiftStart 全部 shifted=0，15 次方案实证），
///   SetSelection 返回 OK 但被忽略。最终用 SendInput 模拟
///   左箭头键——走 Windows 原生输入层，与 TSF 完全解耦。

#include "tsf_composition.h"
#include "debug_log.h"

namespace taishen {

// UTF-8 → 宽字符串
std::wstring CTsfComposition::Utf8ToWide(const std::string& utf8)
{
    if (utf8.empty()) {
        return std::wstring();
    }
    const int len = MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                                        static_cast<int>(utf8.size()),
                                        nullptr, 0);
    if (len <= 0) {
        return std::wstring();
    }
    std::wstring result(static_cast<size_t>(len), L'\0');
    MultiByteToWideChar(CP_UTF8, 0, utf8.c_str(),
                        static_cast<int>(utf8.size()), &result[0], len);
    return result;
}

// ---------------------------------------------------------------------------
// 构造/析构
// ---------------------------------------------------------------------------
CTsfComposition::CTsfComposition()
    : m_cRef(1), m_pComposition(nullptr)
{
}

CTsfComposition::~CTsfComposition()
{
}

// ---------------------------------------------------------------------------
// IUnknown
// ---------------------------------------------------------------------------
STDMETHODIMP CTsfComposition::QueryInterface(REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;
    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfCompositionSink)) {
        *ppvObj = static_cast<ITfCompositionSink*>(this);
    } else {
        return E_NOINTERFACE;
    }
    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTsfComposition::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTsfComposition::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

// ---------------------------------------------------------------------------
// ITfCompositionSink — 组合被外部结束（应用撤销等）
// ---------------------------------------------------------------------------
STDMETHODIMP CTsfComposition::OnCompositionTerminated(
    TfEditCookie /*ec*/, ITfComposition* /*pComposition*/)
{
    m_pComposition = nullptr;
    return S_OK;
}

// ---------------------------------------------------------------------------
// 组合操作
// ---------------------------------------------------------------------------

HRESULT CTsfComposition::StartComposition(TfEditCookie ec, ITfContext* pic,
                                          const std::string& pinyin)
{
    if (pic == nullptr) {
        return E_POINTER;
    }

    if (m_pComposition != nullptr) {
        m_pComposition->EndComposition(ec);
        m_pComposition = nullptr;
    }

    TF_SELECTION selection = {};
    ULONG fetched = 0;
    HRESULT hr = pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1,
                                   &selection, &fetched);
    if (FAILED(hr) || fetched == 0 || selection.range == nullptr) {
        return E_FAIL;
    }

    ITfContextComposition* pContextComp = nullptr;
    hr = pic->QueryInterface(IID_ITfContextComposition,
                             reinterpret_cast<void**>(&pContextComp));
    if (SUCCEEDED(hr) && pContextComp != nullptr) {
        hr = pContextComp->StartComposition(ec, selection.range,
                                            this, &m_pComposition);
        pContextComp->Release();
        if (SUCCEEDED(hr) && m_pComposition != nullptr) {
            const std::wstring text = Utf8ToWide(pinyin);
            hr = selection.range->SetText(ec, 0, text.c_str(),
                                          static_cast<LONG>(text.size()));
        }
    }

    if (selection.range != nullptr) {
        selection.range->Release();
    }
    return hr;
}

HRESULT CTsfComposition::UpdateComposition(TfEditCookie ec, ITfContext* pic,
                                           const std::string& pinyin)
{
    if (m_pComposition == nullptr) {
        return StartComposition(ec, pic, pinyin);
    }
    return ReplaceCompositionText(ec, pic, Utf8ToWide(pinyin));
}

HRESULT CTsfComposition::CommitComposition(TfEditCookie ec, ITfContext* pic,
                                           const std::string& text,
                                           int caretOffset)
{
    if (m_pComposition == nullptr) {
        if (text.empty()) {
            return S_OK;
        }

        // V0.4.x 配对符号：无活跃拼音 → 直接 SetText（不走组合）
        if (caretOffset >= 0) {
            std::wstring wtext = Utf8ToWide(text);
            TF_SELECTION sel = {};
            ULONG fetched = 0;
            HRESULT hr = pic->GetSelection(ec, TF_DEFAULT_SELECTION,
                                           1, &sel, &fetched);
            if (FAILED(hr) || fetched == 0 || sel.range == nullptr) {
                return E_FAIL;
            }
            hr = sel.range->SetText(ec, 0, wtext.c_str(),
                                    static_cast<LONG>(wtext.size()));
            sel.range->Release();
            if (SUCCEEDED(hr)) {
                SendLeftArrow(caretOffset, true);
            }
            taishen::DebugLog("CommitComposition(direct): text=" + text +
                              " caretOffset=" + std::to_string(caretOffset) +
                              " hr=" + (SUCCEEDED(hr) ? "OK" : "FAIL"));
            return hr;
        }

        // 普通路径：先建组合再提交
        HRESULT hrStart = StartComposition(ec, pic, "");
        if (FAILED(hrStart)) {
            return hrStart;
        }
    }

    // 替换组合文本 → 结束组合
    HRESULT hr = ReplaceCompositionText(ec, pic, Utf8ToWide(text));
    if (m_pComposition != nullptr) {
        m_pComposition->EndComposition(ec);
        m_pComposition = nullptr;
    }

    // V0.4.x 配对符号光标定位：SendInput VK_LEFT
    if (caretOffset >= 0 && SUCCEEDED(hr)) {
        SendLeftArrow(caretOffset, true);
    }

    taishen::DebugLog("CommitComposition: text=" + text +
                      " caretOffset=" + std::to_string(caretOffset) +
                      " hr=" + (SUCCEEDED(hr) ? "OK" : "FAIL"));
    return hr;
}

// ---------------------------------------------------------------------------
// SendLeftArrow — V0.4.x 配对符号光标定位
// 15 次方案实证：Scintilla 编辑会话后锁死所有 TSF range 位移。
// SendInput 走 Windows 原生输入层，Shift 按下时临时释放再恢复。
// ---------------------------------------------------------------------------
void CTsfComposition::SendLeftArrow(int count, bool handleShift)
{
    const bool shiftHeld = handleShift &&
        (GetAsyncKeyState(VK_SHIFT) & 0x8000) != 0;
    INPUT inputs[4] = {};
    int n = 0;
    if (shiftHeld) {
        inputs[n].type = INPUT_KEYBOARD;
        inputs[n].ki.wVk = VK_SHIFT;
        inputs[n].ki.dwFlags = KEYEVENTF_KEYUP;
        n++;
    }
    for (int i = 0; i < count; i++) {
        inputs[n].type = INPUT_KEYBOARD;
        inputs[n].ki.wVk = VK_LEFT;
        n++;
        inputs[n].type = INPUT_KEYBOARD;
        inputs[n].ki.wVk = VK_LEFT;
        inputs[n].ki.dwFlags = KEYEVENTF_KEYUP;
        n++;
    }
    if (shiftHeld) {
        inputs[n].type = INPUT_KEYBOARD;
        inputs[n].ki.wVk = VK_SHIFT;
        n++;
    }
    const UINT sent = SendInput(n, inputs, sizeof(INPUT));
    taishen::DebugLog("SendLeftArrow: count=" + std::to_string(count) +
                      " shiftHeld=" + (shiftHeld ? "Y" : "N") +
                      " sent=" + std::to_string(sent));
}

// ---------------------------------------------------------------------------
// ReplaceCompositionText
// ---------------------------------------------------------------------------
HRESULT CTsfComposition::ReplaceCompositionText(TfEditCookie ec,
                                                ITfContext* pic,
                                                const std::wstring& text)
{
    if (pic == nullptr || m_pComposition == nullptr) {
        return E_POINTER;
    }

    ITfRange* pRange = nullptr;
    HRESULT hr = m_pComposition->GetRange(&pRange);
    if (FAILED(hr) || pRange == nullptr) {
        return hr;
    }

    hr = pRange->SetText(ec, 0, text.c_str(), static_cast<LONG>(text.size()));

    if (SUCCEEDED(hr)) {
        pRange->Collapse(ec, TF_ANCHOR_END);
        m_pComposition->ShiftEnd(ec, pRange);
        pRange->Release();
        pRange = nullptr;
    }

    if (pRange != nullptr) {
        pRange->Release();
    }
    return hr;
}

} // namespace taishen
