/// Windows TSF IME — ITfKeyEventSink、快捷键本地快判与远程 IPC 转发
///
/// 对应 DEV-TRACKER: V4.1–V4.2

#include "tsf_text_service.h"

#include "app_container.h"
#include "app_state.h"
#include "debug_log.h"

// ---------------------------------------------------------------------------
// ITfKeyEventSink — 预判与焦点同步
// ---------------------------------------------------------------------------

STDMETHODIMP CTextService::OnSetFocus(BOOL fForeground)
{
    taishen::DebugLog(std::string("OnSetFocus(fForeground=") +
                      (fForeground ? "TRUE" : "FALSE") + ")");
    const bool isContainer = taishen::IsAppContainerProcess();
    if (!isContainer) {
        // 切焦点时检查配置变更，跨进程同步不依赖定时器
        ReloadConfigIfChanged();
    }
    if (!fForeground) {
        m_hasLastCaretRect = false;
        ++m_caretSeq;
        taishen::g_lastCommitEndsWithDigit = false;
        m_lastDigitPunctVk = 0;
        if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
            if (m_pFocusContext != nullptr && m_composition.IsActive()) {
                RunCompositionOp(m_pFocusContext,
                                 CEditSessionComposition::Op::Commit, "");
            }
            SendRemoteResetCommit();
            ClearRemoteState();
        }
        HideCandidateUi();
    }
    return S_OK;
}

STDMETHODIMP CTextService::OnTestKeyDown(ITfContext* pic, WPARAM wParam,
                                         LPARAM lParam, BOOL* pfEaten)
{
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo ||
        IsSecureMode()) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }
    if (pic != nullptr && pic != m_lastInputScopeContext) {
        QueryContextInputScope(pic);
    }
    if (m_inputScopeAscii || IsContextKeyboardDisabled(pic)) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    const int effVk = taishen::NormalizeKeypad(static_cast<int>(wParam));
    const bool vimPass = taishen::AppStateIsVimForeground(m_appCfg);
    if (m_renderClient.IsProtocolIncompatible()) {
        m_renderClient.TryReconnectIfMismatchDue();
    }
    const bool protocolIncompatible = m_renderClient.IsProtocolIncompatible();
    const bool serverUnavailable =
        m_renderClient.IsInHardBackoff() ||
        !m_renderClient.IsServerDictReady() ||
        protocolIncompatible;
    bool eat = taishen::ShouldEatKeyRemote(
        effVk, vimPass, static_cast<int>(wParam),
        m_remoteState, serverUnavailable, protocolIncompatible);
    const bool shiftDown = ((GetKeyState(VK_SHIFT) & 0x8000) != 0) ||
                           taishen::KeypadShiftEquivalent(static_cast<int>(wParam));
    const bool ctrlDown = (GetKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool altDown = (GetKeyState(VK_MENU) & 0x8000) != 0;
    const ULONGLONG nowTick = GetTickCount64();
    if (eat && effVk == m_lastDigitPunctVk && lParam == m_lastDigitPunctLParam &&
        (nowTick - m_lastDigitPunctTick) <= 50ULL) {
        eat = false;
    }
    if (!eat) {
        if (taishen::IsDigitFollowPunctKey(effVk, shiftDown)) {
            m_lastDigitPunctVk = effVk;
            m_lastDigitPunctLParam = lParam;
            m_lastDigitPunctTick = nowTick;
        }
        taishen::UpdateDigitFollowOnPassthrough(
            effVk, static_cast<int>(wParam), shiftDown, ctrlDown, altDown);
    }
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
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    if (m_renderClient.IsProtocolIncompatible()) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    const bool vimPass = taishen::AppStateIsVimForeground(m_appCfg);
    if (pfEaten != nullptr) {
        *pfEaten = (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) ||
                   (vimPass && taishen::IsVimModeKey(static_cast<int>(wParam)))
                       ? TRUE
                       : FALSE;
    }
    return S_OK;
}

// ---------------------------------------------------------------------------
// 远程引擎状态镜像与提交辅助
// ---------------------------------------------------------------------------

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
    m_remoteState.cursor = resp.cursor;
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
    // 全角转换已在 tsf_composition.cpp 提交路径内统一执行
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

// ---------------------------------------------------------------------------
// 按键主流程（本地快捷键/标点快判 + 50ms Server IPC 查询）
// ---------------------------------------------------------------------------

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
        m_inputScopeAscii = false;
        m_lastInputScopeContext = nullptr;
        taishen::DebugLog("OnKeyDown(remote): focus context synced from key event");
    }

    if (IsSecureMode() || IsContextKeyboardDisabled(pic)) {
        HideCandidateUi();
        return finish(false);
    }
    if (pic != nullptr && pic != m_lastInputScopeContext) {
        QueryContextInputScope(pic);
    }
    if (m_inputScopeAscii) {
        if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
            SendRemoteResetCommit();
            ClearRemoteState();
        }
        HideCandidateUi();
        return finish(false);
    }

    const bool isContainer = taishen::IsAppContainerProcess();
    if (!isContainer) {
        ReloadConfigIfChanged();
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

    if (m_renderClient.IsProtocolIncompatible()) {
        if (!m_remoteState.rawPinyin.empty() && pic != nullptr &&
            m_composition.IsActive()) {
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit,
                             m_remoteState.rawPinyin);
        } else if (pic != nullptr && m_composition.IsActive()) {
            RunCompositionOp(pic, CEditSessionComposition::Op::Commit, "");
        }
        ClearRemoteState();
        HideCandidateUi();
        return finish(false);
    }

    // 本地快捷键（0 IPC 往返）
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
        SyncCompartmentsFromState();
        RefreshLangBarItem();
        HideCandidateUi();
        m_shiftTapArmed = false;
        return finish(true);
    }

    if (effVk == VK_SPACE && shiftDown && !ctrlDown && kb.toggleFullwidth == 1) {
        taishen::SetFullWidthMode(!taishen::IsFullWidthMode());
        SyncCompartmentsFromState();
        m_shiftTapArmed = false;
        return finish(true);
    }

    if (effVk == VK_OEM_PERIOD && ctrlDown && !shiftDown && kb.togglePunct == 1) {
        m_remoteState.asciiPunct = !m_remoteState.asciiPunct;
        SyncCompartmentsFromState();
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
                (m_remoteState.candidates[0].reasons &
                 taishen::ipc::kReasonPrefixWord) != 0;
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
        if (m_remoteState.asciiMode || m_remoteState.asciiPunct ||
            ctrlDown || altDown) {
            return false;
        }
        if (taishen::IsDigitFollowPunctKey(effVk, shiftDown)) {
            // 只跟随一个标点：消费掉状态，随后连续标点恢复全角
            taishen::g_lastCommitEndsWithDigit = false;
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

    // hard backoff 或服务端词库未就绪且无活跃输入时直接透传
    if ((m_renderClient.IsInHardBackoff() || !m_renderClient.IsServerDictReady()) &&
        m_remoteState.rawPinyin.empty() && !m_remoteState.inCompose) {
        return finish(false);
    }

    bool routeToServer = false;
    bool isPaginationKey = false;
    bool collapseMultiRow = false;

    if (m_remoteState.inCompose &&
        (effVk == VK_RETURN || (effVk == VK_BACK && !ctrlDown))) {
        routeToServer = true;
    }

    if (!routeToServer &&
        (m_remoteState.inputMode == 1 || m_remoteState.inputMode == 2 ||
         m_remoteState.inputMode == 3)) {
        const bool isSpecialDigit =
            (!shiftRaw || effVk == '8' || effVk == '9' || effVk == '0') &&
            (effVk >= '0' && effVk <= '9');
        const bool isSpecialCalcOp =
            (m_remoteState.inputMode == 1 || m_remoteState.inputMode == 2) &&
            ((keypad &&
              (rawVk == VK_ADD || rawVk == VK_SUBTRACT ||
               rawVk == VK_MULTIPLY || rawVk == VK_DIVIDE ||
               rawVk == VK_DECIMAL || rawVk == VK_SEPARATOR)) ||
             (!shiftRaw &&
              (effVk == VK_OEM_PERIOD || effVk == VK_OEM_PLUS ||
               effVk == VK_OEM_MINUS || effVk == VK_OEM_2 ||
               effVk == VK_OEM_5)) ||
             (shiftRaw && effVk == '6'));
        if (isSpecialDigit || isSpecialCalcOp) {
            routeToServer = true;
        }
    }

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

    if (!routeToServer && (effVk == VK_OEM_4 || effVk == VK_OEM_6) &&
        !shiftDown && kb.pageBrackets && !m_remoteState.candidates.empty()) {
        routeToServer = true;
    }

    if (!routeToServer && effVk == VK_OEM_1 && !shiftRaw &&
        m_remoteState.semicolonKeyActive) {
        routeToServer = true;
        collapseMultiRow = true;
    }

    if (!routeToServer) {
        if (tryHandleRemotePunct()) {
            return finish(true);
        }

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
                const std::wstring pyWide =
                    taishen::Utf8ToWide(m_remoteState.rawPinyin);
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
                ((m_remoteState.candidates[0].reasons &
                  taishen::ipc::kReasonPrefixWord) == 0);
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
                        const std::string chosen =
                            m_remoteState.candidates[idx].text;
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
                } else if (!m_remoteState.rawPinyin.empty() ||
                           m_remoteState.inCompose) {
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
        } else if (effVk == VK_LEFT || effVk == VK_RIGHT ||
                   effVk == VK_HOME || effVk == VK_END) {
            if (!m_remoteState.rawPinyin.empty() && !altDown) {
                routeToServer = true;
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

    // 同步 SendKeyInput（50ms 严格上限，超时立即透传）
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
        taishen::ForceLog(
            "RemoteKeyInput FAILED/TIMEOUT vk=" + std::to_string(rawVk) +
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
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    return HandleRemoteKeyDown(pic, wParam, lParam, pfEaten);
}

STDMETHODIMP CTextService::OnKeyUp(ITfContext* pic, WPARAM wParam,
                                   LPARAM /*lParam*/, BOOL* pfEaten)
{
    if (GetMessageExtraInfo() == taishen::kSynthesizedExtraInfo) {
        if (pfEaten != nullptr) {
            *pfEaten = FALSE;
        }
        return S_OK;
    }

    // 0.2.26 Shift tap 中英切换（<500ms、无其他键、无 Ctrl/Alt 组合）
    if (wParam == VK_SHIFT || wParam == VK_LSHIFT || wParam == VK_RSHIFT) {
        const bool armed = m_shiftTapArmed;
        const bool combo = m_shiftComboUsed;
        const DWORD dur = GetTickCount() - m_shiftDownTick;
        m_shiftTapArmed = false;
        m_shiftComboUsed = false;
        m_shiftActive = false;
        const bool curAscii = m_remoteState.asciiMode;
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
                SyncCompartmentsFromState();
                RefreshLangBarItem();
            }
        }
        m_shiftTapArmed = false;
    }

    // V0.2.36 vim_mode：vim 键（Esc/Ctrl+C/Ctrl+[）keyup 到达后切换为英文
    if (taishen::AppStateIsVimForeground(m_appCfg) &&
        taishen::IsVimModeKey(static_cast<int>(wParam))) {
        if (!m_remoteState.asciiMode) {
            m_remoteState.asciiMode = true;
            taishen::AppStateSetAsciiRemote(true);
            if (!m_remoteState.rawPinyin.empty() || m_remoteState.inCompose) {
                SendRemoteResetCommit();
                ClearRemoteState();
            }
            HideCandidateUi();
            SyncCompartmentsFromState();
            RefreshLangBarItem();
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
