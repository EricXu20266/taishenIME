; ============================================================
;  泰深输入法 — NSIS 安装器
;  版本：${VERSION}
;  构建：makensis /DVERSION=0.2.0 install.nsi
;  前置：先执行 CMake Release 构建（platform\windows\out\taishen_ime.dll）
; ============================================================

!include "MUI2.nsh"
!include "FileFunc.nsh"

; ---- 版本（命令行 /DVERSION=x.y.z 传入，默认 0.2.0） ----
!ifndef VERSION
  !define VERSION "0.2.0"
!endif

; ---- 基本配置 ----
Name "泰深输入法 ${VERSION}"
OutFile "TaishenIME-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\TaishenIME"
RequestExecutionLevel admin    ; TSF 系统注册需要管理员权限
Unicode true
BrandingText "泰深输入法"

; ---- 编译时来源目录 ----
!define SRC_DLL      "..\platform\windows\out\taishen_ime.dll"
!define SRC_DICT     "..\resources\system_dict.db"
!define SRC_DICT_BIN "..\resources\system_dict.db.bin"
!define SRC_DOMAINS  "..\resources\domains\domains.db"
!define SRC_COMMON   "..\resources\common.db"

; ---- 界面 ----
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_LINK "泰深输入法项目主页"
!define MUI_FINISHPAGE_LINK_LOCATION "https://github.com/EricXu20266/taishenIME"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; ---- 安装段 ----
Section "Install"
    SetOutPath "$INSTDIR"

    ; === 1. 复制核心文件 ===
    DetailPrint "安装 DLL..."
    File "${SRC_DLL}"

    DetailPrint "安装系统词库..."
    File "${SRC_DICT}"
    File /nonfatal "${SRC_DICT_BIN}"
    File /nonfatal "${SRC_DOMAINS}"
    File /nonfatal "${SRC_COMMON}"

    ; === 2. 生成 config.ini（首次安装时；已有则跳过） ===
    ${IfNot} ${FileExists} "$INSTDIR\config.ini"
        DetailPrint "生成默认配置..."
        FileOpen $0 "$INSTDIR\config.ini" w
        FileWrite $0 "# 泰深输入法配置$\r$\n"
        FileWrite $0 "# 修改后 2 秒内自动生效，无需重启$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 候选词数量（1-20，默认 5）$\r$\n"
        FileWrite $0 "candidate_count=5$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 系统词库路径（相对 DLL 目录；留空 = 内置词库）$\r$\n"
        FileWrite $0 "dict_path=system_dict.db$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 用户词库路径（留空 = %APPDATA%/taishen-ime/user_dict.db）$\r$\n"
        FileWrite $0 "# user_dict_path=$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 模糊音（平翘舌/前后鼻音/n-l/f-h，默认开）$\r$\n"
        FileWrite $0 "# fuzzy=1$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 智能纠错（键盘相邻键容错，默认开）$\r$\n"
        FileWrite $0 "# correction=1$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 中英混输（中文模式候选末尾加英文，默认开）$\r$\n"
        FileWrite $0 "# mix_mode=1$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 简繁转换（开启后候选/上屏为繁体，默认关）$\r$\n"
        FileWrite $0 "# traditional=0$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 快捷短语（简码→常用文本，默认开）$\r$\n"
        FileWrite $0 "# phrase=1$\r$\n"
        FileWrite $0 "# phrase_path=phrases.txt$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 候选窗口主题色（HEX RRGGBB）$\r$\n"
        FileWrite $0 "# 深色默认：2E2E2E/E8E8E8/1E6FFF/9A9A9A$\r$\n"
        FileWrite $0 "# 浅色示例：$\r$\n"
        FileWrite $0 "# theme_bg=F5F5F5$\r$\n"
        FileWrite $0 "# theme_text=333333$\r$\n"
        FileWrite $0 "# theme_highlight=0078D4$\r$\n"
        FileWrite $0 "# theme_dim=999999$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 双拼模式（微软双拼方案，默认关）$\r$\n"
        FileWrite $0 "# shuangpin=0$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 候选窗字体（默认 Microsoft YaHei）$\r$\n"
        FileWrite $0 "# font_face=Microsoft YaHei$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 候选窗字号（px，12-32，默认 16）$\r$\n"
        FileWrite $0 "# font_size=16$\r$\n"
        FileWrite $0 "$\r$\n"
        FileWrite $0 "# 行内预编辑（拼音写光标处，候选窗不重复，默认开）$\r$\n"
        FileWrite $0 "# inline_preedit=1$\r$\n"
        FileClose $0
    ${EndIf}

    ; === 3. 注册 TSF COM 组件 ===
    DetailPrint "注册输入法组件..."
    ExecWait 'regsvr32 /s "$INSTDIR\taishen_ime.dll"'
    Sleep 500

    ; === 4. 写入卸载器 ===
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; === 5. 开始菜单 ===
    CreateDirectory "$SMPROGRAMS\泰深输入法"
    CreateShortcut "$SMPROGRAMS\泰深输入法\卸载泰深输入法.lnk" \
        "$INSTDIR\uninstall.exe"

    ; === 6. 控制面板卸载信息 ===
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "DisplayName" "泰深输入法 ${VERSION}"
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "Publisher" "泰深"
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "DisplayIcon" "$INSTDIR\taishen_ime.dll"
    WriteRegDWORD HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "NoModify" 1
    WriteRegDWORD HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "NoRepair" 1
    WriteRegDWORD HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "EstimatedSize" $0
    WriteRegDWORD HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "VersionMajor" 0
    WriteRegDWORD HKLM \
        "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "VersionMinor" 2
SectionEnd

; ---- 卸载段 ----
Section "Uninstall"
    ; === 1. 注销 TSF COM ===
    ExecWait 'regsvr32 /s /u "$INSTDIR\taishen_ime.dll"'
    Sleep 500

    ; === 2. 删除安装文件 ===
    Delete "$INSTDIR\taishen_ime.dll"
    Delete "$INSTDIR\system_dict.db"
    Delete "$INSTDIR\system_dict.db.bin"
    Delete "$INSTDIR\domains.db"
    Delete "$INSTDIR\common.db"
    Delete "$INSTDIR\config.ini"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    ; === 3. 清理开始菜单 ===
    RMDir /r "$SMPROGRAMS\泰深输入法"

    ; === 4. 清理注册表 ===
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME"

    ; 注意：用户数据 (%APPDATA%\taishen-ime) 保留不删，包含 user_dict.db 和日志
SectionEnd
