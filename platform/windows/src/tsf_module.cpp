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
#include <mutex>
#include <new>
#include <string>
#include <utility>
#include <vector>

#include "engine_bridge.h"
#include "engine_text.h"
#include "tsf_keyevent.h"
#include "candidate_window.h"
#include "banner_window.h"
#include "tsf_composition.h"
#include "display_attributes.h"
#include "config_reader.h"
#include "config_apply.h"
#include "app_container.h"
#include "app_state.h"
#include "theme.h"
#include "debug_log.h"
#include "dpi_util.h"
#include "ime_ids.h"
#include "ipc_render_client.h"
#include "langbar_item.h"
#include "settings_dialog.h"
#include <thread>

// DLL 自身模块句柄（定义于 dllmain.cpp）
extern HMODULE g_hModule;

// P1-2 数字分隔符状态（定义于 tsf_keyevent.cpp，tsf_keyevent.h 已声明 extern）：
// 最近一次 IME 提交的文本是否以数字结尾——若是，紧随的 , . 直通半角。

// 辅助函数（定义于文件末尾）
static std::wstring GetDllPath();
static std::wstring GetDllDir();
// 托盘中英状态跨进程共享（定义在 TrayWndProc 附近，ToggleAsciiMode 先用）
static void WriteTrayAsciiState(bool ascii);
static void BroadcastTrayRefresh();

// P2-6/V0.2.33 应用级配置：前台进程名获取与 per-app 状态应用已移至 app_state 模块
// （GetForegroundProcessName / AppStateApply / AppStateSetAscii）

// ---------------------------------------------------------------------------
// GUID 定义（正式生成，非占位）
// ---------------------------------------------------------------------------
// CLSID_TAISHEN_IME / GUID_LANGPROFILE / GUID_LANGBARITEM 统一于 include/ime_ids.h
// （langbar_item.cpp 同样引用，避免同一 CLSID 出现两份字面量）

// ---------------------------------------------------------------------------
// 前向声明
// ---------------------------------------------------------------------------
class CTextService;
class CClassFactory;

// 模块引用计数（DllCanUnloadNow 使用）
static LONG g_cRefDll = 0;

// 引擎是进程级单例；TSF 允许同一进程存在多个 TextService 实例。
// 用租约计数限制销毁时机，避免任一实例 Deactivate 清掉其他实例仍在使用的引擎。
//
// 租约归零不立即销毁（2026-11-17）：切换输入法必然经历 Deactivate → Activate，
// 立即 destroy 会让下一次激活重新加载 179MB 词库索引（实测 1.1–1.3 秒，
// 表现就是「切一下卡一下」）。这里改成记录空闲起点，只有空闲超过阈值后
// 的下一次激活才真正重建。dictionary::init 自身按路径幂等，
// 所以阈值内切回来是零重载。
static std::mutex g_engineLifecycleMutex;
static unsigned int g_engineLeaseCount = 0;
// 租约归零的时刻（GetTickCount64）；0 表示当前有租约。
static ULONGLONG g_engineIdleSince = 0;
// 空闲多久之后的下一次激活才重建引擎。取 5 分钟：用户切走又切回几乎总在阈值内，
// 而真正不再使用的进程也不会把词库内存永久占着。
static constexpr ULONGLONG kEngineIdleRebuildMs = 5ULL * 60ULL * 1000ULL;

static int AcquireEngineLease(const char* dictPath)
{
    std::lock_guard<std::mutex> lock(g_engineLifecycleMutex);
    if (g_engineLeaseCount == 0) {
        if (g_engineIdleSince != 0 &&
            GetTickCount64() - g_engineIdleSince > kEngineIdleRebuildMs) {
            // 长时间空闲：释放词库内存，下面重新初始化
            taishen::ForceLog("AcquireEngineLease: idle exceeded, rebuilding engine");
            engine_destroy();
        }
        g_engineIdleSince = 0;
        const int ret = engine_init(dictPath);
        taishen::DebugLog("AcquireEngineLease: engine_init ret=" + std::to_string(ret));
        if (ret != 0) {
            return ret;
        }
    }
    ++g_engineLeaseCount;
    return 0;
}

static void ReleaseEngineLease()
{
    std::lock_guard<std::mutex> lock(g_engineLifecycleMutex);
    if (g_engineLeaseCount == 0) {
        return;
    }
    --g_engineLeaseCount;
    if (g_engineLeaseCount == 0) {
        // 只记录空闲起点，不销毁：下一次激活可能就在几百毫秒之后
        g_engineIdleSince = GetTickCount64();
    }
}

// 用户数据落盘：AppContainer 内 %APPDATA% 被重定向到包内虚拟目录，
// 用户词库不可达也不可写，写回只会往包内虚拟目录写垃圾。直接跳过并记一次 INFO。
static void FlushUserWritesIfWritable(int force)
{
    if (taishen::IsAppContainerProcess()) {
        static std::once_flag s_containerFlushOnce;
        std::call_once(s_containerFlushOnce, []() {
            taishen::ForceLog(
                "AppContainer=1: skip user-dict write-back (user dict lives outside container)");
        });
        return;
    }
    engine_flush_user_writes(force);
}

// ---------------------------------------------------------------------------
// CTextService — TSF Text Service 核心
// ---------------------------------------------------------------------------
class CTextService : public ITfTextInputProcessorEx,
                     public ITfKeyEventSink,
                     public ITfThreadMgrEventSink,
                     public ITfDisplayAttributeProvider,
                     public ILangBarHost
{
public:
    CTextService();
    virtual ~CTextService();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // ITfDisplayAttributeProvider（V0.6.x）：组合串分段着色。
    // TSF 通过 CLSID 的 CoCreateInstance + QI 拿到本接口，
    // 再按 GUID 取每个属性的绘制样式。
    STDMETHODIMP EnumDisplayAttributeInfo(
        IEnumTfDisplayAttributeInfo** ppEnum) override;
    STDMETHODIMP GetDisplayAttributeInfo(
        REFGUID guid, ITfDisplayAttributeInfo** ppInfo) override;

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
    // 组合串唯一拼装点（V0.6.x）：
    //   组词态 → 引擎已选前缀（engine_compose_prefix）+ 待选后缀 pending
    //   普通输入 → pending（引擎拼音显示串）
    // 所有 Start/Update 都必须经过这里，禁止各处自行拼串。
    std::string BuildCompositionText(const std::string& pending) const;
    // 把当前组合态推给宿主：pending 为空时自动取引擎音节串，组词态自动带已选前缀。
    void SyncComposition(ITfContext* pic, const std::string& pending,
                         bool forceStart = false);
    // 获取光标屏幕坐标（编辑会话）→ 更新候选窗口
    // caretContext：优先用调用方手里的当前上下文（按键回调的 pic）。
    // 切换窗口后 m_pFocusContext 可能是 null 或旧窗口的 context，
    // 只靠它会取不到光标而落到鼠标位置（Eric 实测的候选窗错位）。
    void UpdateCandidateWindow(ITfContext* caretContext = nullptr);
    // 编辑会话回调（ITfEditSession）：异步/不强制同步获取光标坐标
    HRESULT GetCaretRectFromContext(ITfContext* pic, RECT* pRect);
    // V4 D1：异步光标会话完成回调（STA UI 线程执行）
    void OnCaretRectResolved(uint64_t seq, const RECT& rect);
    // V4 D2：是否走进程外渲染服务（AppContainer 恒为 true；普通宿主受 config 控制）
    bool UseRemoteRender() const;
    // 统一隐藏候选窗（本地 + 进程外 server）
    void HideCandidateUi();
    // V4 D3：服务端引擎镜像清理/同步/异步提交与远端按键主流程
    void ClearRemoteState();
    void SendRemoteResetCommit();
    void SendRemoteCommitWithLog(const taishen::ipc::CommitRequest& cm);
    void SyncRemoteModesFromPong();
    void ApplyRemoteResponse(const taishen::ipc::CandidateListResponse& resp);
    void CommitRemoteText(ITfContext* pic, const std::wstring& wideText,
                          int caretOffset = -1);
    HRESULT HandleRemoteKeyDown(ITfContext* pic, WPARAM wParam, LPARAM lParam,
                                BOOL* pfEaten);
    // 候选窗口鼠标点击选词（0.1.13）：选择并提交候选
    void OnCandidateClicked(int index);
    // 候选窗口鼠标右键（V0.5.7）：弹菜单（置顶/取消置顶/降权/恢复候选）
    void OnCandidateRightClicked(int index);

    // S-07：TSF UILess 候选 UIElement 协商
    class CCandidateListUIElement final : public ITfCandidateListUIElement
    {
    public:
        explicit CCandidateListUIElement(CTextService* owner);

        STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
        STDMETHODIMP_(ULONG) AddRef() override;
        STDMETHODIMP_(ULONG) Release() override;
        STDMETHODIMP GetDescription(BSTR* pbstrDescription) override;
        STDMETHODIMP GetGUID(GUID* pguid) override;
        STDMETHODIMP Show(BOOL bShow) override;
        STDMETHODIMP IsShown(BOOL* pbShow) override;
        STDMETHODIMP GetUpdatedFlags(DWORD* pdwFlags) override;
        STDMETHODIMP GetDocumentMgr(ITfDocumentMgr** ppdim) override;
        STDMETHODIMP GetCount(UINT* puCount) override;
        STDMETHODIMP GetSelection(UINT* puIndex) override;
        STDMETHODIMP GetString(UINT uIndex, BSTR* pstr) override;
        STDMETHODIMP GetPageIndex(UINT* pIndex, UINT uSize,
                                  UINT* puPageCnt) override;
        STDMETHODIMP SetPageIndex(UINT* pIndex, UINT uPageCnt) override;
        STDMETHODIMP GetCurrentPage(UINT* puPage) override;

        void MarkUpdated(DWORD flags);

    private:
        LONG m_cRef;
        CTextService* m_owner;
        BOOL m_shown;
        DWORD m_updatedFlags;
    };

    HRESULT EnsureCandidateUiElement();
    void EndCandidateUiElement();
    void OnCandidateUiElementShow(BOOL show);
    // 宿主拒绝 TSF 候选 UI 时是否改由我们自绘候选窗（见 app_state.h 说明）
    bool ShouldSelfDrawWhileSuppressed();

    // 候选窗口（Direct2D 渲染）
    taishen::CCandidateWindow m_candidateWindow;
    // TSF 组合管理（选词上屏）
    taishen::CTsfComposition m_composition;

    // 内部编辑会话：更新组合（Start/Update/Commit 共用，V4 D1 异步安全：持有 COM 引用）
    class CEditSessionComposition : public ITfEditSession
    {
    public:
        enum class Op { Start, Update, Commit };

        CEditSessionComposition(CTextService* owner,
                                ITfContext* pic, Op op,
                                const std::string& text,
                                taishen::CTsfComposition* comp,
                                int caretOffset = -1,
                                LONG convertedChars = 0,
                                bool forceStart = false)
            : m_cRef(1), m_pOwner(owner), m_pic(pic), m_op(op), m_text(text), m_pComp(comp),
              m_caretOffset(caretOffset), m_convertedChars(convertedChars),
              m_forceStart(forceStart)
        {
            if (m_pOwner != nullptr) { m_pOwner->AddRef(); }
            if (m_pic != nullptr) { m_pic->AddRef(); }
        }

        ~CEditSessionComposition()
        {
            if (m_pic != nullptr) { m_pic->Release(); }
            if (m_pOwner != nullptr) { m_pOwner->Release(); }
        }

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
            if (m_pComp == nullptr || m_pic == nullptr) { return E_POINTER; }
            switch (m_op) {
            case Op::Start:
                // 异步排队下前一条 Start 可能已生效，非 forceStart 时无缝转 Update 防闪烁重建
                if (!m_forceStart && m_pComp->IsActive()) {
                    return m_pComp->UpdateComposition(ec, m_pic, m_text,
                                                      m_convertedChars);
                }
                return m_pComp->StartComposition(ec, m_pic, m_text);
            case Op::Update:
                return m_pComp->UpdateComposition(ec, m_pic, m_text,
                                                  m_convertedChars);
            case Op::Commit:
                return m_pComp->CommitComposition(ec, m_pic, m_text,
                                                  m_caretOffset);
            }
            return E_FAIL;
        }

    private:
        LONG m_cRef;
        CTextService* m_pOwner;
        ITfContext* m_pic;
        Op m_op;
        std::string m_text;
        taishen::CTsfComposition* m_pComp;
        int m_caretOffset;
        // 已选前缀的宽字符数（组词态分段着色）
        LONG m_convertedChars;
        bool m_forceStart;
    };

    // 编辑会话调度辅助：在异步安全编辑会话中执行组合操作（不使用 TF_ES_SYNC）
    // convertedChars：已选前缀宽字符数，仅 Update 使用
    HRESULT RunCompositionOp(ITfContext* pic, CEditSessionComposition::Op op,
                             const std::string& text, int caretOffset = -1,
                             LONG convertedChars = 0, bool forceStart = false);

    // 内部编辑会话：获取光标屏幕坐标（V4 D1 异步安全：不持栈指针，持有 COM 引用并回调 owner）
    class CEditSessionGetCaret : public ITfEditSession
    {
    public:
        CEditSessionGetCaret(CTextService* owner, ITfContext* pic, uint64_t seq)
            : m_cRef(1), m_pOwner(owner), m_pic(pic), m_seq(seq)
        {
            if (m_pOwner != nullptr) { m_pOwner->AddRef(); }
            if (m_pic != nullptr) { m_pic->AddRef(); }
        }

        ~CEditSessionGetCaret()
        {
            if (m_pic != nullptr) { m_pic->Release(); }
            if (m_pOwner != nullptr) { m_pOwner->Release(); }
        }

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
            if (m_pic != nullptr && m_pOwner != nullptr) {
                TF_SELECTION selection = {};
                ULONG fetched = 0;
                hr = m_pic->GetSelection(ec, TF_DEFAULT_SELECTION, 1,
                                         &selection, &fetched);
                if (SUCCEEDED(hr) && fetched > 0 && selection.range != nullptr) {
                    ITfContextView* pView = nullptr;
                    hr = m_pic->GetActiveView(&pView);
                    if (SUCCEEDED(hr) && pView != nullptr) {
                        RECT rc = {0, 0, 0, 0};
                        BOOL clipped = FALSE;
                        hr = pView->GetTextExt(ec, selection.range,
                                               &rc, &clipped);
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

    private:
        LONG m_cRef;
        CTextService* m_pOwner;
        ITfContext* m_pic;
        uint64_t m_seq;
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
    // S-07：UIElement 管理器与当前候选列表元素
    ITfUIElementMgr* m_pUIElementMgr;
    CCandidateListUIElement* m_candidateUiElement;
    DWORD m_candidateUiElementId;
    bool m_candidateUiSuppressed;
    // 宿主是否真的读过候选数据（GetCount / GetString / GetPageIndex 等）。
    // show=FALSE 只代表宿主不让系统画 UIElement，不代表它自己会画——
    // 魔兽世界（wow.exe）返回 show=FALSE 却从不读数据，必须由我们自绘兜底。
    LONG m_uiElementDataRead;
    // 当前拼音串（UTF-8，来自引擎）
    std::string m_pinyin;
    // 当前候选词列表（UTF-8）
    std::vector<std::string> m_candidates;
    // 最后一次选词提交的文本
    std::wstring m_committedText;
    // 组词已选前缀不再在平台层维护（V0.6.x）：
    // 唯一状态源是引擎的 compose_chars，平台只在显示/提交时读取，
    // 避免「引擎已回退、平台还留着旧字」造成字词重复上屏（B-22）。
    // 是否为前台激活
    BOOL m_fActive;
    // 当前实例是否持有进程级引擎租约
    bool m_engineLease;
    // V4 D3：是否走服务端引擎（AppContainer 强制开启，普通宿主受 use_server_engine 控制）
    bool m_useServerEngine = false;
    taishen::RemoteEngineMirror m_remoteState;
    uint64_t m_ipcKeySeq = 0;
    bool m_remoteTradOverride = false;
    bool m_remoteShuangpinOverride = false;
    bool m_remotePongSynced = false;

    // ── 语言栏按钮（输入指示器 + 中英切换）──
    // ActivateEx 时 AddItem 到系统语言栏，Deactivate/析构时摘除
    CLangBarItemButton* m_pLangBarItem;
    CLangBarItemButton* m_pModeItem;

    // ── 托盘图标（0.2.13）──
    // 隐藏消息窗口（托盘图标宿主）
    HWND m_trayHwnd;
    // 托盘图标是否已添加（品牌图标 / 中英状态图标各一）
    bool m_trayAdded;
    bool m_modeTrayAdded;
    bool m_trayVisible = false;   // 图标当前是否显示（跟随前台，0.1.54）
    // 托盘图标当前文本（"中"/"英"）
    std::wstring m_trayLabel;

    // ── 配置热加载（对标 rime-ice 重新部署）──
    std::wstring m_configPath;      // config.ini 绝对路径
    FILETIME m_configMtime = {};    // 上次读取的 mtime（变化检测）
    bool m_configWatchStarted = false; // 定时器是否已启动
    // V0.2.33 per-app 状态记忆：最近一次 ApplyConfig 的配置快照
    // （OnSetFocus 时用于 AppStateApply 的 app_ascii/app_cn/app_inline 判断）
    taishen::ImeConfig m_appCfg;

    // V4 D1/D2：异步光标缓存与进程外候选窗渲染客户端
    taishen::CRenderIpcClient m_renderClient;
    uint64_t m_renderSessionId = 0;
    uint64_t m_caretSeq = 0;
    uint64_t m_lastResolvedCaretSeq = 0;
    RECT m_lastCaretRect = {0, 0, 0, 0};
    bool m_hasLastCaretRect = false;
    std::string m_lastDisplayPinyin;
    int m_lastPage = 0;
    int m_lastTotalPages = 0;

    // 语言栏按钮（输入指示器 + 中英切换）
    void AttachLangBarItem();
    void DetachLangBarItem();
    void RefreshLangBarItem();
    // 语言栏管理器与挂载状态：Attach/Detach 共用同一实例——
    // Attach 走 CoCreate 兜底时 Detach 的 QI 可能失败，项就摘不掉（宿主持有裸指针）
    ITfLangBarItemMgr* m_langBarMgr = nullptr;
    bool m_brandAttached = false;
    bool m_modeAttached = false;
    // ILangBarHost：语言栏按钮回调
    void OnLangBarToggleAscii() override;
    bool IsAsciiMode() const override;
    void OnLangBarNeedRefresh() override;
    void OnLangBarToggleToolbar() override;

    // 托盘管理
    bool InitTrayIcon();       // 激活时添加
    void SetTrayVisible(bool visible);   // 跟随前台显隐（0.1.54）
    void HideTrayIcons();                // 只删图标，保留消息窗口
    static void OnForegroundTaishenChanged(bool fgTaishen);
    void ReAddTrayIcon();      // explorer 重启后重新添加（0.1.25）
    void UpdateTrayIcon();     // 模式切换时更新图标+tooltip
    void UpdateTrayIconFor(bool asciiMode);  // 按显式状态刷新（跨进程广播用）
    void OnTrayCmdBroadcast(int cmd);        // 托盘命令广播：前台进程执行
    void RemoveTrayIcon();     // 停用时移除
    bool TrayIconAlive();      // 探活：GUID 图标是否仍在通知区（跨进程可能被删）
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
    void LogDictLoadStatusOnce();  // 词库加载结果记一次（异步加载完成后）

    // 多行展开状态（0.2.14）
    bool m_multiRowExpanded;

    // ── Shift tap 中英切换（0.2.26，主流输入法标准）──
    // Shift 快速按下-松开（期间无其他键、<500ms）→ 切换中英；
    // Shift+字母/符号组合不误切（按住期间按下其它键 → m_shiftComboUsed）。
    bool m_shiftTapArmed;
    bool m_shiftComboUsed;   // Shift 按住期间按了其它键 → 组合键，keyup 不切
    bool m_shiftActive;      // Shift 物理按住中：自动重复的 keydown 不重置判定
    DWORD m_shiftDownTick;

    // ── 配对引号（0.2.28，V0.4.x 成对时开闭状态仅作开关关闭时的退化路径）──
    // 配对引号开闭状态（' 单引号 / " 双引号）
    bool m_quoteOpen;
    bool m_dquoteOpen;

    // 词库加载结果日志只记一次（异步加载完成后由定时器/焦点事件补记）
    bool m_dictStatusLogged;
};

// ---------------------------------------------------------------------------
// CTextService 实现
// ---------------------------------------------------------------------------
CTextService::CCandidateListUIElement::CCandidateListUIElement(
    CTextService* owner)
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
    // 每个 TextService 实例使用同一稳定 GUID，宿主据此识别候选元素。
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
        // 读了数据 = 有自绘能力，之后 show=FALSE 时尊重宿主，不叠第二层窗口
        InterlockedExchange(&m_owner->m_uiElementDataRead, 1);
    }
    if (m_owner == nullptr) {
        *puCount = 0;
    } else if (m_owner->m_useServerEngine) {
        *puCount = static_cast<UINT>((std::max)(0, m_owner->m_remoteState.totalCandidates));
    } else {
        *puCount = static_cast<UINT>((std::max)(0, engine_get_total_candidate_count()));
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
    if (m_owner->m_useServerEngine) {
        const size_t pageSz = static_cast<size_t>((std::max)(1, m_owner->m_remoteState.pageSize));
        const size_t offset = static_cast<size_t>((std::max)(0, m_owner->m_remoteState.page)) * pageSz;
        if (uIndex >= offset && (uIndex - offset) < m_owner->m_candidates.size()) {
            candidate = m_owner->m_candidates[uIndex - offset];
        } else if (uIndex < m_owner->m_candidates.size()) {
            candidate = m_owner->m_candidates[uIndex];
        } else {
            return E_INVALIDARG;
        }
    } else if (!taishen::ReadEngineCandidateAt(static_cast<int>(uIndex), candidate)) {
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
    const int rawPages = (m_owner == nullptr)
                             ? 0
                             : (m_owner->m_useServerEngine ? m_owner->m_remoteState.totalPages
                                                           : engine_get_total_pages());
    const UINT pageCount = m_owner == nullptr
                               ? 0
                               : static_cast<UINT>((std::max)(1, rawPages));
    *puPageCnt = pageCount;
    if (pIndex == nullptr) {
        return S_OK;
    }
    if (uSize < pageCount) {
        return E_INVALIDARG;
    }
    const int rawPageSize = (m_owner == nullptr)
                                ? 1
                                : (m_owner->m_useServerEngine ? m_owner->m_remoteState.pageSize
                                                              : engine_get_page_size());
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
    // 页面翻页仍由引擎按键处理；宿主设置页索引时保持协议可用，
    // 不在 UIElement 回调中重入引擎状态机。
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
    const int curPage = (m_owner == nullptr)
                            ? 0
                            : (m_owner->m_useServerEngine ? m_owner->m_remoteState.page
                                                          : engine_get_current_page());
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
    // 新建元素前清零：宿主可能在 BeginUIElement 返回之前就读数据
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

/// 宿主拒绝 TSF 候选 UI 时，是否由我们自绘候选窗。
/// 依据：app_self_draw / app_no_self_draw 显式配置 → 宿主是否读过候选数据。
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

CTextService::CTextService()
    : m_cRef(1), m_pThreadMgr(nullptr), m_tid(0),
      m_dwThreadMgrEventSinkCookie(0),
      m_pFocusContext(nullptr), m_pUIElementMgr(nullptr),
      m_candidateUiElement(nullptr), m_candidateUiElementId(0),
      m_candidateUiSuppressed(false), m_uiElementDataRead(0),
      m_fActive(FALSE), m_engineLease(false),
      m_pLangBarItem(nullptr), m_pModeItem(nullptr),
      m_trayHwnd(nullptr), m_trayAdded(false), m_modeTrayAdded(false),
      m_trayLabel(L"中"),
      m_multiRowExpanded(false), m_shiftTapArmed(false),
      m_shiftComboUsed(false), m_shiftActive(false), m_shiftDownTick(0),
      m_quoteOpen(false), m_dquoteOpen(false), m_dictStatusLogged(false)
{
    static volatile LONG64 s_nextRenderSid = 0;
    m_renderSessionId = (static_cast<uint64_t>(GetCurrentProcessId()) << 32) |
                        static_cast<uint32_t>(InterlockedIncrement64(&s_nextRenderSid));
    InterlockedIncrement(&g_cRefDll);
    // 候选窗口鼠标点击选词（0.1.13）：回调在此上下文执行选词提交
    m_candidateWindow.SetClickCallback(
        [this](int index) { this->OnCandidateClicked(index); });
    // 候选窗口鼠标右键（V0.5.7）：回调弹菜单（置顶/取消置顶/降权/恢复候选）
    m_candidateWindow.SetRightClickCallback(
        [this](int index) { this->OnCandidateRightClicked(index); });
}

CTextService::~CTextService()
{
    EndCandidateUiElement();
    DetachLangBarItem();
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
    } else if (IsEqualIID(riid, IID_ITfDisplayAttributeProvider)) {
        *ppvObj = static_cast<ITfDisplayAttributeProvider*>(this);
    } else {
        return E_NOINTERFACE;
    }

    AddRef();
    return S_OK;
}

// ---------------------------------------------------------------------------
// ITfDisplayAttributeProvider — 组合串分段着色（V0.6.x）
// ---------------------------------------------------------------------------
STDMETHODIMP CTextService::EnumDisplayAttributeInfo(
    IEnumTfDisplayAttributeInfo** ppEnum)
{
    if (ppEnum == nullptr) {
        return E_POINTER;
    }
    *ppEnum = taishen::CreateDisplayAttributeEnum();
    return *ppEnum != nullptr ? S_OK : E_OUTOFMEMORY;
}

STDMETHODIMP CTextService::GetDisplayAttributeInfo(
    REFGUID guid, ITfDisplayAttributeInfo** ppInfo)
{
    if (ppInfo == nullptr) {
        return E_POINTER;
    }
    *ppInfo = taishen::CreateDisplayAttributeInfo(guid);
    return *ppInfo != nullptr ? S_OK : E_INVALIDARG;
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
// 语言栏按钮（输入指示器 + 中英切换）
// ---------------------------------------------------------------------------
// Windows 的输入指示器对 TSF TIP 的图标取自 ITfLangBarItemButton::GetIcon。
// 注册表 IconFile/IconIndex 只在旧式语言栏生效，且要求 DLL 内确有 ICON 资源，
// 因此必须在激活期把 langbar item 挂到 ITfLangBarItemMgr 上。
// 对齐微软拼音：一个输入法标识图标 + 一个中英切换按钮。
void CTextService::AttachLangBarItem()
{
    if (m_pThreadMgr == nullptr) {
        return;
    }
    if (m_brandAttached && m_modeAttached) {
        return;   // 两项都已挂上，避免重复 AddItem
    }
    if (m_pLangBarItem == nullptr) {
        m_pLangBarItem =
            new (std::nothrow) CLangBarItemButton(this, CLangBarItemButton::Kind::Brand);
    }
    if (m_pModeItem == nullptr) {
        m_pModeItem =
            new (std::nothrow) CLangBarItemButton(this, CLangBarItemButton::Kind::Mode);
    }
    if (m_pLangBarItem == nullptr || m_pModeItem == nullptr) {
        taishen::DebugLog("AttachLangBarItem: alloc failed");
        return;
    }
    if (m_langBarMgr == nullptr) {
        ITfLangBarItemMgr* pItemMgr = nullptr;
        HRESULT hr = m_pThreadMgr->QueryInterface(
            IID_ITfLangBarItemMgr, reinterpret_cast<void**>(&pItemMgr));
        if (FAILED(hr) || pItemMgr == nullptr) {
            // 兜底：独立 CoCreate 语言栏管理器（部分宿主线程 QI 拿不到）
            hr = CoCreateInstance(CLSID_TF_LangBarItemMgr, nullptr,
                                  CLSCTX_INPROC_SERVER, IID_ITfLangBarItemMgr,
                                  reinterpret_cast<void**>(&pItemMgr));
            taishen::DebugLogHr("AttachLangBarItem: CoCreate(CLSID_TF_LangBarItemMgr)", hr);
        }
        if (FAILED(hr) || pItemMgr == nullptr) {
            taishen::DebugLogHr("AttachLangBarItem: QI(ITfLangBarItemMgr)", hr);
            return;
        }
        // 保存管理器实例：Detach 与重挂必须走同一个，否则 CoCreate 兜底挂上的项摘不掉
        m_langBarMgr = pItemMgr;
    }
    if (!m_brandAttached) {
        const HRESULT hrBrand = m_langBarMgr->AddItem(m_pLangBarItem);
        m_brandAttached = SUCCEEDED(hrBrand);
        taishen::DebugLogHr("AttachLangBarItem: AddItem(brand)", hrBrand);
    }
    if (!m_modeAttached) {
        const HRESULT hrMode = m_langBarMgr->AddItem(m_pModeItem);
        m_modeAttached = SUCCEEDED(hrMode);
        taishen::DebugLogHr("AttachLangBarItem: AddItem(mode)", hrMode);
    }
}

void CTextService::DetachLangBarItem()
{
    // 与 Attach 共用同一个管理器实例：CoCreate 兜底挂上的项也能摘掉
    if (m_langBarMgr != nullptr) {
        if (m_brandAttached && m_pLangBarItem != nullptr) {
            m_langBarMgr->RemoveItem(m_pLangBarItem);
        }
        if (m_modeAttached && m_pModeItem != nullptr) {
            m_langBarMgr->RemoveItem(m_pModeItem);
        }
        m_langBarMgr->Release();
        m_langBarMgr = nullptr;
    }
    m_brandAttached = false;
    m_modeAttached = false;
    if (m_pLangBarItem != nullptr) {
        m_pLangBarItem->Release();
        m_pLangBarItem = nullptr;
    }
    if (m_pModeItem != nullptr) {
        m_pModeItem->Release();
        m_pModeItem = nullptr;
    }
}

/// 中英状态变化后让语言栏重绘：优先走 ITfSource sink 通知（OnUpdate，不断开语言栏
/// 与 item 的关系）；sink 尚未装上时退回 RemoveItem/AddItem 强制刷新。
void CTextService::RefreshLangBarItem()
{
    if (m_pThreadMgr == nullptr) {
        return;
    }
    bool notified = false;
    if (m_pModeItem != nullptr) {
        notified = m_pModeItem->NotifyUpdate() || notified;
    }
    if (m_pLangBarItem != nullptr) {
        notified = m_pLangBarItem->NotifyUpdate() || notified;
    }
    if (notified) {
        return;
    }
    // 有项还没挂上 → 补挂（幂等）
    if (!m_brandAttached || !m_modeAttached) {
        AttachLangBarItem();
        return;
    }
    // 已挂上但 sink 为空（语言栏未回调 AdviseSink）→ 摘除重挂逼系统重新取图标/文本
    if (m_langBarMgr != nullptr) {
        if (m_modeAttached && m_pModeItem != nullptr) {
            m_langBarMgr->RemoveItem(m_pModeItem);
            m_modeAttached = SUCCEEDED(m_langBarMgr->AddItem(m_pModeItem));
        }
        if (m_brandAttached && m_pLangBarItem != nullptr) {
            m_langBarMgr->RemoveItem(m_pLangBarItem);
            m_brandAttached = SUCCEEDED(m_langBarMgr->AddItem(m_pLangBarItem));
        }
    }
}

// ILangBarHost — 语言栏按钮回调
void CTextService::OnLangBarToggleAscii()
{
    ToggleAsciiMode();
}

bool CTextService::IsAsciiMode() const
{
    if (m_useServerEngine) {
        return m_remoteState.asciiMode;
    }
    return engine_get_ascii_mode() == 1;
}

void CTextService::OnLangBarNeedRefresh()
{
    RefreshLangBarItem();
}

/// 语言栏右键菜单「显示/隐藏工具栏」：与托盘菜单同一条路径——
/// 写 config.ini 而不是只改本进程内存，否则切到别的程序又变回默认
void CTextService::OnLangBarToggleToolbar()
{
    const bool next = !taishen::CBannerWindow::Instance().IsEnabled();
    taishen::CBannerWindow::Instance().SetEnabled(next);
    const std::wstring dllDir = GetDllDir();
    taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    cfg.toolbar_enabled = next;
    taishen::SaveConfig(dllDir, cfg);
    taishen::DebugLog("LangBar: toolbar -> " + std::to_string(next));
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
    // 激活耗时埋点（2026-11-17 任务栏搜索假死排查）：
    // 容器内宿主 UI 线程会被 ActivateEx 里的同步操作阻塞，逐段记耗时才能定位。
    const ULONGLONG tAct = GetTickCount64();
    auto markStep = [tAct](const char* label) {
        taishen::ForceLog(std::string("ActivateEx step: ") + label + " +" +
                          std::to_string(GetTickCount64() - tAct) + "ms");
    };
    markStep("enter");
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
    markStep("load-config");

    // AppContainer 检测（阶段 1）：容器内 %APPDATA%/%LOCALAPPDATA% 被重定向到
    // 包内虚拟目录，用户词库与配置写回不可达——先判定环境，再逐项降级。
    // 容器内 config.ini 不可读 → debug_log 开关默认关闭，必须用 ForceLog 绕过开关，
    // 否则「AppContainer=1」永远落不到日志里。
    const bool isContainer = taishen::IsAppContainerProcess();
    markStep("detect-container");
    if (isContainer) {
        taishen::ForceLog("ActivateEx: AppContainer=1");
    } else {
        taishen::ForceLog("ActivateEx: AppContainer=0");
    }

    // V4 D3：AppContainer 强制走 server 引擎；普通宿主受 config.ini use_server_engine 控制
    m_useServerEngine = isContainer || cfg.use_server_engine;

    if (m_useServerEngine) {
        m_engineLease = false;
        taishen::ForceLog("ActivateEx: AppContainer=" + std::string(isContainer ? "1" : "0") +
                          " useServerEngine=1 (skip in-process engine)");
    } else {
        // 初始化引擎：词库路径（空 → 回退内置词库）
        std::wstring dictPath = taishen::ResolveDictPath(cfg, dllDir);
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
        markStep("before-engine-lease");
        const int initRet = AcquireEngineLease(
            dictPathUtf8.empty() ? nullptr : dictPathUtf8.c_str());
        markStep("engine-lease");
        m_engineLease = (initRet == 0);
        {
            char buildBuf[128] = {0};
            const int buildLen = engine_build_id(buildBuf, static_cast<int>(sizeof(buildBuf)));
            taishen::DebugLog(std::string("ActivateEx: engine build=") +
                              (buildLen > 0 ? buildBuf : "unknown") +
                              " lease ret=" + std::to_string(initRet) +
                              " dict=" + dictPathUtf8);
        }
        // 词库为异步加载：此处通常还是「加载中」，最终结果由 LogDictLoadStatusOnce 补记
        taishen::DebugLog(std::string("ActivateEx: dict load status=") +
                          std::to_string(engine_dict_load_status()) +
                          " entries=" + std::to_string(engine_dict_entry_count()));
        if (initRet != 0) {
            taishen::ForceLog("ActivateEx: engine lease failed -> E_FAIL");
            m_pThreadMgr->Release();
            m_pThreadMgr = nullptr;
            return E_FAIL;
        }

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
    }

    // 应用配置到引擎/候选窗（抽取自 ActivateEx，供热加载复用）
    markStep("before-apply-config");
    ApplyConfig(cfg, dllDir);
    markStep("apply-config");

    // V4 D2（V1 规则）：仅普通权限宿主进程拉起 `taishen_server.exe`；
    // AppContainer 进程内绝不拉起（子进程会继承容器令牌），只连已运行的 server。
    if (!isContainer) {
        m_renderClient.EnsureServerRunning(dllDir);
    }

    if (!m_useServerEngine && !isContainer) {
        // 拆字反查词库（V0.2.25）：DLL 同目录 radical_pinyin.dict.yaml（2.1MB，需解析）
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

        // 上下文先验表（可选）：DLL 同目录 context_prior.db，缺失时引擎直接跳过
        {
            const std::wstring priorPath = dllDir + L"context_prior.db";
            std::string priorPathUtf8;
            const int len = WideCharToMultiByte(CP_UTF8, 0, priorPath.c_str(),
                                                static_cast<int>(priorPath.size()),
                                                nullptr, 0, nullptr, nullptr);
            if (len > 0) {
                priorPathUtf8.resize(static_cast<size_t>(len));
                WideCharToMultiByte(CP_UTF8, 0, priorPath.c_str(),
                                    static_cast<int>(priorPath.size()),
                                    &priorPathUtf8[0], len, nullptr, nullptr);
            }
            const int priorRet = engine_set_context_prior_path(
                priorPathUtf8.empty() ? nullptr : priorPathUtf8.c_str());
            taishen::DebugLog("ActivateEx: engine_set_context_prior_path ret=" +
                              std::to_string(priorRet));
        }

        // bigram 搭配表（可选）：DLL 同目录 bigram.db
        {
            const std::wstring bigramPath = dllDir + L"bigram.db";
            std::string bigramPathUtf8;
            const int bigramLen = WideCharToMultiByte(CP_UTF8, 0, bigramPath.c_str(),
                                                      static_cast<int>(bigramPath.size()),
                                                      nullptr, 0, nullptr, nullptr);
            if (bigramLen > 0) {
                bigramPathUtf8.resize(static_cast<size_t>(bigramLen));
                WideCharToMultiByte(CP_UTF8, 0, bigramPath.c_str(),
                                    static_cast<int>(bigramPath.size()),
                                    &bigramPathUtf8[0], bigramLen, nullptr, nullptr);
            }
            const int bigramRet =
                engine_set_bigram_path(bigramPathUtf8.empty() ? nullptr : bigramPathUtf8.c_str());
            taishen::DebugLog("ActivateEx: engine_set_bigram_path ret=" +
                              std::to_string(bigramRet));
        }
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

    // 语言栏按钮（输入指示器图标）
    AttachLangBarItem();

    if (!isContainer) {
        // 托盘图标（0.2.13）：显示中英状态，右键菜单切换
        // 0.1.54：图标跟随前台——进程内一次性注册前台归属回调
        {
            static std::once_flag s_trayFgOnce;
            std::call_once(s_trayFgOnce, []() {
                taishen::CBannerWindow::Instance().SetForegroundCallback(
                    &CTextService::OnForegroundTaishenChanged);
            });
        }
        // 状态工具栏（0.1.26）：注册本线程到激活集合，前台判断驱动显示
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
        // 托盘图标跟随前台（0.1.54）：激活完成后再对齐一次
        SetTrayVisible(taishen::CBannerWindow::Instance().IsForegroundTaishen());
    }
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

    // 语言栏按钮：先摘除，再释放线程管理器
    DetachLangBarItem();

    if (m_pFocusContext != nullptr) {
        m_pFocusContext->Release();
        m_pFocusContext = nullptr;
    }

    if (m_pThreadMgr != nullptr) {
        m_pThreadMgr->Release();
        m_pThreadMgr = nullptr;
    }

    if (m_useServerEngine && (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose)) {
        SendRemoteResetCommit();
        ClearRemoteState();
    }
    if (m_engineLease) {
        ReleaseEngineLease();
        m_engineLease = false;
    }
    m_fActive = FALSE;
    m_hasLastCaretRect = false;
    ++m_caretSeq;
    EndCandidateUiElement();
    HideCandidateUi();
    m_renderClient.Disconnect();
    m_composition.Reset();
    if (!taishen::IsAppContainerProcess()) {
        // 配置热加载：停止轮询（托盘窗口将销毁）
        StopConfigWatch();
        // 状态横幅：本线程停用 → 前台判断自动隐藏（若其他线程仍激活则保持）
        taishen::CBannerWindow::Instance().UnregisterThread(GetCurrentThreadId());
        RemoveTrayIcon();
    }
    return S_OK;
}

// ---------------------------------------------------------------------------
// 托盘图标（0.2.13）
// ---------------------------------------------------------------------------

// 托盘图标跟随前台（0.1.54）：本进程内所有持有图标的 CTextService 实例。
// GUID 图标 Shell 全局去重，任一实例增删即生效；这里统一广播状态。
static std::recursive_mutex g_trayInstMutex;
static std::vector<CTextService*> g_trayInstances;

/// 托盘自定义消息（窗口过程区分）
#define WM_TRAYICON (WM_APP + 1)
/// 托盘菜单命令 ID
#define ID_TRAY_TOGGLE_ASCII 1
#define ID_TRAY_TOGGLE_TRAD 2
#define ID_TRAY_TOGGLE_SHUANGPIN 4
#define ID_TRAY_TOGGLE_TOOLBAR 5
#define ID_TRAY_EXIT 3
#define ID_TRAY_OPEN_KEYS 6

/// 托盘图标固定 GUID（0.3.x 修复"托盘双图标"）：
/// TSF DLL 进程内注入，每个激活泰深输入法的进程/线程都会执行
/// InitTrayIcon → NIM_ADD。无 GUID 时各进程图标各自独立 → 托盘重复。
/// 加 NIF_GUID 后 Shell 按 GUID 自动合并去重（微软拼音同款做法）。
/// 0.1.67：GUID 换新——旧 {B3F6A2C9-...-7E2F3A4B5C6D} 已被 Shell 残留占位，
/// NIM_ADD/NIM_DELETE 一律返回 E_FAIL(0x80004005)，图标再也注册不上。
/// 探针实测：旧 GUID ADD 失败、新 GUID ADD/DELETE 成功且可重复。
static const GUID kTrayIconGuid = {
    0xA7D3E1F4, 0x6B21, 0x4C9E,
    {0x8D, 0x52, 0x3F, 0x1A, 0x9C, 0x4E, 0x6B, 0x71}};

/// 中英状态图标的独立 GUID（与品牌图标分开，Shell 不会合并成一个）
/// {A7D3E1F4-6B21-4C9E-8D52-3F1A9C4E6B72}
static const GUID kTrayModeGuid = {
    0xA7D3E1F4, 0x6B21, 0x4C9E,
    {0x8D, 0x52, 0x3F, 0x1A, 0x9C, 0x4E, 0x6B, 0x72}};

// 前向声明（GetTaishenIcon 定义在文件后部，InitTrayIcon 先使用）
static HICON GetTaishenIcon();
static HICON GetModeTrayIcon(bool asciiMode);

/// 中英状态托盘图标（32x32）：中文=品牌蓝底白"中"，英文=中性灰底白"英"
/// 与语言栏按钮同款视觉，用户一眼能看出当前处于哪种模式
/// 与品牌图标并存（0.1.54）：品牌图标负责菜单，状态图标负责一键切中英
static HICON GetModeTrayIcon(bool asciiMode)
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

    RECT rc = {0, 0, size, size};
    HBRUSH bg = CreateSolidBrush(asciiMode ? RGB(0x6B, 0x72, 0x80)
                                           : RGB(0x1E, 0x6F, 0xFF));
    FillRect(memDC, &rc, bg);
    DeleteObject(bg);

    SetBkMode(memDC, TRANSPARENT);
    SetTextColor(memDC, RGB(255, 255, 255));
    HFONT font = CreateFontW(-(size - 6), 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
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
    s_icons[idx] = icon;
    return icon;
}

bool CTextService::InitTrayIcon()
{
    // 0.1.67：托盘图标停用。中英状态改由语言栏「中/英」按钮显示
    // （GUID_LBI_INPUTMODE，Win11 任务栏输入指示器旁），不再占用通知区。
    // 工具栏显隐入口保留在语言栏按钮右键菜单「显示工具栏」。
    return false;
}

void CTextService::ReAddTrayIcon()
{
    // explorer 重启（TaskbarCreated）后旧图标引用失效，需重新 NIM_ADD
    if (m_trayHwnd == nullptr) {
        return;
    }
    // 两个图标都要重建：Shell 重启后 GUID 注册一并丢失
    std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
    const bool wasVisible = m_trayVisible;
    m_trayAdded = false;
    m_modeTrayAdded = false;
    InitTrayIcon();
    // 前台不是泰深时，explorer 重启不该把图标"复活"
    if (!wasVisible) {
        HideTrayIcons();
    }
    taishen::DebugLog("ReAddTrayIcon: rebuilt after TaskbarCreated");
}

void CTextService::UpdateTrayIcon()
{
    const bool ascii = m_useServerEngine ? m_remoteState.asciiMode
                                         : (engine_get_ascii_mode() == 1);
    UpdateTrayIconFor(ascii);
}

void CTextService::UpdateTrayIconFor(bool ascii)
{
    if (m_trayHwnd == nullptr) {
        return;
    }
    // 根据给定模式更新 label + 两个图标的 icon/tooltip
    m_trayLabel = ascii ? L"英" : L"中";

    if (m_trayAdded) {
        NOTIFYICONDATAW nid = {};
        nid.cbSize = sizeof(nid);
        nid.hWnd = m_trayHwnd;
        nid.uID = 0;   // NIF_GUID 模式要求 0
        nid.uFlags = NIF_TIP | NIF_ICON | NIF_GUID;
        nid.guidItem = kTrayIconGuid;
        nid.hIcon = GetModeTrayIcon(ascii);
        const std::wstring tip = L"泰深输入法 - " + m_trayLabel +
                                 L"文模式（左键切换中英，右键菜单）";
        wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
        Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// 探活：本实例的 GUID 图标是否仍在通知区。
/// 跨进程共用同一 GUID，别的进程 NIM_DELETE 后本实例的 m_trayAdded 会失真；
/// 用一次 NIM_MODIFY 判断——返回 FALSE 说明图标已不存在。
/// 副作用仅刷新 tooltip（正好也是当前模式文案）。
bool CTextService::TrayIconAlive()
{
    if (m_trayHwnd == nullptr) {
        return false;
    }
    NOTIFYICONDATAW nid = {};
    nid.cbSize = sizeof(nid);
    nid.hWnd = m_trayHwnd;
    nid.uID = 0;   // NIF_GUID 模式要求 0
    nid.uFlags = NIF_TIP | NIF_GUID;
    nid.guidItem = kTrayIconGuid;
    const std::wstring tip =
        L"泰深输入法 - " + m_trayLabel + L"文模式（左键切换中英，右键菜单）";
    wcsncpy_s(nid.szTip, tip.c_str(), _TRUNCATE);
    return Shell_NotifyIconW(NIM_MODIFY, &nid) != FALSE;
}

void CTextService::HideTrayIcons()
{
    // GUID 图标进程级唯一：任一实例 NIM_DELETE 后，其它实例的 m_trayAdded 也要清，
    // 否则它们以为自己还持有图标 → SetTrayVisible 早退、InitTrayIcon 被挡，图标再也回不来。
    {
        std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
        for (CTextService* svc : g_trayInstances) {
            svc->m_trayVisible = false;
        }
    }
    if (m_trayHwnd == nullptr) {
        m_trayVisible = false;
        return;
    }
    if (m_trayAdded) {
        NOTIFYICONDATAW nid = {};
        nid.cbSize = sizeof(nid);
        nid.hWnd = m_trayHwnd;
        nid.uID = 0;   // NIF_GUID 模式要求 0
        // NIF_GUID 模式删除必须携带相同 GUID（与 NIM_ADD 一致）
        nid.uFlags = NIF_GUID;
        nid.guidItem = kTrayIconGuid;
        Shell_NotifyIconW(NIM_DELETE, &nid);
        m_trayAdded = false;
    }
    if (m_modeTrayAdded) {
        NOTIFYICONDATAW nid = {};
        nid.cbSize = sizeof(nid);
        nid.hWnd = m_trayHwnd;
        nid.uID = 0;   // NIF_GUID 模式要求 0
        nid.uFlags = NIF_GUID;
        nid.guidItem = kTrayModeGuid;
        Shell_NotifyIconW(NIM_DELETE, &nid);
        m_modeTrayAdded = false;
    }
    {
        std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
        for (CTextService* svc : g_trayInstances) {
            svc->m_trayAdded = false;
            svc->m_modeTrayAdded = false;
            svc->m_trayVisible = false;
        }
    }
    m_trayVisible = false;
}

void CTextService::RemoveTrayIcon()
{
    // 全程持递归锁：前台回调也拿同一把锁遍历实例表，
    // 否则快照里的裸指针可能已被析构（跨线程 use-after-free）
    std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
    HideTrayIcons();
    g_trayInstances.erase(
        std::remove(g_trayInstances.begin(), g_trayInstances.end(), this),
        g_trayInstances.end());
    if (m_trayHwnd != nullptr) {
        // 窗口归属创建它的线程：跨线程 DestroyWindow 会失败并留下引用，
        // 这里只在同线程销毁，跨线程的留给进程退出统一回收。
        if (GetWindowThreadProcessId(m_trayHwnd, nullptr) ==
            GetCurrentThreadId()) {
            DestroyWindow(m_trayHwnd);
        }
        m_trayHwnd = nullptr;
    }
}

void CTextService::SetTrayVisible(bool visible)
{
    std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
    if (visible) {
        // 已持有图标 → 先探活，仍在就直接返回。
        // 重复 NIM_ADD 对同一 GUID 会失败，旧实现在这里无条件 m_trayAdded=false
        // 再重建，ADD 失败就把标记错误地留在 false，之后 SetTrayVisible(false)
        // 三者皆 false 直接 return，该隐藏的图标就残留了。
        // 探活失败说明别的进程删了同一 GUID（0.1.68 修的「图标永久消失」），
        // 这时才重建。
        if (m_trayAdded && TrayIconAlive()) {
            m_trayVisible = true;
            return;
        }
        m_trayAdded = false;
        InitTrayIcon();
        m_trayVisible = m_trayAdded;
        taishen::DebugLog("SetTrayVisible(true): added=" +
                          std::to_string(m_trayAdded));
        return;
    }
    if (!m_trayVisible && !m_trayAdded && !m_modeTrayAdded) {
        return;
    }
    HideTrayIcons();
}

void CTextService::OnForegroundTaishenChanged(bool fgTaishen)
{
    // 0.1.68：恢复「托盘图标跟随前台」。之前一版改成常驻，用户实测不符合预期。
    std::lock_guard<std::recursive_mutex> lk(g_trayInstMutex);
    taishen::DebugLog("Tray fg changed: fgTaishen=" +
                      std::to_string(fgTaishen) +
                      " instances=" + std::to_string(g_trayInstances.size()));
    for (CTextService* svc : g_trayInstances) {
        svc->SetTrayVisible(fgTaishen);
    }
}
void CTextService::ToggleAsciiMode()
{
    if (m_useServerEngine) {
        const bool next = !m_remoteState.asciiMode;
        if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
            if (m_pFocusContext != nullptr && m_composition.IsActive()) {
                RunCompositionOp(m_pFocusContext, CEditSessionComposition::Op::Commit, "");
            }
            SendRemoteResetCommit();
            ClearRemoteState();
        }
        m_remoteState.asciiMode = next;
        taishen::AppStateSetAsciiRemote(next);
        RefreshLangBarItem();
        HideCandidateUi();
        return;
    }
    // 与 Ctrl+Space 等效：切换中英（V0.2.33 走 per-app 记忆，更新当前进程状态）
    const int cur = engine_get_ascii_mode();
    taishen::AppStateSetAscii(cur ? false : true);
    taishen::DebugLog("Tray: ToggleAsciiMode -> " +
                      std::to_string(engine_get_ascii_mode()));
    // 0.1.67 起系统托盘图标已停用，不再同步写 tray_state.txt 或 HWND_BROADCAST
    RefreshLangBarItem();
    HideCandidateUi();
}

void CTextService::ToggleTraditional()
{
    const bool isContainer = taishen::IsAppContainerProcess();
    if (m_useServerEngine) {
        SyncRemoteModesFromPong();
        const bool next = !m_remoteState.traditional;
        m_remoteState.traditional = next;
        m_appCfg.traditional_enabled = next;
        m_remoteTradOverride = true;
        if (!isContainer) {
            const std::wstring dllDir = GetDllDir();
            taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
            cfg.traditional_enabled = next;
            taishen::SaveConfig(dllDir, cfg);
            taishen::CBannerWindow::Instance().Refresh();
            m_renderClient.SendReloadConfig();
        }
        taishen::DebugLog("Tray: ToggleTraditional(remote) -> " +
                          std::to_string(next ? 1 : 0));
        return;
    }
    // 0.1.68：写回 config.ini，与工具栏按钮/设置页保持同一份状态
    const bool next = engine_get_traditional() != 1;
    engine_set_traditional(next ? 1 : 0);
    const std::wstring dllDir = GetDllDir();
    taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    cfg.traditional_enabled = next;
    taishen::SaveConfig(dllDir, cfg);
    taishen::DebugLog("Tray: ToggleTraditional -> " +
                      std::to_string(engine_get_traditional()) + " (config updated)");
}

void CTextService::ToggleShuangpin()
{
    const bool isContainer = taishen::IsAppContainerProcess();
    if (m_useServerEngine) {
        SyncRemoteModesFromPong();
        const bool next = !m_appCfg.shuangpin_mode;
        m_appCfg.shuangpin_mode = next;
        m_remoteShuangpinOverride = true;
        SendRemoteResetCommit();
        ClearRemoteState();
        HideCandidateUi();
        if (!isContainer) {
            const std::wstring dllDir = GetDllDir();
            taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
            cfg.shuangpin_mode = next;
            taishen::SaveConfig(dllDir, cfg);
            taishen::CBannerWindow::Instance().Refresh();
            m_renderClient.SendReloadConfig();
        }
        taishen::DebugLog("Tray: ToggleShuangpin(remote) -> " +
                          std::to_string(next ? 1 : 0));
        return;
    }
    // 与 config.ini shuangpin=1 等效：切换双拼/全拼（0.1.26）
    // engine_set_shuangpin 内部切换时清空未完成拼音（引擎 reset）
    // 0.1.68：写回 config.ini——原来只改本进程引擎，设置页开关读不到变化，
    // 工具栏与设置页各说各话；写盘后其他进程经热加载跟随，重启也保持。
    const bool next = engine_get_shuangpin() != 1;
    engine_set_shuangpin(next ? 1 : 0);
    const std::wstring dllDir = GetDllDir();
    taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    cfg.shuangpin_mode = next;
    taishen::SaveConfig(dllDir, cfg);
    taishen::DebugLog("Tray: ToggleShuangpin -> " +
                      std::to_string(engine_get_shuangpin()) + " (config updated)");
}

std::wstring CTextService::BuildBannerText()
{
    std::wstring text;
    const bool ascii = m_useServerEngine ? m_remoteState.asciiMode
                                         : (engine_get_ascii_mode() == 1);
    const bool trad = m_useServerEngine ? m_remoteState.traditional
                                        : (engine_get_traditional() == 1);
    const bool sp = m_useServerEngine ? m_appCfg.shuangpin_mode
                                      : (engine_get_shuangpin() == 1);
    text += ascii ? L"英文模式" : L"中文模式";
    if (trad) {
        text += L" · 简繁";
    }
    if (sp) {
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
    const bool ascii = m_useServerEngine ? m_remoteState.asciiMode
                                         : (engine_get_ascii_mode() == 1);
    const bool trad = m_useServerEngine ? m_remoteState.traditional
                                        : (engine_get_traditional() == 1);
    const bool shuangpin = m_useServerEngine ? m_appCfg.shuangpin_mode
                                             : (engine_get_shuangpin() == 1);
    const bool toolbarOn = taishen::CBannerWindow::Instance().IsEnabled();
    const std::wstring asciiItem = ascii ? L"切换到中文模式" : L"切换到英文模式";
    const std::wstring tradItem = trad ? L"关闭简繁转换" : L"开启简繁转换";
    const std::wstring spItem = shuangpin ? L"切换到全拼输入" : L"切换到双拼输入";
    const std::wstring tbItem = toolbarOn ? L"隐藏工具栏" : L"显示工具栏";
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_ASCII, asciiItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_TRAD, tradItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_SHUANGPIN, spItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_TOGGLE_TOOLBAR, tbItem.c_str());
    AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN_KEYS, L"快捷键设置…");
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
        // 0.1.47：写回 config.ini——原来只改本进程内存，切到别的程序打字就没了
        const bool next = !toolbarOn;
        taishen::CBannerWindow::Instance().SetEnabled(next);
        const std::wstring dllDir = GetDllDir();
        taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        cfg.toolbar_enabled = next;
        taishen::SaveConfig(dllDir, cfg);
        taishen::DebugLog("Tray: toolbar -> " + std::to_string(next));
    } else if (cmd == ID_TRAY_OPEN_KEYS) {
        // 快捷键设置：打开设置窗口的「按键」页（索引 6）。
        // 托盘回调跑在宿主进程 UI 线程，同步弹模态会与宿主冲突 → 独立线程
        const std::wstring dllDir = GetDllDir();
        std::thread([dllDir]() {
            try {
                taishen::ShowSettingsDialog(nullptr, dllDir, 6);
            } catch (const std::exception& e) {
                taishen::ForceLog(std::string("Settings dialog crashed: ") + e.what());
            } catch (...) {
                taishen::ForceLog("Settings dialog crashed: unknown exception");
            }
        }).detach();
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
static HICON GetTaishenIcon()
{
    static HICON s_icon = nullptr;
    if (s_icon != nullptr) {
        return s_icon;
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

    // 品牌图标固定「泰」：中英状态交给独立的状态图标（中/英）表达。
    // 早期版本会切成「t ENG」，字号与「泰」不一致、切换时视觉跳动，已移除。
    SetTextColor(memDC, RGB(232, 232, 232));
    HFONT font = CreateFontW(24, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                             DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                             CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY,
                             DEFAULT_PITCH, L"Microsoft YaHei");
    HGDIOBJ oldFont = SelectObject(memDC, font);
    DrawTextW(memDC, L"泰", -1, &rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    SelectObject(memDC, oldFont);
    DeleteObject(font);

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

    s_icon = icon;
    return icon;
}

/// explorer 重启广播消息（注册一次，进程内复用）
static UINT GetTaskbarCreatedMsg()
{
    static const UINT msg = RegisterWindowMessageW(L"TaskbarCreated");
    return msg;
}

/// 托盘命令跨进程广播消息（0.1.47）
/// 理由：托盘图标用固定 GUID 去重，Shell 只把点击消息发给**一个**进程的窗口。
/// 若用户此刻在别的程序里打字，直接执行会切错进程。因此广播出去，
/// 由「当前前台进程」执行，其余进程只同步图标显示。
static UINT GetTrayCmdMsg()
{
    static const UINT msg = RegisterWindowMessageW(L"TaishenIME.TrayCmd");
    return msg;
}

/// 0.1.67 起托盘已停用：保留空操作桩，避免热路径同步磁盘写与 HWND_BROADCAST
static void BroadcastTrayRefresh()
{
}

static void WriteTrayAsciiState(bool /*ascii*/)
{
}

static bool ReadTrayAsciiState(bool& /*ascii*/)
{
    return false;
}

/// 收到托盘命令广播：只有前台进程执行，其余进程同步图标
void CTextService::OnTrayCmdBroadcast(int cmd)
{
    // cmd=0：只要图标跟上状态（Shift / 键盘切换路径触发），不动任何引擎状态
    if (cmd == 0) {
        bool ascii = false;
        if ((m_trayAdded || m_modeTrayAdded) && ReadTrayAsciiState(ascii)) {
            UpdateTrayIconFor(ascii);
        }
        return;
    }

    HWND fg = GetForegroundWindow();
    DWORD fgPid = 0;
    if (fg != nullptr) {
        GetWindowThreadProcessId(fg, &fgPid);
    }
    const bool fgMine = (fgPid != 0 && fgPid == GetCurrentProcessId());

    if (fgMine) {
        switch (cmd) {
        case ID_TRAY_TOGGLE_ASCII:
            ToggleAsciiMode();   // 内部写状态文件 + 刷新图标
            break;
        case ID_TRAY_TOGGLE_TRAD:
            ToggleTraditional();
            break;
        case ID_TRAY_TOGGLE_SHUANGPIN:
            ToggleShuangpin();
            break;
        default:
            break;
        }
        return;
    }

    // 非前台进程：只让图标跟上状态，不动本进程引擎状态（per-app 记忆保持独立）
    bool ascii = false;
    if ((m_trayAdded || m_modeTrayAdded) && ReadTrayAsciiState(ascii)) {
        if ((ascii ? L"英" : L"中") != m_trayLabel) {
            UpdateTrayIconFor(ascii);
        }
    }
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
    // 托盘命令广播（来自任意进程的托盘点击）：前台进程执行
    if (msg == GetTrayCmdMsg()) {
        if (self != nullptr) {
            self->OnTrayCmdBroadcast(static_cast<int>(wParam));
        }
        return 0;
    }
    if (msg == WM_TRAYICON) {
        if (self != nullptr) {
            UINT event = 0;
            if (lParam == WM_LBUTTONUP || lParam == WM_RBUTTONUP || lParam == WM_CONTEXTMENU) {
                // 旧版 NOTIFYICONDATA：lParam 即事件
                event = static_cast<UINT>(lParam);
            } else {
                // NOTIFYICON_VERSION_4：LOWORD(lParam) 为事件，HIWORD(lParam) 为 uID
                event = LOWORD(lParam);
            }

            if (event == WM_LBUTTONUP || event == NIN_SELECT) {
                // 左键：切换中英
                self->ToggleAsciiMode();
            } else if (event == WM_RBUTTONUP || event == WM_CONTEXTMENU) {
                // 右键：弹出托盘快捷菜单
                self->ShowTrayMenu();
            }
        }
        return 0;
    }
    // 配置热加载轮询（对标 rime-ice 重新部署，2s 检查 config.ini mtime）
    if (msg == WM_TIMER && wParam == 1) {
        if (self != nullptr) {
            self->ReloadConfigIfChanged();
            self->LogDictLoadStatusOnce();
        }
        // 用户数据写入队列落盘（force=0，队列满 20 条或距上次写满 2 秒才真正写）
        // AppContainer 内用户词库不可写，helper 内部跳过
        FlushUserWritesIfWritable(0);
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
    const bool isContainer = taishen::IsAppContainerProcess();
    if (!isContainer) {
        // 0.1.68：切焦点时先查一次配置变更——跨进程同步不依赖 2s 轮询
        ReloadConfigIfChanged();
        LogDictLoadStatusOnce();
    }
    const DWORD tid = GetCurrentThreadId();
    if (fForeground) {
        if (!isContainer) {
            taishen::CBannerWindow::Instance().OnServiceFocus(tid);
            SetTrayVisible(true);
        }
    } else {
        if (!isContainer) {
            taishen::CBannerWindow::Instance().OnServiceLostFocus(tid);
            SetTrayVisible(false);
        }
        m_hasLastCaretRect = false;
        ++m_caretSeq;
        if (m_useServerEngine && (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose)) {
            if (m_pFocusContext != nullptr && m_composition.IsActive()) {
                RunCompositionOp(m_pFocusContext, CEditSessionComposition::Op::Commit, "");
            }
            SendRemoteResetCommit();
            ClearRemoteState();
        }
        HideCandidateUi();
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyDown(ITfContext* /*pic*/, WPARAM wParam,
                                         LPARAM lParam, BOOL* pfEaten)
{
    // 过滤输入法自身合成按键（SendLeftArrow 等），避免自产自销干扰按键状态
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    // 预测试：仅判断是否吞键——绝不修改引擎状态！
    const int effVk = taishen::NormalizeKeypad(static_cast<int>(wParam));
    const bool vimPass = taishen::AppStateIsVimForeground(m_appCfg);
    const bool eat = m_useServerEngine
                         ? taishen::ShouldEatKeyRemote(
                               effVk, vimPass, static_cast<int>(wParam),
                               m_remoteState,
                               m_renderClient.IsInHardBackoff() ||
                                   !m_renderClient.IsServerDictReady())
                         : taishen::ShouldEatKey(effVk, vimPass,
                                                 static_cast<int>(wParam));
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
    // 过滤输入法自身合成按键
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

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

void CTextService::ClearRemoteState()
{
    m_remoteState.ClearInput();
    m_pinyin.clear();
    m_candidates.clear();
    m_multiRowExpanded = false;
    if (m_candidateUiElement != nullptr) {
        m_candidateUiElement->MarkUpdated(
            TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING |
            TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE);
    }
}

void CTextService::SendRemoteResetCommit()
{
    taishen::ipc::CommitRequest req;
    req.sessionId = m_renderSessionId;
    req.hostPid = GetCurrentProcessId();
    m_renderClient.SendCommit(req);
}

void CTextService::SendRemoteCommitWithLog(const taishen::ipc::CommitRequest& cm)
{
    if (!m_renderClient.SendCommit(cm) && !cm.word.empty()) {
        taishen::ForceLog(
            "RemoteCommit FAILED (dropped user learning): pinyin=" + cm.pinyin +
            " word=" + cm.word + " index=" + std::to_string(cm.index));
    }
}

void CTextService::SyncRemoteModesFromPong()
{
    if (!taishen::IsAppContainerProcess() || m_remotePongSynced ||
        !m_renderClient.IsConnected()) {
        return;
    }
    const auto& pong = m_renderClient.LastPongInfo();
    if (!m_remoteTradOverride) {
        m_remoteState.traditional = pong.traditional;
        m_appCfg.traditional_enabled = pong.traditional;
    }
    if (!m_remoteShuangpinOverride) {
        m_appCfg.shuangpin_mode = pong.shuangpin;
    }
    m_remotePongSynced = true;
}

void CTextService::ApplyRemoteResponse(
    const taishen::ipc::CandidateListResponse& resp)
{
    m_remoteState.rawPinyin = resp.pinyin;
    m_remoteState.preedit = resp.preedit;
    m_remoteState.syllableDisplay = resp.syllable;
    m_remoteState.candidates = resp.candidates;
    m_remoteState.page = resp.page;
    m_remoteState.totalPages = resp.totalPages;
    m_remoteState.totalCandidates = resp.totalCandidates;
    m_remoteState.pageSize = (resp.pageSize > 0) ? resp.pageSize : 5;
    m_remoteState.selectedIndex = resp.selectedIndex;
    m_remoteState.inCompose = resp.inCompose;
    m_remoteState.composePrefix = resp.composePrefix;
    m_remoteState.inputMode = resp.inputMode;
    m_remoteState.isSymbolPrefix = resp.symbolPrefix;
    m_remoteState.semicolonKeyActive = resp.semicolonActive;

    m_pinyin = m_remoteState.preedit;
    m_candidates.clear();
    m_candidates.reserve(resp.candidates.size());
    for (const auto& item : resp.candidates) {
        m_candidates.push_back(item.text);
    }
    if (m_candidateUiElement != nullptr) {
        m_candidateUiElement->MarkUpdated(
            TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING |
            TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE);
    }
}

void CTextService::CommitRemoteText(ITfContext* pic,
                                    const std::wstring& wideText,
                                    int caretOffset)
{
    // 全角转换已在 tsf_composition.cpp 提交路径内统一执行，此处不重复调用 ToFullWidth。
    taishen::g_lastCommitEndsWithDigit = false;
    if (!wideText.empty()) {
        const wchar_t last = wideText.back();
        if (last >= L'0' && last <= L'9') {
            taishen::g_lastCommitEndsWithDigit = true;
        }
    }
    m_committedText = wideText;
    const std::string utf8 = taishen::WideToUtf8(wideText);
    RunCompositionOp(pic, CEditSessionComposition::Op::Commit, utf8, caretOffset);
}

HRESULT CTextService::HandleRemoteKeyDown(ITfContext* pic, WPARAM wParam,
                                          LPARAM /*lParam*/, BOOL* pfEaten)
{
    auto finish = [pfEaten](bool eaten) -> HRESULT {
        if (pfEaten != nullptr) {
            *pfEaten = eaten ? TRUE : FALSE;
        }
        return S_OK;
    };

    if (pic != nullptr && pic != m_pFocusContext) {
        pic->AddRef();
        if (m_pFocusContext != nullptr) {
            m_pFocusContext->Release();
        }
        m_pFocusContext = pic;
        taishen::DebugLog("OnKeyDown(remote): focus context synced from key event");
    }

    const bool isContainer = taishen::IsAppContainerProcess();
    if (!isContainer) {
        ReloadConfigIfChanged();
        taishen::CBannerWindow::Instance().EvaluateForeground();
    }

    const int rawVk = static_cast<int>(wParam);
    if (rawVk == VK_SHIFT || rawVk == VK_LSHIFT || rawVk == VK_RSHIFT) {
        if (!m_shiftActive) {
            m_shiftActive = true;
            m_shiftTapArmed = true;
            m_shiftComboUsed = false;
            m_shiftDownTick = GetTickCount();
        }
    } else {
        if (m_shiftActive) {
            m_shiftComboUsed = true;
        }
        m_shiftTapArmed = false;
    }

    taishen::AppStateApplyRemote(m_appCfg, m_remoteState.asciiMode);
    const bool inlinePreedit = taishen::AppStateInlinePreeditEnabled(m_appCfg);

    const int effVk = taishen::NormalizeKeypad(rawVk);
    const bool keypad = taishen::IsKeypadKey(rawVk);
    const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
    const bool shiftRaw = (GetKeyState(VK_SHIFT) & 0x8000) != 0;
    const bool shiftDown = shiftRaw || taishen::KeypadShiftEquivalent(rawVk);
    const bool capsLock = (GetKeyState(VK_CAPITAL) & 0x0001) != 0;
    const auto& kb = taishen::GetKeyBindings();

    if (taishen::AppStateIsVimForeground(m_appCfg) &&
        taishen::IsVimModeKey(rawVk)) {
        return finish(false);
    }

    if (rawVk == VK_SHIFT || rawVk == VK_LSHIFT || rawVk == VK_RSHIFT) {
        return finish(false);
    }

    // ── 本地快捷键（0 IPC 往返）──
    if (effVk == VK_SPACE && ctrlDown && kb.toggleAsciiCtrlSpace) {
        if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
            if (pic != nullptr && m_composition.IsActive()) {
                RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
            }
            SendRemoteResetCommit();
            ClearRemoteState();
        }
        m_remoteState.asciiMode = !m_remoteState.asciiMode;
        taishen::AppStateSetAsciiRemote(m_remoteState.asciiMode);
        RefreshLangBarItem();
        if (!isContainer) {
            UpdateTrayIcon();
            taishen::CBannerWindow::Instance().Refresh();
        }
        HideCandidateUi();
        m_shiftTapArmed = false;
        return finish(true);
    }

    if (effVk == VK_SPACE && shiftDown && !ctrlDown && kb.toggleFullwidth == 1) {
        taishen::SetFullWidthMode(!taishen::IsFullWidthMode());
        m_shiftTapArmed = false;
        return finish(true);
    }

    if (effVk == VK_OEM_PERIOD && ctrlDown && !shiftDown && kb.togglePunct == 1) {
        m_remoteState.asciiPunct = !m_remoteState.asciiPunct;
        return finish(true);
    }

    if (kb.toggleTraditionalKey != 0 && effVk == kb.toggleTraditionalKey &&
        ctrlDown && shiftDown) {
        ToggleTraditional();
        return finish(true);
    }

    if (effVk >= '1' && effVk <= '9' && ctrlDown && shiftDown) {
        return finish(true);
    }

    auto consumePrefixForPunct = [&](std::string& outPrefixUtf8) {
        outPrefixUtf8.clear();
        if (m_remoteState.inCompose) {
            outPrefixUtf8 = m_remoteState.composePrefix;
            SendRemoteResetCommit();
            ClearRemoteState();
        } else if (!m_remoteState.candidates.empty()) {
            outPrefixUtf8 = m_remoteState.candidates[0].text;
            const bool isPrefix =
                (m_remoteState.candidates[0].reasons & taishen::ipc::kReasonPrefixWord) != 0;
            taishen::ipc::CommitRequest cm;
            cm.sessionId = m_renderSessionId;
            cm.hostPid = GetCurrentProcessId();
            cm.pinyin = m_remoteState.rawPinyin;
            cm.word = outPrefixUtf8;
            cm.index = isPrefix ? -1 : 0;
            SendRemoteCommitWithLog(cm);
            ClearRemoteState();
        } else if (!m_remoteState.rawPinyin.empty()) {
            SendRemoteResetCommit();
            ClearRemoteState();
        }
    };

    auto tryHandleRemotePunct = [&]() -> bool {
        if (m_remoteState.asciiMode || m_remoteState.asciiPunct || ctrlDown || altDown) {
            return false;
        }
        if ((effVk == VK_OEM_COMMA || effVk == VK_OEM_PERIOD) && !shiftDown &&
            taishen::g_lastCommitEndsWithDigit) {
            return false;
        }
        if (effVk == VK_OEM_7) {
            std::string prefix;
            consumePrefixForPunct(prefix);
            std::wstring pair;
            int caretOffset = -1;
            if (taishen::IsPairPunctEnabled()) {
                taishen::ExpandPairPunct(shiftDown ? L"“" : L"‘", pair, caretOffset);
            } else if (shiftDown) {
                pair = m_dquoteOpen ? L"”" : L"“";
                m_dquoteOpen = !m_dquoteOpen;
            } else {
                pair = m_quoteOpen ? L"’" : L"‘";
                m_quoteOpen = !m_quoteOpen;
            }
            const std::wstring committed = taishen::Utf8ToWide(prefix) + pair;
            CommitRemoteText(pic, committed, caretOffset);
            HideCandidateUi();
            m_shiftTapArmed = false;
            return true;
        }
        const wchar_t* punct = taishen::MapFullWidthPunctForKey(effVk, shiftDown);
        if (punct != nullptr) {
            std::string prefix;
            consumePrefixForPunct(prefix);
            std::wstring pair;
            int caretOffset = -1;
            taishen::ExpandPairPunct(std::wstring(punct), pair, caretOffset);
            const std::wstring committed = taishen::Utf8ToWide(prefix) + pair;
            CommitRemoteText(pic, committed, caretOffset);
            HideCandidateUi();
            m_shiftTapArmed = false;
            return true;
        }
        return false;
    };

    // hard backoff 或服务端词库未就绪时且无活跃输入：直接透传，避免静默吞字或阻塞宿主 UI
    if ((m_renderClient.IsInHardBackoff() || !m_renderClient.IsServerDictReady()) &&
        m_remoteState.rawPinyin.empty() && !m_remoteState.inCompose) {
        return finish(false);
    }

    // ── 需优先于标点语义送往 server 的按键分类 ──
    bool routeToServer = false;
    bool isPaginationKey = false;
    bool collapseMultiRow = false;

    // 1) 组词态 Enter / Backspace
    if (m_remoteState.inCompose &&
        (effVk == VK_RETURN || (effVk == VK_BACK && !ctrlDown))) {
        routeToServer = true;
    }

    // 2) 特殊模式（计算器 c / 数字大写 R / Unicode U）
    if (!routeToServer &&
        (m_remoteState.inputMode == 1 || m_remoteState.inputMode == 2 ||
         m_remoteState.inputMode == 3)) {
        const bool isSpecialDigit = (!shiftRaw || effVk == '8' || effVk == '9' || effVk == '0') &&
                                    (effVk >= '0' && effVk <= '9');
        const bool isSpecialCalcOp =
            (m_remoteState.inputMode == 1 || m_remoteState.inputMode == 2) &&
            ((keypad && (rawVk == VK_ADD || rawVk == VK_SUBTRACT ||
                         rawVk == VK_MULTIPLY || rawVk == VK_DIVIDE ||
                         rawVk == VK_DECIMAL || rawVk == VK_SEPARATOR)) ||
             (!shiftRaw && (effVk == VK_OEM_PERIOD || effVk == VK_OEM_PLUS ||
                            effVk == VK_OEM_MINUS || effVk == VK_OEM_2 ||
                            effVk == VK_OEM_5)) ||
             (shiftRaw && effVk == '6'));
        if (isSpecialDigit || isSpecialCalcOp) {
            routeToServer = true;
        }
    }

    // 3) 翻页键（优先于标点）
    if (!routeToServer && !m_remoteState.candidates.empty()) {
        const bool plusMinus =
            (effVk == VK_OEM_PLUS || effVk == VK_OEM_MINUS) &&
            (keypad ? kb.pageNumpad : kb.pageMinusEqual);
        const bool commaPeriod =
            (effVk == VK_OEM_COMMA || effVk == VK_OEM_PERIOD) &&
            !keypad && !shiftDown && kb.pageCommaPeriod;
        if (effVk == VK_PRIOR || effVk == VK_NEXT || plusMinus || commaPeriod) {
            routeToServer = true;
            isPaginationKey = true;
        }
    }

    // 4) 以词定字（[ / ]）
    if (!routeToServer && (effVk == VK_OEM_4 || effVk == VK_OEM_6) && !shiftDown &&
        kb.pageBrackets && !m_remoteState.candidates.empty()) {
        routeToServer = true;
    }

    // 5) 双拼分号编码键
    if (!routeToServer && effVk == VK_OEM_1 && !shiftRaw &&
        m_remoteState.semicolonKeyActive) {
        routeToServer = true;
        collapseMultiRow = true;
    }

    if (!routeToServer) {
        // 6) 本地全角标点与配对符号
        if (tryHandleRemotePunct()) {
            return finish(true);
        }

        // 7) 字母 A-Z
        if (effVk >= 'A' && effVk <= 'Z') {
            if (ctrlDown || altDown || m_remoteState.asciiMode) {
                return finish(false);
            }
            if (capsLock || shiftRaw) {
                std::string prefix;
                consumePrefixForPunct(prefix);
                const wchar_t ch = static_cast<wchar_t>(
                    (capsLock && shiftRaw) ? (effVk + ('a' - 'A')) : effVk);
                std::wstring committed = taishen::Utf8ToWide(prefix);
                committed.push_back(ch);
                CommitRemoteText(pic, committed, -1);
                HideCandidateUi();
                m_shiftTapArmed = false;
                return finish(true);
            }
            routeToServer = true;
            collapseMultiRow = true;
        } else if (effVk == VK_UP || effVk == VK_DOWN) {
            if (m_remoteState.candidates.empty()) {
                return finish(false);
            }
            m_multiRowExpanded = (effVk == VK_DOWN);
            if (!UseRemoteRender()) {
                m_candidateWindow.SetMultiRow(m_multiRowExpanded);
            }
            UpdateCandidateWindow(pic);
            return finish(true);
        } else if (effVk == VK_ESCAPE) {
            if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                if (pic != nullptr && m_composition.IsActive()) {
                    RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
                }
                SendRemoteResetCommit();
                ClearRemoteState();
                HideCandidateUi();
            }
            return finish(false);
        } else if (effVk == VK_RETURN) {
            if (!m_remoteState.rawPinyin.empty()) {
                const std::wstring pyWide = taishen::Utf8ToWide(m_remoteState.rawPinyin);
                CommitRemoteText(pic, pyWide, -1);
                SendRemoteResetCommit();
                ClearRemoteState();
                HideCandidateUi();
                m_shiftTapArmed = false;
                return finish(true);
            }
            return finish(false);
        } else if (effVk == VK_SPACE) {
            if (m_remoteState.candidates.empty()) {
                return finish(false);
            }
            const bool canFastSelect =
                !m_remoteState.inCompose && m_remoteState.inputMode == 0 &&
                !m_remoteState.isSymbolPrefix &&
                ((m_remoteState.candidates[0].reasons & taishen::ipc::kReasonPrefixWord) == 0);
            if (canFastSelect) {
                const std::string chosen = m_remoteState.candidates[0].text;
                const std::string rawPy = m_remoteState.rawPinyin;
                CommitRemoteText(pic, taishen::Utf8ToWide(chosen), -1);
                taishen::ipc::CommitRequest cm;
                cm.sessionId = m_renderSessionId;
                cm.hostPid = GetCurrentProcessId();
                cm.pinyin = rawPy;
                cm.word = chosen;
                cm.index = 0;
                SendRemoteCommitWithLog(cm);
                ClearRemoteState();
                HideCandidateUi();
                m_shiftTapArmed = false;
                return finish(true);
            }
            routeToServer = true;
        } else if (!keypad && effVk >= '1' && effVk <= '9') {
            if (m_remoteState.isSymbolPrefix) {
                routeToServer = true;
            } else {
                const size_t idx = static_cast<size_t>(effVk - '1');
                if (idx < m_remoteState.candidates.size()) {
                    const bool canFastSelect =
                        !m_remoteState.inCompose && m_remoteState.inputMode == 0 &&
                        ((m_remoteState.candidates[idx].reasons &
                          taishen::ipc::kReasonPrefixWord) == 0);
                    if (canFastSelect) {
                        const std::string chosen = m_remoteState.candidates[idx].text;
                        const std::string rawPy = m_remoteState.rawPinyin;
                        CommitRemoteText(pic, taishen::Utf8ToWide(chosen), -1);
                        taishen::ipc::CommitRequest cm;
                        cm.sessionId = m_renderSessionId;
                        cm.hostPid = GetCurrentProcessId();
                        cm.pinyin = rawPy;
                        cm.word = chosen;
                        cm.index = static_cast<int32_t>(idx);
                        SendRemoteCommitWithLog(cm);
                        ClearRemoteState();
                        HideCandidateUi();
                        m_shiftTapArmed = false;
                        return finish(true);
                    }
                    routeToServer = true;
                } else if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                    return finish(true);
                } else {
                    return finish(false);
                }
            }
        } else if (effVk >= '0' && effVk <= '9') {
            if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                return finish(true);
            }
            return finish(false);
        } else if (effVk == VK_BACK) {
            if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                routeToServer = true;
                collapseMultiRow = true;
            } else {
                return finish(false);
            }
        } else if (effVk == VK_TAB) {
            if (!m_remoteState.rawPinyin.empty()) {
                routeToServer = true;
                collapseMultiRow = true;
            } else {
                return finish(false);
            }
        } else if (effVk == VK_DELETE && ctrlDown) {
            if (!m_remoteState.candidates.empty()) {
                routeToServer = true;
                collapseMultiRow = true;
            } else {
                return finish(false);
            }
        } else {
            return finish(false);
        }
    }

    if (!routeToServer) {
        return finish(false);
    }

    // ── 同步 SendKeyInput（50ms 严格上限，超时立即透传）──
    taishen::ipc::KeyInputRequest req;
    req.seq = ++m_ipcKeySeq;
    req.sessionId = m_renderSessionId;
    req.hostPid = GetCurrentProcessId();
    req.vk = static_cast<uint32_t>(effVk);
    req.rawVk = static_cast<uint32_t>(rawVk);
    req.modifiers = (shiftRaw ? taishen::ipc::kModShift : 0u) |
                    (ctrlDown ? taishen::ipc::kModCtrl : 0u) |
                    (altDown ? taishen::ipc::kModAlt : 0u) |
                    (capsLock ? taishen::ipc::kModCapsLock : 0u);
    req.asciiMode = m_remoteState.asciiMode;
    req.fresh = m_remoteState.rawPinyin.empty() && !m_remoteState.inCompose;
    req.flags = 0;
    if (!isContainer || m_remoteTradOverride) {
        req.flags |= taishen::ipc::kFlagHasTraditional;
        if (m_remoteState.traditional) {
            req.flags |= taishen::ipc::kFlagTraditional;
        }
    }
    if (!isContainer || m_remoteShuangpinOverride) {
        req.flags |= taishen::ipc::kFlagHasShuangpin;
        if (m_appCfg.shuangpin_mode) {
            req.flags |= taishen::ipc::kFlagShuangpin;
        }
    }

    LARGE_INTEGER qpcFreq{}, qpcStart{}, qpcEnd{};
    QueryPerformanceFrequency(&qpcFreq);
    QueryPerformanceCounter(&qpcStart);
    taishen::ipc::CandidateListResponse resp;
    const bool ipcOk = m_renderClient.SendKeyInput(req, resp, 50);
    QueryPerformanceCounter(&qpcEnd);
    const double elapsedMs =
        (qpcFreq.QuadPart > 0)
            ? (static_cast<double>(qpcEnd.QuadPart - qpcStart.QuadPart) * 1000.0 /
               static_cast<double>(qpcFreq.QuadPart))
            : 0.0;

    if (!ipcOk) {
        taishen::ForceLog("RemoteKeyInput FAILED/TIMEOUT vk=" + std::to_string(rawVk) +
                          " fresh=" + (req.fresh ? "1" : "0") +
                          " ms=" + std::to_string(static_cast<int>(elapsedMs + 0.5)) +
                          " -> passthrough");
        if (!m_remoteState.rawPinyin.empty() && pic != nullptr &&
            m_composition.IsActive()) {
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             m_remoteState.rawPinyin);
        }
        ClearRemoteState();
        HideCandidateUi();
        return finish(false);
    }
    SyncRemoteModesFromPong();

    const std::string top1 =
        resp.candidates.empty() ? "-" : resp.candidates[0].text;
    const std::string logLine =
        "RemoteKeyInput: vk=" + std::to_string(rawVk) +
        " fresh=" + (req.fresh ? "1" : "0") +
        " eaten=" + (resp.eaten ? "1" : "0") +
        " ms=" + std::to_string(elapsedMs).substr(0, 5) +
        " cands=" + std::to_string(resp.candidates.size()) +
        " top1=" + top1;
    taishen::DebugLog(logLine);
    if (isContainer && (req.fresh || elapsedMs > 20.0)) {
        taishen::ForceLog(logLine);
    }

    if (isPaginationKey && !resp.eaten) {
        if (tryHandleRemotePunct()) {
            return finish(true);
        }
        return finish(false);
    }

    if (!resp.eaten) {
        return finish(false);
    }

    if (collapseMultiRow) {
        m_multiRowExpanded = false;
        if (!UseRemoteRender()) {
            m_candidateWindow.SetMultiRow(false);
        }
    }

    ApplyRemoteResponse(resp);

    if (!resp.committed.empty() && !resp.inCompose) {
        CommitRemoteText(pic, taishen::Utf8ToWide(resp.committed), -1);
        ClearRemoteState();
        HideCandidateUi();
        m_shiftTapArmed = false;
        return finish(true);
    }

    if (!m_pinyin.empty() || m_remoteState.inCompose) {
        if (inlinePreedit) {
            SyncComposition(pic, "");
        } else if (m_composition.IsActive()) {
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
        }
        UpdateCandidateWindow(pic);
        return finish(true);
    }

    if (pic != nullptr && m_composition.IsActive()) {
        RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
    }
    HideCandidateUi();
    return finish(true);
}

STDMETHODIMP CTextService::OnKeyDown(ITfContext* pic, WPARAM wParam,
                                     LPARAM lParam, BOOL* pfEaten)
{
    // 过滤输入法自身合成按键（SendLeftArrow 等）
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    if (m_useServerEngine) {
        return HandleRemoteKeyDown(pic, wParam, lParam, pfEaten);
    }

    // 切换窗口后的首次按键：OnSetFocus(ITfDocumentMgr) 可能尚未触发，
    // ActivateEx 里 GetFocus 也可能返回 S_FALSE（实测：切到新进程后第一次
    // 打字，m_pFocusContext 为 null → 候选窗取不到光标而落到鼠标位置）。
    // 按键回调带的 pic 一定是当前上下文，用它把焦点上下文校正过来。
    if (pic != nullptr && pic != m_pFocusContext) {
        pic->AddRef();
        if (m_pFocusContext != nullptr) {
            m_pFocusContext->Release();
        }
        m_pFocusContext = pic;
        taishen::DebugLog("OnKeyDown: focus context synced from key event");
    }

    // 0.2.34 fix 配置热加载：托盘窗口无消息循环 → WM_TIMER 永不触发 →
    // ReloadConfigIfChanged 从未执行（保存配置后不生效）。改为按键时检查
    // mtime（GetFileAttributesEx 微秒级开销），配置保存后下一次按键即生效。
    ReloadConfigIfChanged();
    // 用户数据落盘兜底：2s 定时器挂在托盘窗口，依赖宿主线程的消息循环；
    // 宿主不 pump 时不会触发，按键时补一次检查（内部有 20 条/2 秒阈值，命中才真写）
    FlushUserWritesIfWritable(0);
    // 词库异步加载完成后补记一次结果（诊断：容器内 ACL 是否放行）
    LogDictLoadStatusOnce();

    // 0.2.26 Shift tap 中英切换：记录 Shift 按下；其他键按下取消 tap 候选
    // （Shift+字母/符号是组合键，不得触发切换）
    // fix：TSF 传递的 Shift 虚拟键是 VK_SHIFT(16) 而非 VK_LSHIFT/RSHIFT(0xA0/0xA1)，
    // 之前只匹配后两者 → tap 永不 armed → 切换失效。三值都要匹配。
    if (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) {
        // 只在 Shift 首次按下时重置判定：按住期间系统会重复投递 Shift keydown，
        // 若重复也重置，会让「Shift+字母后继续按住再松开」把 combo 标记清掉，
        // keyup 时 dur 又从最后一次重复起算 → 误切中英。
        if (!m_shiftActive) {
            m_shiftActive = true;
            m_shiftTapArmed = true;
            m_shiftComboUsed = false;
            m_shiftDownTick = GetTickCount();
        }
    } else {
        // Shift 按住期间出现其它键 → 组合键（Shift+符号/字母），keyup 不切中英。
        if (m_shiftActive) {
            m_shiftComboUsed = true;
        }
        m_shiftTapArmed = false;
    }

    // 0.1.49：Ctrl+Shift+数字 旧快捷键已移除——中英标点改为 Ctrl+.、
    // 简繁改为 Ctrl+Shift+<字母>（可在设置页「按键」配置），统一由
    // tsf_keyevent::HandleKeyDown 处理

    // 0.3.x：工具栏兜底评估——SetWinEventHook 前台回调可能因
    // 注册线程无消息泵/hook 失效而不触发，用户打字时主动重评估，
    // 工具栏"莫名其妙消失"后恢复显示（成本：一次 GetForegroundWindow）
    taishen::CBannerWindow::Instance().EvaluateForeground();

    // V0.2.33 兜底：OnSetFocus 可能不触发（B-4 教训），按键前检测前台进程变化，
    // 应用目标进程的初始/记忆状态。AppStateApply 内部幂等（进程未变化直接返回）。
    taishen::AppStateApply(m_appCfg);
    // V0.6.x 行内预编辑：宿主不渲染 TSF 组合属性时（魔兽世界等），
    // 组合串里的拼音会被当正文插入。命中时不写组合串，拼音只走候选窗。
    const bool inlinePreedit = taishen::AppStateInlinePreeditEnabled(m_appCfg);

    // V0.6.x 组词中间态：回车/退格由组词状态接管。
    // 不接管的话，回车会把完整拼音当原文上屏（已选字 + 拼音一起输出），
    // 退格会退出组词但已选字不回退，下一次提交变成「已选字 + 拼音」。
    if (wParam == VK_RETURN && engine_in_compose() == 1) {
        // 组词中途回车：只提交已选的词，剩余拼音丢弃（引擎是唯一状态源）
        std::string prefix;
        taishen::ReadEngineComposePrefix(prefix);
        if (!prefix.empty()) {
            engine_reset();
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit, prefix);
            RefreshState();
            HideCandidateUi();
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
    }
    if (wParam == VK_BACK && engine_in_compose() == 1) {
        std::string prefix;
        taishen::ReadEngineComposePrefix(prefix);
        // 有已选词 → 撤销上一个词（引擎内部回退 compose_idx 与 compose_chars）。
        // 没有已选词 → 不接管，交回 HandleKeyDown 删拼音字母。
        if (!prefix.empty()) {
            std::string undone;
            taishen::ReadEngineComposeUndo(undone);
            taishen::DebugLog("ComposeBackspace: prefix=" + prefix + " undone=" + undone);
            // 显示完全由引擎状态推导（BuildCompositionText），平台不再维护副本，
            // 杜绝「引擎已回退、平台还留着旧字」造成的重复上屏（B-22）。
            SyncComposition(pic, "");
            RefreshState();
            UpdateCandidateWindow(pic);
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
    }
    // V0.2.36 vim_mode：vim 键（Esc/Ctrl+C/Ctrl+[）由 OnTestKeyDown 透传、
    // OnKeyUp 切英文（透传键 OnKeyDown 不执行，见 OnTestKeyDown/OnKeyUp）

    taishen::KeyEventResult result;
    // P2-4：小键盘键先归一为主键盘等价键（候选选择/计算器/数字模式自动支持）
    const int effVk = taishen::NormalizeKeypad(static_cast<int>(wParam));
    // 0.1.48：记录按键前的中英状态，处理完若变了就同步托盘图标——
    // 覆盖 Ctrl+Space 这类不经过 ToggleAsciiMode 的切换路径
    const int asciiBeforeKey = engine_get_ascii_mode();
    const bool eat = taishen::HandleKeyDown(effVk, lParam, result,
                                            static_cast<int>(wParam));
    taishen::DebugLog("OnKeyDown vk=" + std::to_string(wParam) +
                      " eff=" + std::to_string(effVk) +
                      " eat=" + (eat ? "T" : "F") +
                      " ascii=" + std::to_string(engine_get_ascii_mode()) +
                      " committed=" + (result.committed.empty() ? "-" :
                          taishen::WideToUtf8(result.committed)));

    if (engine_get_ascii_mode() != asciiBeforeKey) {
        WriteTrayAsciiState(engine_get_ascii_mode() == 1);
        UpdateTrayIcon();
        BroadcastTrayRefresh();
    }

    if (eat) {
        // 0.2.28 配对引号：' 单引号（‘’）/ " 双引号（“”），开闭交替上屏
        // 0.2.28 配对引号：' 单引号 / " 双引号。
        // V0.4.x：改为成对上屏（‘’ / “” + 光标居中），对齐主流输入法；
        // 开关关闭时退化为交替开闭（原行为）。
        if (result.punct_quote == 1) {
            // 组词态（B-22）：tsf_keyevent 已把已选前缀放进 committed。
            // 注意 ExpandPairPunct 内部第一行就是 committed = sel，不能直接传进去，
            // 否则前缀被整串覆盖（审计 P1）。先拿引号本身再追加。
            std::wstring pair;
            int caretOffset = -1;
            if (taishen::IsPairPunctEnabled()) {
                taishen::ExpandPairPunct(L"‘", pair, caretOffset);
            } else {
                pair = m_quoteOpen ? L"’" : L"‘";
                m_quoteOpen = !m_quoteOpen;
            }
            const std::wstring committed = result.committed + pair;
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(committed), caretOffset);
            RefreshState();
            HideCandidateUi();
            m_shiftTapArmed = false;
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
        if (result.punct_quote == 2) {
            // 同上：保留组词已选前缀
            std::wstring pair;
            int caretOffset = -1;
            if (taishen::IsPairPunctEnabled()) {
                taishen::ExpandPairPunct(L"“", pair, caretOffset);
            } else {
                pair = m_dquoteOpen ? L"”" : L"“";
                m_dquoteOpen = !m_dquoteOpen;
            }
            const std::wstring committed = result.committed + pair;
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             taishen::WideToUtf8(committed), caretOffset);
            RefreshState();
            HideCandidateUi();
            m_shiftTapArmed = false;
            if (pfEaten != nullptr) { *pfEaten = TRUE; }
            return S_OK;
        }
        // 0.2.28 标点复选：记录候选列表（随后 UpdateCandidateWindow 渲染）
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
            if (!m_pinyin.empty() && inlinePreedit) {
                // V0.5.11 组词中间音节选字（inCompose && 有提交文本）：
                // 不在本分支 Update composition，交给下方 committed 分支
                // 走「Commit 已选字 + Start 剩余音节」——否则剩余音节会被
                // Commit 覆盖导致消失（键盘选字与鼠标点选顺序不一致的 bug）。
                const bool composeMidCommit =
                    (engine_in_compose() == 1 && !result.committed.empty());
                if (!composeMidCommit) {
                    // 统一入口：组词态自动带已选前缀，普通输入显示音节分隔串
                    // （zhongguo → zhong'guo，贴近微软）。
                    SyncComposition(pic, "");
                }
            } else if (m_composition.IsActive() && result.committed.empty()) {
                // 拼音清空（单字母退格删完等）且本次无提交文本：结束组合，
                // 否则 composition 文本残留编辑区——候选窗已关但编辑区
                // 还显示旧拼音（退格删单字母后"字母还在"）。
                RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
            }
            // 刷新候选窗口
            UpdateCandidateWindow(pic);
        }
        if (!result.committed.empty()) {
            // V0.6.x 组词中间音节：不 Commit，只把已选字累积进组合串，
            // 与剩余音节一起 Update：已选词留在组合串里，整段选完才一次性上屏
            if (engine_in_compose() == 1) {
                // 引擎已把本次选中词记进 compose_chars，这里只重推组合串
                SyncComposition(pic, "");
                m_shiftTapArmed = false;
                RefreshState();
                UpdateCandidateWindow(pic);
                if (pfEaten != nullptr) { *pfEaten = TRUE; }
                return S_OK;
            }
            // P1-2 数字分隔符状态：记录本次提交是否以数字结尾（供 , . 直通半角）
            taishen::g_lastCommitEndsWithDigit = false;
            if (!result.committed.empty()) {
                const wchar_t last = result.committed.back();
                if (last >= L'0' && last <= L'9') {
                    taishen::g_lastCommitEndsWithDigit = true;
                }
            }
            // 选词上屏：组合替换为汉字并结束
            // V0.4.x：配对符号成对时携带光标偏移（开符号后居中），
            // CommitComposition 内部用克隆 range 定位光标
            // 最终提交 = 组词累积的已选字 + 本次选中的字（一次性落文档）
            // 组词最后一个音节时引擎返回整词；普通输入就是本次选中的词。
            // 平台不再拼前缀——引擎 reset 前已把完整词一次性返回。
            std::string commitText = taishen::WideToUtf8(result.committed);
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             commitText, result.caret_offset);
            m_committedText = result.committed;
            m_shiftTapArmed = false;
            // 0.1.15 修复：提交后必须 RefreshState 同步清空 C++ 侧
            // m_pinyin/m_candidates（引擎 select_candidate 已 reset）。
            // 不同步会导致：退格被吞（ShouldEatKey 误判）/ 候选窗口
            // 残留旧候选（"不只是退格键的问题"——提交后所有按键行为错乱）
            RefreshState();
            // V0.6.x：组词中间已 return，走到这里必是普通提交或组词收尾 →
            // 组合已由 Commit 结束，候选窗隐藏
            HideCandidateUi();
        }
    } else if (!m_pinyin.empty() && wParam == VK_ESCAPE) {
        // ESC 取消当前输入：结束组合（空文本=撤销）
        RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
        engine_reset();
        RefreshState();
        HideCandidateUi();
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
        HideCandidateUi();
    }

    if (pfEaten != nullptr) {
        *pfEaten = eat ? TRUE : FALSE;
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnKeyUp(ITfContext* pic, WPARAM wParam,
                                   LPARAM /*lParam*/, BOOL* pfEaten)
{
    // 过滤输入法自身合成按键
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    // 0.2.26 Shift tap 中英切换（主流输入法标准）：
    // Shift 快速按下-松开（<500ms、期间无其他键、无 Ctrl/Alt 组合）→ 切换。
    // 保留 Ctrl+Space 备选（HandleKeyDown）。
    // fix：匹配 VK_SHIFT(16)（TSF 实际传递的虚拟键）。
    if (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) {
        const bool armed = m_shiftTapArmed;
        const bool combo = m_shiftComboUsed;
        const DWORD dur = GetTickCount() - m_shiftDownTick;
        m_shiftTapArmed = false;
        m_shiftComboUsed = false;
        m_shiftActive = false;
        const bool curAscii = m_useServerEngine
                                  ? m_remoteState.asciiMode
                                  : (engine_get_ascii_mode() == 1);
        // 诊断日志：每次 Shift 松开都留一条，排查「按了不切」时看 armed/combo/dur
        taishen::DebugLog(
            "Shift keyup: armed=" + std::to_string(armed) +
            " combo=" + std::to_string(combo) +
            " dur=" + std::to_string(dur) +
            " bind=" + std::to_string(taishen::GetKeyBindings().toggleAsciiShift) +
            " ascii=" + std::to_string(curAscii ? 1 : 0));
        if (armed && !combo) {
            const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
            const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
            if (dur < 500 && !ctrlDown && !altDown &&
                taishen::GetKeyBindings().toggleAsciiShift) {
                const bool isContainer = taishen::IsAppContainerProcess();
                if (m_useServerEngine) {
                    const bool hasPinyin =
                        !m_remoteState.rawPinyin.empty() || m_remoteState.inCompose;
                    if (hasPinyin) {
                        const std::string pinyin = m_remoteState.rawPinyin;
                        SendRemoteResetCommit();
                        ClearRemoteState();
                        m_remoteState.asciiMode = true;
                        taishen::AppStateSetAsciiRemote(true);
                        if (pic != nullptr && !pinyin.empty()) {
                            CommitRemoteText(pic, taishen::Utf8ToWide(pinyin), -1);
                        }
                        HideCandidateUi();
                    } else {
                        m_remoteState.asciiMode = !m_remoteState.asciiMode;
                        taishen::AppStateSetAsciiRemote(m_remoteState.asciiMode);
                        HideCandidateUi();
                    }
                    RefreshLangBarItem();
                    if (!isContainer) {
                        taishen::CBannerWindow::Instance().Refresh();
                    }
                } else {
                    const bool hasPinyin = engine_get_pinyin_str(nullptr, 0) > 1;
                    if (hasPinyin) {
                        std::string pinyin;
                        taishen::ReadEnginePinyin(pinyin);
                        engine_reset();
                        taishen::AppStateSetAscii(true);
                        if (pic != nullptr && !pinyin.empty()) {
                            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                                             pinyin, -1);
                            m_committedText = taishen::Utf8ToWide(pinyin);
                            const wchar_t last =
                                m_committedText.empty() ? 0 : m_committedText.back();
                            taishen::g_lastCommitEndsWithDigit =
                                (last >= L'0' && last <= L'9');
                        }
                        taishen::DebugLog("Shift tap: commit pinyin as ascii: " +
                                          pinyin);
                        RefreshState();
                        HideCandidateUi();
                    } else {
                        const int cur = engine_get_ascii_mode();
                        taishen::AppStateSetAscii(cur ? false : true);
                        taishen::DebugLog("Shift tap: ascii_mode -> " +
                                          std::to_string(engine_get_ascii_mode()));
                        RefreshState();
                        UpdateCandidateWindow(pic);
                    }
                    WriteTrayAsciiState(engine_get_ascii_mode() == 1);
                    UpdateTrayIcon();
                    BroadcastTrayRefresh();
                    RefreshLangBarItem();
                    if (!isContainer) {
                        taishen::CBannerWindow::Instance().Refresh();
                    }
                }
            }
        }
        m_shiftTapArmed = false;
    }

    // V0.2.36 vim_mode：vim 键（Esc/Ctrl+C/Ctrl+[）keyup 到达后切换为英文。
    if (taishen::AppStateIsVimForeground(m_appCfg) &&
        taishen::IsVimModeKey(static_cast<int>(wParam))) {
        if (m_useServerEngine) {
            if (!m_remoteState.asciiMode) {
                m_remoteState.asciiMode = true;
                taishen::AppStateSetAsciiRemote(true);
                if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                    SendRemoteResetCommit();
                    ClearRemoteState();
                }
                HideCandidateUi();
                RefreshLangBarItem();
                if (!taishen::IsAppContainerProcess()) {
                    taishen::CBannerWindow::Instance().Refresh();
                }
            }
        } else if (engine_get_ascii_mode() == 0) {
            taishen::AppStateSetAscii(true);
            taishen::DebugLog("vim_mode: Esc/<C-c>/<C-[> -> ascii");
            RefreshState();
            HideCandidateUi();
            UpdateTrayIcon();
            RefreshLangBarItem();
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
    // V0.2.33 per-app 状态记忆：焦点切换 → 应用目标进程的初始/记忆状态
    if (m_useServerEngine) {
        taishen::AppStateApplyRemote(m_appCfg, m_remoteState.asciiMode);
    } else {
        taishen::AppStateApply(m_appCfg);
    }
    if (!taishen::IsAppContainerProcess()) {
        // V0.6.x：行内拼音与文档组合串必须同源——文档不写拼音（游戏宿主）时
        // 候选窗必须画拼音行，否则用户看不到自己打了什么
        m_candidateWindow.SetInlinePreedit(
            taishen::AppStateInlinePreeditEnabled(m_appCfg) &&
            taishen::g_inlineCompositionReadable);
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
// ---------------------------------------------------------------------------
// 组合串拼装与推送（V0.6.x 统一入口）
// ---------------------------------------------------------------------------

/// 组合串唯一拼装点。
///
/// 组词态的组合串 = 引擎已选汉字（engine_compose_prefix）+ 待选音节（pending，带分隔）。
/// 此前拼装散落在 6 处，且已选前缀由平台层自己维护副本——引擎回退时副本不同步，
/// 造成「已选字消失、重新选字后又出现、最终提交重复」。现在前缀只存在引擎里，
/// 平台每次显示都重新读，任何一处组合串都必然与引擎状态一致。
std::string CTextService::BuildCompositionText(const std::string& pending) const
{
    if (m_useServerEngine) {
        return m_remoteState.inCompose ? (m_remoteState.composePrefix + pending) : pending;
    }
    if (engine_in_compose() == 1) {
        std::string prefix;
        taishen::ReadEngineComposePrefix(prefix);
        return prefix + pending;
    }
    return pending;
}

/// 统一把组合态推给宿主。
/// pending 为空 → 自动读引擎音节串（组词态是剩余音节，普通输入是拼音分隔串）。
/// 组合已激活走 Update，否则 Start。
void CTextService::SyncComposition(ITfContext* pic, const std::string& pending,
                                   bool forceStart)
{
    if (pic == nullptr) {
        return;
    }
    const bool inCompose =
        m_useServerEngine ? m_remoteState.inCompose : (engine_in_compose() == 1);
    std::string tail = pending;
    if (tail.empty()) {
        if (m_useServerEngine) {
            tail = m_remoteState.syllableDisplay;
        } else {
            std::string syllable;
            if (taishen::ReadEngineSyllableDisplay(syllable)) {
                tail = syllable;
            }
        }
    }
    const std::string text = BuildCompositionText(tail);
    const CEditSessionComposition::Op op =
        (!forceStart && m_composition.IsActive())
            ? CEditSessionComposition::Op::Update
            : CEditSessionComposition::Op::Start;
    // 已选前缀的宽字符数（组词态分段着色用）：
    // 前缀是 UTF-8，组合串位置按宽字符算，必须换算后再交给 TSF。
    LONG convertedChars = 0;
    if (inCompose) {
        std::string prefix;
        bool hasPrefix = false;
        if (m_useServerEngine) {
            prefix = m_remoteState.composePrefix;
            hasPrefix = !prefix.empty();
        } else {
            hasPrefix = taishen::ReadEngineComposePrefix(prefix) && !prefix.empty();
        }
        if (hasPrefix) {
            convertedChars = static_cast<LONG>(MultiByteToWideChar(
                CP_UTF8, 0, prefix.c_str(), static_cast<int>(prefix.size()),
                nullptr, 0));
        }
    }
    // V0.6.x 诊断：组合串每次推送都落日志（含是否组词 + 全文），
    // 便于按文本核对「已选前缀有没有带上」。
    taishen::DebugLog(std::string("SyncComposition: compose=") +
                      (inCompose ? "Y" : "N") +
                      " op=" + (op == CEditSessionComposition::Op::Start ? "Start" : "Update") +
                      " converted=" + std::to_string(convertedChars) +
                      " text=" + text);
    RunCompositionOp(pic, op, text, -1, convertedChars, forceStart);
}

void CTextService::RefreshState()
{
    // 拉取拼音串：先查询所需容量，避免长文本读取固定栈缓冲区。
    std::string pinyin;
    if (taishen::ReadEngineUtf8(
            [](char* buffer, int capacity) {
                // 双拼模式返回还原后的全拼音节（vs → zhong），全拼模式原样
                return engine_get_preedit_str(buffer, capacity);
            },
            pinyin)) {
        m_pinyin = std::move(pinyin);
    } else {
        m_pinyin.clear();
    }

    // 拉取候选词（引擎已按 candidate_limit 配置截断）。
    m_candidates.clear();
    const int count = engine_get_candidate_count();
    for (int i = 0; i < count; ++i) {
        std::string candidate;
        if (taishen::ReadEngineCandidate(i, candidate)) {
            m_candidates.push_back(std::move(candidate));
        }
    }
    if (m_candidateUiElement != nullptr) {
        m_candidateUiElement->MarkUpdated(
            TF_CLUIE_COUNT | TF_CLUIE_SELECTION | TF_CLUIE_STRING |
            TF_CLUIE_PAGEINDEX | TF_CLUIE_CURRENTPAGE);
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
    const bool isContainer = taishen::IsAppContainerProcess();
    // V0.2.33：保存配置快照供 OnSetFocus 的 AppStateApply 使用
    m_appCfg = cfg;
    if (!isContainer) {
        // 0.1.68：注册一次「配置变更 → 刷新工具栏」回调，设置页保存后立即触发。
        // 无捕获 lambda 可退化为函数指针；静态防重。
        static bool hookRegistered = false;
        if (!hookRegistered) {
            taishen::SetConfigChangedHook([]() {
                taishen::CBannerWindow::Instance().Refresh();
            });
            hookRegistered = true;
        }
    }
    // V0.4.x 配对符号成对上屏开关
    taishen::SetPairPunctEnabled(cfg.pair_punct);
    // 0.1.49：键位绑定 → tsf_keyevent（设置页「按键」页可调，热加载生效）
    {
        taishen::KeyBindingState kb;
        kb.toggleAsciiCtrlSpace = cfg.keys.toggle_ascii_ctrl_space;
        kb.toggleAsciiShift = cfg.keys.toggle_ascii_shift;
        kb.toggleFullwidth = cfg.keys.toggle_fullwidth;
        kb.togglePunct = cfg.keys.toggle_punct;
        kb.toggleTraditionalKey = cfg.keys.toggle_traditional_key;
        kb.pageCommaPeriod = cfg.keys.page_comma_period;
        kb.pageMinusEqual = cfg.keys.page_minus_equal;
        kb.pageBrackets = cfg.keys.page_brackets;
        kb.pageNumpad = cfg.keys.page_numpad;
        taishen::SetKeyBindings(kb);
        taishen::DebugLog(
            "ApplyConfig: pageKeys minus_equal=" +
            std::to_string(kb.pageMinusEqual ? 1 : 0) + " numpad=" +
            std::to_string(kb.pageNumpad ? 1 : 0) + " brackets=" +
            std::to_string(kb.pageBrackets ? 1 : 0) + " comma_period=" +
            std::to_string(kb.pageCommaPeriod ? 1 : 0));
    }
    // V0.4.x 诊断日志开关（默认关，终端用户无开销）
    taishen::SetDebugLogEnabled(cfg.debug_log);
    if (!isContainer) {
        // 浮动工具栏开关（0.1.47）：落配置，托盘菜单切换后所有进程一致
        taishen::CBannerWindow::Instance().SetRemoteEngineMode(
            m_useServerEngine, cfg.traditional_enabled, cfg.shuangpin_mode);
        taishen::CBannerWindow::Instance().SetEnabled(cfg.toolbar_enabled);
    }

    if (m_useServerEngine) {
        m_remoteState.asciiPunct = cfg.ascii_punct;
        if (!isContainer || !m_remoteTradOverride) {
            m_remoteState.traditional = cfg.traditional_enabled;
        }
        m_remoteState.pageSize = (std::max)(1, cfg.candidate_count);
        taishen::AppStateApplyRemote(cfg, m_remoteState.asciiMode);
    } else {
        // 引擎级开关：与设置页保存后走同一份逻辑（config_apply.h），0.1.68
        taishen::ApplyConfigToEngine(cfg);
        // V0.2.32/0.2.33 应用级配置：应用当前前台进程的初始/记忆状态 + 计算 inline 覆盖。
        taishen::AppStateApply(cfg);
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
    }

    if (!isContainer && !UseRemoteRender()) {
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
        // 行内预编辑（V0.2.18）+ 应用级覆盖（V0.2.32）
        m_candidateWindow.SetInlinePreedit(
            taishen::AppStateInlinePreeditEnabled(cfg) &&
            taishen::g_inlineCompositionReadable);
        // P0-1 视觉升级：标签格式 + 布局参数（圆角/内边距/候选间距）
        m_candidateWindow.SetLabelFormat(cfg.label_format);
        m_candidateWindow.SetLayout(cfg.corner_radius, cfg.hilite_corner_radius,
                                    cfg.padding, cfg.candidate_spacing);
        if (!m_useServerEngine) {
            // P0-1 滚轮翻页（候选窗口滚轮 → 引擎翻页 → 刷新并重绘候选窗）
            m_candidateWindow.SetPageCallback([this](int delta) {
                const int dir = (delta < 0) ? 1 : -1; // 滚轮下滚(<0)=下一页(+1)
                const int count = engine_page(dir);
                if (count > 0) {
                    RefreshState();
                    UpdateCandidateWindow();
                }
            });
        }
    }
    taishen::DebugLog("ApplyConfig: 配置已应用");
}

/// 初始化 config.ini 变更热加载。
/// 路径与基准 mtime 必须无条件初始化——不能挂在托盘窗口判断之后：
/// 0.1.67 起托盘窗口不再创建（m_trayHwnd 恒 nullptr），旧写法直接 return，
/// m_configPath 始终为空 → ReloadConfigIfChanged 开头即 return →
/// 设置页保存后不生效，必须重启宿主进程。2s 轮询仍挂托盘窗口（有消息泵
/// 时更及时），无托盘窗口时由 OnKeyDown 的按键检查兜底。
void CTextService::StartConfigWatch()
{
    if (m_configPath.empty()) {
        m_configPath = GetDllDir() + L"config.ini";
        // 记录基准 mtime
        WIN32_FILE_ATTRIBUTE_DATA attrs = {};
        if (GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &attrs)) {
            m_configMtime = attrs.ftLastWriteTime;
        }
    }
    if (m_configWatchStarted || m_trayHwnd == nullptr) {
        return;
    }
    SetTimer(m_trayHwnd, 1, 2000, nullptr);
    m_configWatchStarted = true;
    taishen::DebugLog("StartConfigWatch: 配置热加载轮询已启动 (2s)");
}

/// 停止轮询（Deactivate 时）
void CTextService::StopConfigWatch()
{
    if (m_trayHwnd != nullptr) {
        KillTimer(m_trayHwnd, 1);
    }
    m_configWatchStarted = false;
    if (!m_useServerEngine) {
        // 停用前把用户数据写入队列落盘：这条不能挂在托盘窗口/轮询状态上——
        // 0.1.67 停用托盘后 m_trayHwnd 恒为 nullptr，否则唯一的强制落盘路径不可达，
        // 未写入的学习词会随宿主进程退出丢失。AppContainer 内用户词库不可写，跳过。
        FlushUserWritesIfWritable(1);
    }
    taishen::DebugLog("StopConfigWatch: 停用，用户数据落盘检查完成");
}

/// 词库加载结果记一次（异步加载完成后由定时器/焦点/按键补记）。
/// AppContainer 诊断的关键一行：status=2 且 entries 为大数 → ACL 放行、词库可用；
/// status=4 → 拒绝访问（安装目录 ACL 未生效）。
void CTextService::LogDictLoadStatusOnce()
{
    if (m_dictStatusLogged) {
        return;
    }
    if (m_useServerEngine) {
        m_dictStatusLogged = true;
        return;
    }
    const int status = engine_dict_load_status();
    if (status == 1) {
        return; // 仍在异步加载，等下一次时机
    }
    const int entries = engine_dict_entry_count();
    const std::string line =
        std::string("dict load: status=") + std::to_string(status) +
        " entries=" + std::to_string(entries) +
        " AppContainer=" + (taishen::IsAppContainerProcess() ? "1" : "0");
    if (taishen::IsAppContainerProcess()) {
        taishen::ForceLog(line); // 容器内 debug 开关不可信（config 不可读）
    } else {
        taishen::DebugLog(line);
    }
    m_dictStatusLogged = true;
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
    // 0.1.68：配置变了要把工具栏按钮文字/高亮重读一遍——原来只应用引擎，
    // 设置页关闭双拼后工具栏仍显示「双拼」，两边各说各话。
    taishen::CBannerWindow::Instance().Refresh();
    taishen::DebugLog("ReloadConfigIfChanged: config.ini 已变更，热重载完成");
    m_renderClient.SendReloadConfig();
}

/// 候选窗口鼠标点击选词（0.1.13）：
/// 选择引擎候选 → 通过 TSF 组合提交上屏 → 隐藏候选窗口。
/// 与键盘选词共用同一提交链路（RunCompositionOp + Commit）。
void CTextService::OnCandidateClicked(int index)
{
    if (m_useServerEngine) {
        return;
    }
    // 没有焦点上下文时不能先消费引擎候选，否则宿主恢复后无法重试。
    if (m_pFocusContext == nullptr) {
        taishen::DebugLog("OnCandidateClicked: no focus context, keep candidate unconsumed");
        return;
    }
    // 先读取但不消费候选，宿主编辑会话成功后再真正选择。
    std::string text;
    const bool candidateOk = taishen::ReadEngineCandidate(index, text);
    const bool inCompose = (engine_in_compose() == 1);
    if (!candidateOk) {
        if (inCompose) {
            RefreshState();
            UpdateCandidateWindow();
        }
        return;
    }

    // 2. V0.6.x 组词中间选字：不落文档，只更新组合串（引擎是唯一状态源）
    if (inCompose) {
        std::string consumed;
        if (!taishen::ReadEngineSelectedCandidate(index, consumed)) {
            taishen::DebugLog("OnCandidateClicked: compose consume failed");
            RefreshState();
            UpdateCandidateWindow();
            return;
        }
        if (engine_in_compose() != 1) {
            // 最后一个音节：引擎已 reset，consumed 是整词 → 一次性提交。
            // 若仍按「中间音节」处理，整词会丢（组合串已空、也没落文档）。
            RunCompositionOp(m_pFocusContext, CEditSessionComposition::Op::Commit,
                             consumed);
            RefreshState();
            HideCandidateUi();
            return;
        }
        // 与键盘路径共用同一入口（组词态自动带已选前缀）
        SyncComposition(m_pFocusContext, "");
        RefreshState();
        UpdateCandidateWindow();
        return;
    }

    // 3. 通过组合提交上屏（需有焦点上下文）
    const HRESULT commitHr = RunCompositionOp(
        m_pFocusContext, CEditSessionComposition::Op::Commit, text);
    if (FAILED(commitHr)) {
        taishen::DebugLogHr("OnCandidateClicked: commit failed", commitHr);
        RefreshState();
        UpdateCandidateWindow();
        return;
    }
    std::string consumed;
    if (!taishen::ReadEngineSelectedCandidate(index, consumed)) {
        taishen::DebugLog("OnCandidateClicked: candidate consume failed after commit");
        RefreshState();
        UpdateCandidateWindow();
        return;
    }

    // 4. 刷新状态；普通选词隐藏候选窗
    RefreshState();
    HideCandidateUi();
}

/// 候选窗口鼠标右键（V0.5.7）：
/// 弹出菜单（置顶/取消置顶/降权/恢复候选）→ 调引擎 FFI → 刷新候选窗口。
/// 菜单项动态显示：按 engine_is_pinned / engine_is_demoted 决定文案与可用项。
void CTextService::OnCandidateRightClicked(int index)
{
    if (m_useServerEngine) {
        return;
    }
    // 1. 取当前候选词（UTF-8）
    if (index < 0 || static_cast<size_t>(index) >= m_candidates.size()) {
        return;
    }
    const std::string& word = m_candidates[static_cast<size_t>(index)];

    // 2. 查询状态（菜单动态显示）
    const bool pinned = (engine_is_pinned(word.c_str()) == 1);
    const bool demoted = (engine_is_demoted(word.c_str()) == 1);

    // 3. 构建菜单
    HMENU menu = CreatePopupMenu();
    if (menu == nullptr) {
        return;
    }
    // 置顶 / 取消置顶（MENU_APPEND，ID 1001）
    AppendMenuW(menu, MF_STRING, 1001,
                pinned ? L"取消置顶" : L"置顶");
    // 降权 / 恢复候选（ID 1002）
    AppendMenuW(menu, MF_STRING, 1002,
                demoted ? L"恢复候选" : L"降权（移出前两屏）");
    // 分隔线 + 删除用户词（ID 1003，V0.5.7 Eric 需求）：
    // 组词组错的热词必须能删——按词删除，全拼/简拼均不再出现
    AppendMenuW(menu, MF_SEPARATOR, 0, nullptr);
    AppendMenuW(menu, MF_STRING, 1003, L"删除该词");

    // 4. 弹出（返回命令 ID，不依赖 WM_COMMAND——候选窗 WS_EX_NOACTIVATE 不抢焦点）
    POINT pt = {};
    GetCursorPos(&pt);
    const UINT cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                                    pt.x, pt.y, 0, m_candidateWindow.Hwnd(), nullptr);
    DestroyMenu(menu);
    if (cmd == 0) {
        return;
    }

    // 5. 执行操作（引擎内部重查并更新候选）
    switch (cmd) {
    case 1001:
        if (pinned) {
            engine_unpin_word(word.c_str());
        } else {
            engine_pin_word(word.c_str());
        }
        break;
    case 1002:
        if (demoted) {
            engine_undemote_word(word.c_str());
        } else {
            engine_demote_word(word.c_str());
        }
        break;
    case 1003:
        // 删除该词（组词组错的热词）：从用户词库按词移除（全拼/简拼均清理）
        if (index >= 0 && static_cast<size_t>(index) < m_candidates.size()) {
            engine_delete_candidate(index);
        }
        break;
    default:
        return;
    }

    // 6. 刷新候选窗口（引擎已重查，同步 C++ 侧状态）
    RefreshState();
    UpdateCandidateWindow();
}

bool CTextService::UseRemoteRender() const
{
    return taishen::IsAppContainerProcess() || m_appCfg.use_server_render ||
           m_useServerEngine;
}

void CTextService::HideCandidateUi()
{
    if (!taishen::IsAppContainerProcess()) {
        m_candidateWindow.Hide();
    }
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

    // 若为异步延迟到达（沉浸式宿主），且候选仍处于活跃状态，平滑修正候选窗坐标
    if (!m_candidates.empty() && !m_lastDisplayPinyin.empty()) {
        if (UseRemoteRender()) {
            m_renderClient.SendMove(m_renderSessionId, rect);
        } else if (m_candidateWindow.IsVisible()) {
            m_candidateWindow.UpdateState(m_lastDisplayPinyin, m_candidates, rect,
                                          m_lastPage, m_lastTotalPages);
        }
    }
}

/// 通过 TSF 编辑会话获取光标屏幕坐标（V4 D1：不使用 TF_ES_SYNC）。
/// - 普通宿主：TF_ES_ASYNCDONTCARE | TF_ES_READ（无锁冲突时立即同步完成，返回 S_OK）
/// - AppContainer 宿主：TF_ES_ASYNC | TF_ES_READ（纯异步不阻塞跨进程存区，返回 TF_S_ASYNC，
///   回调到达后由 OnCaretRectResolved 修正候选窗坐标）
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

/// 二级光标兜底：取前台线程的 Win32 caret（GetGUIThreadInfo）。
/// TSF GetTextExt 在「切换窗口后第一次按键」时可能失败（宿主还没 layout），
/// 标准编辑控件此时已有 Win32 caret——比鼠标坐标准得多。
static bool GetCaretRectFromGuiThread(RECT* pRect)
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
    POINT topLeft{ gti.rcCaret.left, gti.rcCaret.top };
    POINT bottomRight{ gti.rcCaret.right, gti.rcCaret.bottom };
    if (!ClientToScreen(gti.hwndCaret, &topLeft) ||
        !ClientToScreen(gti.hwndCaret, &bottomRight)) {
        return false;
    }
    pRect->left = topLeft.x;
    pRect->top = topLeft.y;
    pRect->right = bottomRight.x;
    pRect->bottom = bottomRight.y;
    // 与 GetCaretRectFromContext 同源换算：宿主 DPI-unaware 时是 96-DPI 逻辑像素
    taishen::CaretToPhysicalPixel(gti.hwndCaret, pRect);
    return true;
}

/// 更新候选窗口：拉取光标坐标 → 传入拼音/候选 → 显示或隐藏
void CTextService::UpdateCandidateWindow(ITfContext* caretContext)
{
    // V0.5.7 语音候选：pinyin 空但候选非空（语音转写结果注入）也要显示——
    // 对齐 0.2.28 复选标点逻辑（候选窗判定只看候选，不看 pinyin）。
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
    // V0.6.x：候选数据仍通过 TSF UIElement 提供给宿主——
    // 游戏（经 msctfime 桥接）靠这条通道拿候选，不注册就等于没有候选。
    const HRESULT uiElementHr = EnsureCandidateUiElement();
    if (FAILED(uiElementHr)) {
        taishen::DebugLogHr("UpdateCandidateWindow: EnsureCandidateUiElement", uiElementHr);
    } else if (m_pUIElementMgr != nullptr) {
        const HRESULT updateHr = m_pUIElementMgr->UpdateUIElement(
            m_candidateUiElementId);
        taishen::DebugLogHr("UpdateCandidateWindow: UpdateUIElement", updateHr);
    }
    // 归属：宿主读过候选数据说明它自己会画（游戏），不再叠自绘窗口；
    // 没读过才由我们兜底。
    const bool isContainer = taishen::IsAppContainerProcess();
    if (m_candidateUiSuppressed) {
        if (!ShouldSelfDrawWhileSuppressed()) {
            if (!isContainer) {
                HideCandidateUi();
                return;
            }
            taishen::ForceLog("UpdateCandidateWindow(AC): suppressed=1 read=" +
                              std::to_string(m_uiElementDataRead != 0 ? 1 : 0) +
                              " bypass suppress for remote render");
        }
    }
    // V0.5.13 音节可视化 / V0.6.x：候选窗拼音行只在组词时用剩余音节串，
    // 普通输入显示原始拼音串（避免 ces 被拆成 c'e's）
    std::string displayPinyin = m_pinyin;
    const bool inCompose =
        m_useServerEngine ? m_remoteState.inCompose : (engine_in_compose() == 1);
    if (inCompose) {
        std::string tail;
        if (m_useServerEngine) {
            tail = m_remoteState.syllableDisplay;
        } else {
            taishen::ReadEngineSyllableDisplay(tail);
        }
        displayPinyin = BuildCompositionText(tail);
    }
    // 诊断：只打数量看不出内容，把前 3 个候选一起打出来
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

    // 获取光标屏幕坐标（V4 D1 异步安全）：
    // 1) TSF 编辑会话（同步即得，或沉浸式宿主异步返回 TF_S_ASYNC 后回调 OnCaretRectResolved）
    // 2) 若本次排队异步：先用同焦点会话缓存的上一次 TSF 光标，再试 Win32 caret，最后鼠标兜底
    RECT caretRect = {0, 0, 0, 0};
    bool gotCaret = false;
    ITfContext* caretCtx =
        (caretContext != nullptr) ? caretContext : m_pFocusContext;
    if (caretCtx != nullptr) {
        const HRESULT caretHr = GetCaretRectFromContext(caretCtx, &caretRect);
        if (caretHr == S_OK) {
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
    taishen::DebugLog("UpdateCandidateWindow: gotCaret=" + std::string(gotCaret ? "T" : "F") +
                      " remote=" + std::string(UseRemoteRender() ? "1" : "0") +
                      " caret=(" + std::to_string(caretRect.left) + "," +
                      std::to_string(caretRect.top) + ")");

    // 翻页指示（0.1.13）：当前页/总页数来自引擎或远程镜像
    const int page = m_useServerEngine ? m_remoteState.page : engine_get_current_page();
    const int totalPages =
        m_useServerEngine ? m_remoteState.totalPages : engine_get_total_pages();
    const bool inlinePreedit =
        taishen::AppStateInlinePreeditEnabled(m_appCfg) &&
        taishen::g_inlineCompositionReadable;

    m_lastDisplayPinyin = displayPinyin;
    m_lastPage = page;
    m_lastTotalPages = totalPages;

    // V4 D2：AppContainer 进程（或配置开启 use_server_render）走独立 taishen_server.exe 绘制，
    // 彻底避开 AppContainer 内 CreateWindowExW + D2D 阻塞宿主 UI 线程的问题。
    if (UseRemoteRender()) {
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
        }
        return;
    }

    m_candidateWindow.SetInlinePreedit(inlinePreedit);
    m_candidateWindow.UpdateState(displayPinyin, m_candidates, caretRect,
                                  page, totalPages);
}

/// 在异步安全编辑会话中执行组合操作（Start/Update/Commit，V4 D1：不使用 TF_ES_SYNC）
/// - 普通宿主：TF_ES_ASYNCDONTCARE | TF_ES_READWRITE（无冲突同步、遇跨进程/锁冲突自动异步）
/// - AppContainer 宿主：TF_ES_ASYNC | TF_ES_READWRITE（杜绝沉浸式文本存区 22 秒阻塞）
HRESULT CTextService::RunCompositionOp(ITfContext* pic,
                                       CEditSessionComposition::Op op,
                                       const std::string& text,
                                       int caretOffset,
                                       LONG convertedChars,
                                       bool forceStart)
{
    if (pic == nullptr) {
        return E_POINTER;
    }

    CEditSessionComposition* pSession =
        new (std::nothrow) CEditSessionComposition(this, pic, op, text,
                                                   &m_composition,
                                                   caretOffset,
                                                   convertedChars,
                                                   forceStart);
    if (pSession == nullptr) {
        return E_OUTOFMEMORY;
    }

    const DWORD flags = taishen::IsAppContainerProcess()
                            ? (TF_ES_ASYNC | TF_ES_READWRITE)
                            : (TF_ES_ASYNCDONTCARE | TF_ES_READWRITE);
    HRESULT hrSession = E_FAIL;
    const HRESULT hr = pic->RequestEditSession(
        m_tid, pSession, flags, &hrSession);
    pSession->Release();
    taishen::DebugLogHr("RunCompositionOp: RequestEditSession", hr);
    taishen::DebugLogHr("RunCompositionOp: session", hrSession);
    if (FAILED(hr)) {
        return hr;
    }
    return hrSession;
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
        const std::wstring uiElementCategory =
            ClsidToString(GUID_TFCAT_TIPCAP_UIELEMENTENABLED);

        for (const wchar_t* cat : kCategories) {
            // 新式：Category\Category\{类别GUID}\{CLSID}
            const std::wstring catNewKey =
                tipKey + L"\\Category\\Category\\" + cat + L"\\" + clsidStr;
            WriteRegKey(root, catNewKey, L"", L"");
            // 旧式兼容：Category\{类别GUID}
            const std::wstring catOldKey = tipKey + L"\\Category\\" + cat;
            WriteRegKey(root, catOldKey, L"", L"");
        }

        // S-07：声明 GUID_TFCAT_TIPCAP_UIELEMENTENABLED，
        // 使 UIElementMgr 能在 UILess 宿主中激活候选列表协商。
        const std::wstring uiCatNewKey =
            tipKey + L"\\Category\\Category\\" + uiElementCategory + L"\\" +
            clsidStr;
        WriteRegKey(root, uiCatNewKey, L"", L"");
        const std::wstring uiCatOldKey =
            tipKey + L"\\Category\\" + uiElementCategory;
        WriteRegKey(root, uiCatOldKey, L"", L"");

        // Category\Item\{CLSID} 类别项（含描述）
        const std::wstring catItemKey =
            tipKey + L"\\Category\\Item\\" + clsidStr;
        WriteRegKey(root, catItemKey, L"Description", L"泰深输入法");
        for (const wchar_t* cat : kCategories) {
            const std::wstring catItemSub =
                catItemKey + L"\\" + cat;
            WriteRegKey(root, catItemSub, L"", L"");
        }
        WriteRegKey(root, catItemKey + L"\\" + uiElementCategory, L"", L"");
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




