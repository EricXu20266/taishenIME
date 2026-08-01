# ============================================================
#  Taishen IME - Uninstall script (MVP basic version)
#  Unregister TSF COM + delete install dir (keep %APPDATA%)
#  Usage: right-click "Run with PowerShell"
# ============================================================

$ErrorActionPreference = "Continue"

$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"

Write-Host "============================================"
Write-Host "  Taishen IME Uninstall"
Write-Host "============================================"

# 1. Unregister TSF COM component
$dll = Join-Path $dest "taishen_ime.dll"
if (Test-Path $dll) {
    Write-Host "[..] Unregistering TSF component..."
    & regsvr32 /s /u $dll
    Write-Host "[OK] Unregistered"
}

# 2. Delete install dir
if (Test-Path $dest) {
    Write-Host "[..] Deleting install dir..."
    Remove-Item -Recurse -Force $dest
    Write-Host "[OK] Deleted $dest"
}

Write-Host ""
Write-Host "Uninstall complete. User data (%APPDATA%\taishen-ime) preserved."
Read-Host "Press Enter to exit"
