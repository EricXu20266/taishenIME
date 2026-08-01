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

# 3. Copy system dictionary (from out\ or resources\)
$dictSrc = Join-Path $src "system_dict.db"
if (-not (Test-Path $dictSrc)) {
    $dictSrc = Join-Path $PSScriptRoot "..\resources\system_dict.db"
}
if (Test-Path $dictSrc) {
    Copy-Item $dictSrc (Join-Path $dest "system_dict.db") -Force
    Write-Host "[OK] Dictionary copied"
}

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

# 5. Register TSF COM component
Write-Host "[..] Registering TSF component..."
& regsvr32 /s (Join-Path $dest "taishen_ime.dll")
Start-Sleep -Milliseconds 500

# Verify registration by checking registry (more reliable than regsvr32 exit code)
$regKey = "HKCU:\Software\Classes\CLSID\{7D77E4AA-276E-4582-B952-94B6EFAADA28}\InprocServer32"
if (Test-Path $regKey) {
    Write-Host "[OK] TSF component registered"
} else {
    Write-Host "[ERROR] regsvr32 registration failed (registry not written)" -ForegroundColor Red
    Read-Host "Press Enter to exit"
    exit 1
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
