/// TSF 语言栏按钮 — 输入指示器（任务栏右下角语言区）的图标来源
///
/// Windows 的输入指示器对 TSF TIP 的图标取自 ITfLangBarItemButton::GetIcon，
/// 光在注册表写 IconFile/IconIndex 不足以让图标出现（且需 DLL 内有 ICON 资源）。
/// 因此这里实现完整的 langbar item，并在 ActivateEx 时通过 ITfLangBarItemMgr
/// 挂到系统语言栏。

#pragma once

#include <windows.h>
#include <msctf.h>

class CTextService;

class CLangBarItemButton : public ITfLangBarItemButton
{
public:
    explicit CLangBarItemButton(CTextService* pService);
    virtual ~CLangBarItemButton();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // ITfLangBarItem
    STDMETHODIMP GetInfo(TF_LANGBARITEMINFO* pInfo) override;
    STDMETHODIMP GetStatus(DWORD* pdwStatus) override;
    STDMETHODIMP Show(BOOL fShow) override;
    STDMETHODIMP GetTooltipString(BSTR* pbstrToolTip) override;

    // ITfLangBarItemButton
    STDMETHODIMP OnClick(TfLBIClick click, POINT pt, const RECT* prcArea) override;
    STDMETHODIMP InitMenu(ITfMenu* pMenu) override;
    STDMETHODIMP OnMenuSelect(UINT wID) override;
    STDMETHODIMP GetIcon(HICON* phIcon) override;
    STDMETHODIMP GetText(BSTR* pbstrText) override;

private:
    LONG m_cRef;
    // 弱引用：生命周期由 CTextService 负责，不 AddRef（避免循环引用）
    CTextService* m_pService;
    BOOL m_fShown;
};
