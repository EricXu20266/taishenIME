/// Windows TSF IME — TSF Text Service 实现
///
/// 对应 SPEC: docs/modules/interface-layer/SPEC.md
/// 覆盖 DEV-TRACKER: 0.1.2 (TSF DLL 骨架) + 0.1.5 (KeyEvent 捕获与 FFI 对接)
///
/// 组件：
///   - CClassFactory  : IClassFactory，创建 CTextService
///   - CTextService   : ITfTextInputProcessorEx + ITfKeyEventSink + ITfThreadMgrEventSink
///   - DLL 导出       : DllGetClassObject / DllCanUnloadNow / DllRegisterServer / DllUnregisterServer
///
/// 注册（HKCU，调试期无需管理员）：
///   regsvr32 /i taishen_ime.dll   或  rundll32 taishen_ime.dll,DllRegisterServer

#include <windows.h>
#include <msctf.h>
#include <string>
#include <vector>

#include "engine_bridge.h"
#include "tsf_keyevent.h"
#include "candidate_window.h"
#include "tsf_composition.h"

// DLL 自身模块句柄（定义于 dllmain.cpp）
extern HMODULE g_hModule;

// ---------------------------------------------------------------------------
// GUID 定义（正式生成，非占位）
// ---------------------------------------------------------------------------
// CLSID_TAISHEN_IME: Text Service 组件标识
// {7D77E4AA-276E-4582-B952-94B6EFAADA28}
static const CLSID CLSID_TAISHEN_IME = {
    0x7D77E4AA, 0x276E, 0x4582,
    {0xB9, 0x52, 0x94, 0xB6, 0xEF, 0xAA, 0xDA, 0x28}};

// GUID_LANGPROFILE: 语言配置文件标识（中文 zh-CN 0x0804）
// {882067BC-3F50-4904-ADC9-282C8F0CF91E}
static const GUID GUID_LANGPROFILE = {
    0x882067BC, 0x3F50, 0x4904,
    {0xAD, 0xC9, 0x28, 0x2C, 0x8F, 0x0C, 0xF9, 0x1E}};

// ---------------------------------------------------------------------------
// 前向声明
// ---------------------------------------------------------------------------
class CTextService;
class CClassFactory;

// 模块引用计数（DllCanUnloadNow 使用）
static LONG g_cRefDll = 0;

// ---------------------------------------------------------------------------
// CTextService — TSF Text Service 核心
// ---------------------------------------------------------------------------
class CTextService : public ITfTextInputProcessorEx,
                     public ITfKeyEventSink,
                     public ITfThreadMgrEventSink
{
public:
    CTextService();
    virtual ~CTextService();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // ITfTextInputProcessorEx
    STDMETHODIMP Activate(ITfThreadMgr* ptim, TfClientId tid) override;
    STDMETHODIMP ActivateEx(ITfThreadMgr* ptim, TfClientId tid,
                            DWORD dwFlags) override;
    STDMETHODIMP Deactivate() override;

    // ITfKeyEventSink
    STDMETHODIMP OnSetFocus(BOOL fForeground) override;
    STDMETHODIMP OnTestKeyDown(ITfContext* pic, WPARAM wParam,
                               LPARAM lParam, BOOL* pfEaten) override;
    STDMETHODIMP OnTestKeyUp(ITfContext* pic, WPARAM wParam,
                             LPARAM lParam, BOOL* pfEaten) override;
    STDMETHODIMP OnKeyDown(ITfContext* pic, WPARAM wParam,
                           LPARAM lParam, BOOL* pfEaten) override;
    STDMETHODIMP OnKeyUp(ITfContext* pic, WPARAM wParam,
                         LPARAM lParam, BOOL* pfEaten) override;
    STDMETHODIMP OnPreservedKey(ITfContext* pic, REFGUID rguid,
                                BOOL* pfEaten) override;

    // ITfThreadMgrEventSink
    STDMETHODIMP OnInitDocumentMgr(ITfDocumentMgr* pdim) override;
    STDMETHODIMP OnUninitDocumentMgr(ITfDocumentMgr* pdim) override;
    STDMETHODIMP OnSetFocus(ITfDocumentMgr* pdimFocus,
                            ITfDocumentMgr* pdimPrevFocus) override;
    STDMETHODIMP OnPushContext(ITfContext* pic) override;
    STDMETHODIMP OnPopContext(ITfContext* pic) override;

private:
    // 刷新拼音串 + 候选状态（供 0.1.6 候选窗口使用）
    void RefreshState();
    // 获取光标屏幕坐标（编辑会话）→ 更新候选窗口
    void UpdateCandidateWindow();
    // 编辑会话回调（ITfEditSession）：获取光标坐标
    HRESULT GetCaretRectFromContext(ITfContext* pic, RECT* pRect);

    // 候选窗口（Direct2D 渲染）
    taishen::CCandidateWindow m_candidateWindow;
    // TSF 组合管理（选词上屏）
    taishen::CTsfComposition m_composition;

    // 内部编辑会话：更新组合（Start/Update/Commit 共用）
    class CEditSessionComposition : public ITfEditSession
    {
    public:
        enum class Op { Start, Update, Commit };

        CEditSessionComposition(ITfContext* pic, Op op,
                                const std::string& text,
                                taishen::CTsfComposition* comp)
            : m_cRef(1), m_pic(pic), m_op(op), m_text(text), m_pComp(comp) {}

        // IUnknown
        STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
        {
            if (ppvObj == nullptr) { return E_POINTER; }
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
        STDMETHODIMP_(ULONG) AddRef() override
        {
            return InterlockedIncrement(&m_cRef);
        }
        STDMETHODIMP_(ULONG) Release() override
        {
            const ULONG ref = InterlockedDecrement(&m_cRef);
            if (ref == 0) { delete this; }
            return ref;
        }

        // ITfEditSession
        STDMETHODIMP DoEditSession(TfEditCookie ec) override
        {
            if (m_pComp == nullptr) { return E_POINTER; }
            switch (m_op) {
            case Op::Start:
                return m_pComp->StartComposition(ec, m_pic, m_text);
            case Op::Update:
                return m_pComp->UpdateComposition(ec, m_pic, m_text);
            case Op::Commit:
                return m_pComp->CommitComposition(ec, m_pic, m_text);
            }
            return E_FAIL;
        }

    private:
        LONG m_cRef;
        ITfContext* m_pic;
        Op m_op;
        std::string m_text;
        taishen::CTsfComposition* m_pComp;
    };

    // 编辑会话调度辅助：在同步编辑会话中执行组合操作
    void RunCompositionOp(ITfContext* pic, CEditSessionComposition::Op op,
                          const std::string& text);

    // 内部编辑会话：获取光标屏幕坐标
    class CEditSessionGetCaret : public ITfEditSession
    {
    public:
        CEditSessionGetCaret(ITfContext* pic, RECT* pRect)
            : m_cRef(1), m_pic(pic), m_pRect(pRect) {}

        // IUnknown
        STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
        {
            if (ppvObj == nullptr) { return E_POINTER; }
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
        STDMETHODIMP_(ULONG) AddRef() override
        {
            return InterlockedIncrement(&m_cRef);
        }
        STDMETHODIMP_(ULONG) Release() override
        {
            const ULONG ref = InterlockedDecrement(&m_cRef);
            if (ref == 0) { delete this; }
            return ref;
        }

        // ITfEditSession
        STDMETHODIMP DoEditSession(TfEditCookie ec) override
        {
            HRESULT hr = E_FAIL;
            if (m_pic != nullptr && m_pRect != nullptr) {
                // 1. 获取光标选区
                TF_SELECTION selection = {};
                ULONG fetched = 0;
                hr = m_pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1,
                                         &selection, &fetched);
                if (SUCCEEDED(hr) && fetched > 0 && selection.range != nullptr) {
                    // 2. 通过视图获取选区屏幕坐标
                    ITfContextView* pView = nullptr;
                    hr = m_pic->GetActiveView(&pView);
                    if (SUCCEEDED(hr) && pView != nullptr) {
                        BOOL clipped = FALSE;
                        hr = pView->GetTextExt(ec, selection.range,
                                               m_pRect, &clipped);
                        pView->Release();
                    }
                    selection.range->Release();
                }
            }
            return hr;
        }

    private:
        LONG m_cRef;
        ITfContext* m_pic;
        RECT* m_pRect;
    };

    // IUnknown 引用计数
    LONG m_cRef;
    // 线程管理器（TSF 框架注入）
    ITfThreadMgr* m_pThreadMgr;
    // 语言环境上下文（用于注册 KeyEventSink）
    TfClientId m_tid;
    // 线程管理器事件接收器 cookie（注销用）
    DWORD m_dwThreadMgrEventSinkCookie;
    // 当前焦点上下文（OnSetFocus 记录）
    ITfContext* m_pFocusContext;
    // 当前拼音串（UTF-8，来自引擎）
    std::string m_pinyin;
    // 当前候选词列表（UTF-8）
    std::vector<std::string> m_candidates;
    // 最后一次选词提交的文本
    std::wstring m_committedText;
    // 是否为前台激活
    BOOL m_fActive;
};

// ---------------------------------------------------------------------------
// CTextService 实现
// ---------------------------------------------------------------------------
CTextService::CTextService()
    : m_cRef(1), m_pThreadMgr(nullptr), m_tid(0),
      m_dwThreadMgrEventSinkCookie(0),
      m_pFocusContext(nullptr), m_fActive(FALSE)
{
    InterlockedIncrement(&g_cRefDll);
}

CTextService::~CTextService()
{
    InterlockedDecrement(&g_cRefDll);
}

STDMETHODIMP CTextService::QueryInterface(REFIID riid, void** ppvObj)
{
    if (ppvObj == nullptr) {
        return E_POINTER;
    }
    *ppvObj = nullptr;

    if (IsEqualIID(riid, IID_IUnknown) ||
        IsEqualIID(riid, IID_ITfTextInputProcessorEx)) {
        *ppvObj = static_cast<ITfTextInputProcessorEx*>(this);
    } else if (IsEqualIID(riid, IID_ITfKeyEventSink)) {
        *ppvObj = static_cast<ITfKeyEventSink*>(this);
    } else if (IsEqualIID(riid, IID_ITfThreadMgrEventSink)) {
        *ppvObj = static_cast<ITfThreadMgrEventSink*>(this);
    } else {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

STDMETHODIMP_(ULONG) CTextService::AddRef()
{
    return InterlockedIncrement(&m_cRef);
}

STDMETHODIMP_(ULONG) CTextService::Release()
{
    const ULONG ref = InterlockedDecrement(&m_cRef);
    if (ref == 0) {
        delete this;
    }
    return ref;
}

// ---------------------------------------------------------------------------
// ITfTextInputProcessorEx
// ---------------------------------------------------------------------------
STDMETHODIMP CTextService::Activate(ITfThreadMgr* ptim, TfClientId tid)
{
    // ITfTextInputProcessor 纯虚方法，转发到 Ex 版本
    return ActivateEx(ptim, tid, 0);
}

STDMETHODIMP CTextService::ActivateEx(ITfThreadMgr* ptim, TfClientId tid,
                                      DWORD /*dwFlags*/)
{
    if (ptim == nullptr) {
        return E_INVALIDARG;
    }

    m_pThreadMgr = ptim;
    m_pThreadMgr->AddRef();
    m_tid = tid;

    // 初始化引擎（词库路径留空 → 回退内置词库，词库加载通道 0.1.4 已完成）
    engine_init(nullptr);

    // 注册线程管理器事件接收器（焦点变化通知）
    // ITfThreadMgr 通过 ITfSource::AdviseSink 注册事件接收器
    ITfSource* pSource = nullptr;
    HRESULT hr = m_pThreadMgr->QueryInterface(IID_ITfSource,
                                              reinterpret_cast<void**>(&pSource));
    if (SUCCEEDED(hr) && pSource != nullptr) {
        hr = pSource->AdviseSink(IID_ITfThreadMgrEventSink,
                                 static_cast<ITfThreadMgrEventSink*>(this),
                                 &m_dwThreadMgrEventSinkCookie);
        pSource->Release();
        if (FAILED(hr)) {
            return hr;
        }
    }

    // 注册键盘事件接收器（按键捕获）
    ITfKeystrokeMgr* pKeystrokeMgr = nullptr;
    hr = m_pThreadMgr->QueryInterface(IID_ITfKeystrokeMgr,
                                      reinterpret_cast<void**>(&pKeystrokeMgr));
    if (SUCCEEDED(hr) && pKeystrokeMgr != nullptr) {
        // 第三个参数 fForeground=TRUE：只在获得焦点时接收按键
        hr = pKeystrokeMgr->AdviseKeyEventSink(
            tid, static_cast<ITfKeyEventSink*>(this), TRUE);
        pKeystrokeMgr->Release();
        if (FAILED(hr)) {
            return hr;
        }
    }

    m_fActive = TRUE;
    return S_OK;
}

STDMETHODIMP CTextService::Deactivate()
{
    // 注销事件接收器
    if (m_pThreadMgr != nullptr) {
        ITfSource* pSource = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfSource, reinterpret_cast<void**>(&pSource))) &&
            pSource != nullptr) {
            pSource->UnadviseSink(m_dwThreadMgrEventSinkCookie);
            pSource->Release();
        }
    }

    if (m_pFocusContext != nullptr) {
        m_pFocusContext->Release();
        m_pFocusContext = nullptr;
    }

    if (m_pThreadMgr != nullptr) {
        m_pThreadMgr->Release();
        m_pThreadMgr = nullptr;
    }

    engine_destroy();
    m_fActive = FALSE;
    m_candidateWindow.Hide();
    m_composition.Reset();
    return S_OK;
}

// ---------------------------------------------------------------------------
// ITfKeyEventSink
// ---------------------------------------------------------------------------
STDMETHODIMP CTextService::OnSetFocus(BOOL /*fForeground*/)
{
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyDown(ITfContext* /*pic*/, WPARAM wParam,
                                         LPARAM lParam, BOOL* pfEaten)
{
    // 预测试：我们是否要吞这个键（不实际执行）
    taishen::KeyEventResult result;
    const bool eat = taishen::HandleKeyDown(static_cast<int>(wParam), lParam,
                                            result);
    if (pfEaten != nullptr) {
        *pfEaten = eat ? TRUE : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyUp(ITfContext* /*pic*/, WPARAM /*wParam*/,
                                       LPARAM /*lParam*/, BOOL* pfEaten)
{
    if (pfEaten != nullptr) {
        *pfEaten = FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnKeyDown(ITfContext* pic, WPARAM wParam,
                                     LPARAM lParam, BOOL* pfEaten)
{
    taishen::KeyEventResult result;
    const bool eat = taishen::HandleKeyDown(static_cast<int>(wParam), lParam,
                                            result);

    if (eat) {
        if (result.state_changed) {
            RefreshState();
            // 更新/创建组合（拼音输入时目标应用显示拼音）
            if (!m_pinyin.empty()) {
                if (m_composition.IsActive()) {
                    RunCompositionOp(pic, CEditSessionComposition::Op::Update,
                                     m_pinyin);
                } else {
                    RunCompositionOp(pic, CEditSessionComposition::Op::Start,
                                     m_pinyin);
                }
            }
            // 刷新候选窗口
            UpdateCandidateWindow();
        }
        if (!result.committed.empty()) {
            // 选词上屏：组合替换为汉字并结束
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(result.committed));
            m_committedText = result.committed;
            // 提交后候选清空，隐藏候选窗口
            m_candidateWindow.Hide();
        }
    } else if (!m_pinyin.empty() && wParam == VK_ESCAPE) {
        // ESC 取消当前输入：结束组合（空文本=撤销）
        RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
        engine_reset();
        RefreshState();
        m_candidateWindow.Hide();
    } else if (!m_pinyin.empty() && !m_composition.IsActive() &&
               (wParam == VK_LEFT || wParam == VK_RIGHT ||
                wParam == VK_UP || wParam == VK_DOWN)) {
        // 光标键：结束组合但保留拼音（候选窗口仍显示）
        RunCompositionOp(pic, CEditSessionComposition::Op::Commit, m_pinyin);
        engine_reset();
        RefreshState();
    }

    if (pfEaten != nullptr) {
        *pfEaten = eat ? TRUE : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnKeyUp(ITfContext* /*pic*/, WPARAM /*wParam*/,
                                   LPARAM /*lParam*/, BOOL* pfEaten)
{
    if (pfEaten != nullptr) {
        *pfEaten = FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnPreservedKey(ITfContext* /*pic*/, REFGUID /*rguid*/,
                                          BOOL* pfEaten)
{
    if (pfEaten != nullptr) {
        *pfEaten = FALSE;
    }
    return S_OK;
}

// ---------------------------------------------------------------------------
// ITfThreadMgrEventSink
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
    // 记录当前焦点上下文（候选窗口定位与文本提交用）
    if (m_pFocusContext != nullptr) {
        m_pFocusContext->Release();
        m_pFocusContext = nullptr;
    }
    if (pdimFocus != nullptr) {
        pdimFocus->GetTop(&m_pFocusContext);
    }
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
// 内部辅助
// ---------------------------------------------------------------------------
void CTextService::RefreshState()
{
    // 拉取拼音串
    char buf[512] = {0};
    const int len = engine_get_pinyin_str(buf, sizeof(buf));
    if (len > 0) {
        m_pinyin.assign(buf, static_cast<size_t>(len - 1));
    } else {
        m_pinyin.clear();
    }

    // 拉取候选词
    m_candidates.clear();
    const int count = engine_get_candidate_count();
    for (int i = 0; i < count && i < 16; ++i) {
        char cbuf[512] = {0};
        const int clen = engine_get_candidate(i, cbuf, sizeof(cbuf));
        if (clen > 0) {
            m_candidates.emplace_back(cbuf, static_cast<size_t>(clen - 1));
        }
    }
}

// ---------------------------------------------------------------------------
// 候选窗口
// ---------------------------------------------------------------------------

/// 通过 TSF 编辑会话获取光标屏幕坐标。
/// 流程：RequestEditSession(TF_ES_SYNC) → DoEditSession → GetSelection
///       → GetActiveView → GetTextExt
/// 失败时返回 E_FAIL，调用方降级为屏幕左上角。
HRESULT CTextService::GetCaretRectFromContext(ITfContext* pic, RECT* pRect)
{
    if (pic == nullptr || pRect == nullptr) {
        return E_POINTER;
    }

    // 请求同步编辑会话（TSF 会回调我们的 CEditSessionGetCaret）
    CEditSessionGetCaret* pSession = new (std::nothrow) CEditSessionGetCaret(pic, pRect);
    if (pSession == nullptr) {
        return E_OUTOFMEMORY;
    }

    HRESULT hrSession = E_FAIL;
    HRESULT hr = pic->RequestEditSession(m_tid, pSession,
                                         TF_ES_SYNC | TF_ES_READ,
                                         &hrSession);
    pSession->Release();
    if (FAILED(hr)) {
        return hr;
    }
    return hrSession;
}

/// 更新候选窗口：拉取光标坐标 → 传入拼音/候选 → 显示或隐藏
void CTextService::UpdateCandidateWindow()
{
    // 拼音为空时引擎已清候选，直接隐藏
    if (m_pinyin.empty()) {
        m_candidateWindow.Hide();
        return;
    }

    // 获取光标屏幕坐标（降级：屏幕左上角）
    RECT caretRect = {0, 0, 0, 0};
    if (m_pFocusContext != nullptr) {
        if (FAILED(GetCaretRectFromContext(m_pFocusContext, &caretRect))) {
            caretRect = {0, 0, 0, 0};
        }
    }

    m_candidateWindow.UpdateState(m_pinyin, m_candidates, caretRect);
}

/// 在同步编辑会话中执行组合操作（Start/Update/Commit）
void CTextService::RunCompositionOp(ITfContext* pic,
                                    CEditSessionComposition::Op op,
                                    const std::string& text)
{
    if (pic == nullptr) {
        return;
    }

    CEditSessionComposition* pSession =
        new (std::nothrow) CEditSessionComposition(pic, op, text,
                                                   &m_composition);
    if (pSession == nullptr) {
        return;
    }

    HRESULT hrSession = E_FAIL;
    pic->RequestEditSession(m_tid, pSession, TF_ES_SYNC | TF_ES_READWRITE,
                            &hrSession);
    pSession->Release();
}

// ---------------------------------------------------------------------------
// CClassFactory — COM 类工厂
// ---------------------------------------------------------------------------
class CClassFactory : public IClassFactory
{
public:
    CClassFactory() : m_cRef(1)
    {
        InterlockedIncrement(&g_cRefDll);
    }

    virtual ~CClassFactory()
    {
        InterlockedDecrement(&g_cRefDll);
    }

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
        *ppvObj = nullptr;
        if (IsEqualIID(riid, IID_IUnknown) || IsEqualIID(riid, IID_IClassFactory)) {
            *ppvObj = static_cast<IClassFactory*>(this);
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

    // IClassFactory
    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid,
                                void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
        *ppvObj = nullptr;

        // 不支持聚合
        if (pUnkOuter != nullptr) {
            return CLASS_E_NOAGGREGATION;
        }

        CTextService* pService = new (std::nothrow) CTextService();
        if (pService == nullptr) {
            return E_OUTOFMEMORY;
        }

        const HRESULT hr = pService->QueryInterface(riid, ppvObj);
        pService->Release(); // 初始引用由 QI 接管
        return hr;
    }

    STDMETHODIMP LockServer(BOOL fLock) override
    {
        if (fLock) {
            InterlockedIncrement(&g_cRefDll);
        } else {
            InterlockedDecrement(&g_cRefDll);
        }
        return S_OK;
    }

private:
    LONG m_cRef;
};

// ---------------------------------------------------------------------------
// DLL 导出函数
// ---------------------------------------------------------------------------

// 导出方式：.def 文件（platform/windows/taishen_ime.def）
// 注意：DllGetClassObject / DllCanUnloadNow 已被 Windows SDK 预声明（STDAPI），
//       这里按 SDK 签名实现，导出交给 .def 文件处理。

/// DllGetClassObject — 返回类工厂
STDAPI DllGetClassObject(REFCLSID rclsid, REFIID riid, LPVOID* ppv)
{
    if (ppv == nullptr) {
        return E_POINTER;
    }
    *ppv = nullptr;

    if (!IsEqualCLSID(rclsid, CLSID_TAISHEN_IME)) {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    CClassFactory* pFactory = new (std::nothrow) CClassFactory();
    if (pFactory == nullptr) {
        return E_OUTOFMEMORY;
    }

    const HRESULT hr = pFactory->QueryInterface(riid, ppv);
    pFactory->Release();
    return hr;
}

/// DllCanUnloadNow — 无活动引用时可卸载
STDAPI DllCanUnloadNow(void)
{
    return (g_cRefDll == 0) ? S_OK : S_FALSE;
}

// ---------------------------------------------------------------------------
// 注册表辅助
// ---------------------------------------------------------------------------

/// 写注册表键值（HKCU）
static bool WriteRegKey(HKEY hkRoot, const std::wstring& subKey,
                        const std::wstring& valueName,
                        const std::wstring& valueData)
{
    HKEY hKey = nullptr;
    const LSTATUS status = RegCreateKeyExW(hkRoot, subKey.c_str(), 0, nullptr,
                                           REG_OPTION_NON_VOLATILE, KEY_WRITE,
                                           nullptr, &hKey, nullptr);
    if (status != ERROR_SUCCESS) {
        return false;
    }

    const DWORD dataSize = static_cast<DWORD>((valueData.size() + 1) * sizeof(wchar_t));
    const LSTATUS setStatus = RegSetValueExW(
        hKey, valueName.c_str(), 0, REG_SZ,
        reinterpret_cast<const BYTE*>(valueData.c_str()), dataSize);
    RegCloseKey(hKey);
    return setStatus == ERROR_SUCCESS;
}

/// 删除注册表键（递归，HKCU）
static void DeleteRegKey(HKEY hkRoot, const std::wstring& subKey)
{
    RegDeleteTreeW(hkRoot, subKey.c_str());
}

/// 获取当前 DLL 的完整路径
static std::wstring GetDllPath()
{
    // 用 DLL 自身模块句柄（g_hModule），而非 GetModuleFileNameW(nullptr)
    // —— nullptr 会返回宿主进程路径（regsvr32.exe 加载时就是 regsvr32 的路径）
    wchar_t path[MAX_PATH] = {0};
    const DWORD len = GetModuleFileNameW(g_hModule, path, MAX_PATH);
    if (len == 0) {
        return std::wstring();
    }
    return std::wstring(path, len);
}

/// 生成 CLSID 的注册表字符串 "{xxxxxxxx-xxxx-...}"
static std::wstring ClsidToString(const CLSID& clsid)
{
    wchar_t buf[64] = {0};
    StringFromGUID2(clsid, buf, 64);
    return std::wstring(buf);
}

/// TSF Text Service 注册表键名（调试期 HKCU，无需管理员权限）
static std::wstring GetTipRegKey(const std::wstring& clsidStr)
{
    return L"Software\\Microsoft\\CTF\\TIP\\" + clsidStr;
}

/// DllRegisterServer — 注册 TSF Text Service（HKCU）
///
/// 注册内容：
///   1. HKCU\Software\Classes\CLSID\{CLSID}\InprocServer32 → DLL 路径
///   2. HKCU\Software\Microsoft\CTF\TIP\{CLSID} （TSF 识别）
///   3. LanguageProfile：0x0804 (zh-CN) + GUID_LANGPROFILE
///   4. Category：GUID_TFCAT_TIP_KEYBOARD（键盘输入法类别）
STDAPI DllRegisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);
    const std::wstring dllPath = GetDllPath();
    if (dllPath.empty()) {
        return E_FAIL;
    }

    // 1. CLSID → InprocServer32（HKCU\Software\Classes）
    const std::wstring clsidKey =
        L"Software\\Classes\\CLSID\\" + clsidStr + L"\\InprocServer32";
    if (!WriteRegKey(HKEY_CURRENT_USER, clsidKey, L"", dllPath)) {
        return E_FAIL;
    }
    WriteRegKey(HKEY_CURRENT_USER, clsidKey, L"ThreadingModel", L"Apartment");

    // 2. CTF\TIP 主键 + 描述
    const std::wstring tipKey = GetTipRegKey(clsidStr);
    if (!WriteRegKey(HKEY_CURRENT_USER, tipKey, L"Description",
                     L"泰深输入法")) {
        return E_FAIL;
    }

    // 3. LanguageProfile（zh-CN 0x0804）
    const std::wstring langKey = tipKey + L"\\LanguageProfile\\0x00000804\\" +
                                 ClsidToString(GUID_LANGPROFILE);
    if (!WriteRegKey(HKEY_CURRENT_USER, langKey, L"Description",
                     L"泰深拼音")) {
        return E_FAIL;
    }
    WriteRegKey(HKEY_CURRENT_USER, langKey, L"Enable", L"1");

    // 4. Category — 键盘输入法（GUID_TFCAT_TIP_KEYBOARD）
    // {34745C63-B2F0-4784-8B67-5E12C8701A31}
    const std::wstring catKey =
        tipKey + L"\\Category\\{34745C63-B2F0-4784-8B67-5E12C8701A31}";
    if (!WriteRegKey(HKEY_CURRENT_USER, catKey, L"", L"")) {
        return E_FAIL;
    }

    return S_OK;
}

/// DllUnregisterServer — 注销 TSF Text Service（HKCU）
STDAPI DllUnregisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);

    // 删除 CLSID 注册
    DeleteRegKey(HKEY_CURRENT_USER,
                 L"Software\\Classes\\CLSID\\" + clsidStr);

    // 删除 CTF\TIP 注册
    DeleteRegKey(HKEY_CURRENT_USER, GetTipRegKey(clsidStr));

    return S_OK;
}




