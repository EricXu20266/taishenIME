/// Windows TSF IME — Text Service 生命周期、配置热加载与 COM 导出
///
/// 对应 SPEC: docs/modules/interface-layer/SPEC.md
/// 覆盖 DEV-TRACKER: V4.1–V4.2

#include "tsf_text_service.h"

#include <algorithm>
#include <new>

#include "app_container.h"
#include "app_state.h"
#include "debug_log.h"
#include "display_attributes.h"
#include "ime_ids.h"

namespace {

// 模块引用计数（DllCanUnloadNow 使用）
LONG g_cRefDll = 0;

bool WriteRegKey(HKEY hkRoot, const std::wstring& subKey,
                 const std::wstring& valueName,
                 const std::wstring& valueData)
{
    HKEY hKey = nullptr;
    const LSTATUS status = RegCreateKeyExW(
        hkRoot, subKey.c_str(), 0, nullptr, REG_OPTION_NON_VOLATILE,
        KEY_WRITE, nullptr, &hKey, nullptr);
    if (status != ERROR_SUCCESS) {
        return false;
    }

    const DWORD dataSize =
        static_cast<DWORD>((valueData.size() + 1) * sizeof(wchar_t));
    const LSTATUS setStatus = RegSetValueExW(
        hKey, valueName.c_str(), 0, REG_SZ,
        reinterpret_cast<const BYTE*>(valueData.c_str()), dataSize);
    RegCloseKey(hKey);
    return setStatus == ERROR_SUCCESS;
}

bool WriteRegDword(HKEY hkRoot, const std::wstring& subKey,
                   const std::wstring& valueName, DWORD value)
{
    HKEY hKey = nullptr;
    const LSTATUS status = RegCreateKeyExW(
        hkRoot, subKey.c_str(), 0, nullptr, REG_OPTION_NON_VOLATILE,
        KEY_WRITE, nullptr, &hKey, nullptr);
    if (status != ERROR_SUCCESS) {
        return false;
    }

    const LSTATUS setStatus = RegSetValueExW(
        hKey, valueName.c_str(), 0, REG_DWORD,
        reinterpret_cast<const BYTE*>(&value), sizeof(value));
    RegCloseKey(hKey);
    return setStatus == ERROR_SUCCESS;
}

void DeleteRegKey(HKEY hkRoot, const std::wstring& subKey)
{
    RegDeleteTreeW(hkRoot, subKey.c_str());
}

std::wstring ClsidToString(const CLSID& clsid)
{
    wchar_t buf[64] = {0};
    StringFromGUID2(clsid, buf, 64);
    return std::wstring(buf);
}

std::wstring GetTipRegKey(const std::wstring& clsidStr)
{
    return L"Software\\Microsoft\\CTF\\TIP\\" + clsidStr;
}

class CClassFactory final : public IClassFactory
{
public:
    CClassFactory() : m_cRef(1)
    {
        InterlockedIncrement(&g_cRefDll);
    }

    ~CClassFactory()
    {
        InterlockedDecrement(&g_cRefDll);
    }

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

    STDMETHODIMP CreateInstance(IUnknown* pUnkOuter, REFIID riid,
                                void** ppvObj) override
    {
        if (ppvObj == nullptr) {
            return E_POINTER;
        }
        *ppvObj = nullptr;

        if (pUnkOuter != nullptr) {
            return CLASS_E_NOAGGREGATION;
        }

        CTextService* pService = new (std::nothrow) CTextService();
        if (pService == nullptr) {
            return E_OUTOFMEMORY;
        }

        const HRESULT hr = pService->QueryInterface(riid, ppvObj);
        pService->Release();
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

} // namespace

std::wstring TaishenTsfGetDllPath()
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

std::wstring TaishenTsfGetDllDir()
{
    const std::wstring path = TaishenTsfGetDllPath();
    const size_t slash = path.find_last_of(L"\\/");
    if (slash == std::wstring::npos) {
        return std::wstring();
    }
    return path.substr(0, slash + 1);
}

bool TaishenShouldEmitRateLimitedLog(std::atomic<ULONGLONG>& lastTick,
                                     ULONGLONG intervalMs)
{
    const ULONGLONG now = GetTickCount64();
    const ULONGLONG stamp = (now == 0) ? 1 : now;
    ULONGLONG prev = lastTick.load(std::memory_order_relaxed);
    if (prev != 0 && (stamp - prev) < intervalMs) {
        return false;
    }
    return lastTick.compare_exchange_strong(prev, stamp, std::memory_order_relaxed);
}

// ---------------------------------------------------------------------------
// CTextService — 构造 / COM 基础接口 / 生命周期
// ---------------------------------------------------------------------------

CTextService::CTextService()
    : m_cRef(1), m_pThreadMgr(nullptr), m_tid(0),
      m_dwThreadMgrEventSinkCookie(0),
      m_pFocusContext(nullptr), m_pUIElementMgr(nullptr),
      m_candidateUiElement(nullptr), m_candidateUiElementId(0),
      m_candidateUiSuppressed(false), m_uiElementDataRead(0),
      m_fActive(FALSE),
      m_pLangBarItem(nullptr), m_pModeItem(nullptr),
      m_multiRowExpanded(false), m_shiftTapArmed(false),
      m_shiftComboUsed(false), m_shiftActive(false), m_shiftDownTick(0),
      m_quoteOpen(false), m_dquoteOpen(false)
{
    static volatile LONG64 s_nextRenderSid = 0;
    m_renderSessionId =
        (static_cast<uint64_t>(GetCurrentProcessId()) << 32) |
        static_cast<uint32_t>(InterlockedIncrement64(&s_nextRenderSid));
    InterlockedIncrement(&g_cRefDll);
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
        // ITfTextInputProcessorEx 继承自它，同一对象可安全双暴露
        *ppvObj = static_cast<ITfTextInputProcessor*>(this);
    } else if (IsEqualIID(riid, IID_ITfKeyEventSink)) {
        *ppvObj = static_cast<ITfKeyEventSink*>(this);
    } else if (IsEqualIID(riid, IID_ITfThreadMgrEventSink)) {
        *ppvObj = static_cast<ITfThreadMgrEventSink*>(this);
    } else if (IsEqualIID(riid, IID_ITfDisplayAttributeProvider)) {
        *ppvObj = static_cast<ITfDisplayAttributeProvider*>(this);
    } else if (IsEqualIID(riid, IID_ITfCompartmentEventSink)) {
        *ppvObj = static_cast<ITfCompartmentEventSink*>(this);
    } else if (IsEqualIID(riid, IID_ITfFunctionProvider)) {
        *ppvObj = static_cast<ITfFunctionProvider*>(this);
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

// ---------------------------------------------------------------------------
// ITfTextInputProcessorEx
// ---------------------------------------------------------------------------

STDMETHODIMP CTextService::Activate(ITfThreadMgr* ptim, TfClientId tid)
{
    return ActivateEx(ptim, tid, 0);
}

STDMETHODIMP CTextService::ActivateEx(ITfThreadMgr* ptim, TfClientId tid,
                                      DWORD dwFlags)
{
    const ULONGLONG tAct = GetTickCount64();
    auto markStep = [tAct](const char* label) {
        taishen::ForceLog(std::string("ActivateEx step: ") + label + " +" +
                          std::to_string(GetTickCount64() - tAct) + "ms");
    };
    markStep("enter");
    taishen::DebugLog("ActivateEx enter dwFlags=" + std::to_string(dwFlags));
    if (ptim == nullptr) {
        taishen::DebugLog("ActivateEx: ptim==nullptr -> E_INVALIDARG");
        return E_INVALIDARG;
    }

    m_dwActivateFlags = dwFlags;
    if (taishen::IsNoActivateTipFlags(dwFlags)) {
        taishen::DebugLog("ActivateEx: TF_TMAE_NOACTIVATETIP -> skip activation");
        return S_OK;
    }

    m_pThreadMgr = ptim;
    m_pThreadMgr->AddRef();
    m_tid = tid;

    const std::wstring dllDir = TaishenTsfGetDllDir();
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    markStep("load-config");

    const bool isContainer = taishen::IsAppContainerProcess();
    const bool secureMode = IsSecureMode();
    markStep("detect-container");
    taishen::ForceLog("ActivateEx: AppContainer=" +
                      std::string(isContainer ? "1" : "0") +
                      " thinDll=1 dwFlags=" + std::to_string(dwFlags));

    markStep("before-apply-config");
    ApplyConfig(cfg, dllDir);
    markStep("apply-config");

    // V4 D2（V1 规则）：仅非容器、非安全桌面宿主拉起 `taishen_server.exe`
    if (!isContainer && !secureMode) {
        m_renderClient.EnsureServerRunning(dllDir);
    }

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
            m_dwThreadMgrEventSinkCookie = 0;
        }
    }

    // 主动获取当前焦点文档管理器的顶层上下文（0.1.15 修复）：
    // ITfThreadMgrEventSink::OnSetFocus 不一定在首次激活时触发。
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

    ITfKeystrokeMgr* pKeystrokeMgr = nullptr;
    hr = m_pThreadMgr->QueryInterface(IID_ITfKeystrokeMgr,
                                      reinterpret_cast<void**>(&pKeystrokeMgr));
    if (SUCCEEDED(hr) && pKeystrokeMgr != nullptr) {
        // 先注销保证幂等，避免重复注册返回 TF_E_NOLOCK
        pKeystrokeMgr->UnadviseKeyEventSink(tid);
        hr = pKeystrokeMgr->AdviseKeyEventSink(
            tid, static_cast<ITfKeyEventSink*>(this), TRUE);
        pKeystrokeMgr->Release();
        taishen::DebugLogHr("ActivateEx: AdviseKeyEventSink", hr);
    }

    m_fActive = TRUE;
    taishen::DebugLog("ActivateEx done, m_fActive=TRUE");

    InitCompartments();
    InitFunctionProvider();
    AttachLangBarItem();
    if (m_pFocusContext != nullptr) {
        QueryContextInputScope(m_pFocusContext);
    }

    if (!isContainer && !secureMode) {
        StartConfigWatch();
    }
    return S_OK;
}

STDMETHODIMP CTextService::Deactivate()
{
    taishen::DebugLog("Deactivate");
    {
        static std::atomic<ULONGLONG> s_lastDeactivateProbeTick{0};
        if (TaishenShouldEmitRateLimitedLog(s_lastDeactivateProbeTick, 30000)) {
            taishen::ForceLog(
                "[FOCUSPROBE] Deactivate pid=" + std::to_string(GetCurrentProcessId()) +
                " tid=" + std::to_string(GetCurrentThreadId()) +
                " active=" + std::string(m_fActive ? "1" : "0"));
        }
    }
    UninitFunctionProvider();
    UninitCompartments();
    if (m_pThreadMgr != nullptr) {
        ITfSource* pSource = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfSource, reinterpret_cast<void**>(&pSource))) &&
            pSource != nullptr) {
            pSource->UnadviseSink(m_dwThreadMgrEventSinkCookie);
            pSource->Release();
        }

        ITfKeystrokeMgr* pKeystrokeMgr = nullptr;
        if (SUCCEEDED(m_pThreadMgr->QueryInterface(
                IID_ITfKeystrokeMgr,
                reinterpret_cast<void**>(&pKeystrokeMgr))) &&
            pKeystrokeMgr != nullptr) {
            pKeystrokeMgr->UnadviseKeyEventSink(m_tid);
            pKeystrokeMgr->Release();
        }
    }

    DetachLangBarItem();

    if (m_pFocusContext != nullptr) {
        m_pFocusContext->Release();
        m_pFocusContext = nullptr;
    }

    if (m_pThreadMgr != nullptr) {
        m_pThreadMgr->Release();
        m_pThreadMgr = nullptr;
    }

    if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
        SendRemoteResetCommit();
        ClearRemoteState();
    }
    m_fActive = FALSE;
    m_dwActivateFlags = 0;
    m_inputScopeAscii = false;
    m_lastInputScopeContext = nullptr;
    m_hasLastCaretRect = false;
    ++m_caretSeq;
    EndCandidateUiElement();
    HideCandidateUi();
    m_renderClient.Disconnect();
    m_composition.Reset();
    if (!taishen::IsAppContainerProcess()) {
        StopConfigWatch();
    }
    return S_OK;
}

// ---------------------------------------------------------------------------
// 配置热加载（对标 rime-ice 重新部署）
// ---------------------------------------------------------------------------

void CTextService::ApplyConfig(const taishen::ImeConfig& cfg,
                               const std::wstring& /*dllDir*/)
{
    const bool isContainer = taishen::IsAppContainerProcess();
    m_appCfg = cfg;
    taishen::SetPairPunctEnabled(cfg.pair_punct);
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
    taishen::SetDebugLogEnabled(cfg.debug_log);

    m_remoteState.asciiPunct = cfg.ascii_punct;
    if (!isContainer || !m_remoteTradOverride) {
        m_remoteState.traditional = cfg.traditional_enabled;
    }
    m_remoteState.pageSize = (std::max)(1, cfg.candidate_count);
    taishen::AppStateApplyRemote(cfg, m_remoteState.asciiMode);
    if (IsSecureMode()) {
        m_remoteState.asciiMode = true;
    }
    if (m_fActive) {
        SyncCompartmentsFromState();
    }

    taishen::DebugLog("ApplyConfig: 配置已应用");
}

void CTextService::StartConfigWatch()
{
    // 路径与基准 mtime 无条件初始化（热加载由焦点切换与按键检查触发）
    m_configPath = TaishenTsfGetDllDir() + L"config.ini";
    WIN32_FILE_ATTRIBUTE_DATA attrs = {};
    if (GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        m_configMtime = attrs.ftLastWriteTime;
    } else {
        m_configMtime = {};
    }
    taishen::DebugLog("StartConfigWatch: 配置热加载基准已就绪");
}

void CTextService::StopConfigWatch()
{
    m_configPath.clear();
    m_configMtime = {};
    taishen::DebugLog("StopConfigWatch: 配置热加载状态已清理");
}

void CTextService::ReloadConfigIfChanged()
{
    if (m_configPath.empty() || IsSecureMode()) {
        return;
    }
    WIN32_FILE_ATTRIBUTE_DATA attrs = {};
    if (!GetFileAttributesExW(m_configPath.c_str(), GetFileExInfoStandard, &attrs)) {
        return;
    }
    if (attrs.ftLastWriteTime.dwHighDateTime == m_configMtime.dwHighDateTime &&
        attrs.ftLastWriteTime.dwLowDateTime == m_configMtime.dwLowDateTime) {
        return;
    }
    m_configMtime = attrs.ftLastWriteTime;
    const std::wstring dllDir = TaishenTsfGetDllDir();
    const taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
    ApplyConfig(cfg, dllDir);
    taishen::DebugLog("ReloadConfigIfChanged: config.ini 已变更，热重载完成");
    m_renderClient.SendReloadConfig();
}

// ---------------------------------------------------------------------------
// DLL 导出函数（.def 绑定）
// ---------------------------------------------------------------------------

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

STDAPI DllCanUnloadNow(void)
{
    return (g_cRefDll == 0) ? S_OK : S_FALSE;
}

STDAPI DllRegisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);
    const std::wstring dllPath = TaishenTsfGetDllPath();
    if (dllPath.empty()) {
        return E_FAIL;
    }

    const HKEY kRoots[] = {HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER};
    const wchar_t* kClsidPrefixes[] = {
        L"SOFTWARE\\Classes\\CLSID\\",
        L"Software\\Classes\\CLSID\\",
    };

    for (int i = 0; i < 2; ++i) {
        const HKEY root = kRoots[i];
        const std::wstring clsidPrefix = kClsidPrefixes[i];

        const std::wstring clsidKey =
            clsidPrefix + clsidStr + L"\\InprocServer32";
        WriteRegKey(root, clsidKey, L"", dllPath);
        WriteRegKey(root, clsidKey, L"ThreadingModel", L"Apartment");

        const std::wstring tipKey = L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsidStr;
        WriteRegKey(root, tipKey, L"Description", L"泰深输入法");

        const std::wstring langKey = tipKey + L"\\LanguageProfile\\0x00000804\\" +
                                     ClsidToString(GUID_LANGPROFILE);
        WriteRegKey(root, langKey, L"Description", L"泰深拼音");
        WriteRegKey(root, langKey, L"DisplayDescription", L"泰深拼音");
        WriteRegDword(root, langKey, L"Enable", 1);
        WriteRegKey(root, langKey, L"IconFile", dllPath);
        WriteRegDword(root, langKey, L"IconIndex", 0);

        const wchar_t* kCategories[] = {
            L"{34745C63-B2F0-4784-8B67-5E12C8701A31}", // GUID_TFCAT_TIP_KEYBOARD
            L"{13A016DF-560B-46CD-947A-4C3AF1E0E35D}", // GUID_TFCAT_TIPCAP_IMMERSIVESUPPORT
            L"{25504FB4-7BAB-4BC1-9C69-CF81890F0EF5}", // GUID_TFCAT_TIPCAP_SYSTRAYSUPPORT
            L"{046B8C80-1647-40F7-9B21-B93B81AABC1B}", // GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER
        };
        const std::wstring uiElementCategory =
            ClsidToString(GUID_TFCAT_TIPCAP_UIELEMENTENABLED);

        for (const wchar_t* cat : kCategories) {
            const std::wstring catNewKey =
                tipKey + L"\\Category\\Category\\" + cat + L"\\" + clsidStr;
            WriteRegKey(root, catNewKey, L"", L"");
            const std::wstring catOldKey = tipKey + L"\\Category\\" + cat;
            WriteRegKey(root, catOldKey, L"", L"");
        }

        const std::wstring uiCatNewKey =
            tipKey + L"\\Category\\Category\\" + uiElementCategory + L"\\" +
            clsidStr;
        WriteRegKey(root, uiCatNewKey, L"", L"");
        const std::wstring uiCatOldKey =
            tipKey + L"\\Category\\" + uiElementCategory;
        WriteRegKey(root, uiCatOldKey, L"", L"");

        const std::wstring catItemKey =
            tipKey + L"\\Category\\Item\\" + clsidStr;
        WriteRegKey(root, catItemKey, L"Description", L"泰深输入法");
        for (const wchar_t* cat : kCategories) {
            const std::wstring catItemSub = catItemKey + L"\\" + cat;
            WriteRegKey(root, catItemSub, L"", L"");
        }
        WriteRegKey(root, catItemKey + L"\\" + uiElementCategory, L"", L"");
    }

    return S_OK;
}

STDAPI DllUnregisterServer(void)
{
    const std::wstring clsidStr = ClsidToString(CLSID_TAISHEN_IME);

    DeleteRegKey(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Classes\\CLSID\\" + clsidStr);
    DeleteRegKey(HKEY_LOCAL_MACHINE, L"SOFTWARE\\Microsoft\\CTF\\TIP\\" + clsidStr);

    DeleteRegKey(HKEY_CURRENT_USER, L"Software\\Classes\\CLSID\\" + clsidStr);
    DeleteRegKey(HKEY_CURRENT_USER, GetTipRegKey(clsidStr));

    return S_OK;
}
