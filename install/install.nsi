; ============================================================
;  泰深输入法 — NSIS 安装器蓝图（MVP 二期构建用）
;  环境要求：NSIS 3.x（https://nsis.sourceforge.io）
;  构建：makensis install.nsi → TaishenIME-Setup.exe
;  前置：先执行 platform/windows 的 CMake 构建，产物在 out\
; ============================================================

!include "MUI2.nsh"

; ---- 基本配置 ----
Name "泰深输入法"
OutFile "TaishenIME-Setup.exe"
InstallDir "$LOCALAPPDATA\TaishenIME"
RequestExecutionLevel user          ; HKCU 注册，无需管理员
Unicode true

; ---- 界面 ----
!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; ---- 安装段 ----
Section "Install"
    ; 1. 复制 DLL / 词库 / 卸载器
    SetOutPath "$INSTDIR"
    File "..\platform\windows\out\taishen_ime.dll"
    File "..\resources\system_dict.db"
    File "${NSISDIR}\Uninst.exe"  ; 占位，实际用 WriteUninstaller

    ; 2. 生成 config.ini（不存在时）
    IfFileExists "$INSTDIR\config.ini" +2
        FileOpen $0 "$INSTDIR\config.ini" w
        FileWrite $0 "# 泰深输入法配置$\r$\ncandidate_count=9$\r$\ndict_path=system_dict.db$\r$\n"
        FileClose $0

    ; 3. 注册 TSF COM 组件
    ExecWait 'regsvr32 /s "$INSTDIR\taishen_ime.dll"'

    ; 4. 卸载器
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; 5. 开始菜单快捷方式
    CreateDirectory "$SMPROGRAMS\泰深输入法"
    CreateShortcut "$SMPROGRAMS\泰深输入法\卸载泰深输入法.lnk" "$INSTDIR\uninstall.exe"

    ; 6. 注册表卸载信息（控制面板可卸载）
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "DisplayName" "泰深输入法"
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "NoModify" 1
    WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" \
        "NoRepair" 1
SectionEnd

; ---- 卸载段 ----
Section "Uninstall"
    ; 1. 注销 TSF COM 组件
    ExecWait 'regsvr32 /s /u "$INSTDIR\taishen_ime.dll"'

    ; 2. 删除文件与目录
    Delete "$INSTDIR\taishen_ime.dll"
    Delete "$INSTDIR\system_dict.db"
    Delete "$INSTDIR\config.ini"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    ; 3. 删除开始菜单与卸载信息
    RMDir /r "$SMPROGRAMS\泰深输入法"
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME"

    ; 注意：%APPDATA%\taishen-ime 用户数据保留（词库学习/日志二期）
SectionEnd
