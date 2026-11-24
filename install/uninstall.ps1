# ============================================================
#  Taishen IME - Uninstall script
#  Unregister TSF COM + delete %ProgramFiles%\TaishenIME (keep %APPDATA%\taishen-ime)
#  Usage: right-click "Run with PowerShell" (administrator)
# ============================================================

$ErrorActionPreference = "Stop"

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[..] 需要管理员权限，正在提权..."
    $elevArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'))
    try {
        Start-Process powershell -Verb RunAs -ArgumentList $elevArgs -Wait
    } catch {
        Write-Host "[ERROR] 提权被拒绝。请右键「以管理员身份运行」。" -ForegroundColor Red
        Read-Host "按回车退出"
        exit 1
    }
    exit 0
}

$dest = Join-Path $env:ProgramFiles "TaishenIME"
$legacyDest = Join-Path $env:LOCALAPPDATA "TaishenIME"

# 重启后删除：MoveFileEx(MOVEFILE_DELAY_UNTIL_REBOOT=0x4)，需管理员权限
Add-Type -Namespace TaishenNative -Name FileUtil -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern bool MoveFileEx(string lpExistingFileName, string lpNewFileName, int dwFlags);
'@

$pending = New-Object System.Collections.Generic.List[string]
$stuck = New-Object System.Collections.Generic.List[string]

function Remove-OrSchedule([string]$path) {
    if (-not (Test-Path -LiteralPath $path)) { return }
    try {
        Remove-Item -LiteralPath $path -Force -ErrorAction Stop
        return
    } catch { }

    # 被加载中：改名（唯一后缀）避开固定名冲突，再登记重启删除
    $old = "$path.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
    $target = $path
    try {
        Move-Item -LiteralPath $path -Destination $old -Force -ErrorAction Stop
        $target = $old
    } catch {
        Write-Host "  [WARN] 改名失败：$path" -ForegroundColor Yellow
    }
    $scheduled = $false
    try { $scheduled = [TaishenNative.FileUtil]::MoveFileEx($target, $null, 4) } catch { }
    if ($scheduled) {
        $pending.Add($target)
        Write-Host "  [PENDING] $target 已登记，重启后自动删除" -ForegroundColor Yellow
    } else {
        $stuck.Add($target)
        Write-Host "  [FAIL] $target 删除失败（请重启后用管理员权限手动删除）" -ForegroundColor Red
    }
}

Write-Host "============================================"
Write-Host "  Taishen IME Uninstall"
Write-Host "============================================"

# 0. Stop taishen_server & clean logon autorun + legacy Root CA
Stop-Process -Name "taishen_server" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 150
Remove-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" -Name "TaishenIMEServer" -ErrorAction SilentlyContinue
& certutil -delstore "Root" E940406690BC7898EEAE6889C5486C8AC9B053A6 2>$null | Out-Null

# 1. Unregister TSF COM & IMM32 components (check both %ProgramFiles% and legacy %LOCALAPPDATA%)
$syswowReg = Join-Path $env:WINDIR "SysWOW64\regsvr32.exe"
foreach ($dir in @($dest, $legacyDest)) {
    if (-not (Test-Path $dir)) { continue }
    $dll = Join-Path $dir "taishen_ime.dll"
    if (Test-Path $dll) {
        Write-Host "[..] Unregistering TSF component ($dll)..."
        & regsvr32 /s /u $dll
        Start-Sleep -Milliseconds 300
    }
    $dllX86 = Join-Path $dir "taishen_ime_x86.dll"
    if ((Test-Path $dllX86) -and (Test-Path $syswowReg)) {
        & $syswowReg /s /u $dllX86
        Start-Sleep -Milliseconds 200
    }
    $imm32 = Join-Path $dir "taishen_ime_imm32.ime"
    if (Test-Path $imm32) {
        & regsvr32 /s /u $imm32
        Start-Sleep -Milliseconds 200
    }
}

$legacyLayout = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
if (Test-Path $legacyLayout) {
    $legacyFile = (Get-ItemProperty -Path $legacyLayout -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($legacyFile -and $legacyFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $legacyLayout -Recurse -Force -ErrorAction SilentlyContinue
    }
}

foreach ($unKey in @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME"
)) {
    if (Test-Path $unKey) { Remove-Item -Path $unKey -Recurse -Force -ErrorAction SilentlyContinue }
}

# 2. Delete %ProgramFiles%\TaishenIME —— 逐个文件处理，被加载的走「改名 + 重启删除」
if (Test-Path $dest) {
    Write-Host "[..] Deleting install dir ($dest)..."
    Remove-OrSchedule (Join-Path $dest "taishen_ime.dll")
    Remove-OrSchedule (Join-Path $dest "taishen_ime_x86.dll")
    Remove-OrSchedule (Join-Path $dest "taishen_ime_imm32.ime")
    Remove-OrSchedule (Join-Path $dest "taishen_ime_imm32_x86.ime")

    Get-ChildItem -LiteralPath $dest -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-OrSchedule $_.FullName }

    try {
        Remove-Item -LiteralPath $dest -Recurse -Force -ErrorAction Stop
        Write-Host "[OK] Deleted $dest"
    } catch {
        if ($pending.Count -gt 0) {
            Write-Host "[PENDING] 安装目录将在重启后清空：$dest" -ForegroundColor Yellow
        } else {
            $stuck.Add($dest)
            Write-Host "[FAIL] 目录删除失败：$dest" -ForegroundColor Red
        }
    }
}

# 2b. 清理历史 %LOCALAPPDATA%\TaishenIME 程序文件（保留 ime_debug.log* 与 crashes\）
if ((Test-Path $legacyDest) -and ($legacyDest -ne $dest)) {
    foreach ($rel in @(
        "taishen_ime.dll", "taishen_ime_x86.dll", "taishen_ime_imm32.ime", "taishen_ime_imm32_x86.ime",
        "taishen_server.exe", "TaishenIME-Dev-Root.cer", "system_dict.db", "system_dict.db.bin",
        "domains\domains.db", "domains\domains.db.bin", "lm\lm_cache.bin", "lm\lm_data.bin",
        "lm\lm_words.json", "common.db", "bigram.db", "bigram.db.bin", "config.ini", "uninstall.exe",
        "tools\enable-imm32.ps1", "tools\disable-imm32.ps1", "tools\fix-login-loop.ps1"
    )) {
        Remove-OrSchedule (Join-Path $legacyDest $rel)
    }
}

Write-Host ""
if ($stuck.Count -eq 0) {
    Write-Host "Uninstall complete. 用户数据（%APPDATA%\taishen-ime，含 user_dict.db 与 config.ini）保留未删。" -ForegroundColor Green
    if ($pending.Count -gt 0) {
        Write-Host "有 $($pending.Count) 个文件被占用，已登记重启后删除。" -ForegroundColor Yellow
    }
} else {
    Write-Host "卸载未完全成功，以下项目需重启后手动删除：" -ForegroundColor Red
    $stuck | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
}
Read-Host "Press Enter to exit"
