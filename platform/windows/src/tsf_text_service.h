/// Windows TSF IME — CTextService 内部共享声明（V4 模块化拆分，V4.1–V4.2）
///
/// 由以下 5 个实现单元共用，不对外暴露为公共 API：
///   - tsf_text_service.cpp : 生命周期、ActivateEx/Deactivate、ApplyConfig、COM 导出与注册
///   - tsf_key_sink.cpp     : ITfKeyEventSink、远程引擎 IPC 转发与按键处理
///   - tsf_edit_session.cpp : CEditSessionComposition / CEditSessionGetCaret、组合串同步
///   - tsf_langbar.cpp      : 语言栏图标挂载与 ILangBarHost 交互入口
///   - tsf_focus_caret.cpp  : ITfThreadMgrEventSink、UIElement 协商、光标取位与候选窗同步

#pragma once

#include <windows.h>
#include <msctf.h>

#include <atomic>
#include <cstdint>
#include <string>
#include <vector>

#include "config_reader.h"
#include "ipc_render_client.h"
#include "langbar_item.h"
#include "tsf_composition.h"
#include "tsf_keyevent.h"

// DLL 自身模块句柄（定义于 dllmain.cpp）
extern HMODULE g_hModule;

/// 获取当前 TSF DLL 的完整路径与所在目录（带尾分隔符）
std::wstring TaishenTsfGetDllPath();
std::wstring TaishenTsfGetDllDir();

/// 限频日志门控（防止高频异常日志刷屏）
bool TaishenShouldEmitRateLimitedLog(std::atomic<ULONGLONG>& lastTick,
                                     ULONGLONG intervalMs);

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

    // ITfDisplayAttributeProvider（V0.6.x）：组合串分段着色
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
    // 组合串唯一拼装点（V0.6.x）：
    //   组词态 → 引擎已选前缀 + 待选后缀 pending
    //   普通输入 → pending（引擎拼音显示串）
    std::string BuildCompositionText(const std::string& pending) const;
    void SyncComposition(ITfContext* pic, const std::string& pending,
                         bool forceStart = false);
    void UpdateCandidateWindow(ITfContext* caretContext = nullptr);
    HRESULT GetCaretRectFromContext(ITfContext* pic, RECT* pRect);
    void OnCaretRectResolved(uint64_t seq, const RECT& rect);
    void HideCandidateUi();

    // 服务端引擎镜像清理/同步/异步提交与远端按键主流程
    void ClearRemoteState();
    void SendRemoteResetCommit();
    void SendRemoteCommitWithLog(const taishen::ipc::CommitRequest& cm);
    void SyncRemoteModesFromPong();
    void ApplyRemoteResponse(const taishen::ipc::CandidateListResponse& resp);
    void CommitRemoteText(ITfContext* pic, const std::wstring& wideText,
                          int caretOffset = -1);
    HRESULT HandleRemoteKeyDown(ITfContext* pic, WPARAM wParam, LPARAM lParam,
                                BOOL* pfEaten);

    // TSF UILess 候选 UIElement 协商
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
    bool ShouldSelfDrawWhileSuppressed();

    // 内部编辑会话：更新组合（Start/Update/Commit，不使用 TF_ES_SYNC）
    class CEditSessionComposition final : public ITfEditSession
    {
    public:
        enum class Op { Start, Update, Commit };

        CEditSessionComposition(CTextService* owner,
                                ITfContext* pic, Op op,
                                const std::string& text,
                                taishen::CTsfComposition* comp,
                                int caretOffset = -1,
                                LONG convertedChars = 0,
                                bool forceStart = false,
                                int cursorChars = -1);
        ~CEditSessionComposition();

        STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
        STDMETHODIMP_(ULONG) AddRef() override;
        STDMETHODIMP_(ULONG) Release() override;
        STDMETHODIMP DoEditSession(TfEditCookie ec) override;

    private:
        LONG m_cRef;
        CTextService* m_pOwner;
        ITfContext* m_pic;
        Op m_op;
        std::string m_text;
        taishen::CTsfComposition* m_pComp;
        int m_caretOffset;
        LONG m_convertedChars;
        bool m_forceStart;
        int m_cursorChars;
    };

    HRESULT RunCompositionOp(ITfContext* pic, CEditSessionComposition::Op op,
                             const std::string& text, int caretOffset = -1,
                             LONG convertedChars = 0, bool forceStart = false,
                             int cursorChars = -1);

    // 内部编辑会话：获取光标屏幕坐标（异步安全）
    class CEditSessionGetCaret final : public ITfEditSession
    {
    public:
        CEditSessionGetCaret(CTextService* owner, ITfContext* pic, uint64_t seq);
        ~CEditSessionGetCaret();

        STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
        STDMETHODIMP_(ULONG) AddRef() override;
        STDMETHODIMP_(ULONG) Release() override;
        STDMETHODIMP DoEditSession(TfEditCookie ec) override;

    private:
        LONG m_cRef;
        CTextService* m_pOwner;
        ITfContext* m_pic;
        uint64_t m_seq;
    };

    // TSF 核心状态
    LONG m_cRef;
    ITfThreadMgr* m_pThreadMgr;
    TfClientId m_tid;
    DWORD m_dwThreadMgrEventSinkCookie;
    ITfContext* m_pFocusContext;
    ITfUIElementMgr* m_pUIElementMgr;
    CCandidateListUIElement* m_candidateUiElement;
    DWORD m_candidateUiElementId;
    bool m_candidateUiSuppressed;
    LONG m_uiElementDataRead;

    // 组合与候选状态
    taishen::CTsfComposition m_composition;
    std::string m_pinyin;
    std::vector<std::string> m_candidates;
    std::wstring m_committedText;
    BOOL m_fActive;

    // V3.1：进程内引擎已摘除，所有引擎状态统一经 IPC 镜像自 taishen_server.exe
    taishen::RemoteEngineMirror m_remoteState;
    uint64_t m_ipcKeySeq = 0;
    bool m_remoteTradOverride = false;
    bool m_remoteShuangpinOverride = false;
    bool m_remotePongSynced = false;

    // 语言栏按钮（输入指示器 + 中英切换）
    CLangBarItemButton* m_pLangBarItem;
    CLangBarItemButton* m_pModeItem;
    ITfLangBarItemMgr* m_langBarMgr = nullptr;
    bool m_brandAttached = false;
    bool m_modeAttached = false;

    // 配置热加载（对标 rime-ice 重新部署）
    std::wstring m_configPath;
    FILETIME m_configMtime = {};
    taishen::ImeConfig m_appCfg;

    // 异步光标缓存与进程外候选窗渲染客户端
    taishen::CRenderIpcClient m_renderClient;
    uint64_t m_renderSessionId = 0;
    uint64_t m_caretSeq = 0;
    uint64_t m_lastResolvedCaretSeq = 0;
    RECT m_lastCaretRect = {0, 0, 0, 0};
    bool m_hasLastCaretRect = false;
    std::string m_lastDisplayPinyin;
    int m_lastPage = 0;
    int m_lastTotalPages = 0;

    // 语言栏与状态切换入口
    void AttachLangBarItem();
    void DetachLangBarItem();
    void RefreshLangBarItem();
    void OnLangBarToggleAscii() override;
    bool IsAsciiMode() const override;
    bool IsTraditionalMode() const override;
    bool IsShuangpinMode() const override;
    bool IsAsciiPunctMode() const override;
    bool IsToolbarEnabled() const override;
    void OnLangBarNeedRefresh() override;
    void OnLangBarToggleTraditional() override;
    void OnLangBarToggleShuangpin() override;
    void OnLangBarToggleAsciiPunct() override;
    void OnLangBarToggleToolbar() override;
    void OnLangBarOpenSettings(int tabIndex = 0) override;
    void OpenSettingsWindow(int tabIndex = 0);
    void ToggleAsciiMode();
    void ToggleTraditional();
    void ToggleShuangpin();

    // 配置热加载
    void ApplyConfig(const taishen::ImeConfig& cfg, const std::wstring& dllDir);
    void StartConfigWatch();
    void StopConfigWatch();
    void ReloadConfigIfChanged();

    // 按键交互辅助状态
    bool m_multiRowExpanded;
    bool m_shiftTapArmed;
    bool m_shiftComboUsed;
    bool m_shiftActive;
    DWORD m_shiftDownTick;
    bool m_quoteOpen;
    bool m_dquoteOpen;
    int m_lastDigitPunctVk = 0;
    LPARAM m_lastDigitPunctLParam = 0;
    ULONGLONG m_lastDigitPunctTick = 0;
};
