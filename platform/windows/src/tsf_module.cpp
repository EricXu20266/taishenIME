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
#include "config_reader.h"
#include "debug_log.h"

// DLL 自身模块句柄（定义于 dllmain.cpp）
extern HMODULE g_hModule;

// 辅助函数（定义于文件末尾）
static std::wstring GetDllPath();
static std::wstring GetDllDir();

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
    // 候选窗口鼠标点击选词（0.1.13）：选择并提交候选
    void OnCandidateClicked(int index);

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
    // 候选窗口鼠标点击选词（0.1.13）：回调在此上下文执行选词提交
    m_candidateWindow.SetClickCallback(
        [this](int index) { this->OnCandidateClicked(index); });
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
    } else if (IsEqualIID(riid, IID_ITfTextInputProcessor)) {
        // 老接口：TSF 可能请求 ITfTextInputProcessor（非 Ex）
        // ITfTextInputProcessorEx 继承自它，同一对象可安全双暴露
        *ppvObj = static_cast<ITfTextInputProcessor*>(this);
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
    taishen::DebugLog("ActivateEx enter");
    if (ptim == nullptr) {
        taishen::DebugLog("ActivateEx: ptim==nullptr -> E_INVALIDARG");
        return E_INVALIDARG;
    }

    m_pThreadMgr = ptim;
    m_pThreadMgr->AddRef();
    m_tid = tid;

    // 读取配置（config.ini，失败回退默认值）
    const std::wstring dllDir = GetDllDir();
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);

    // 初始化引擎：词库路径（空 → 回退内置词库）
    const std::wstring dictPath = taishen::ResolveDictPath(cfg, dllDir);
    std::string dictPathUtf8;
    if (!dictPath.empty()) {
        const int len = WideCharToMultiByte(CP_UTF8, 0, dictPath.c_str(),
                                            static_cast<int>(dictPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            dictPathUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, dictPath.c_str(),
                                static_cast<int>(dictPath.size()),
                                &dictPathUtf8[0], len, nullptr, nullptr);
        }
    }
    const int initRet = engine_init(dictPathUtf8.empty() ? nullptr : dictPathUtf8.c_str());
    taishen::DebugLog("ActivateEx: engine_init ret=" + std::to_string(initRet) +
                      " dict=" + dictPathUtf8);

    // 设置用户词库路径（V0.2.2）：默认 %APPDATA%/taishen-ime/user_dict.db
    const std::wstring userDictPath = taishen::ResolveUserDictPath(cfg, dllDir);
    std::string userDictPathUtf8;
    if (!userDictPath.empty()) {
        const int len = WideCharToMultiByte(CP_UTF8, 0, userDictPath.c_str(),
                                            static_cast<int>(userDictPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            userDictPathUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, userDictPath.c_str(),
                                static_cast<int>(userDictPath.size()),
                                &userDictPathUtf8[0], len, nullptr, nullptr);
        }
    }
    const int userRet = engine_set_user_dict_path(
        userDictPathUtf8.empty() ? nullptr : userDictPathUtf8.c_str());
    taishen::DebugLog("ActivateEx: engine_set_user_dict_path ret=" +
                      std::to_string(userRet) + " path=" + userDictPathUtf8);

    // 设置候选数上限
    engine_set_candidate_count(cfg.candidate_count);

    // 模糊音开关（RIME 拼写变体，0.1.14）
    engine_set_fuzzy(cfg.fuzzy_enabled ? 1 : 0);

    // 双拼模式（RIME 微软双拼方案，0.1.14）
    engine_set_shuangpin(cfg.shuangpin_mode ? 1 : 0);

    // 智能纠错开关（键盘相邻键容错，0.2.10）
    engine_set_correction(cfg.correction_enabled ? 1 : 0);

    // 中英混输开关（中文模式候选末尾英文候选，0.2.8）
    engine_set_mix_mode(cfg.mix_mode_enabled ? 1 : 0);

    // 注册线程管理器事件接收器（焦点变化通知）
    // ITfThreadMgr 通过 ITfSource::AdviseSink 注册事件接收器
    // 注意：注册失败不阻断激活——TSF 严格检查 ActivateEx 返回值
    ITfSource* pSource = nullptr;
    HRESULT hr = m_pThreadMgr->QueryInterface(IID_ITfSource,
                                              reinterpret_cast<void**>(&pSource));
    if (SUCCEEDED(hr) && pSource != nullptr) {
        hr = pSource->AdviseSink(IID_ITfThreadMgrEventSink,
                                 static_cast<ITfThreadMgrEventSink*>(this),
                                 &m_dwThreadMgrEventSinkCookie);
        pSource->Release();
        taishen::DebugLogHr("ActivateEx: AdviseSink(ThreadMgrEventSink)", hr);
        if (FAILED(hr)) {
            // 记录但不阻断（部分线程管理器场景 AdviseSink 可能受限）
            m_dwThreadMgrEventSinkCookie = 0;
        }
    }

    // 主动获取当前焦点文档管理器的顶层上下文（0.1.15 修复）：
    // ITfThreadMgrEventSink::OnSetFocus 不一定触发（首次激活时焦点文档
    // 无变化），导致 m_pFocusContext 为 null → 候选窗口光标坐标获取失败
    // → 降级为鼠标位置（弹窗不跟随光标）。
    ITfDocumentMgr* pDocMgr = nullptr;
    hr = m_pThreadMgr->GetFocus(&pDocMgr);
    if (SUCCEEDED(hr) && pDocMgr != nullptr) {
        if (m_pFocusContext != nullptr) {
            m_pFocusContext->Release();
        }
        pDocMgr->GetTop(&m_pFocusContext);
        pDocMgr->Release();
        taishen::DebugLog("ActivateEx: GetFocus got context=" +
                          std::to_string(reinterpret_cast<long long>(m_pFocusContext)));
    } else {
        taishen::DebugLogHr("ActivateEx: GetFocus", hr);
    }

    // 注册键盘事件接收器（按键捕获）
    ITfKeystrokeMgr* pKeystrokeMgr = nullptr;
    hr = m_pThreadMgr->QueryInterface(IID_ITfKeystrokeMgr,
                                      reinterpret_cast<void**>(&pKeystrokeMgr));
    if (SUCCEEDED(hr) && pKeystrokeMgr != nullptr) {
        // 先注销（幂等）：Deactivate 可能未清理上次的 sink 注册，
        // 重复注册会返回 TF_E_NOLOCK (0x80040201) → 按键不达 → 英文直出
        // 这是 0.1.15 定位到的"无法输入"根因
        pKeystrokeMgr->UnadviseKeyEventSink(tid);
        // 第三个参数 fForeground=TRUE：只在获得焦点时接收按键
        hr = pKeystrokeMgr->AdviseKeyEventSink(
            tid, static_cast<ITfKeyEventSink*>(this), TRUE);
        pKeystrokeMgr->Release();
        taishen::DebugLogHr("ActivateEx: AdviseKeyEventSink", hr);
        if (FAILED(hr)) {
            // 记录但不阻断（按键接收失败只是无法捕获按键，激活仍应成功）
        }
    }

    m_fActive = TRUE;
    taishen::DebugLog("ActivateEx done, m_fActive=TRUE");
    return S_OK;
}

STDMETHODIMP CTextService::Deactivate()
{
    taishen::DebugLog("Deactivate");
    // 注销事件接收器
    if (m_pThreadMgr != nullptr) {
        ITfSource* pSource = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfSource, reinterpret_cast<void**>(&pSource))) &&
            pSource != nullptr) {
            pSource->UnadviseSink(m_dwThreadMgrEventSinkCookie);
            pSource->Release();
        }

        // 注销按键 sink（0.1.15 修复：不注销会导致下次激活
        // AdviseKeyEventSink 返回 TF_E_NOLOCK → 按键不达 → 无法输入）
        ITfKeystrokeMgr* pKeystrokeMgr = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfKeystrokeMgr,
                reinterpret_cast<void**>(&pKeystrokeMgr))) &&
            pKeystrokeMgr != nullptr) {
            pKeystrokeMgr->UnadviseKeyEventSink(m_tid);
            pKeystrokeMgr->Release();
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
STDMETHODIMP CTextService::OnSetFocus(BOOL fForeground)
{
    taishen::DebugLog(std::string("OnSetFocus(fForeground=") +
                      (fForeground ? "TRUE" : "FALSE") + ")");
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyDown(ITfContext* /*pic*/, WPARAM wParam,
                                         LPARAM lParam, BOOL* pfEaten)
{
    // 预测试：仅判断是否吞键——绝不修改引擎状态！
    // 曾经在此调用 HandleKeyDown（有副作用），导致每个按键被处理两次：
    //   OnTestKeyDown 累积一次拼音 + OnKeyDown 再累积一次 → 拼音错乱/候选为空
    //   OnTestKeyDown 删除一次 + OnKeyDown 再删一次 → 退格"不能删除"
    // 现在改为只读判断，真正的处理只在 OnKeyDown 中进行一次。
    const bool eat = taishen::ShouldEatKey(static_cast<int>(wParam));
    taishen::DebugLog("OnTestKeyDown vk=" + std::to_string(wParam) +
                      " eat=" + (eat ? "T" : "F"));
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
    taishen::DebugLog("OnKeyDown vk=" + std::to_string(wParam) +
                      " eat=" + (eat ? "T" : "F") +
                      " ascii=" + std::to_string(engine_get_ascii_mode()) +
                      " committed=" + (result.committed.empty() ? "-" :
                          taishen::WideToUtf8(result.committed)));

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
            // 0.1.15 修复：提交后必须 RefreshState 同步清空 C++ 侧
            // m_pinyin/m_candidates（引擎 select_candidate 已 reset）。
            // 不同步会导致：退格被吞（ShouldEatKey 误判）/ 候选窗口
            // 残留旧候选（"不只是退格键的问题"——提交后所有按键行为错乱）
            RefreshState();
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

    // 拉取候选词（引擎已按 candidate_limit 配置截断）
    m_candidates.clear();
    const int count = engine_get_candidate_count();
    for (int i = 0; i < count; ++i) {
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

/// 候选窗口鼠标点击选词（0.1.13）：
/// 选择引擎候选 → 通过 TSF 组合提交上屏 → 隐藏候选窗口。
/// 与键盘选词共用同一提交链路（RunCompositionOp + Commit）。
void CTextService::OnCandidateClicked(int index)
{
    // 1. 引擎选择候选（返回提交文本，同时重置状态）
    char buf[512] = {0};
    const int len = engine_select_candidate(index, buf, sizeof(buf));
    if (len <= 0) {
        return;
    }
    const std::string text(buf, static_cast<size_t>(len - 1));

    // 2. 通过组合提交上屏（需有焦点上下文）
    if (m_pFocusContext != nullptr) {
        RunCompositionOp(m_pFocusContext,
                         CEditSessionComposition::Op::Commit, text);
    }

    // 3. 刷新状态 + 隐藏候选窗口
    RefreshState();
    m_candidateWindow.Hide();
}

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
    taishen::DebugLog("UpdateCandidateWindow: pinyin=" + m_pinyin +
                      " cands=" + std::to_string(m_candidates.size()));

    // 获取光标屏幕坐标（0.1.13 健壮性增强）：
    // 1) 优先 TSF 编辑会话获取光标
    // 2) 失败降级为当前鼠标位置（而非钉在屏幕左上角——用户会误以为窗口没显示）
    RECT caretRect = {0, 0, 0, 0};
    bool gotCaret = false;
    if (m_pFocusContext != nullptr) {
        if (SUCCEEDED(GetCaretRectFromContext(m_pFocusContext, &caretRect))) {
            gotCaret = true;
        }
    }
    if (!gotCaret) {
        POINT pt = {};
        GetCursorPos(&pt);
        caretRect.left = pt.x;
        caretRect.top = pt.y;
        caretRect.bottom = pt.y;
        caretRect.right = pt.x;
    }
    taishen::DebugLog("UpdateCandidateWindow: gotCaret=" + std::string(gotCaret ? "T" : "F") +
                      " caret=(" + std::to_string(caretRect.left) + "," +
                      std::to_string(caretRect.top) + ")");

    // 翻页指示（0.1.13）：当前页/总页数来自引擎
    const int page = engine_get_current_page();
    const int totalPages = engine_get_total_pages();

    m_candidateWindow.UpdateState(m_pinyin, m_candidates, caretRect,
                                  page, totalPages);
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

/// 写注册表键值（REG_SZ）
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

/// 写注册表 DWORD 值
static bool WriteRegDword(HKEY hkRoot, const std::wstring& subKey,
                          const std::wstring& valueName, DWORD value)
{
    HKEY hKey = nullptr;
    const LSTATUS status = RegCreateKeyExW(hkRoot, subKey.c_str(), 0, nullptr,
                                           REG_OPTION_NON_VOLATILE, KEY_WRITE,
                                           nullptr, &hKey, nullptr);
    if (status != ERROR_SUCCESS) {
        return false;
    }

    const LSTATUS setStatus = RegSetValueExW(
        hKey, valueName.c_str(), 0, REG_DWORD,
        reinterpret_cast<const BYTE*>(&value), sizeof(value));
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

/// 获取 DLL 所在目录（带尾分隔符）
static std::wstring GetDllDir()
{
    const std::wstring path = GetDllPath();
    const size_t slash = path.find_last_of(L"\\/");
    if (slash == std::wstring::npos) {
        return std::wstring();
    }
    return path.substr(0, slash + 1);
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

/// DllRegisterServer — 注册 TSF Text Service（HKCU + HKLM）
///
/// 注册内容（两处都写，保证语言设置 UI 可枚举）：
///   1. CLSID\InprocServer32 → DLL 路径（HKCR/HKLM + HKCU）
///   2. CTF\TIP\{CLSID}（TSF 识别）
///   3. LanguageProfile：0x0804 (zh-CN) + GUID_LANGPROFILE（Enable=DWORD）
///   4. Category：GUID_TFCAT_TIP_KEYBOARD（键盘输入法类别）
STDAPI DllRegisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);
    const std::wstring dllPath = GetDllPath();
    if (dllPath.empty()) {
        return E_FAIL;
    }

    // 需要写 HKLM（系统级注册）——失败不阻断 HKCU 注册
    const HKEY kRoots[] = {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER};
    const wchar_t* kClsidPrefixes[] = {
        L"SOFTWARE\\Classes\\CLSID\\",
        L"Software\\Classes\\CLSID\\",
    };

    for (int i = 0; i < 2; ++i) {
        const HKEY root = kRoots[i];
        const std::wstring clsidPrefix = kClsidPrefixes[i];

        // 1. CLSID → InprocServer32
        const std::wstring clsidKey =
            clsidPrefix + clsidStr + L"\\InprocServer32";
        WriteRegKey(root, clsidKey, L"", dllPath);
        WriteRegKey(root, clsidKey, L"ThreadingModel", L"Apartment");

        // 2. CTF\TIP 主键 + 描述
        const std::wstring tipKey = L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsidStr;
        WriteRegKey(root, tipKey, L"Description", L"泰深输入法");

        // 3. LanguageProfile（zh-CN 0x0804）— Enable 用 DWORD
        const std::wstring langKey = tipKey + L"\\LanguageProfile\\0x00000804\\" +
                                     ClsidToString(GUID_LANGPROFILE);
        WriteRegKey(root, langKey, L"Description", L"泰深拼音");
        WriteRegKey(root, langKey, L"DisplayDescription", L"泰深拼音");
        WriteRegDword(root, langKey, L"Enable", 1);
        WriteRegKey(root, langKey, L"IconFile", dllPath);
        WriteRegDword(root, langKey, L"IconIndex", 0);

        // 4. Category — 新式类别结构（对齐微软拼音）
        //    Category\Category\{类别GUID}\{CLSID}    — 类别声明
        //    Category\Item\{CLSID}\{类别GUID}        — 类别项
        const wchar_t* kCategories[] = {
            // GUID_TFCAT_TIP_KEYBOARD（键盘输入法）
            L"{34745C63-B2F0-4784-8B67-5E12C8701A31}",
            // GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT（现代 UI 输入必需）
            L"{13A016DF-560B-46CD-947A-4C3AF1E0E35D}",
            // GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT（托盘支持）
            L"{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}",
            // GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER（显示属性）
            L"{046B8C80-1647-40F7-9B21-B93B81AABC1B}",
        };

        for (const wchar_t* cat : kCategories) {
            // 新式：Category\Category\{类别GUID}\{CLSID}
            const std::wstring catNewKey =
                tipKey + L"\\Category\\Category\\" + cat + L"\\" + clsidStr;
            WriteRegKey(root, catNewKey, L"", L"");
            // 旧式兼容：Category\{类别GUID}
            const std::wstring catOldKey = tipKey + L"\\Category\\" + cat;
            WriteRegKey(root, catOldKey, L"", L"");
        }

        // Category\Item\{CLSID} 类别项（含描述）
        const std::wstring catItemKey =
            tipKey + L"\\Category\\Item\\" + clsidStr;
        WriteRegKey(root, catItemKey, L"Description", L"泰深输入法");
        for (const wchar_t* cat : kCategories) {
            const std::wstring catItemSub =
                catItemKey + L"\\" + cat;
            WriteRegKey(root, catItemSub, L"", L"");
        }
    }

    return S_OK;
}

/// DllUnregisterServer — 注销 TSF Text Service（HKCU + HKLM）
STDAPI DllUnregisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);

    // 删除 HKLM 注册
    DeleteRegKey(HKEY_LOCAL_MACHINE,
                 L"SOFTWARE\\Classes\\CLSID\\" + clsidStr);
    DeleteRegKey(HKEY_LOCAL_MACHINE,
                 L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsidStr);

    // 删除 HKCU 注册
    DeleteRegKey(HKEY_CURRENT_USER,
                 L"Software\\Classes\\CLSID\\" + clsidStr);
    DeleteRegKey(HKEY_CURRENT_USER, GetTipRegKey(clsidStr));

    return S_OK;
}




