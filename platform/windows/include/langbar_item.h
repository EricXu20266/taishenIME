/// TSF 语言栏按钮 — 输入指示器 + 中英切换按钮
///
/// Windows 的输入指示器对 TSF TIP 的图标取自 ITfLangBarItemButton::GetIcon，
/// 光在注册表写 IconFile/IconIndex 不足以让图标出现（且需 DLL 内有 ICON 资源）。
/// 因此这里实现完整的 langbar item，并在 ActivateEx 时通过 ITfLangBarItemMgr
/// 挂到系统语言栏。
///
/// 对齐微软拼音的两个图标：
///   Kind::Brand — 输入法标识（蓝底"泰"）
///   Kind::Mode  — 中英切换按钮（蓝底"中" / 灰底"英"，点击切换）

#pragma once

#include <windows.h>
#include <msctf.h>

/// 语言栏按钮 → 宿主（CTextService）的回调。
/// 用接口而不是直接持有 CTextService*，避免 langbar 代码依赖 TextService 实现细节。
class ILangBarHost
{
public:
    virtual ~ILangBarHost() = default;

    /// 点击中英按钮：切换当前中英模式
    virtual void OnLangBarToggleAscii() = 0;

    /// 当前是否英文模式（决定按钮显示"英"还是"中"）
    virtual bool IsAsciiMode() const = 0;

    /// 请宿主重新挂载语言栏按钮（系统据此重新调用 GetIcon）
    virtual void OnLangBarNeedRefresh() = 0;

    /// 切换浮动工具栏显示（写回配置，跨进程生效）
    virtual void OnLangBarToggleToolbar() = 0;
};

class CLangBarItemButton : public ITfLangBarItemButton, public ITfSource
{
public:
    enum class Kind
    {
        Brand,  // 输入法标识图标
        Mode,   // 中/英切换按钮
    };

    CLangBarItemButton(ILangBarHost* pHost, Kind kind);
    virtual ~CLangBarItemButton();

    // IUnknown
    STDMETHODIMP QueryInterface(REFIID riid, void** ppvObj) override;
    STDMETHODIMP_(ULONG) AddRef() override;
    STDMETHODIMP_(ULONG) Release() override;

    // ITfSource（微软 TSF 语言栏对象的硬性要求：语言栏 AddItem 时会
    // AdviseSink(IID_ITfLangBarItemSink)，对象必须能接受，否则 AddItem 直接 E_FAIL）
    STDMETHODIMP AdviseSink(REFIID riid, IUnknown* punk, DWORD* pdwCookie) override;
    STDMETHODIMP UnadviseSink(DWORD dwCookie) override;

    /// 状态变化后请语言栏重绘（图标/文本/提示）。
    /// @return false = 语言栏还没装上 sink（调用方应回退 RemoveItem/AddItem）
    bool NotifyUpdate();

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
    ILangBarHost* m_pHost;
    Kind m_kind;
    BOOL m_fShown;
    ITfLangBarItemSink* m_pSink;   // 语言栏装上的 sink（ITfSource）
    DWORD m_dwSinkCookie;
};
