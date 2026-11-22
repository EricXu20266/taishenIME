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

; 安装时是否清掉了旧版 IMM32 注册（用于完成前提告知）
Var HadLegacyImm32

; ---- 基本配置 ----
Name "泰深输入法 ${VERSION}"
OutFile "TaishenIME-Setup-${VERSION}.exe"
InstallDir "$LOCALAPPDATA\TaishenIME"
RequestExecutionLevel admin    ; TSF 系统注册需要管理员权限
Unicode true
BrandingText "泰深输入法"

; ---- 编译时来源目录 ----
!define SRC_DLL      "..\platform\windows\out\taishen_ime.dll"
!define SRC_DLL_X86  "..\platform\windows\out\taishen_ime_x86.dll"
!define SRC_IMM32    "..\platform\windows\out\taishen_ime_imm32.ime"
!define SRC_IMM32_X86 "..\platform\windows\out\taishen_ime_imm32_x86.ime"
!define SRC_SERVER   "..\platform\windows\out\taishen_server.exe"
!define SRC_DICT     "..\resources\system_dict.db"
!define SRC_DICT_BIN "..\resources\system_dict.db.bin"
!define SRC_DOMAINS  "..\resources\domains\domains.db"
!define SRC_DOMAINS_BIN "..\resources\domains\domains.db.bin"
!define SRC_LM_CACHE "..\resources\lm\lm_cache.bin"
!define SRC_LM_DATA  "..\resources\lm\lm_data.bin"
!define SRC_LM_WORDS "..\resources\lm\lm_words.json"
!define SRC_COMMON   "..\resources\common.db"
!define SRC_BIGRAM   "..\resources\bigram.db"

; ---- 界面 ----
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_LINK "泰深输入法项目主页"
!define MUI_FINISHPAGE_LINK_LOCATION "https://github.com/EricXu20266/taishenIME"
; 覆盖安装时旧 DLL 可能仍被宿主进程（explorer/ctfmon/正在打字的程序）加载，
; 新文件要等这些进程重启才生效——结束时明确提示。
!define MUI_FINISHPAGE_TEXT "泰深输入法 ${VERSION} 已安装。$\r$\n$\r$\n如果安装前正在其他程序中用输入法，请重启那些程序（或重启系统）以加载新版本。$\r$\n$\r$\n老游戏兼容层默认未启用。需要时请以管理员身份运行 $INSTDIR\tools\enable-imm32.ps1。"
; 让用户可以直接选“立即重启”：延迟删除的旧 DLL 只有真正重启才会被清理
!define MUI_FINISHPAGE_REBOOT
!define MUI_FINISHPAGE_REBOOTLATERON
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

; ---- 清理历史版本的系统级 IMM32 注册（BUG-2026-10-31-01） ----
; 只删指向本输入法的布局项。Keyboard Layouts 键删除后，用户 Preload 中的
; e0c00804 因找不到布局而被系统忽略，登录界面不再加载该 IME。
!macro RemoveLegacyImeRegistrationMacro
    ReadRegStr $R0 HKLM "SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804" "Layout File"
    ${If} $R0 != ""
        System::Call 'shlwapi::StrStrW(w "$R0", w "taishen_ime_imm32") p .R1'
        ${If} $R1 != ""
        ${AndIf} $R1 != "0"
            DeleteRegKey HKLM "SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
            StrCpy $HadLegacyImm32 1
            DetailPrint "已清除历史版本的 IMM32 系统级注册"
        ${EndIf}
    ${EndIf}

    ; Substitutes 中的 KLID 映射（存在才删，不存在也无害）
    StrCpy $R2 0
    ${Do}
        EnumRegKey $R3 HKU "" $R2
        ${If} $R3 == ""
            ${ExitDo}
        ${EndIf}
        IntOp $R2 $R2 + 1
        DeleteRegValue HKU "$R3\Keyboard Layout\Substitutes" "e0c00804"
    ${Loop}
!macroend

Function RemoveLegacyImeRegistration
    !insertmacro RemoveLegacyImeRegistrationMacro
FunctionEnd

Function un.RemoveLegacyImeRegistration
    !insertmacro RemoveLegacyImeRegistrationMacro
FunctionEnd

; ---- 安装段 ----
Section "Install"
    SetOutPath "$INSTDIR"

    ; === 0. 先清理历史版本的系统级 IMM32 注册（BUG-2026-10-31-01） ===
    ; 0.1.51–0.1.69 会把 taishen_ime_imm32.ime 注册进
    ; HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804。
    ; 该注册对登录界面（winlogon/LogonUI）同样生效，而旧版 DLL 在模块被卸载后
    ; 仍有词库加载线程和 IME 窗口类指向已卸载代码 → 0xc0000005 → 登录无限循环。
    ; 0.1.70 起默认不做系统级注册；升级时先摘掉旧注册，重启后登录界面不再加载它。
    Call RemoveLegacyImeRegistration

    ; === 0. 让出被占用的旧文件 ===
    ; TSF DLL 会被注入到 explorer / ctfmon / 所有用过输入法的进程，IMM32 兼容层
    ; 也会被 ImmInstallIME 加载。这些文件处于「正在使用」状态时直接覆盖会中止安装
    ; （NSIS 报 "Error opening file for writing: ...taishen_ime.dll"）。
    ; Windows 允许重命名已加载的文件 → 先改名腾出原文件名，再写新文件。
    ; 关键：后缀必须唯一。上一次安装留下的 .old 在重启前仍被映射、删不掉，
    ; 用固定名改名会撞名失败，下次覆盖安装又回到原始报错。
    System::Call 'kernel32::GetTickCount() i .r0'
    ${If} ${FileExists} "$INSTDIR\taishen_ime.dll"
        Delete /REBOOTOK "$INSTDIR\taishen_ime.dll.old.*"
        Rename "$INSTDIR\taishen_ime.dll" "$INSTDIR\taishen_ime.dll.old.$0"
        ; 刚改名出的 .old 仍被旧进程映射，删不掉 → 登记 PendingFileRenameOperations，
        ; 由系统在下次重启时清理
        Delete /REBOOTOK "$INSTDIR\taishen_ime.dll.old.$0"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\taishen_ime_x86.dll"
        Delete /REBOOTOK "$INSTDIR\taishen_ime_x86.dll.old.*"
        Rename "$INSTDIR\taishen_ime_x86.dll" "$INSTDIR\taishen_ime_x86.dll.old.$0"
        Delete /REBOOTOK "$INSTDIR\taishen_ime_x86.dll.old.$0"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\taishen_ime_imm32.ime"
        Delete /REBOOTOK "$INSTDIR\taishen_ime_imm32.ime.old.*"
        Rename "$INSTDIR\taishen_ime_imm32.ime" "$INSTDIR\taishen_ime_imm32.ime.old.$0"
        ; Rename 失败（文件仍被独占）→ 后面的 File /nonfatal 会静默跳过，这里显式提示
        IfErrors 0 +2
        DetailPrint "警告：IMM32 兼容层改名失败，本次未更新（重启系统后重装即可）"
        Delete /REBOOTOK "$INSTDIR\taishen_ime_imm32.ime.old.$0"
    ${EndIf}
    ; 已 mmap 映射的预编译索引允许改名但禁止原地覆写，先改名腾出原路径
    ${If} ${FileExists} "$INSTDIR\system_dict.db.bin"
        Delete /REBOOTOK "$INSTDIR\system_dict.db.bin.old.*"
        Rename "$INSTDIR\system_dict.db.bin" "$INSTDIR\system_dict.db.bin.old.$0"
        Delete /REBOOTOK "$INSTDIR\system_dict.db.bin.old.$0"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\domains\domains.db.bin"
        Delete /REBOOTOK "$INSTDIR\domains\domains.db.bin.old.*"
        Rename "$INSTDIR\domains\domains.db.bin" "$INSTDIR\domains\domains.db.bin.old.$0"
        Delete /REBOOTOK "$INSTDIR\domains\domains.db.bin.old.$0"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\lm\lm_cache.bin"
        Delete /REBOOTOK "$INSTDIR\lm\lm_cache.bin.old.*"
        Rename "$INSTDIR\lm\lm_cache.bin" "$INSTDIR\lm\lm_cache.bin.old.$0"
        Delete /REBOOTOK "$INSTDIR\lm\lm_cache.bin.old.$0"
    ${EndIf}

    ; 先停掉旧版进程外渲染服务，确保覆盖写入不被占用
    ExecWait 'taskkill /F /IM taishen_server.exe'
    Sleep 200

    ; === 1. 复制核心文件 ===
    DetailPrint "安装 DLL..."
    File "${SRC_DLL}"

    ; 32 位 TSF DLL：32 位宿主（战网等）从 WOW6432Node 查注册并加载这一份
    DetailPrint "安装 32 位 DLL..."
    File "${SRC_DLL_X86}"

    ; V4 D2 进程外渲染服务（供 AppContainer / 任务栏搜索 / 跨宿主候选窗渲染）
    DetailPrint "安装进程外渲染服务..."
    File "${SRC_SERVER}"

    ; IMM32 兼容层（老游戏/老应用）：缺此文件时只有 TSF 层可用
    DetailPrint "安装 IMM32 兼容层..."
    File /nonfatal "${SRC_IMM32}"
    ; 32 位老游戏进程只能加载 32 位 .ime（LOL 等）
    File /nonfatal "${SRC_IMM32_X86}"

    DetailPrint "安装系统词库..."
    File "${SRC_DICT}"
    File /nonfatal "${SRC_DICT_BIN}"

    ; 引擎从 system_dict.db 同级的 domains\domains.db 读取领域库。
    ; 单独切换输出目录，避免把 domains.db 错装到安装根目录。
    SetOutPath "$INSTDIR\domains"
    File /nonfatal "${SRC_DOMAINS}"
    ; 领域零拷贝容器：缺失时首启会从 domains.db 重建，加载时间与内存峰值都会退化
    File /nonfatal "${SRC_DOMAINS_BIN}"

    ; 语言模型：lm_cache.bin 是预生成容器（首启秒开）；
    ; lm_data.bin / lm_words.json 是重建源，cache 失效时可重建而不是直接失去能力。
    SetOutPath "$INSTDIR\lm"
    File /nonfatal "${SRC_LM_CACHE}"
    File /nonfatal "${SRC_LM_DATA}"
    File /nonfatal "${SRC_LM_WORDS}"

    SetOutPath "$INSTDIR"
    File /nonfatal "${SRC_COMMON}"

    ; bigram 搭配表（拆组内相邻词打分，可选）：DLL 同目录，缺失时引擎按无表降级
    ; 旧 .bin 缓存先删：缓存有效性按 mtime 判断，覆盖 db 不一定更新 mtime
    Delete "$INSTDIR\bigram.db.bin"
    File /nonfatal "${SRC_BIGRAM}"

    ; === 2b. 兼容层手动开关 / 登录循环救援脚本 ===
    ; IMM32 默认不注册；老游戏用户手动运行 enable-imm32.ps1
    SetOutPath "$INSTDIR\tools"
    File /nonfatal "enable-imm32.ps1"
    File /nonfatal "disable-imm32.ps1"
    File /nonfatal "fix-login-loop.ps1"
    SetOutPath "$INSTDIR"

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

    ; === 2d. V4 D1+D2 AppContainer 最小白名单权限（默认开启；/NO_APPCONTAINER_ACL 可关）===
    ; 同时授权 ALL APPLICATION PACKAGES (S-1-15-2-1) 与 LPAC (S-1-15-2-2)：
    ;   - TSF DLL：RX（让任务栏搜索与商店应用能够加载薄 DLL）
    ;   - 只读词库：R（不授权 ime_debug.log、config.ini 与 %APPDATA% 用户词库）
    ${GetParameters} $R0
    ClearErrors
    ${GetOptions} $R0 "/NO_APPCONTAINER_ACL" $R1
    ${If} ${Errors}
        DetailPrint "设置 AppContainer/LPAC 最小只读权限..."
        IfFileExists "$INSTDIR\taishen_ime.dll" 0 +2
          ExecWait 'icacls "$INSTDIR\taishen_ime.dll" /grant "*S-1-15-2-1:(RX)" /grant "*S-1-15-2-2:(RX)" /Q'
        IfFileExists "$INSTDIR\taishen_ime_x86.dll" 0 +2
          ExecWait 'icacls "$INSTDIR\taishen_ime_x86.dll" /grant "*S-1-15-2-1:(RX)" /grant "*S-1-15-2-2:(RX)" /Q'
        IfFileExists "$INSTDIR\system_dict.db" 0 +2
          ExecWait 'icacls "$INSTDIR\system_dict.db" /grant "*S-1-15-2-1:(R)" /grant "*S-1-15-2-2:(R)" /Q'
        IfFileExists "$INSTDIR\system_dict.db.bin" 0 +2
          ExecWait 'icacls "$INSTDIR\system_dict.db.bin" /grant "*S-1-15-2-1:(R)" /grant "*S-1-15-2-2:(R)" /Q'
        IfFileExists "$INSTDIR\domains\domains.db" 0 +2
          ExecWait 'icacls "$INSTDIR\domains\domains.db" /grant "*S-1-15-2-1:(R)" /grant "*S-1-15-2-2:(R)" /Q'
        IfFileExists "$INSTDIR\common.db" 0 +2
          ExecWait 'icacls "$INSTDIR\common.db" /grant "*S-1-15-2-1:(R)" /grant "*S-1-15-2-2:(R)" /Q'
        IfFileExists "$INSTDIR\bigram.db" 0 +2
          ExecWait 'icacls "$INSTDIR\bigram.db" /grant "*S-1-15-2-1:(R)" /grant "*S-1-15-2-2:(R)" /Q'
    ${Else}
        DetailPrint "已跳过 AppContainer ACL（/NO_APPCONTAINER_ACL）"
    ${EndIf}

    ; === 2e. 注册并启动 taishen_server.exe（V4 D2 登录自启 + 中等完整性启动）===
    ; 必须登录自启，确保用户在登录后第一时间点击任务栏搜索时 server 已在线（容器 DLL 只连不拉）。
    ; 通过 explorer.exe 拉起可确保在提权安装器下仍以普通用户 Medium Integrity 运行。
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TaishenIMEServer" '"$INSTDIR\taishen_server.exe"'
    Exec 'explorer.exe "$INSTDIR\taishen_server.exe"'

    ; === 3. 注册 TSF COM 组件 ===
    DetailPrint "注册输入法组件..."
    ExecWait 'regsvr32 /s "$INSTDIR\taishen_ime.dll"'
    Sleep 500

    ; === 3a. 注册 32 位 TSF（WOW6432Node 视图）===
    ; 必须用 SysWOW64 下的 regsvr32，否则写进 64 位视图，32 位宿主依然看不到。
    ${If} ${FileExists} "$INSTDIR\taishen_ime_x86.dll"
        DetailPrint "注册 32 位输入法组件..."
        ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s "$INSTDIR\taishen_ime_x86.dll"'
        Sleep 500
    ${EndIf}

    ; === 3b. IMM32 兼容层（老游戏/老应用）：默认不注册 ===
    ; 历史版本在这里执行 regsvr32 + ImmInstallIME，把 .ime 写进
    ; HKLM\Keyboard Layouts\E0C00804；登录界面也会加载，卸载残留导致 winlogon 崩溃
    ; （BUG-2026-10-31-01）。0.1.70 起只安装文件，不写系统级注册。
    ; 需要老游戏适配的用户，手动运行 $INSTDIR\tools\enable-imm32.ps1（管理员）。
    ${If} ${FileExists} "$INSTDIR\taishen_ime_imm32.ime"
        DetailPrint "IMM32 兼容层已安装（默认未注册，老游戏需要时手动启用）"
    ${EndIf}

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

    ; === 7. 老游戏兼容层告知（仅当本次清掉了旧注册） ===
    ; 静默安装不弹窗，避免阻塞自动化部署
    ${If} $HadLegacyImm32 == 1
        IfSilent +2
        MessageBox MB_ICONINFORMATION|MB_OK "检测到旧版 IMM32 兼容层注册，已自动解除（修复登录循环）。$\r$\n$\r$\n老游戏需要时，请以管理员身份运行：$\r$\n$INSTDIR\tools\enable-imm32.ps1"
    ${EndIf}
SectionEnd

; ---- 卸载段 ----
Section "Uninstall"
    ; === 0. 停止进程外渲染服务并移除自启项 ===
    ExecWait 'taskkill /F /IM taishen_server.exe'
    Sleep 200
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TaishenIMEServer"

    ; === 1. 注销 TSF COM ===
    ExecWait 'regsvr32 /s /u "$INSTDIR\taishen_ime.dll"'
    Sleep 500

    ; === 1a. 注销 32 位 TSF（WOW6432Node）===
    ${If} ${FileExists} "$INSTDIR\taishen_ime_x86.dll"
        ExecWait '"$WINDIR\SysWOW64\regsvr32.exe" /s /u "$INSTDIR\taishen_ime_x86.dll"'
        Sleep 300
    ${EndIf}

    ; === 1b. 注销 IMM32 兼容层（先注销注册表，再删文件） ===
    ${If} ${FileExists} "$INSTDIR\taishen_ime_imm32.ime"
        ExecWait 'regsvr32 /s /u "$INSTDIR\taishen_ime_imm32.ime"'
        Sleep 300
    ${EndIf}
    ; regsvr32 在文件已被占用/缺失时会静默失败，这里兜底清一次（只删自己的）
    Call un.RemoveLegacyImeRegistration

    ; === 2. 删除安装文件 ===
    ; 卸载时 DLL/IME 也可能仍被加载：改名 + 重启后删除，避免「文件正在使用」中止卸载
    ${If} ${FileExists} "$INSTDIR\taishen_ime.dll"
        Rename "$INSTDIR\taishen_ime.dll" "$INSTDIR\taishen_ime.dll.old"
    ${EndIf}
    Delete /REBOOTOK "$INSTDIR\taishen_ime.dll"
    Delete /REBOOTOK "$INSTDIR\taishen_ime.dll.old"
    ${If} ${FileExists} "$INSTDIR\taishen_ime_x86.dll"
        Rename "$INSTDIR\taishen_ime_x86.dll" "$INSTDIR\taishen_ime_x86.dll.old"
    ${EndIf}
    Delete /REBOOTOK "$INSTDIR\taishen_ime_x86.dll"
    Delete /REBOOTOK "$INSTDIR\taishen_ime_x86.dll.old"
    ${If} ${FileExists} "$INSTDIR\taishen_ime_imm32.ime"
        Rename "$INSTDIR\taishen_ime_imm32.ime" "$INSTDIR\taishen_ime_imm32.ime.old"
    ${EndIf}
    Delete /REBOOTOK "$INSTDIR\taishen_ime_imm32.ime"
    Delete /REBOOTOK "$INSTDIR\taishen_ime_imm32.ime.old"
    Delete /REBOOTOK "$INSTDIR\taishen_ime_imm32_x86.ime"
    Delete /REBOOTOK "$INSTDIR\taishen_server.exe"
    Delete "$INSTDIR\system_dict.db"
    ${If} ${FileExists} "$INSTDIR\system_dict.db.bin"
        Rename "$INSTDIR\system_dict.db.bin" "$INSTDIR\system_dict.db.bin.old"
    ${EndIf}
    Delete /REBOOTOK "$INSTDIR\system_dict.db.bin"
    Delete /REBOOTOK "$INSTDIR\system_dict.db.bin.old"
    Delete /REBOOTOK "$INSTDIR\system_dict.db.bin.old.*"
    Delete "$INSTDIR\domains\domains.db"
    ${If} ${FileExists} "$INSTDIR\domains\domains.db.bin"
        Rename "$INSTDIR\domains\domains.db.bin" "$INSTDIR\domains\domains.db.bin.old"
    ${EndIf}
    Delete /REBOOTOK "$INSTDIR\domains\domains.db.bin"
    Delete /REBOOTOK "$INSTDIR\domains\domains.db.bin.old"
    Delete /REBOOTOK "$INSTDIR\domains\domains.db.bin.old.*"
    RMDir "$INSTDIR\domains"
    Delete "$INSTDIR\common.db"
    Delete "$INSTDIR\bigram.db"
    Delete "$INSTDIR\bigram.db.bin"
    Delete "$INSTDIR\config.ini"
    Delete "$INSTDIR\tools\enable-imm32.ps1"
    Delete "$INSTDIR\tools\disable-imm32.ps1"
    Delete "$INSTDIR\tools\fix-login-loop.ps1"
    RMDir "$INSTDIR\tools"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    ; === 3. 清理开始菜单 ===
    RMDir /r "$SMPROGRAMS\泰深输入法"

    ; === 4. 清理注册表 ===
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME"

    ; 注意：用户数据 (%APPDATA%\taishen-ime) 保留不删，包含 user_dict.db 和日志
SectionEnd
