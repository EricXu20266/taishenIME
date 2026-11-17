# ============================================================
#  Taishen IME - Install (0.1.24, non-interactive)
#  Copy artifacts + regsvr32 register
# ============================================================
$ErrorActionPreference = "Stop"

# 安装需要写 HKLM：未提权时自提权（非交互脚本，不提权则直接失败）
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[..] Elevating for HKLM registration..."
    try {
        Start-Process powershell -Verb RunAs -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"')) -Wait
    } catch {
        Write-Host "[ERROR] Elevation declined. Run as administrator." -ForegroundColor Red
        exit 1
    }
    exit 0
}

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

# 2b. 32 位 TSF DLL（32 位宿主进程用）
$dll32Src = Join-Path $src "taishen_ime_x86.dll"
if (Test-Path $dll32Src) {
    $dll32Dest = Join-Path $dest "taishen_ime_x86.dll"
    if (Test-Path $dll32Dest) {
        $oldDll32 = "$dll32Dest.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
        try { Move-Item $dll32Dest $oldDll32 -Force -ErrorAction Stop } catch {}
    }
    Copy-Item $dll32Src $dll32Dest -Force
    Write-Host "[OK] x86 DLL copied"
} else {
    Write-Host "[SKIP] 未找到 taishen_ime_x86.dll"
}

# 2c. IMM32 兼容层（64 + 32 位）：老游戏的 32 位进程只能加载 32 位 .ime
foreach ($ime in @("taishen_ime_imm32.ime", "taishen_ime_imm32_x86.ime")) {
    $imeSrc = Join-Path $src $ime
    if (-not (Test-Path $imeSrc)) {
        $imeSrc = Join-Path $repoRoot "platform\windows\out\$ime"
    }
    if (Test-Path $imeSrc) {
        Copy-Item $imeSrc (Join-Path $dest $ime) -Force
        Write-Host "[OK] $ime copied"
    }
}

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
Copy-Dict "bigram.db" "Bigram table"
# 旧 .bin 缓存必须失效：覆盖 db 不一定更新 mtime
$bigramBinDest = Join-Path $dest "bigram.db.bin"
if (Test-Path $bigramBinDest) { Remove-Item $bigramBinDest -Force -ErrorAction SilentlyContinue }

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

# 4a. AppContainer 读权限：TSF DLL 会被加载进容器进程（任务栏搜索、商店应用），
#     容器读不到宿主 %LOCALAPPDATA%，词库加载会静默降级到内置词表。
#     给安装目录加 ALL APPLICATION PACKAGES（S-1-15-2-1）只读 ACE；只读不写。
#     用户词库在 %APPDATA%\taishen-ime，不给容器权限（容器内走只读模式）。
Write-Host "[..] Granting AppContainer read access (ALL APPLICATION PACKAGES)..."
# 白名单授权，不用目录级 /T：目录级会把 ime_debug.log（记录按键码与激活行为）
# 一并开放给所有 AppContainer 应用。
$aclTargets = @(
    'system_dict.db', 'system_dict.db.bin',
    'domains.db', 'common.db',
    'bigram.db', 'bigram.db.bin',
    'context_prior.db', 'context_prior.db.bin'
)
$granted = 0
foreach ($name in $aclTargets) {
    $aclPath = Join-Path $dest $name
    if (Test-Path $aclPath) {
        icacls $aclPath /grant '*S-1-15-2-1:(R)' /Q | Out-Null
        if ($LASTEXITCODE -eq 0) { $granted++ }
    }
}
if ($granted -gt 0) {
    Write-Host "[OK] AppContainer read access granted ($granted files)"
} else {
    Write-Host "[WARN] 容器读权限设置失败：容器内应用将使用内置词表" -ForegroundColor Yellow
}

# 4b. 清理历史版本的系统级 IMM32 注册（BUG-2026-10-31-01）
#     旧版把 .ime 注册进 HKLM Keyboard Layouts，登录界面会加载并导致 winlogon 崩溃。
#     0.1.70 起默认不注册；老游戏需要时手动运行 install\enable-imm32.ps1。
$legacyLayout = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
if (Test-Path $legacyLayout) {
    $legacyFile = (Get-ItemProperty -Path $legacyLayout -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($legacyFile -and $legacyFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $legacyLayout -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path $legacyLayout) {
            Write-Host "[WARN] Legacy IMM32 registration could not be removed (need admin)" -ForegroundColor Yellow
        } else {
            Write-Host "[OK] Removed legacy IMM32 system registration"
        }
    }
}

# 4c. 拷贝兼容层开关 / 登录循环救援脚本
$toolsDir = Join-Path $dest "tools"
New-Item -ItemType Directory -Path $toolsDir -Force | Out-Null
foreach ($n in @('enable-imm32.ps1', 'disable-imm32.ps1', 'fix-login-loop.ps1')) {
    $p = Join-Path $PSScriptRoot $n
    if (Test-Path $p) { Copy-Item $p (Join-Path $toolsDir $n) -Force }
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

# 5b. 注册 32 位 TSF（WOW6432Node 视图）
# 32 位宿主（战网等）只认这一步写进去的注册项；
# 缺它是「输入法在 32 位应用里完全不出现」的根因。
$dll32 = Join-Path $dest "taishen_ime_x86.dll"
if (Test-Path $dll32) {
    $regsvr32x86 = Join-Path $env:WINDIR "SysWOW64\regsvr32.exe"
    Write-Host "[..] Registering TSF x86 (WOW6432Node)..."
    Start-Process $regsvr32x86 -ArgumentList '/s', $dll32 -Wait
    Start-Sleep -Milliseconds 500
    try {
        Start-Process $regsvr32x86 -ArgumentList '/s', $dll32 -Verb RunAs -Wait -ErrorAction Stop
        Write-Host "[OK] TSF x86 registered"
    } catch {
        Write-Host "[WARN] TSF x86 elevation declined"
    }
} else {
    Write-Host "[SKIP] taishen_ime_x86.dll 不存在（未构建 32 位版本）"
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
