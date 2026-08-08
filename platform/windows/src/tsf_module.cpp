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
#include <algorithm>
#include <cwctype>
#include <string>
#include <vector>

#include "engine_bridge.h"
#include "tsf_keyevent.h"
#include "candidate_window.h"
#include "banner_window.h"
#include "tsf_composition.h"
#include "config_reader.h"
#include "app_state.h"
#include "theme.h"
#include "debug_log.h"
#include "dpi_util.h"

// DLL 自身模块句柄（定义于 dllmain.cpp）
extern HMODULE g_hModule;

// P1-2 数字分隔符状态（定义于 tsf_keyevent.cpp，tsf_keyevent.h 已声明 extern）：
// 最近一次 IME 提交的文本是否以数字结尾——若是，紧随的 , . 直通半角。

// 辅助函数（定义于文件末尾）
static std::wstring GetDllPath();
static std::wstring GetDllDir();

// P2-6/V0.2.33 应用级配置：前台进程名获取与 per-app 状态应用已移至 app_state 模块
// （GetForegroundProcessName / AppStateApply / AppStateSetAscii）

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
                                taishen::CTsfComposition* comp,
                                int caretOffset = -1)
            : m_cRef(1), m_pic(pic), m_op(op), m_text(text), m_pComp(comp),
              m_caretOffset(caretOffset) {}

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
            case Op::Commit: {
                HRESULT hr = m_pComp->CommitComposition(ec, m_pic, m_text);
                // V0.4.x 配对符号成对上屏：提交后将光标移到成对文本中间
                // （开符号之后）。用编辑会话内 SetSelection 定位。
                if (SUCCEEDED(hr) && m_caretOffset >= 0) {
                    MoveCaretTo(ec, m_caretOffset);
                }
                return hr;
            }
            }
            return E_FAIL;
        }

        /// 将插入点移到当前文档位置 + offset 字符处（offset 相对提交文本起点）。
        /// 提交后光标位于文本末尾，向回移动 (textLen - offset) 个字符即可。
        void MoveCaretTo(TfEditCookie ec, int offset)
        {
            if (m_pic == nullptr || offset < 0) { return; }
            const int textLen = static_cast<int>(m_text.size());
            if (textLen <= offset) { return; } // 无需移动（已在末尾或越界）

            TF_SELECTION sel = {};
            ULONG fetched = 0;
            HRESULT hr = m_pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1,
                                             &sel, &fetched);
            if (FAILED(hr) || fetched == 0 || sel.range == nullptr) {
                return;
            }
            // 从末尾向回移动（textLen - offset）个字符到开符号之后
            const LONG back = static_cast<LONG>(textLen - offset);
            LONG shifted = 0;
            hr = sel.range->ShiftStart(ec, -back, &shifted, nullptr);
            if (SUCCEEDED(hr) && shifted != -back) {
                // 字符数移动不完整（如跨 CRLF/组合字符）——仍尽力而为
            }
            shifted = 0;
            hr = sel.range->ShiftEnd(ec, -back, &shifted, nullptr);
            if (FAILED(hr)) {
                sel.range->Release();
                return;
            }
            hr = m_pic->SetSelection(ec, 1, &sel);
            sel.range->Release();
        }

    private:
        LONG m_cRef;
        ITfContext* m_pic;
        Op m_op;
        std::string m_text;
        taishen::CTsfComposition* m_pComp;
        int m_caretOffset;
    };

    // 编辑会话调度辅助：在同步编辑会话中执行组合操作
    void RunCompositionOp(ITfContext* pic, CEditSessionComposition::Op op,
                          const std::string& text, int caretOffset = -1);

    // 内部编辑会话：获取光标屏幕坐标
    class CEditSessionGetCaret : public ITfEditSession
    {
    public:
        CEditSessionGetCaret(ITfContext* pic, RECT* pRect, HWND* pHostWnd)
            : m_cRef(1), m_pic(pic), m_pRect(pRect), m_pHostWnd(pHostWnd) {}

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
                        // V0.4.3-P1B：取宿主窗口（GetCaretRectFromContext
                        // 用它检测 DPI 感知模式，换算坐标单位）
                        if (m_pHostWnd != nullptr) {
                            pView->GetWnd(m_pHostWnd);
                        }
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
        HWND* m_pHostWnd;
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

    // ── 托盘图标（0.2.13）──
    // 隐藏消息窗口（托盘图标宿主）
    HWND m_trayHwnd;
    // 托盘图标是否已添加
    bool m_trayAdded;
    // 托盘图标当前文本（"中"/"英"）
    std::wstring m_trayLabel;

    // ── 配置热加载（对标 rime-ice 重新部署）──
    std::wstring m_configPath;      // config.ini 绝对路径
    FILETIME m_configMtime = {};    // 上次读取的 mtime（变化检测）
    bool m_configWatchStarted = false; // 定时器是否已启动
    // V0.2.33 per-app 状态记忆：最近一次 ApplyConfig 的配置快照
    // （OnSetFocus 时用于 AppStateApply 的 app_ascii/app_cn/app_inline 判断）
    taishen::ImeConfig m_appCfg;

    // 托盘管理
    bool InitTrayIcon();       // 激活时添加
    void ReAddTrayIcon();      // explorer 重启后重新添加（0.1.25）
    void UpdateTrayIcon();     // 模式切换时更新图标+tooltip
    void RemoveTrayIcon();     // 停用时移除
    void ShowTrayMenu();       // 右键菜单
    void ToggleAsciiMode();    // 切换中英（托盘左键/菜单）
    void ToggleTraditional();  // 切换简繁（托盘菜单）
    void ToggleShuangpin();    // 切换双拼/全拼（托盘菜单，0.1.26）
    std::wstring BuildBannerText();  // 状态横幅文字（0.1.26）
    static LRESULT CALLBACK TrayWndProc(HWND, UINT, WPARAM, LPARAM);

    // 配置热加载（对标 rime-ice 重新部署）
    void ApplyConfig(const taishen::ImeConfig& cfg, const std::wstring& dllDir);
    void StartConfigWatch();   // 启动 2s 轮询（ActivateEx 调用）
    void StopConfigWatch();    // 停止轮询（Deactivate 调用）
    void ReloadConfigIfChanged();  // 定时器回调：mtime 变化 → 重载

    // 多行展开状态（0.2.14）
    bool m_multiRowExpanded;

    // ── Shift tap 中英切换（0.2.26，主流输入法标准）──
    // Shift 快速按下-松开（期间无其他键、<300ms）→ 切换中英；
    // Shift+字母/符号组合不误切（组合键由 OnKeyDown 取消 armed）。
    bool m_shiftTapArmed;
    DWORD m_shiftDownTick;

    // ── 标点复选 + 配对引号（0.2.28）──
    // 复选标点候选（如 《〈«‹），非空 = 复选状态（数字/空格选择，Esc 取消）
    std::vector<std::wstring> m_punctCandidates;
    // 配对引号开闭状态（' 单引号 / " 双引号）
    bool m_quoteOpen;
    bool m_dquoteOpen;
};

// ---------------------------------------------------------------------------
// CTextService 实现
// ---------------------------------------------------------------------------
CTextService::CTextService()
    : m_cRef(1), m_pThreadMgr(nullptr), m_tid(0),
      m_dwThreadMgrEventSinkCookie(0),
      m_pFocusContext(nullptr), m_fActive(FALSE),
      m_trayHwnd(nullptr), m_trayAdded(false), m_trayLabel(L"中"),
      m_multiRowExpanded(false), m_shiftTapArmed(false), m_shiftDownTick(0),
      m_quoteOpen(false), m_dquoteOpen(false)
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

    // 应用配置到引擎/候选窗（抽取自 ActivateEx，供热加载复用）
    ApplyConfig(cfg, dllDir);

    // 拆字反查词库（V0.2.25）：DLL 同目录 radical_pinyin.dict.yaml
    {
        const std::wstring radicalPath = dllDir + L"radical_pinyin.dict.yaml";
        std::string radicalPathUtf8;
        const int len = WideCharToMultiByte(CP_UTF8, 0, radicalPath.c_str(),
                                            static_cast<int>(radicalPath.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            radicalPathUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, radicalPath.c_str(),
                                static_cast<int>(radicalPath.size()),
                                &radicalPathUtf8[0], len, nullptr, nullptr);
        }
        engine_set_radical_path(radicalPathUtf8.empty() ? nullptr : radicalPathUtf8.c_str());
    }

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

    // 托盘图标（0.2.13）：显示中英状态，右键菜单切换
    InitTrayIcon();
    // 状态工具栏（0.1.26）：注册本线程到激活集合，前台判断驱动显示
    // （切到非泰深窗口自动隐藏；按钮状态实时从引擎读取）
    taishen::CBannerWindow::Instance().RegisterThread(GetCurrentThreadId());
    // 工具栏主题跟随系统（V0.2.20）：初始化深浅色
    {
        const int sysTheme = taishen::GetSystemAppTheme();
        if (sysTheme >= 0) {
            taishen::CBannerWindow::Instance().SetLightTheme(sysTheme == 1);
        }
    }
    // 配置热加载（对标 rime-ice 重新部署）：监听 config.ini 变更
    StartConfigWatch();
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
    // 配置热加载：停止轮询（托盘窗口将销毁）
    StopConfigWatch();
    // 状态横幅：本线程停用 → 前台判断自动隐藏（若其他线程仍激活则保持）
    taishen::CBannerWindow::Instance().UnregisterThread(GetCurrentThreadId());
    RemoveTrayIcon();
    return S_OK;
}

// ---------------------------------------------------------------------------
// 托盘图标（0.2.13）
// ---------------------------------------------------------------------------

/// 托盘自定义消息（窗口过程区分）
#define WM_TRAYICON (WM_APP + 1)
/// 托盘菜单命令 ID
#define ID_TRAY_TOGGLE_ASCII 1
#define ID_TRAY_TOGGLE_TRAD 2
#define ID_TRAY_TOGGLE_SHUANGPIN 4
#define ID_TRAY_TOGGLE_TOOLBAR 5
#define ID_TRAY_EXIT 3

/// 托盘图标固定 GUID（0.3.x 修复"托盘双图标"）：
/// TSF DLL 进程内注入，每个激活泰深输入法的进程/线程都会执行
/// InitTrayIcon → NIM_ADD。无 GUID 时各进程图标各自独立 → 托盘重复。
/// 加 NIF_GUID 后 Shell 按 GUID 自动合并去重（微软拼音同款做法）。
/// {B3F6A2C9-5E4D-4A8B-9C1D-7E2F3A4B5C6D}
static const GUID kTrayIconGuid = {
    0xB3F6A2C9, 0x5E4D, 0x4A8B,
    {0x9C, 0x1D, 0x7E, 0x2F, 0x3A, 0x4B, 0x5C, 0x6D}};

// 前向声明（GetTaishenIcon 定义在文件后部，InitTrayIcon 先使用）
static HICON GetTaishenIcon(bool asciiMode);

bool CTextService::InitTrayIcon()
{
    // 已添加则跳过（避免重复）
    if (m_trayAdded) {
        return true;
    }
    // 隐藏消息窗口（托盘图标宿主，接收 WM_TRAYICON）
    if (m_trayHwnd == nullptr) {
        WNDCLASSEXW wc = {};
        wc.cbSize = sizeof(wc);
        wc.lpfnWndProc = CTextService::TrayWndProc;
        wc.hInstance = GetModuleHandleW(nullptr);
        wc.lpszClassName = L"TaishenIMETrayWnd";
        RegisterClassExW(&wc);
        m_trayHwnd = CreateWindowExW(0, L"TaishenIMETrayWnd", L"",
                                     WS_OVERLAPPED, 0, 0, 0, 0,
                                     nullptr, nullptr, wc.hInstance, this);
        if (m_trayHwnd == nullptr) {
            taishen::DebugLog("InitTrayIcon: CreateWindowExW failed");
            return false;
        }
    }

    // 托盘图标（0.2.13 务实版：系统图标 + tooltip 显示状态；
    // D2D 文字图标列为后续增强）
    NOTIFYICONDATAW nid = {};
    nid.cbSize = sizeof(nid);
    nid.hWnd = m_trayHwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_GUID;
    nid.guidItem = kTrayIconGuid;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = GetTaishenIcon(engine_get_ascii_mode() == 1);
    // tooltip：当前中英状态
    const std::wstring tip = L"泰深输入法 - " + m_trayLabel + L"文模式";
    wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
    if (!Shell_NotifyIconW(NIM_ADD, &nid)) {
        taishen::DebugLog("InitTrayIcon: Shell_NotifyIcon NIM_ADD failed");
        return false;
    }
    m_trayAdded = true;
    taishen::DebugLog("InitTrayIcon: added");
    return true;
}

void CTextService::ReAddTrayIcon()
{
    // explorer 重启（TaskbarCreated）后旧图标引用失效，需重新 NIM_ADD
    if (!m_trayAdded || m_trayHwnd == nullptr) {
        return;
    }
    NOTIFYICONDATAW nid = {};
    nid.cbSize = sizeof(nid);
    nid.hWnd = m_trayHwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_GUID;
    nid.guidItem = kTrayIconGuid;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = GetTaishenIcon(engine_get_ascii_mode() == 1);
    const std::wstring tip = L"泰深输入法 - " + m_trayLabel + L"文模式";
    wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
    if (Shell_NotifyIconW(NIM_ADD, &nid)) {
        taishen::DebugLog("ReAddTrayIcon: re-added after TaskbarCreated");
    } else {
        taishen::DebugLog("ReAddTrayIcon: NIM_ADD failed");
    }
}

void CTextService::UpdateTrayIcon()
{
    if (!m_trayAdded || m_trayHwnd == nullptr) {
        return;
    }
    // 根据当前模式更新 label + tooltip
    m_trayLabel = (engine_get_ascii_mode() == 1) ? L"英" : L"中";
    NOTIFYICONDATAW nid = {};
    nid.cbSize = sizeof(nid);
    nid.hWnd = m_trayHwnd;
    nid.uID = 1;
    nid.uFlags = NIF_TIP | NIF_ICON | NIF_GUID;
    nid.guidItem = kTrayIconGuid;
    nid.hIcon = GetTaishenIcon(engine_get_ascii_mode() == 1);
    const std::wstring tip = L"泰深输入法 - " + m_trayLabel + L"文模式";
    wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}

void CTextService::RemoveTrayIcon()
{
    if (m_trayAdded && m_trayHwnd != nullptr) {
        NOTIFYICONDATAW nid = {};
        nid.cbSize = sizeof(nid);
        nid.hWnd = m_trayHwnd;
        nid.uID = 1;
        // NIF_GUID 模式删除必须携带相同 GUID（与 NIM_ADD 一致）
        nid.uFlags = NIF_GUID;
        nid.guidItem = kTrayIconGuid;
        Shell_NotifyIconW(NIM_DELETE, &nid);
        m_trayAdded = false;
    }
    if (m_trayHwnd != nullptr) {
        DestroyWindow(m_trayHwnd);
        m_trayHwnd = nullptr;
    }
}

void CTextService::ToggleAsciiMode()
{
    // 与 Ctrl+Space 等效：切换中英（V0.2.33 走 per-app 记忆，更新当前进程状态）
    const int cur = engine_get_ascii_mode();
    taishen::AppStateSetAscii(cur ? false : true);
    taishen::DebugLog("Tray: ToggleAsciiMode -> " +
                      std::to_string(engine_get_ascii_mode()));
    UpdateTrayIcon();
    m_candidateWindow.Hide();
}

void CTextService::ToggleTraditional()
{
    const int cur = engine_get_traditional();
    engine_set_traditional(cur ? 0 : 1);
    taishen::DebugLog("Tray: ToggleTraditional -> " +
                      std::to_string(engine_get_traditional()));
}

void CTextService::ToggleShuangpin()
{
    // 与 config.ini shuangpin=1 等效：切换双拼/全拼（0.1.26）
    // engine_set_shuangpin 内部切换时清空未完成拼音（引擎 reset）
    const int cur = engine_get_shuangpin();
    engine_set_shuangpin(cur ? 0 : 1);
    taishen::DebugLog("Tray: ToggleShuangpin -> " +
                      std::to_string(engine_get_shuangpin()));
}

std::wstring CTextService::BuildBannerText()
{
    std::wstring text;
    text += (engine_get_ascii_mode() == 1) ? L"英文模式" : L"中文模式";
    if (engine_get_traditional() == 1) {
        text += L" · 简繁";
    }
    if (engine_get_shuangpin() == 1) {
        text += L" · 双拼";
    }
    return text;
}

void CTextService::ShowTrayMenu()
{
    if (m_trayHwnd == nullptr) {
        return;
    }
    HMENU menu = CreatePopupMenu();
    // 菜单项文案带当前状态
    const bool ascii = (engine_get_ascii_mode() == 1);
    const bool trad = (engine_get_traditional() == 1);
    const bool shuangpin = (engine_get_shuangpin() == 1);
    const bool toolbarOn = taishen::CBannerWindow::Instance().IsEnabled();
    const std::wstring asciiItem = ascii ? L"切换到中文模式" : L"切换到英文模式";
    const std::wstring tradItem = trad ? L"关闭简繁转换" : L"开启简繁转换";
    const std::wstring spItem = shuangpin ? L"切换到全拼输入" : L"切换到双拼输入";
    const std::wstring tbItem = toolbarOn ? L"隐藏工具栏" : L"显示工具栏";
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_ASCII, asciiItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_TRAD, tradItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_SHUANGPIN, spItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_TOOLBAR, tbItem.c_str());
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, ID_TRAY_EXIT, L"退出输入法");

    POINT pt;
    GetCursorPos(&pt);
    SetForegroundWindow(m_trayHwnd);
    const int cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY,
                                   pt.x, pt.y, 0, m_trayHwnd, nullptr);
    DestroyMenu(menu);
    if (cmd == ID_TRAY_TOGGLE_ASCII) {
        ToggleAsciiMode();
    } else if (cmd == ID_TRAY_TOGGLE_TRAD) {
        ToggleTraditional();
    } else if (cmd == ID_TRAY_TOGGLE_SHUANGPIN) {
        ToggleShuangpin();
    } else if (cmd == ID_TRAY_TOGGLE_TOOLBAR) {
        // 显示/隐藏工具栏（0.1.26，右键输入法图标菜单）
        taishen::CBannerWindow::Instance().SetEnabled(!toolbarOn);
    } else if (cmd == ID_TRAY_EXIT) {
        taishen::DebugLog("Tray: Exit requested");
        // 退出 = 通知 TSF 停用（Deactivate 会移除托盘）
        if (m_pThreadMgr != nullptr) {
            // 简化：仅记录（TSF 停用由系统管理，此处不做强制注销）
        }
    }
}

/// 泰深托盘图标：深色圆角底 + 白色文字，进程内双缓存（中/英各一）
///   中文模式：大"泰"字（24pt 粗体）
///   英文模式：大"t" + 小"ENG"
/// 运行时 GDI 绘制 32x32，无外部资源依赖
static HICON GetTaishenIcon(bool asciiMode)
{
    static HICON s_icons[2] = {nullptr, nullptr};
    const int idx = asciiMode ? 1 : 0;
    if (s_icons[idx] != nullptr) {
        return s_icons[idx];
    }

    const int size = 32;
    HDC hdc = GetDC(nullptr);
    HDC memDC = CreateCompatibleDC(hdc);
    HBITMAP colorBM = CreateCompatibleBitmap(hdc, size, size);
    HBITMAP maskBM = CreateBitmap(size, size, 1, 1, nullptr);
    HGDIOBJ oldColor = SelectObject(memDC, colorBM);

    // 背景：深色（与候选窗口 0x2E2E2E 同系）
    RECT rc = {0, 0, size, size};
    HBRUSH bg = CreateSolidBrush(RGB(46, 46, 46));
    FillRect(memDC, &rc, bg);
    DeleteObject(bg);

    SetBkMode(memDC, TRANSPARENT);

    if (asciiMode) {
        // 英文模式：大"t"（偏上）+ 小"ENG"（底部）
        SetTextColor(memDC, RGB(232, 232, 232));
        RECT rcBig = {0, 0, size, size - 7};
        HFONT fontBig = CreateFontW(22, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                                    DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                    CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                    DEFAULT_PITCH, L"Segoe UI");
        HGDIOBJ oldFontBig = SelectObject(memDC, fontBig);
        DrawTextW(memDC, L"t", -1, &rcBig, DT_CENTER | DT_BOTTOM | DT_SINGLELINE);
        SelectObject(memDC, oldFontBig);
        DeleteObject(fontBig);

        RECT rcSmall = {0, size - 13, size, size};
        HFONT fontSmall = CreateFontW(11, 0, 0, 0, FW_SEMIBOLD, FALSE, FALSE, FALSE,
                                      DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                      CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                      DEFAULT_PITCH, L"Segoe UI");
        HGDIOBJ oldFontSmall = SelectObject(memDC, fontSmall);
        SetTextColor(memDC, RGB(160, 160, 160));
        DrawTextW(memDC, L"ENG", -1, &rcSmall, DT_CENTER | DT_TOP | DT_SINGLELINE);
        SelectObject(memDC, oldFontSmall);
        DeleteObject(fontSmall);
    } else {
        // 中文模式：大"泰"字（24pt 比原来 20pt 更大更清晰）
        SetTextColor(memDC, RGB(232, 232, 232));
        HFONT font = CreateFontW(24, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                                 DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                                 CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                                 DEFAULT_PITCH, L"Microsoft YaHei");
        HGDIOBJ oldFont = SelectObject(memDC, font);
        DrawTextW(memDC, L"泰", -1, &rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(memDC, oldFont);
        DeleteObject(font);
    }

    // 掩码：全 0（不透明）
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

    s_icons[idx] = icon;
    return icon;
}

/// explorer 重启广播消息（注册一次，进程内复用）
static UINT GetTaskbarCreatedMsg()
{
    static const UINT msg = RegisterWindowMessageW(L"TaskbarCreated");
    return msg;
}

LRESULT CALLBACK CTextService::TrayWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
    CTextService* self = nullptr;
    if (msg == WM_NCCREATE) {
        const auto* cs = reinterpret_cast<CREATESTRUCT*>(lParam);
        self = static_cast<CTextService*>(cs->lpCreateParams);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA,
                          reinterpret_cast<LONG_PTR>(self));
    } else {
        self = reinterpret_cast<CTextService*>(
            GetWindowLongPtrW(hwnd, GWLP_USERDATA));
    }
    // explorer 重启（TaskbarCreated）→ 重新添加托盘图标
    // 否则图标引用失效永久丢失（0.1.25 修复）
    if (msg == GetTaskbarCreatedMsg()) {
        if (self != nullptr) {
            self->ReAddTrayIcon();
        }
        return 0;
    }
    if (msg == WM_TRAYICON) {
        if (self != nullptr) {
            if (lParam == WM_LBUTTONUP) {
                self->ToggleAsciiMode();  // 左键：切换中英
            } else if (lParam == WM_RBUTTONUP) {
                self->ShowTrayMenu();     // 右键：菜单
            }
        }
        return 0;
    }
    // 配置热加载轮询（对标 rime-ice 重新部署，2s 检查 config.ini mtime）
    if (msg == WM_TIMER && wParam == 1) {
        if (self != nullptr) {
            self->ReloadConfigIfChanged();
        }
        return 0;
    }
    return DefWindowProcW(hwnd, msg, wParam, lParam);
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
    // P2-4：小键盘键先归一为主键盘等价键
    // V0.2.36：vim_mode 进程的 vim 键（Esc/Ctrl+C/Ctrl+[）强制透传
    const int effVk = taishen::NormalizeKeypad(static_cast<int>(wParam));
    const bool vimPass = taishen::AppStateIsVimForeground(m_appCfg);
    const bool eat = taishen::ShouldEatKey(effVk, vimPass);
    taishen::DebugLog("OnTestKeyDown vk=" + std::to_string(wParam) +
                      " eat=" + (eat ? "T" : "F"));
    if (pfEaten != nullptr) {
        *pfEaten = eat ? TRUE : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyUp(ITfContext* /*pic*/, WPARAM wParam,
                                       LPARAM /*lParam*/, BOOL* pfEaten)
{
    // Shift 放行（0.2.26 fix）：OnTestKeyUp 返回 eaten=TRUE 才能确保
    // OnKeyUp 到达——Shift tap 切换依赖松开事件（<300ms 判定）。
    // V0.2.36：vim_mode 进程的 vim 键同样放行，OnKeyUp 到达后切英文。
    const bool vimPass = taishen::AppStateIsVimForeground(m_appCfg);
    if (pfEaten != nullptr) {
        *pfEaten = (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) ||
                   (vimPass && taishen::IsVimModeKey(static_cast<int>(wParam)))
                       ? TRUE
                       : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnKeyDown(ITfContext* pic, WPARAM wParam,
                                     LPARAM lParam, BOOL* pfEaten)
{
    // 0.2.26 Shift tap 中英切换：记录 Shift 按下；其他键按下取消 tap 候选
    // （Shift+字母/符号是组合键，不得触发切换）
    // fix：TSF 传递的 Shift 虚拟键是 VK_SHIFT(16) 而非 VK_LSHIFT/RSHIFT(0xA0/0xA1)，
    // 之前只匹配后两者 → tap 永不 armed → 切换失效。三值都要匹配。
    if (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) {
        m_shiftTapArmed = true;
        m_shiftDownTick = GetTickCount();
    } else {
        m_shiftTapArmed = false;
    }

    // P0-2 运行时开关快捷键（对标 rime Control+Shift+3/4）：
    //   Ctrl+Shift+3 切换中英标点，Ctrl+Shift+4 切换简繁
    // （ShouldEatKey 已放行，此处吃键处理，不透传给应用）
    {
        const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
        const bool shiftDown = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
        if (ctrlDown && shiftDown) {
            bool handled = false;
            switch (wParam) {
            case '3': {
                const int cur = engine_get_ascii_punct();
                engine_set_ascii_punct(cur ? 0 : 1);
                handled = true;
                break;
            }
            case '4': {
                const int cur = engine_get_traditional();
                engine_set_traditional(cur ? 0 : 1);
                handled = true;
                break;
            }
            default: break;
            }
            if (handled) {
                taishen::DebugLog("P0-2 toggle: vk=" + std::to_string(wParam) +
                                  " ascii_punct=" +
                                  std::to_string(engine_get_ascii_punct()) +
                                  " traditional=" +
                                  std::to_string(engine_get_traditional()));
                RefreshState();
                m_candidateWindow.Hide();
                UpdateTrayIcon();
                taishen::CBannerWindow::Instance().Refresh();
                if (pfEaten != nullptr) { *pfEaten = TRUE; }
                return S_OK;
            }
        }
    }

    // 0.3.x：工具栏兜底评估——SetWinEventHook 前台回调可能因
    // 注册线程无消息泵/hook 失效而不触发，用户打字时主动重评估，
    // 工具栏"莫名其妙消失"后恢复显示（成本：一次 GetForegroundWindow）
    taishen::CBannerWindow::Instance().EvaluateForeground();

    // 0.2.28 标点复选：数字/空格选择复选标点，Esc 取消，其他键清空继续
    if (!m_punctCandidates.empty()) {
        if (wParam >= '1' && wParam <= '9') {
            const size_t idx = static_cast<size_t>(wParam - '1');
            if (idx < m_punctCandidates.size()) {
                const std::wstring sel = m_punctCandidates[idx];
                m_punctCandidates.clear();
                m_candidateWindow.Hide();
                // V0.4.x：开符号成对输出+光标居中（《 → 《》光标居中）
                std::wstring committed;
                int caretOffset = -1;
                taishen::ExpandPairPunct(sel, committed, caretOffset);
                RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                                 taishen::WideToUtf8(committed), caretOffset);
                if (pfEaten != nullptr) { *pfEaten = TRUE; }
                return S_OK;
            }
        } else if (wParam == VK_SPACE) {
            const std::wstring sel = m_punctCandidates[0];
            m_punctCandidates.clear();
            m_candidateWindow.Hide();
            std::wstring committed;
            int caretOffset = -1;
            taishen::ExpandPairPunct(sel, committed, caretOffset);
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(committed), caretOffset);
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        } else if (wParam == VK_ESCAPE) {
            m_punctCandidates.clear();
            m_candidateWindow.Hide();
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
        // 其他键：清空复选状态（候选窗隐藏），继续正常按键处理
        m_punctCandidates.clear();
        m_candidateWindow.Hide();
    }

    // V0.2.33 兜底：OnSetFocus 可能不触发（B-4 教训），按键前检测前台进程变化，
    // 应用目标进程的初始/记忆状态。AppStateApply 内部幂等（进程未变化直接返回）。
    taishen::AppStateApply(m_appCfg);
    // V0.2.36 vim_mode：vim 键（Esc/Ctrl+C/Ctrl+[）由 OnTestKeyDown 透传、
    // OnKeyUp 切英文（透传键 OnKeyDown 不执行，见 OnTestKeyDown/OnKeyUp）

    taishen::KeyEventResult result;
    // P2-4：小键盘键先归一为主键盘等价键（候选选择/计算器/数字模式自动支持）
    const int effVk = taishen::NormalizeKeypad(static_cast<int>(wParam));
    const bool eat = taishen::HandleKeyDown(effVk, lParam, result);
    taishen::DebugLog("OnKeyDown vk=" + std::to_string(wParam) +
                      " eff=" + std::to_string(effVk) +
                      " eat=" + (eat ? "T" : "F") +
                      " ascii=" + std::to_string(engine_get_ascii_mode()) +
                      " committed=" + (result.committed.empty() ? "-" :
                          taishen::WideToUtf8(result.committed)));

    if (eat) {
        // 0.2.28 配对引号：' 单引号（‘’）/ " 双引号（“”），开闭交替上屏
        // 0.2.28 配对引号：' 单引号 / " 双引号。
        // V0.4.x：改为成对上屏（‘’ / “” + 光标居中），对齐主流输入法；
        // 开关关闭时退化为交替开闭（原行为）。
        if (result.punct_quote == 1) {
            std::wstring committed;
            int caretOffset = -1;
            if (taishen::IsPairPunctEnabled()) {
                taishen::ExpandPairPunct(L"‘", committed, caretOffset);
            } else {
                committed = m_quoteOpen ? L"’" : L"‘";
                m_quoteOpen = !m_quoteOpen;
            }
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(committed), caretOffset);
            RefreshState();
            m_candidateWindow.Hide();
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
        if (result.punct_quote == 2) {
            std::wstring committed;
            int caretOffset = -1;
            if (taishen::IsPairPunctEnabled()) {
                taishen::ExpandPairPunct(L"“", committed, caretOffset);
            } else {
                committed = m_dquoteOpen ? L"”" : L"“";
                m_dquoteOpen = !m_dquoteOpen;
            }
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(committed), caretOffset);
            RefreshState();
            m_candidateWindow.Hide();
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
        // 0.2.28 标点复选：记录候选列表（随后 UpdateCandidateWindow 渲染）
        if (!result.punct_candidates.empty()) {
            m_punctCandidates = result.punct_candidates;
        }
        // 多行展开/收起请求（0.2.14 + V0.3.x 三态修复）：
        // ↓ 展开 / ↑ 收起 / 拼音变化自动复位单行（字母/退格置 multirow_collapse）
        if (result.multirow_requested) {
            m_candidateWindow.SetMultiRow(true);
            m_multiRowExpanded = true;
        } else if (result.multirow_collapse) {
            m_candidateWindow.SetMultiRow(false);
            m_multiRowExpanded = false;
        }
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
            // P1-2 数字分隔符状态：记录本次提交是否以数字结尾（供 , . 直通半角）
            taishen::g_lastCommitEndsWithDigit = false;
            if (!result.committed.empty()) {
                const wchar_t last = result.committed.back();
                if (last >= L'0' && last <= L'9') {
                    taishen::g_lastCommitEndsWithDigit = true;
                }
            }
            // 选词上屏：组合替换为汉字并结束
            // V0.4.x：配对符号成对时携带光标偏移（开符号后居中）
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(result.committed),
                             result.caret_offset);
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
    } else if (result.state_changed) {
        // V0.4.3 回车收尾修复：Enter 在 HandleKeyDown 已 engine_reset
        // （丢弃未提交拼音）但 eat=false 透传，OnKeyDown else 分支此前
        // 不消费 state_changed → 候选窗口残留不关闭（0.1.26 半截修复）。
        // 此处补收尾：刷新状态 + 隐藏候选窗。
        // 组合不显式结束——由应用收到 Enter 时自然提交（weo 等非标
        // 拼音即借此上屏英文），与微软拼音行为一致。
        RefreshState();
        m_candidateWindow.Hide();
    }

    if (pfEaten != nullptr) {
        *pfEaten = eat ? TRUE : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnKeyUp(ITfContext* /*pic*/, WPARAM wParam,
                                   LPARAM /*lParam*/, BOOL* pfEaten)
{
    // 0.2.26 Shift tap 中英切换（主流输入法标准）：
    // Shift 快速按下-松开（<300ms、期间无其他键、无 Ctrl/Alt 组合）→ 切换。
    // 保留 Ctrl+Space 备选（HandleKeyDown）。
    // fix：匹配 VK_SHIFT(16)（TSF 实际传递的虚拟键）。
    if (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) {
        if (m_shiftTapArmed) {
            const DWORD dur = GetTickCount() - m_shiftDownTick;
            const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
            const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
            if (dur < 300 && !ctrlDown && !altDown) {
                // V0.2.33：走 per-app 记忆，更新当前进程状态
                const int cur = engine_get_ascii_mode();
                taishen::AppStateSetAscii(cur ? false : true);
                taishen::DebugLog("Shift tap: ascii_mode -> " +
                                  std::to_string(engine_get_ascii_mode()));
                // 刷新候选窗 + 托盘图标 + 工具栏按钮高亮
                // V0.3.x：切换保留未提交拼音（引擎 set_ascii_mode 不再 reset），
                // 候选窗口继续显示——用户可按空格将已打的词上屏（问题 9 修复）。
                // 拼音为空时 UpdateCandidateWindow 内部会 Hide。
                RefreshState();
                UpdateCandidateWindow();
                UpdateTrayIcon();
                taishen::CBannerWindow::Instance().Refresh();
            }
        }
        m_shiftTapArmed = false;
    }

    // V0.2.36 vim_mode：vim 键（Esc/Ctrl+C/Ctrl+[）keyup 到达后切换为英文。
    // 这些键在 OnTestKeyDown 已透传给应用（vim 收到 keydown），切英文副作用在此完成
    // （透传键的 OnKeyDown 不执行）。
    if (taishen::AppStateIsVimForeground(m_appCfg) &&
        taishen::IsVimModeKey(static_cast<int>(wParam))) {
        if (engine_get_ascii_mode() == 0) {
            taishen::AppStateSetAscii(true);
            taishen::DebugLog("vim_mode: Esc/<C-c>/<C-[> -> ascii");
            // 刷新候选窗（隐藏）+ 托盘图标 + 工具栏按钮高亮
            RefreshState();
            m_candidateWindow.Hide();
            UpdateTrayIcon();
            taishen::CBannerWindow::Instance().Refresh();
        }
    }

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
    // V0.2.33 per-app 状态记忆：焦点切换 → 应用目标进程的初始/记忆状态
    // （OnSetFocus 可能不触发，OnKeyDown 有兜底，见 OnKeyDown 前台变化检测）
    const taishen::AppStateResult r = taishen::AppStateApply(m_appCfg);
    m_candidateWindow.SetInlinePreedit(
        r.inline_hit ? true : m_appCfg.inline_preedit);
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

// ---------------------------------------------------------------------------
// 配置热加载（对标 rime-ice 重新部署）
// ---------------------------------------------------------------------------

/// 应用配置到引擎/候选窗/工具栏（ActivateEx 与热加载共用）
/// 注意：词库初始化（engine_init/radical）不在本函数内——热加载不重载词库
void CTextService::ApplyConfig(const taishen::ImeConfig& cfg,
                               const std::wstring& /*dllDir*/)
{
    // V0.2.33：保存配置快照供 OnSetFocus 的 AppStateApply 使用
    m_appCfg = cfg;
    // 候选数上限
    engine_set_candidate_count(cfg.candidate_count);
    // 候选排序模式（P0-2）：0=默认 1=单字优先 2=长词优先
    engine_set_sort_mode(cfg.sort_mode);
    // 上下文联想（P1-1）：前文搭配词前置
    engine_set_context_assoc(cfg.context_assoc ? 1 : 0);
    // 模糊音开关（RIME 拼写变体，0.1.14）
    engine_set_fuzzy(cfg.fuzzy_enabled ? 1 : 0);
    // 双拼模式（RIME 微软双拼方案，0.1.14）
    engine_set_shuangpin(cfg.shuangpin_mode ? 1 : 0);
    // 双拼方案（P2-7）：mspy/flypy/sogou/zrm/ziguang/jiajia
    engine_set_shuangpin_scheme(cfg.shuangpin_scheme.c_str());
    // 智能纠错开关（键盘相邻键容错，0.2.10）
    engine_set_correction(cfg.correction_enabled ? 1 : 0);
    // 中英混输开关（0.2.8）
    engine_set_mix_mode(cfg.mix_mode_enabled ? 1 : 0);
    // 简繁转换开关（0.2.11）
    engine_set_traditional(cfg.traditional_enabled ? 1 : 0);
    // 中英标点开关（P0-2）
    engine_set_ascii_punct(cfg.ascii_punct ? 1 : 0);
    // V0.4.x 配对符号成对上屏开关
    taishen::SetPairPunctEnabled(cfg.pair_punct);
    // Emoji 开关（P2-5）
    engine_set_emoji(cfg.emoji_enabled ? 1 : 0);
    // V0.2.32/0.2.33 应用级配置（对标 rime weasel app_options）：
    // 应用当前前台进程的初始/记忆状态 + 计算 inline 覆盖。
    // 原 P2-6「每次按键强制英文」逻辑删除——语义改为「首次进入初始状态 + per-app 记忆」。
    const taishen::AppStateResult appState = taishen::AppStateApply(cfg);
    // 候选窗口主题（V0.2.4 + V0.2.20 跟随系统）
    {
        taishen::CandidateTheme theme;
        if (taishen::ApplyThemeWithSystem(theme, cfg)) {
            m_candidateWindow.SetFollowSystemTheme(true);
        } else {
            m_candidateWindow.SetFollowSystemTheme(false);
        }
        m_candidateWindow.SetTheme(theme);
    }
    // 候选窗字体/字号（V0.2.21）
    m_candidateWindow.SetFont(cfg.font_face, cfg.font_size);
    // 行内预编辑（V0.2.18）+ 应用级覆盖（V0.2.32，对标 weasel firefox inline_preedit）
    m_candidateWindow.SetInlinePreedit(
        appState.inline_hit ? true : cfg.inline_preedit);
    // P0-1 视觉升级：标签格式 + 布局参数（圆角/内边距/候选间距）
    m_candidateWindow.SetLabelFormat(cfg.label_format);
    m_candidateWindow.SetLayout(cfg.corner_radius, cfg.hilite_corner_radius,
                                cfg.padding, cfg.candidate_spacing);
    // P0-1 滚轮翻页（候选窗口滚轮 → 引擎翻页 → 刷新）
    m_candidateWindow.SetPageCallback([this](int delta) {
        const int count = engine_page(delta);
        if (count > 0 || m_pinyin.size() > 1) {
            RefreshState();
        }
    });
    // 快捷短语开关（0.2.12）+ 自定义短语文件
    engine_set_phrase_enabled(cfg.phrase_enabled ? 1 : 0);
    if (!cfg.phrase_path.empty()) {
        std::string phrasePathUtf8;
        const int len = WideCharToMultiByte(CP_UTF8, 0, cfg.phrase_path.c_str(),
                                            static_cast<int>(cfg.phrase_path.size()),
                                            nullptr, 0, nullptr, nullptr);
        if (len > 0) {
            phrasePathUtf8.resize(static_cast<size_t>(len));
            WideCharToMultiByte(CP_UTF8, 0, cfg.phrase_path.c_str(),
                                static_cast<int>(cfg.phrase_path.size()),
                                &phrasePathUtf8[0], len, nullptr, nullptr);
        }
        engine_set_phrase_path(phrasePathUtf8.empty() ? nullptr : phrasePathUtf8.c_str());
    }
    taishen::DebugLog("ApplyConfig: 配置已应用");
}

/// 启动 config.ini 变更轮询（2s 间隔，托盘消息窗口挂定时器）
void CTextService::StartConfigWatch()
{
    if (m_configWatchStarted || m_trayHwnd == nullptr) {
        return;
    }
    m_configPath = GetDllDir() + L"config.ini";
    // 记录基准 mtime
    WIN32_FILE_ATTRIBUTE_DATA attrs = {};
    if (GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        m_configMtime = attrs.ftLastWriteTime;
    }
    SetTimer(m_trayHwnd, 1, 2000, nullptr);
    m_configWatchStarted = true;
    taishen::DebugLog("StartConfigWatch: 配置热加载轮询已启动 (2s)");
}

/// 停止轮询（Deactivate 时）
void CTextService::StopConfigWatch()
{
    if (m_configWatchStarted && m_trayHwnd != nullptr) {
        KillTimer(m_trayHwnd, 1);
        m_configWatchStarted = false;
        taishen::DebugLog("StopConfigWatch: 配置热加载轮询已停止");
    }
}

/// 定时器回调：config.ini mtime 变化 → 重载配置
void CTextService::ReloadConfigIfChanged()
{
    if (m_configPath.empty()) {
        return;
    }
    WIN32_FILE_ATTRIBUTE_DATA attrs = {};
    if (!GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        return; // 文件暂时不可达（编辑中/被删），跳过
    }
    // mtime 变化才重载
    if (attrs.ftLastWriteTime.dwHighDateTime == m_configMtime.dwHighDateTime &&
        attrs.ftLastWriteTime.dwLowDateTime == m_configMtime.dwLowDateTime) {
        return;
    }
    m_configMtime = attrs.ftLastWriteTime;
    const taishen::ImeConfig cfg = taishen::LoadConfig(GetDllDir());
    ApplyConfig(cfg, GetDllDir());
    // 工具栏主题跟随系统（V0.2.20）
    const int sysTheme = taishen::GetSystemAppTheme();
    if (sysTheme >= 0) {
        taishen::CBannerWindow::Instance().SetLightTheme(sysTheme == 1);
    }
    taishen::DebugLog("ReloadConfigIfChanged: config.ini 已变更，热重载完成");
}

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
/// V0.4.3-P1B：成功返回前把坐标换算为物理像素（DPI-unaware 宿主返回
///            96-DPI 逻辑像素，与 GetCursorPos/SetWindowPos 单位不一致）。
HRESULT CTextService::GetCaretRectFromContext(ITfContext* pic, RECT* pRect)
{
    if (pic == nullptr || pRect == nullptr) {
        return E_POINTER;
    }

    // 请求同步编辑会话（TSF 会回调我们的 CEditSessionGetCaret）
    HWND hostWnd = nullptr;
    CEditSessionGetCaret* pSession =
        new (std::nothrow) CEditSessionGetCaret(pic, pRect, &hostWnd);
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
    if (SUCCEEDED(hrSession) && hostWnd != nullptr) {
        taishen::CaretToPhysicalPixel(hostWnd, pRect);
    }
    return hrSession;
}

/// 更新候选窗口：拉取光标坐标 → 传入拼音/候选 → 显示或隐藏
void CTextService::UpdateCandidateWindow()
{
    // 0.2.28 标点复选：显示复选标点候选（不走引擎拼音路径）
    if (!m_punctCandidates.empty()) {
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
        taishen::DebugLog("UpdateCandidateWindow: punct-cands=" +
                          std::to_string(m_punctCandidates.size()));
        // 候选窗接收 UTF-8 列表，宽字符转 UTF-8
        std::vector<std::string> punctUtf8;
        punctUtf8.reserve(m_punctCandidates.size());
        for (const auto& w : m_punctCandidates) {
            punctUtf8.push_back(taishen::WideToUtf8(w));
        }
        m_candidateWindow.UpdateState("", punctUtf8, caretRect, 1, 1);
        return;
    }

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
                                    const std::string& text,
                                    int caretOffset)
{
    if (pic == nullptr) {
        return;
    }

    CEditSessionComposition* pSession =
        new (std::nothrow) CEditSessionComposition(pic, op, text,
                                                   &m_composition,
                                                   caretOffset);
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
    if (slash == std::wstring::npos) {        return std::wstring();
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




