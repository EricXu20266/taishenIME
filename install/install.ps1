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

# 6. Register IMM32 IME (Keyboard Layouts + ImmInstallIME) - V0.6
#    Win10/Win11 只写注册表不够——必须调 ImmInstallIME 创建 HKL 系统才真正加载 .ime
#    参考: rime/home#744（小狼毫同样的坑，regsvr32 后 Win10 不加载 weasel.ime）
$imm32 = Join-Path $dest "taishen_ime_imm32.ime"
if (Test-Path $imm32) {
    Write-Host "[..] Registering IMM32 IME..."

    # Step 6a: regsvr32 — 写 Keyboard Layouts 注册表
    $regsvrOk = $false
    try {
        Start-Process regsvr32 -ArgumentList '/s', $imm32 -Verb RunAs -Wait -ErrorAction Stop
        Start-Sleep -Milliseconds 800
        $klid = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
        if (Test-Path $klid) {
            Write-Host "[OK] regsvr32: Keyboard Layouts E0C00804 written"
            $regsvrOk = $true
        } else {
            Write-Host "[ERROR] regsvr32: Keyboard Layouts not written" -ForegroundColor Red
        }
    } catch {
        Write-Host "[WARN] regsvr32 skipped (user declined elevation)" -ForegroundColor Yellow
    }

    # Step 6b: ImmInstallIME — 创建 HKL 让系统真正加载 .ime（解决 Win10 不加载问题）
    if ($regsvrOk) {
        Write-Host "[..] ImmInstallIME — creating HKL for system loading..."
        try {
            # P/Invoke ImmInstallIME from imm32.dll
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Imm32Helper {
    [DllImport("imm32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr ImmInstallIME(string lpszIMEFileName, string lpszLayoutText);
}
'@ -ErrorAction Stop

            $hkl = [Imm32Helper]::ImmInstallIME($imm32, "泰深拼音")
            if ($hkl -ne [IntPtr]::Zero) {
                Write-Host "[OK] ImmInstallIME SUCCESS — HKL = 0x$($hkl.ToString('X'))"
                Write-Host "      IMM32 IME will now be loaded by the system (valid until reboot)"
                Write-Host "      To make permanent: re-run this step after reboot, or add to startup"
            } else {
                $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
                Write-Host "[WARN] ImmInstallIME returned NULL (error $err) — may already be installed" -ForegroundColor Yellow
                Write-Host "       If LOL still can't switch, run: install\install_latest.ps1"
            }
        } catch {
            Write-Host "[WARN] ImmInstallIME failed: $_" -ForegroundColor Yellow
            Write-Host "       The IMM32 IME may not load in games until this is resolved."
        }
    } else {
        Write-Host "[WARN] Skipping ImmInstallIME — regsvr32 must succeed first" -ForegroundColor Yellow
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
