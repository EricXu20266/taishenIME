/// Direct2D 候选窗口 UI Automation (UIA) 提供器（Q3.1）
///
/// 仅用于 `taishen_server.exe`（候选窗宿主）与服务端测试，不链接入 IME DLL。
/// 动态加载 `uiautomationcore.dll`（不引入静态导入库依赖），无屏幕阅读器/UIA 客户端时零开销。

#pragma once

#include <UIAutomation.h>
#include <windows.h>

#include <string>

namespace taishen {

/// CCandidateWindow 向 UIA Provider 暴露的只读/交互宿主机接口
class ICandidateUiaHost {
public:
    virtual ~ICandidateUiaHost() = default;

    virtual HWND UiaHwnd() const = 0;
    virtual std::wstring UiaPinyin() const = 0;
    virtual size_t UiaCandidateCount() const = 0;
    virtual std::wstring UiaCandidateText(size_t index) const = 0;
    virtual int UiaSelectedIndex() const = 0;
    virtual int UiaPage() const = 0;
    virtual int UiaTotalPages() const = 0;
    virtual RECT UiaItemScreenRect(size_t index) const = 0;
    virtual int UiaHitTestScreenPoint(int xScreen, int yScreen) const = 0;
    virtual void UiaSelectCandidate(int index) = 0;
};

/// 候选窗 UIA 根提供器前向声明（COM 对象，引用计数管理）
class CandidateWindowUiaProvider;

/// 创建或复用根 UIA Provider；宿主的生命周期结束时必须调用 Detach() + Release()
CandidateWindowUiaProvider* CreateCandidateWindowUiaProvider(ICandidateUiaHost* host);

void DetachCandidateWindowUiaProvider(CandidateWindowUiaProvider* provider);
void ReleaseCandidateWindowUiaProvider(CandidateWindowUiaProvider* provider);

LRESULT HandleCandidateUiaWmGetObject(CandidateWindowUiaProvider* provider,
                                      HWND hwnd,
                                      WPARAM wParam,
                                      LPARAM lParam);

HRESULT QueryCandidateWindowUiaSimple(CandidateWindowUiaProvider* provider,
                                      IRawElementProviderSimple** ppOut);

void NotifyCandidateUiaUpdated(CandidateWindowUiaProvider* provider);
void NotifyCandidateUiaSelectionChanged(CandidateWindowUiaProvider* provider);

} // namespace taishen
