# ============================================================
#  Taishen IME - Uninstall script
#  Unregister TSF COM + delete install dir (keep %APPDATA%)
#  Usage: right-click "Run with PowerShell" (administrator recommended)
# ============================================================

$ErrorActionPreference = "Stop"

$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"

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

# 1. Unregister TSF COM component (HKCU + HKLM)
$dll = Join-Path $dest "taishen_ime.dll"
if (Test-Path $dll) {
    Write-Host "[..] Unregistering TSF component..."
    & regsvr32 /s /u $dll
    try {
        Start-Process regsvr32 -ArgumentList '/s', '/u', $dll -Verb RunAs -Wait -ErrorAction Stop
        Start-Sleep -Milliseconds 500
        Write-Host "[OK] Unregistered (HKCU + HKLM)"
    } catch {
        Write-Host "[WARN] HKLM unregister skipped (user declined elevation)" -ForegroundColor Yellow
    }
}

# 1b. Unregister IMM32 IME (Keyboard Layouts) - V0.6
$imm32 = Join-Path $dest "taishen_ime_imm32.ime"
if (Test-Path $imm32) {
    Write-Host "[..] Unregistering IMM32 IME..."
    & regsvr32 /s /u $imm32
    try {
        Start-Process regsvr32 -ArgumentList '/s', '/u', $imm32 -Verb RunAs -Wait -ErrorAction Stop
        Start-Sleep -Milliseconds 500
        Write-Host "[OK] IMM32 unregistered"
    } catch {
        Write-Host "[WARN] HKLM IMM32 unregister skipped (user declined elevation)" -ForegroundColor Yellow
    }
}

# 2. Delete install dir —— 逐个文件处理，被加载的走“改名 + 重启删除”，不再假报成功
if (Test-Path $dest) {
    Write-Host "[..] Deleting install dir..."
    # DLL/IME 先处理（最可能被 explorer / ctfmon / 各应用加载）
    Remove-OrSchedule (Join-Path $dest "taishen_ime.dll")
    Remove-OrSchedule (Join-Path $dest "taishen_ime_imm32.ime")

    Get-ChildItem -LiteralPath $dest -Recurse -File -ErrorAction SilentlyContinue |
        ForEach-Object { Remove-OrSchedule $_.FullName }

    # 目录本身：文件都清掉了才能删；有 PENDING 时留给重启后
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

Write-Host ""
if ($stuck.Count -eq 0) {
    Write-Host "Uninstall complete. 用户数据（%APPDATA%\taishen-ime）保留未删。" -ForegroundColor Green
    if ($pending.Count -gt 0) {
        Write-Host "有 $($pending.Count) 个文件被占用，已登记重启后删除。" -ForegroundColor Yellow
    }
} else {
    Write-Host "卸载未完全成功，以下项目需重启后手动删除：" -ForegroundColor Red
    $stuck | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
}
Read-Host "Press Enter to exit"
