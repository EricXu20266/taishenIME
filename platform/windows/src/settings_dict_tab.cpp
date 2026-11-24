/// 现代化设置窗体 — 页 8「词库与数据」（Q2.2）
///
/// 提供系统/扩展/用户词库状态查看、目录快捷打开、UTF-8 词表导出/导入与学习记录清空。

#include "settings_window.h"
#include "engine_bridge.h"
#include "settings_ui_helpers.h"

#include <commdlg.h>
#include <cstdio>
#include <shellapi.h>

namespace taishen {

namespace {

using settings_ui::ActionRowLayout;
using settings_ui::CardLayout;
using settings_ui::FormRow;
using settings_ui::HintText;

std::string ToUtf8(const std::wstring& w)
{
    if (w.empty()) {
        return std::string();
    }
    const int len = WideCharToMultiByte(
        CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
        nullptr, 0, nullptr, nullptr);
    if (len <= 0) {
        return std::string();
    }
    std::string out(static_cast<size_t>(len), '\0');
    WideCharToMultiByte(
        CP_UTF8, 0, w.c_str(), static_cast<int>(w.size()),
        &out[0], len, nullptr, nullptr);
    return out;
}

bool TryGetFileBytes(const std::wstring& path, ULONGLONG& outBytes)
{
    WIN32_FILE_ATTRIBUTE_DATA attrs{};
    if (!GetFileAttributesExW(path.c_str(), GetFileExInfoStandard, &attrs) ||
        (attrs.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        outBytes = 0;
        return false;
    }
    outBytes = (static_cast<ULONGLONG>(attrs.nFileSizeHigh) << 32) | attrs.nFileSizeLow;
    return true;
}

std::wstring FormatSizeText(ULONGLONG bytes)
{
    wchar_t buf[64] = {};
    if (bytes >= 1024ULL * 1024ULL) {
        swprintf_s(buf, L"%.2f MB", static_cast<double>(bytes) / (1024.0 * 1024.0));
    } else if (bytes >= 1024ULL) {
        swprintf_s(buf, L"%.1f KB", static_cast<double>(bytes) / 1024.0);
    } else {
        swprintf_s(buf, L"%llu B", bytes);
    }
    return buf;
}

std::wstring ParentDirOf(const std::wstring& path)
{
    const size_t pos = path.find_last_of(L"\\/");
    return (pos == std::wstring::npos) ? L"" : path.substr(0, pos + 1);
}

std::wstring ResolveAbsoluteWithBase(const std::wstring& relOrAbs, const std::wstring& baseDir)
{
    if (relOrAbs.empty()) {
        return L"";
    }
    if (relOrAbs.size() >= 2 &&
        ((relOrAbs[0] >= L'A' && relOrAbs[0] <= L'Z') ||
         (relOrAbs[0] >= L'a' && relOrAbs[0] <= L'z')) &&
        relOrAbs[1] == L':') {
        return relOrAbs;
    }
    return baseDir + relOrAbs;
}

std::wstring PickExportFilePath(HWND owner)
{
    wchar_t fileBuf[MAX_PATH] = L"taishen_user_dict.txt";
    OPENFILENAMEW ofn{};
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = L"UTF-8 文本词库 (*.txt;*.tsv)\0*.txt;*.tsv\0所有文件 (*.*)\0*.*\0";
    ofn.lpstrFile = fileBuf;
    ofn.nMaxFile = MAX_PATH;
    ofn.lpstrDefExt = L"txt";
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    return GetSaveFileNameW(&ofn) ? std::wstring(fileBuf) : std::wstring();
}

std::wstring PickImportFilePath(HWND owner)
{
    wchar_t fileBuf[MAX_PATH] = {};
    OPENFILENAMEW ofn{};
    ofn.lStructSize = sizeof(ofn);
    ofn.hwndOwner = owner;
    ofn.lpstrFilter = L"UTF-8 文本词库 (*.txt;*.tsv)\0*.txt;*.tsv\0所有文件 (*.*)\0*.*\0";
    ofn.lpstrFile = fileBuf;
    ofn.nMaxFile = MAX_PATH;
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    return GetOpenFileNameW(&ofn) ? std::wstring(fileBuf) : std::wstring();
}

} // namespace

void CSettingsWindow::BuildDictPage()
{
    UILayout* page = m_pageRoots[8];

    // 卡片 1：词库与数据概览
    auto* card1 = new CardLayout(L"词库与数据概览");
    m_lblSysDictInfo = new UILabel(L"检测中…");
    card1->AddChild(FormRow(L"系统词库", m_lblSysDictInfo, 96));

    m_lblExtraDictInfo = new UILabel(L"检测中…");
    card1->AddChild(FormRow(L"扩展词库", m_lblExtraDictInfo, 96));

    m_lblUserDictPath = new UILabel(L"默认 %APPDATA%\\taishen-ime\\user_dict.db");
    m_lblUserDictPath->SetDim(true);
    card1->AddChild(FormRow(L"用户词库位置", m_lblUserDictPath, 96));

    m_lblUserDictStats = new UILabel(L"统计中…");
    m_lblUserDictStats->SetBold(true);
    card1->AddChild(FormRow(L"学习数据统计", m_lblUserDictStats, 96));

    auto* dirActions = new ActionRowLayout();
    auto* btnOpenUserDir = new UIButton(L"打开用户数据目录");
    btnOpenUserDir->SetOnClick([this]() {
        ImeConfig tmp = m_cfg;
        if (m_editUserDictPath != nullptr) {
            tmp.user_dict_path = m_editUserDictPath->Text();
        }
        const std::wstring dbPath = ResolveUserDictPath(tmp, m_dllDir);
        const std::wstring dir = ParentDirOf(dbPath);
        if (!dir.empty()) {
            CreateDirectoryW(dir.c_str(), nullptr);
            ShellExecuteW(Hwnd(), L"open", dir.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
        }
    });
    auto* btnOpenSysDir = new UIButton(L"打开词库目录");
    btnOpenSysDir->SetOnClick([this]() {
        ShellExecuteW(Hwnd(), L"open", m_dllDir.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    });
    auto* btnRefresh = new UIButton(L"刷新统计");
    btnRefresh->SetOnClick([this]() { RefreshDictStats(); });
    dirActions->AddChild(btnOpenUserDir);
    dirActions->AddChild(btnOpenSysDir);
    dirActions->AddChild(btnRefresh);
    card1->AddChild(dirActions);
    page->AddChild(card1);

    // 卡片 2：用户词库导入 / 导出 / 清理
    auto* card2 = new CardLayout(L"用户词库管理");
    card2->AddChild(new HintText(
        L"支持 UTF-8 文本词表导入导出（每行「词条<Tab>拼音<Tab>词频」或「词条 拼音」），导入或清空立即生效。",
        2));
    auto* dictActions = new ActionRowLayout();
    auto* btnExport = new UIButton(L"导出用户词库…");
    btnExport->SetOnClick([this]() { OnExportUserDict(); });
    auto* btnImport = new UIButton(L"导入用户词库…");
    btnImport->SetOnClick([this]() { OnImportUserDict(); });
    auto* btnClear = new UIButton(L"清空学习记录");
    btnClear->SetOnClick([this]() { OnClearUserDict(); });
    dictActions->AddChild(btnExport);
    dictActions->AddChild(btnImport);
    dictActions->AddChild(btnClear);
    card2->AddChild(dictActions);
    page->AddChild(card2);

    // 卡片 3：自定义文件路径
    auto* card3 = new CardLayout(L"路径配置（高级）");
    m_editUserDictPath = new UIEdit();
    m_editUserDictPath->SetPlaceholder(L"留空 = 默认 %APPDATA%\\taishen-ime\\user_dict.db");
    card3->AddChild(FormRow(L"用户词库路径", m_editUserDictPath, 110));

    m_editPhrasePath = new UIEdit();
    m_editPhrasePath->SetPlaceholder(L"留空 = 仅内置快捷短语（每行 code=text）");
    card3->AddChild(FormRow(L"自定义短语文件", m_editPhrasePath, 110));

    auto* phraseActions = new ActionRowLayout();
    auto* btnOpenPhrase = new UIButton(L"打开短语文件");
    btnOpenPhrase->SetOnClick([this]() {
        const std::wstring raw = m_editPhrasePath != nullptr ? m_editPhrasePath->Text() : L"";
        const std::wstring full = ResolveAbsoluteWithBase(raw, m_dllDir);
        ULONGLONG sz = 0;
        if (full.empty() || !TryGetFileBytes(full, sz)) {
            MessageBoxW(Hwnd(), L"尚未配置自定义短语文件，或指定路径尚不存在。",
                        L"泰深输入法", MB_OK | MB_ICONINFORMATION);
            return;
        }
        ShellExecuteW(Hwnd(), L"open", full.c_str(), nullptr, nullptr, SW_SHOWNORMAL);
    });
    phraseActions->AddChild(btnOpenPhrase);
    card3->AddChild(phraseActions);
    card3->AddChild(new HintText(
        L"修改「用户词库路径」需点击「确定」保存并重启输入法服务生效；导出/导入/清空即时写入。",
        2));
    page->AddChild(card3);
}

void CSettingsWindow::RefreshDictStats()
{
    if (m_lblSysDictInfo == nullptr) {
        return;
    }
    std::wstring sysPath = ResolveDictPath(m_cfg, m_dllDir);
    if (sysPath.empty()) {
        const std::wstring cand = m_dllDir + L"system_dict.db";
        ULONGLONG tmp = 0;
        if (TryGetFileBytes(cand, tmp)) {
            sysPath = cand;
        }
    }
    ULONGLONG sysBytes = 0;
    if (!sysPath.empty() && TryGetFileBytes(sysPath, sysBytes)) {
        const int entries = engine_dict_entry_count();
        std::wstring text = L"已就绪（" + FormatSizeText(sysBytes);
        if (entries > 0) {
            text += L"，" + std::to_wstring(entries) + L" 条";
        }
        text += L"）";
        m_lblSysDictInfo->SetText(text);
    } else {
        m_lblSysDictInfo->SetText(L"内置最小词库（同目录未放置 system_dict.db）");
    }

    const wchar_t* const kExtraFiles[] = { L"common.db", L"domains.db", L"bigram.db" };
    int extraFound = 0;
    ULONGLONG extraBytes = 0;
    for (const wchar_t* name : kExtraFiles) {
        ULONGLONG b = 0;
        if (TryGetFileBytes(m_dllDir + name, b)) {
            ++extraFound;
            extraBytes += b;
        }
    }
    m_lblExtraDictInfo->SetText(
        extraFound > 0
            ? (std::to_wstring(extraFound) + L"/3 个就绪（" + FormatSizeText(extraBytes) + L"）")
            : L"未检测到扩展词库文件（common/domains/bigram）");

    ImeConfig tmpCfg = m_cfg;
    if (m_editUserDictPath != nullptr) {
        tmpCfg.user_dict_path = m_editUserDictPath->Text();
    }
    const std::wstring userDb = ResolveUserDictPath(tmpCfg, m_dllDir);
    m_lblUserDictPath->SetText(userDb.empty() ? L"未配置" : userDb);

    int wordCount = 0;
    int pairCount = 0;
    const std::string userDbUtf8 = ToUtf8(userDb);
    const int rc = engine_user_dict_stats(
        userDbUtf8.empty() ? nullptr : userDbUtf8.c_str(), &wordCount, &pairCount);
    ULONGLONG userBytes = 0;
    const bool exists = !userDb.empty() && TryGetFileBytes(userDb, userBytes);
    if (rc == 0) {
        std::wstring msg = std::to_wstring(wordCount) + L" 条用户词 · " +
                           std::to_wstring(pairCount) + L" 条搭配";
        if (exists) {
            msg += L"（" + FormatSizeText(userBytes) + L"）";
        }
        m_lblUserDictStats->SetText(msg);
    } else {
        m_lblUserDictStats->SetText(L"无法读取用户词库统计");
    }
}

void CSettingsWindow::ApplyDictPageToUI()
{
    if (m_editUserDictPath != nullptr) {
        m_editUserDictPath->SetText(m_cfg.user_dict_path);
    }
    if (m_editPhrasePath != nullptr) {
        m_editPhrasePath->SetText(m_cfg.phrase_path);
    }
    RefreshDictStats();
}

void CSettingsWindow::CollectDictPageFromUI()
{
    if (m_editUserDictPath != nullptr) {
        m_cfg.user_dict_path = m_editUserDictPath->Text();
    }
    if (m_editPhrasePath != nullptr) {
        m_cfg.phrase_path = m_editPhrasePath->Text();
    }
}

void CSettingsWindow::OnExportUserDict()
{
    const std::wstring outPath = PickExportFilePath(Hwnd());
    if (outPath.empty()) {
        return;
    }
    ImeConfig tmpCfg = m_cfg;
    if (m_editUserDictPath != nullptr) {
        tmpCfg.user_dict_path = m_editUserDictPath->Text();
    }
    const std::wstring userDb = ResolveUserDictPath(tmpCfg, m_dllDir);
    const std::string dbUtf8 = ToUtf8(userDb);
    const std::string outUtf8 = ToUtf8(outPath);
    const int exported = engine_export_user_dict(
        dbUtf8.empty() ? nullptr : dbUtf8.c_str(), outUtf8.c_str());
    if (exported >= 0) {
        const std::wstring msg = L"已导出 " + std::to_wstring(exported) + L" 条用户词到：\n" + outPath;
        MessageBoxW(Hwnd(), msg.c_str(), L"泰深输入法", MB_OK | MB_ICONINFORMATION);
    } else {
        MessageBoxW(Hwnd(), L"导出用户词库失败，请检查目标路径权限。", L"泰深输入法",
                    MB_OK | MB_ICONWARNING);
    }
}

void CSettingsWindow::OnImportUserDict()
{
    const std::wstring inPath = PickImportFilePath(Hwnd());
    if (inPath.empty()) {
        return;
    }
    ImeConfig tmpCfg = m_cfg;
    if (m_editUserDictPath != nullptr) {
        tmpCfg.user_dict_path = m_editUserDictPath->Text();
    }
    const std::wstring userDb = ResolveUserDictPath(tmpCfg, m_dllDir);
    const std::string dbUtf8 = ToUtf8(userDb);
    const std::string inUtf8 = ToUtf8(inPath);
    const int imported = engine_import_user_dict(
        dbUtf8.empty() ? nullptr : dbUtf8.c_str(), inUtf8.c_str());
    RefreshDictStats();
    if (imported >= 0) {
        const std::wstring msg = L"成功导入 " + std::to_wstring(imported) + L" 条用户词，已即时生效。";
        MessageBoxW(Hwnd(), msg.c_str(), L"泰深输入法", MB_OK | MB_ICONINFORMATION);
    } else {
        MessageBoxW(Hwnd(), L"导入失败：请确认文件为 UTF-8 编码，格式为「词条<Tab>拼音」。",
                    L"泰深输入法", MB_OK | MB_ICONWARNING);
    }
}

void CSettingsWindow::OnClearUserDict()
{
    const int choice = MessageBoxW(
        Hwnd(),
        L"确定要清空全部自学习用户词条与上下文搭配记录吗？\n此操作不可撤销（系统词库不受影响）。",
        L"泰深输入法 - 清空学习记录",
        MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2);
    if (choice != IDYES) {
        return;
    }
    ImeConfig tmpCfg = m_cfg;
    if (m_editUserDictPath != nullptr) {
        tmpCfg.user_dict_path = m_editUserDictPath->Text();
    }
    const std::wstring userDb = ResolveUserDictPath(tmpCfg, m_dllDir);
    const std::string dbUtf8 = ToUtf8(userDb);
    const int rc = engine_clear_user_dict(dbUtf8.empty() ? nullptr : dbUtf8.c_str());
    RefreshDictStats();
    if (rc == 0) {
        MessageBoxW(Hwnd(), L"已清空全部用户学习记录。", L"泰深输入法", MB_OK | MB_ICONINFORMATION);
    } else {
        MessageBoxW(Hwnd(), L"清空用户词库失败。", L"泰深输入法", MB_OK | MB_ICONWARNING);
    }
}

} // namespace taishen
