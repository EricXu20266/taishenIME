/// Windows TSF IME — 语言栏图标挂载、ILangBarHost 回调与模式切换
///
/// 对应 DEV-TRACKER: V4.1–V4.2

#include "tsf_text_service.h"

#include <new>

#include "app_container.h"
#include "app_state.h"
#include "debug_log.h"

// ---------------------------------------------------------------------------
// 语言栏按钮挂载与刷新
// ---------------------------------------------------------------------------

void CTextService::AttachLangBarItem()
{
    if (m_pThreadMgr == nullptr) {
        return;
    }
    if (m_brandAttached && m_modeAttached) {
        return;
    }
    if (m_pLangBarItem == nullptr) {
        m_pLangBarItem = new (std::nothrow) CLangBarItemButton(
            this, CLangBarItemButton::Kind::Brand);
    }
    if (m_pModeItem == nullptr) {
        m_pModeItem = new (std::nothrow) CLangBarItemButton(
            this, CLangBarItemButton::Kind::Mode);
    }
    if (m_pLangBarItem == nullptr || m_pModeItem == nullptr) {
        taishen::DebugLog("AttachLangBarItem: alloc failed");
        return;
    }
    if (m_langBarMgr == nullptr) {
        ITfLangBarItemMgr* pItemMgr = nullptr;
        HRESULT hr = m_pThreadMgr->QueryInterface(
            IID_ITfLangBarItemMgr, reinterpret_cast<void**>(&pItemMgr));
        if ((FAILED(hr) || pItemMgr == nullptr) && !IsComlessMode()) {
            hr = CoCreateInstance(CLSID_TF_LangBarItemMgr, nullptr,
                                  CLSCTX_INPROC_SERVER, IID_ITfLangBarItemMgr,
                                  reinterpret_cast<void**>(&pItemMgr));
            taishen::DebugLogHr("AttachLangBarItem: CoCreate(CLSID_TF_LangBarItemMgr)", hr);
        }
        if (FAILED(hr) || pItemMgr == nullptr) {
            taishen::DebugLogHr("AttachLangBarItem: QI(ITfLangBarItemMgr)", hr);
            return;
        }
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
    if (!m_brandAttached || !m_modeAttached) {
        AttachLangBarItem();
        return;
    }
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

// ---------------------------------------------------------------------------
// ILangBarHost — 语言栏按钮回调与模式切换
// ---------------------------------------------------------------------------

void CTextService::OnLangBarToggleAscii()
{
    ToggleAsciiMode();
}

bool CTextService::IsAsciiMode() const
{
    return IsEffectiveAsciiMode();
}

bool CTextService::IsTraditionalMode() const
{
    return m_remoteState.traditional;
}

bool CTextService::IsShuangpinMode() const
{
    return m_appCfg.shuangpin_mode;
}

bool CTextService::IsAsciiPunctMode() const
{
    return m_remoteState.asciiPunct;
}

bool CTextService::IsToolbarEnabled() const
{
    return m_appCfg.toolbar_enabled;
}

void CTextService::OnLangBarNeedRefresh()
{
    RefreshLangBarItem();
}

void CTextService::OnLangBarToggleTraditional()
{
    ToggleTraditional();
    RefreshLangBarItem();
}

void CTextService::OnLangBarToggleShuangpin()
{
    ToggleShuangpin();
    RefreshLangBarItem();
}

void CTextService::OnLangBarToggleAsciiPunct()
{
    const bool next = !m_remoteState.asciiPunct;
    m_remoteState.asciiPunct = next;
    m_appCfg.ascii_punct = next;
    SyncCompartmentsFromState();
    if (!taishen::IsAppContainerProcess()) {
        const std::wstring dllDir = TaishenTsfGetDllDir();
        if (!dllDir.empty()) {
            taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
            cfg.ascii_punct = next;
            taishen::SaveConfig(dllDir, cfg);
            m_renderClient.SendReloadConfig();
        }
    }
    taishen::DebugLog("LangBar: ascii_punct -> " + std::to_string(next ? 1 : 0));
}

void CTextService::OnLangBarToggleToolbar()
{
    const bool next = !m_appCfg.toolbar_enabled;
    m_appCfg.toolbar_enabled = next;
    if (!taishen::IsAppContainerProcess()) {
        const std::wstring dllDir = TaishenTsfGetDllDir();
        if (!dllDir.empty()) {
            taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
            cfg.toolbar_enabled = next;
            taishen::SaveConfig(dllDir, cfg);
            m_renderClient.SendReloadConfig();
        }
    }
    taishen::DebugLog("LangBar: toolbar -> " + std::to_string(next ? 1 : 0));
}

void CTextService::OpenSettingsWindow(int tabIndex)
{
    // V3.1：本地设置窗口已移至 taishen_server.exe；不可达时尝试拉起 server 并短等重试
    if (taishen::IsAppContainerProcess() || IsSecureMode() || IsUiElementOnlyMode()) {
        taishen::ForceLog("OpenSettingsWindow: skipped in AppContainer/Secure/UIElementOnly");
        return;
    }
    if (m_renderClient.SendOpenSettings(tabIndex, 250)) {
        taishen::DebugLog("OpenSettingsWindow: forwarded to taishen_server tab=" +
                          std::to_string(tabIndex));
        return;
    }
    const std::wstring dllDir = TaishenTsfGetDllDir();
    if (!dllDir.empty()) {
        m_renderClient.EnsureServerRunning(dllDir);
        for (int i = 0; i < 3; ++i) {
            Sleep(80);
            if (m_renderClient.SendOpenSettings(tabIndex, 150)) {
                taishen::DebugLog("OpenSettingsWindow: forwarded after server launch tab=" +
                                  std::to_string(tabIndex));
                return;
            }
        }
    }
    taishen::ForceLog("OpenSettingsWindow: taishen_server unreachable tab=" +
                      std::to_string(tabIndex));
}

void CTextService::OnLangBarOpenSettings(int tabIndex)
{
    OpenSettingsWindow(tabIndex);
}

void CTextService::ToggleAsciiMode()
{
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
    SyncCompartmentsFromState();
    RefreshLangBarItem();
    HideCandidateUi();
}

void CTextService::ToggleTraditional()
{
    const bool isContainer = taishen::IsAppContainerProcess();
    SyncRemoteModesFromPong();
    const bool next = !m_remoteState.traditional;
    m_remoteState.traditional = next;
    m_appCfg.traditional_enabled = next;
    m_remoteTradOverride = true;
    if (!isContainer) {
        const std::wstring dllDir = TaishenTsfGetDllDir();
        taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        cfg.traditional_enabled = next;
        taishen::SaveConfig(dllDir, cfg);
        m_renderClient.SendReloadConfig();
    }
    taishen::DebugLog("ToggleTraditional(remote) -> " + std::to_string(next ? 1 : 0));
}

void CTextService::ToggleShuangpin()
{
    const bool isContainer = taishen::IsAppContainerProcess();
    SyncRemoteModesFromPong();
    const bool next = !m_appCfg.shuangpin_mode;
    m_appCfg.shuangpin_mode = next;
    m_remoteShuangpinOverride = true;
    SendRemoteResetCommit();
    ClearRemoteState();
    HideCandidateUi();
    if (!isContainer) {
        const std::wstring dllDir = TaishenTsfGetDllDir();
        taishen::ImeConfig cfg = taishen::LoadConfig(dllDir);
        cfg.shuangpin_mode = next;
        taishen::SaveConfig(dllDir, cfg);
        m_renderClient.SendReloadConfig();
    }
    taishen::DebugLog("ToggleShuangpin(remote) -> " + std::to_string(next ? 1 : 0));
}
