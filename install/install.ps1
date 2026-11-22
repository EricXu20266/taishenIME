# ============================================================
#  Taishen IME - Install script (MVP basic version)
#  Zero-dependency: copy artifacts + regsvr32 register (HKCU)
#  Usage: right-click "Run with PowerShell", or:
#         powershell -ExecutionPolicy Bypass -File install.ps1
# ============================================================

param(
    [switch]$DisableAppContainerAcl
)

$ErrorActionPreference = "Stop"

# 安装需要写 HKLM（TSF 注册 + 清理历史 IMM32 注册）：未提权时自提权
$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[..] 需要管理员权限，正在提权..."
    $elevArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'))
    if ($DisableAppContainerAcl) { $elevArgs += '-DisableAppContainerAcl' }
    try {
        Start-Process powershell -Verb RunAs -ArgumentList $elevArgs -Wait
    } catch {
        Write-Host "[ERROR] 提权被拒绝。请右键「以管理员身份运行」。" -ForegroundColor Red
        Read-Host "按回车退出"
        exit 1
    }
    exit 0
}

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

# 2c. Copy 32-bit TSF DLL（32 位宿主：战网、老游戏）
# 缺这一份时 32 位应用从 WOW6432Node 找不到 InprocServer，输入法根本不出现。
$x86DllSrc = Join-Path $src "taishen_ime_x86.dll"
if (-not (Test-Path $x86DllSrc)) {
    $x86DllSrc = Join-Path $PSScriptRoot "..\platform\windows\out\taishen_ime_x86.dll"
}
if (Test-Path $x86DllSrc) {
    $x86Dest = Join-Path $dest "taishen_ime_x86.dll"
    if (Test-Path $x86Dest) {
        $oldX86 = "$x86Dest.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
        try { Move-Item $x86Dest $oldX86 -Force -ErrorAction Stop }
        catch { Write-Warning "旧 32 位 DLL 仍被占用，改名失败：$($_.Exception.Message)。请关闭 32 位应用后重装。" }
    }
    Copy-Item $x86DllSrc $x86Dest -Force
    Write-Host "[OK] 32-bit DLL copied"
} else {
    Write-Host "[WARN] taishen_ime_x86.dll not found - 32-bit hosts (Battle.net etc.) won't see the IME" -ForegroundColor Yellow
}

# 2d. Copy 32-bit IMM32（老游戏的 32 位进程只能加载 32 位 .ime）
$imeX86Src = Join-Path $src "taishen_ime_imm32_x86.ime"
if (-not (Test-Path $imeX86Src)) {
    $imeX86Src = Join-Path $PSScriptRoot "..\platform\windows\out\taishen_ime_imm32_x86.ime"
}
if (Test-Path $imeX86Src) {
    Copy-Item $imeX86Src (Join-Path $dest "taishen_ime_imm32_x86.ime") -Force
    Write-Host "[OK] 32-bit IMM32 IME copied"
}

# 3. Copy system dictionary (from out\ or resources\)
# 已 mmap 映射的 .bin 不允许原地覆写，覆盖前先改名 .old.<ts>（与 DLL 同一保护策略）
function Move-Aside-Mapped ($targetPath) {
    Get-Item "$targetPath.old.*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
    if (Test-Path $targetPath) {
        $oldPath = "$targetPath.old.$(Get-Date -Format 'yyyyMMddHHmmss')"
        try { Move-Item $targetPath $oldPath -Force -ErrorAction Stop }
        catch { Write-Warning "旧文件 $targetPath 仍被进程占用，改名失败：$($_.Exception.Message)。请重启系统后重装。" }
    }
}

function Copy-From-Src ($name, $label) {
    $s = Join-Path $src $name
    if (-not (Test-Path $s)) { $s = Join-Path $PSScriptRoot "..\resources\$name" }
    if (Test-Path $s) {
        $target = Join-Path $dest $name
        if ($name -like "*.bin") { Move-Aside-Mapped $target }
        Copy-Item $s $target -Force
        Write-Host "[OK] $label"
    }
}

Copy-From-Src "system_dict.db" "System dict"
Copy-From-Src "system_dict.db.bin" "Precompiled index"
Copy-From-Src "bigram.db" "Bigram table"
# 旧 .bin 缓存必须失效：覆盖 db 不一定更新 mtime，旧缓存可能被误判为可用
$bigramBinDest = Join-Path $dest "bigram.db.bin"
if (Test-Path $bigramBinDest) {
    Remove-Item $bigramBinDest -Force -ErrorAction SilentlyContinue
    Write-Host "[OK] stale bigram.db.bin removed"
}

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
        Move-Aside-Mapped (Join-Path $domainDest "domains.db.bin")
        Copy-Item $s (Join-Path $domainDest "domains.db") -Force
        Write-Host "[OK] Domain dict"
    } else {
        Write-Host "[WARN] Domain dict not found" -ForegroundColor Yellow
    }
}
Copy-DomainDict
Move-Aside-Mapped (Join-Path $dest "lm\lm_cache.bin")
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

# 4a. 安装并注册 V4 D2 进程外渲染服务（taishen_server.exe）
$serverSrc = Join-Path $src "taishen_server.exe"
if (Test-Path $serverSrc) {
    Stop-Process -Name "taishen_server" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 150
    Copy-Item $serverSrc (Join-Path $dest "taishen_server.exe") -Force
    Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
        -Name "TaishenIMEServer" -Value ('"' + (Join-Path $dest "taishen_server.exe") + '"')
    Write-Host "[OK] taishen_server.exe installed & registered for logon auto-start"
}

# 4b. V4 D1+D2 AppContainer/LPAC 最小白名单权限（默认开启；-DisableAppContainerAcl 可关）
if (-not $DisableAppContainerAcl) {
    Write-Host "[..] Granting AppContainer/LPAC minimal read/execute access..."
    $rxTargets = @('taishen_ime.dll', 'taishen_ime_x86.dll')
    $rTargets = @(
        'system_dict.db', 'system_dict.db.bin',
        'domains\domains.db', 'common.db',
        'bigram.db', 'bigram.db.bin',
        'context_prior.db'
    )
    $granted = 0
    foreach ($name in $rxTargets) {
        $p = Join-Path $dest $name
        if (Test-Path $p) {
            icacls $p /grant '*S-1-15-2-1:(RX)' /grant '*S-1-15-2-2:(RX)' /Q | Out-Null
            if ($LASTEXITCODE -eq 0) { $granted++ }
        }
    }
    foreach ($name in $rTargets) {
        $p = Join-Path $dest $name
        if (Test-Path $p) {
            icacls $p /grant '*S-1-15-2-1:(R)' /grant '*S-1-15-2-2:(R)' /Q | Out-Null
            if ($LASTEXITCODE -eq 0) { $granted++ }
        }
    }
    Write-Host "[OK] AppContainer/LPAC minimal ACL granted ($granted files)"
} else {
    Write-Host "[SKIP] AppContainer ACL disabled (-DisableAppContainerAcl)"
}
if (Test-Path (Join-Path $dest "taishen_server.exe")) {
    Start-Process explorer.exe -ArgumentList ('"' + (Join-Path $dest "taishen_server.exe") + '"')
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

# 5b. Register 32-bit TSF（必须走 SysWOW64 下的 regsvr32，否则写进 64 位视图，
# 32 位宿主依然看不到）
$x86Dll = Join-Path $dest "taishen_ime_x86.dll"
if (Test-Path $x86Dll) {
    $syswow = Join-Path $env:WINDIR "SysWOW64\regsvr32.exe"
    if (Test-Path $syswow) {
        & $syswow /s $x86Dll
        $x86Reg = $LASTEXITCODE
        Start-Sleep -Milliseconds 500
        if ($x86Reg -eq 0) {
            Write-Host "[OK] 32-bit registration done (WOW6432Node)"
        } else {
            Write-Host "[WARN] 32-bit registration returned $x86Reg（注册可能失败）" -ForegroundColor Yellow
        }
    } else {
        Write-Host "[WARN] SysWOW64 regsvr32 not found - 32-bit registration skipped" -ForegroundColor Yellow
    }
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
        if (Test-Path $legacyLayout) {
            Write-Host "[WARN] 旧 IMM32 注册删除失败（权限不足），请以管理员身份重跑" -ForegroundColor Yellow
        } else {
            Write-Host "[OK] Removed legacy IMM32 system registration"
        }
    }
}
Write-Host "[SKIP] IMM32 layer installed but not registered (run tools\enable-imm32.ps1 for old games)"

# 6b. 拷贝兼容层开关 / 登录循环救援脚本
$toolsDir = Join-Path $dest "tools"
New-Item -ItemType Directory -Path $toolsDir -Force | Out-Null
foreach ($n in @('enable-imm32.ps1', 'disable-imm32.ps1', 'fix-login-loop.ps1')) {
    $p = Join-Path $PSScriptRoot $n
    if (Test-Path $p) {
        Copy-Item $p (Join-Path $toolsDir $n) -Force
        Write-Host "[OK] tools\$n"
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
