# ============================================================
#  Taishen IME - Install script (MVP basic version)
#  Zero-dependency: copy artifacts + regsvr32 register (HKCU)
#  Usage: right-click "Run with PowerShell", or:
#         powershell -ExecutionPolicy Bypass -File install.ps1
# ============================================================

$ErrorActionPreference = "Stop"

# ---- Source dir (this script's parent = install\, sibling = out\) ----
$src = Join-Path $PSScriptRoot "out"
if (-not (Test-Path (Join-Path $src "taishen_ime.dll"))) {
    # Try platform\windows\out
    $alt = Join-Path $PSScriptRoot "..\platform\windows\out"
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
Copy-Item (Join-Path $src "taishen_ime.dll") (Join-Path $dest "taishen_ime.dll") -Force
Write-Host "[OK] DLL copied"

# 2b. Copy IMM32 兼容层 IME（V0.6：老游戏/老应用适配）
if (Test-Path (Join-Path $src "taishen_ime_imm32.ime")) {
    Copy-Item (Join-Path $src "taishen_ime_imm32.ime") (Join-Path $dest "taishen_ime_imm32.ime") -Force
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
Copy-From-Src "domains.db" "Domain dict"
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

# 6. Register IMM32 IME (Keyboard Layouts, HKLM needs elevation) - V0.6
$imm32 = Join-Path $dest "taishen_ime_imm32.ime"
if (Test-Path $imm32) {
    Write-Host "[..] Registering IMM32 IME..."
    try {
        Start-Process regsvr32 -ArgumentList '/s', $imm32 -Verb RunAs -Wait -ErrorAction Stop
        Start-Sleep -Milliseconds 800
        # Verify Keyboard Layouts entry
        $klid = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
        if (Test-Path $klid) {
            Write-Host "[OK] IMM32 IME registered (Layout E0C00804)"
        } else {
            Write-Host "[ERROR] IMM32 registration failed (Keyboard Layouts not written)" -ForegroundColor Red
            Read-Host "Press Enter to exit"
            exit 1
        }
    } catch {
        Write-Host "[WARN] IMM32 registration skipped (user declined elevation) - register manually: regsvr32 /s `"$imm32`"" -ForegroundColor Yellow
    }
}

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
