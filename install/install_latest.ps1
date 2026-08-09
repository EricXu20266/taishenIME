# ============================================================
#  Taishen IME - Install (0.1.24, non-interactive)
#  Copy artifacts + regsvr32 register
# ============================================================
$ErrorActionPreference = "Stop"

# 仓库根（脚本位于 install/ 下）
$repoRoot = Split-Path $PSScriptRoot -Parent
$src = Join-Path $repoRoot "platform\windows\out"
$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"

Write-Host "Source: $src"
Write-Host "Target: $dest"

# 1. Install dir
New-Item -ItemType Directory -Path $dest -Force | Out-Null

# 2. Copy DLL
Copy-Item (Join-Path $src "taishen_ime.dll") (Join-Path $dest "taishen_ime.dll") -Force
Write-Host "[OK] DLL copied"

# 3. Copy dict files (from out\ or resources\)
function Copy-Dict ($name, $label) {
    $s = Join-Path $repoRoot "resources\$name"
    if (-not (Test-Path $s)) { $s = Join-Path $src $name }
    if (Test-Path $s) {
        Copy-Item $s (Join-Path $dest $name) -Force
        $mb = [math]::Round((Get-Item $s).Length/1MB, 1)
        Write-Host "[OK] $label ($mb MB)"
    } else {
        Write-Host "[WARN] $name not found"
    }
}

Copy-Dict "system_dict.db" "System dict"
Copy-Dict "system_dict.db.bin" "Precompiled index"
Copy-Dict "domains.db" "Domain dict"
Copy-Dict "common.db" "Common dict"

# 4. Generate config.ini (skip if exists - keep user data)
$cfgPath = Join-Path $dest "config.ini"
if (-not (Test-Path $cfgPath)) {
    @(
        "# Taishen IME config (0.1.24)",
        "candidate_count=9",
        "dict_path=system_dict.db",
        "# user_dict_path=",
        "fuzzy=1",
        "correction=1",
        "mix_mode=1",
        "# traditional=0",
        "phrase=1",
        "# phrase_path="
    ) | Set-Content -Path $cfgPath -Encoding UTF8
    Write-Host "[OK] config.ini generated"
} else {
    Write-Host "[SKIP] config.ini exists (keep user config)"
}

# 5. Register (HKCU first, then HKLM elevated)
Write-Host "[..] Registering TSF (HKCU)..."
Start-Process regsvr32 -ArgumentList '/s', (Join-Path $dest "taishen_ime.dll") -Wait
Start-Sleep -Milliseconds 500

Write-Host "[..] Registering TSF (HKLM elevated)..."
try {
    Start-Process regsvr32 -ArgumentList '/s', (Join-Path $dest "taishen_ime.dll") -Verb RunAs -Wait -ErrorAction Stop
    Start-Sleep -Milliseconds 800
    Write-Host "[OK] HKLM registered"
} catch {
    Write-Host "[WARN] HKLM registration skipped (elevation declined)"
}

# 6. Verify
$regKey = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\{7D77E4AA-276E-4582-B952-94B6EFAADA28}"
if (Test-Path $regKey) {
    Write-Host "[OK] TSF registered (system-level)"
} elseif (Test-Path "HKCU:\Software\Microsoft\CTF\TIP\{7D77E4AA-276E-4582-B952-94B6EFAADA28}") {
    Write-Host "[OK] TSF registered (user-level)"
} else {
    Write-Host "[ERROR] registration failed" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "  Install SUCCESS (0.1.24)"
Write-Host "  Add keyboard: Settings > Time & Language > Language > Chinese > Keyboard > Taishen Pinyin"
Write-Host "  Uninstall: run $dest\uninstall.ps1"
