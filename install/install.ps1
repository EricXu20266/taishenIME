# ============================================================
#  Taishen IME - Install script (MVP basic version)
#  Zero-dependency: copy artifacts + regsvr32 register (HKCU)
#  Usage: right-click "Run with PowerShell", or:
#         powershell -ExecutionPolicy Bypass -File install.ps1
# ============================================================

$ErrorActionPreference = "Stop"

# ---- Source dir: 优先最新构建（platform\windows\out），回退 install\out ----
$src = Join-Path $PSScriptRoot "..\platform\windows\out"
if (-not (Test-Path (Join-Path $src "taishen_ime.dll"))) {
    $alt = Join-Path $PSScriptRoot "out"
    if (Test-Path (Join-Path $alt "taishen_ime.dll")) {
        $src = $alt
    } else {
        Write-Host "[ERROR] taishen_ime.dll not found. Build first (CMake), or place install.ps1 next to out\." -ForegroundColor Red
        Read-Host "Press Enter to exit"
        exit 1
    }
}

# ---- Install dir ----
$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"

Write-Host "============================================"
Write-Host "  Taishen IME Install"
Write-Host "  Source:   $src"
Write-Host "  Target:   $dest"
Write-Host "============================================"

# 1. Create install dir
New-Item -ItemType Directory -Path $dest -Force | Out-Null

# 2. Copy DLL
# 覆盖前先把旧文件改名：TSF DLL 会被 explorer/ctfmon/各应用加载，
# 直接 Copy-Item -Force 会报「文件正由另一进程使用」。
# 后缀唯一：上次的 .old 在重启前删不掉，固定名会撞名失败。
$dllDest = Join-Path $dest "taishen_ime.dll"
if (Test-Path $dllDest) {
    $oldDll = "$dllDest.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
    try { Move-Item $dllDest $oldDll -Force -ErrorAction Stop }
    catch { Write-Warning "旧 DLL 仍被进程占用，改名失败：$($_.Exception.Message)。请重启系统后重装。" }
}
Copy-Item (Join-Path $src "taishen_ime.dll") $dllDest -Force
Write-Host "[OK] DLL copied (旧文件已改名为 .old.时间戳，重启后可删)"

# 2b. Copy IMM32 兼容层 IME（V0.6：老游戏/老应用适配）
if (Test-Path (Join-Path $src "taishen_ime_imm32.ime")) {
    $imeDest = Join-Path $dest "taishen_ime_imm32.ime"
    if (Test-Path $imeDest) {
        $oldIme = "$imeDest.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
        try { Move-Item $imeDest $oldIme -Force -ErrorAction Stop }
        catch { Write-Warning "旧 IMM32 兼容层仍被占用，改名失败：$($_.Exception.Message)" }
    }
    Copy-Item (Join-Path $src "taishen_ime_imm32.ime") $imeDest -Force
    Write-Host "[OK] IMM32 IME copied"
} else {
    Write-Host "[WARN] taishen_ime_imm32.ime not found - IMM32 layer skipped" -ForegroundColor Yellow
}

# 3. Copy system dictionary (from out\ or resources\)
function Copy-From-Src ($name, $label) {
    $s = Join-Path $src $name
    if (-not (Test-Path $s)) { $s = Join-Path $PSScriptRoot "..\resources\$name" }
    if (Test-Path $s) {
        Copy-Item $s (Join-Path $dest $name) -Force
        Write-Host "[OK] $label"
    }
}

Copy-From-Src "system_dict.db" "System dict"
Copy-From-Src "system_dict.db.bin" "Precompiled index"

# Engine resolves the domain database at <install>\domains\domains.db.
function Copy-DomainDict {
    $candidates = @(
        (Join-Path $src "domains\domains.db"),
        (Join-Path $src "domains.db"),
        (Join-Path $PSScriptRoot "..\resources\domains\domains.db")
    )
    $s = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($s) {
        $domainDest = Join-Path $dest "domains"
        New-Item -ItemType Directory -Path $domainDest -Force | Out-Null
        Copy-Item $s (Join-Path $domainDest "domains.db") -Force
        Write-Host "[OK] Domain dict"
    } else {
        Write-Host "[WARN] Domain dict not found" -ForegroundColor Yellow
    }
}
Copy-DomainDict
Copy-From-Src "common.db" "Common dict"

# 4. Generate config.ini (skip if exists)
$cfgPath = Join-Path $dest "config.ini"
if (-not (Test-Path $cfgPath)) {
    @(
        "# Taishen IME config",
        "candidate_count=9",
        "dict_path=system_dict.db"
    ) | Set-Content -Path $cfgPath -Encoding UTF8
    Write-Host "[OK] config.ini generated"
}

# 5. Register TSF COM component (HKLM needs elevation for system-level TIP)
Write-Host "[..] Registering TSF component..."
# HKCU registration (no elevation needed)
& regsvr32 /s (Join-Path $dest "taishen_ime.dll")
Start-Sleep -Milliseconds 500

# HKLM registration (elevated) - required for language settings UI to enumerate
$dllFull = Join-Path $dest "taishen_ime.dll"
try {
    Start-Process regsvr32 -ArgumentList '/s', $dllFull -Verb RunAs -Wait -ErrorAction Stop
    Start-Sleep -Milliseconds 800
    Write-Host "[OK] HKLM registration (elevated) done"
} catch {
    Write-Host "[WARN] HKLM registration skipped (user declined elevation) - HKCU only" -ForegroundColor Yellow
}

# Verify registration by checking registry (more reliable than regsvr32 exit code)
$regKey = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\{7D77E4AA-276E-4582-B952-94B6EFAADA28}"
if (Test-Path $regKey) {
    Write-Host "[OK] TSF component registered (system-level)"
} elseif (Test-Path "HKCU:\Software\Microsoft\CTF\TIP\{7D77E4AA-276E-4582-B952-94B6EFAADA28}") {
    Write-Host "[OK] TSF component registered (user-level)"
} else {
    Write-Host "[ERROR] regsvr32 registration failed (registry not written)" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
}

# 6. IMM32 兼容层：默认不注册（BUG-2026-10-31-01）
#    历史版本在这里 regsvr32 + ImmInstallIME，把 .ime 写进 HKLM Keyboard Layouts，
#    登录界面也会加载；模块卸载后残留线程/窗口类导致 winlogon 崩溃、无限登录循环。
#    0.1.70 起默认不注册；老游戏需要时手动运行 install\enable-imm32.ps1。
$legacyLayout = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
if (Test-Path $legacyLayout) {
    $legacyFile = (Get-ItemProperty -Path $legacyLayout -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($legacyFile -and $legacyFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $legacyLayout -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "[OK] Removed legacy IMM32 system registration"
    }
}
Write-Host "[SKIP] IMM32 layer installed but not registered (run enable-imm32.ps1 for old games)"

Write-Host ""
Write-Host "============================================"
Write-Host "  Install SUCCESS!"
Write-Host ""
Write-Host "  Next steps:"
Write-Host "  1. Settings - Time & Language - Language - Chinese (Simplified)"
Write-Host "  2. Click keyboard icon - Add a keyboard - select 'Taishen Pinyin'"
Write-Host "  3. Switch to Taishen IME and type"
Write-Host ""
Write-Host "  Uninstall: run $dest\uninstall.ps1"
Write-Host "============================================"
Read-Host "Press Enter to exit"
