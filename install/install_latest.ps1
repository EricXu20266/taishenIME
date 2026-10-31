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
# 覆盖前先改名：DLL 被宿主进程加载时 Copy-Item -Force 会失败；
# 后缀唯一，避免与上次未清理的 .old 撞名
$dllDest = Join-Path $dest "taishen_ime.dll"
if (Test-Path $dllDest) {
    $oldDll = "$dllDest.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
    try { Move-Item $dllDest $oldDll -Force -ErrorAction Stop }
    catch { Write-Warning "旧 DLL 仍被进程占用，改名失败：$($_.Exception.Message)。请重启系统后重装。" }
}
Copy-Item (Join-Path $src "taishen_ime.dll") $dllDest -Force
Write-Host "[OK] DLL copied (旧文件已改名为 .old.时间戳，重启后可删)"

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

# Engine resolves the domain database at <install>\domains\domains.db.
$domainSources = @(
    (Join-Path $src "domains\domains.db"),
    (Join-Path $src "domains.db"),
    (Join-Path $repoRoot "resources\domains\domains.db")
)
$domainSource = $domainSources | Where-Object { Test-Path $_ } | Select-Object -First 1
if ($domainSource) {
    $domainDest = Join-Path $dest "domains"
    New-Item -ItemType Directory -Path $domainDest -Force | Out-Null
    Copy-Item $domainSource (Join-Path $domainDest "domains.db") -Force
    Write-Host "[OK] Domain dict"
} else {
    Write-Host "[WARN] Domain dict not found"
}
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

# 4b. 清理历史版本的系统级 IMM32 注册（BUG-2026-10-31-01）
#     旧版把 .ime 注册进 HKLM Keyboard Layouts，登录界面会加载并导致 winlogon 崩溃。
#     0.1.70 起默认不注册；老游戏需要时手动运行 install\enable-imm32.ps1。
$legacyLayout = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
if (Test-Path $legacyLayout) {
    $legacyFile = (Get-ItemProperty -Path $legacyLayout -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($legacyFile -and $legacyFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $legacyLayout -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "[OK] Removed legacy IMM32 system registration"
    }
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
