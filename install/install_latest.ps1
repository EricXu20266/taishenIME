# ============================================================
#  Taishen IME - Install (0.1.24, non-interactive)
#  Copy artifacts + regsvr32 register
# ============================================================
$ErrorActionPreference = "Stop"

$src = "E:\AllinDeepSeek\taishenIME\platform\windows\out"
$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"

Write-Host "Source: $src"
Write-Host "Target: $dest"

# 1. Install dir
New-Item -ItemType Directory -Path $dest -Force | Out-Null

# 2. Copy DLL
Copy-Item (Join-Path $src "taishen_ime.dll") (Join-Path $dest "taishen_ime.dll") -Force
Write-Host "[OK] DLL copied"

# 3. Copy system dictionary
$dictSrc = Join-Path $src "system_dict.db"
if (-not (Test-Path $dictSrc)) {
    $dictSrc = "E:\AllinDeepSeek\taishenIME\resources\system_dict.db"
}
if (Test-Path $dictSrc) {
    Copy-Item $dictSrc (Join-Path $dest "system_dict.db") -Force
    Write-Host "[OK] Dictionary copied"
}

# 3b. Copy precompiled index (.bin, 0.2.29) — 秒加载，避免首次 SQLite 全量重建 6-7s
$binSrc = "E:\AllinDeepSeek\taishenIME\resources\system_dict.db.bin"
if (Test-Path $binSrc) {
    Copy-Item $binSrc (Join-Path $dest "system_dict.db.bin") -Force
    Write-Host "[OK] Precompiled index copied ($([math]::Round((Get-Item $binSrc).Length/1MB,1)) MB)"
} else {
    Write-Host "[WARN] system_dict.db.bin not found — first activation will build it (~6s)"
}

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
