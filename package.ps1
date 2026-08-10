# ============================================================
#  泰深输入法 — 一键打包脚本
#  用法：.\package.ps1 [-Version x.y.z] [-SkipBuild] [-SkipNSIS]
#  流程：DB 构建 → CMake Release → 收集产物 → NSIS 打包
# ============================================================
param(
    [string]$Version = "",
    [switch]$SkipBuild,
    [switch]$SkipNSIS
)

$ErrorActionPreference = "Stop"
$repoRoot = $PSScriptRoot

# ---- 版本号 ----
if (-not $Version) {
    $cargoToml = Join-Path $repoRoot "engine\Cargo.toml"
    if (Test-Path $cargoToml) {
        $match = Select-String -Path $cargoToml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1
        if ($match) { $Version = $match.Matches.Groups[1].Value }
    }
    if (-not $Version) { $Version = "0.5.0" }
}
Write-Host "============================================" -ForegroundColor Cyan
Write-Host "  泰深输入法 v${Version} — 打包" -ForegroundColor Cyan
Write-Host "============================================" -ForegroundColor Cyan
Write-Host ""

# ---- 检查环境 ----
$python = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction SilentlyContinue)
if (-not $python) {
    Write-Host "[ERROR] Python not found" -ForegroundColor Red; exit 1
}
Write-Host "[ENV] Python: $($python.Source)"

# ---- 1/2. 校验词库（构建职责在 taishen-dict，此处只校验存在）----
Write-Host ""
Write-Host "── 1/5 校验词库（来源：taishen-dict 同步）──" -ForegroundColor Yellow
$verFile = Join-Path $repoRoot "resources\VERSION.json"
if (Test-Path $verFile) {
    $ver = (Get-Content $verFile -Raw | ConvertFrom-Json).version
    Write-Host "[OK] 词库版本: $ver" -ForegroundColor Green
} else {
    Write-Host "[WARN] 未找到 resources\VERSION.json——词库可能未同步" -ForegroundColor Yellow
    Write-Host "      运行 taishen-dict: python tools\sync_to_ime.py" -ForegroundColor Yellow
}
$sysDb = Join-Path $repoRoot "resources\system_dict.db"
$domDb = Join-Path $repoRoot "resources\domains\domains.db"
$comDb = Join-Path $repoRoot "resources\common.db"
foreach ($db in @($sysDb, $domDb, $comDb)) {
    if (-not (Test-Path $db)) {
        Write-Host "[ERROR] 词库缺失: $db（先同步 taishen-dict 产物）" -ForegroundColor Red
        exit 1
    }
}
Write-Host "[OK] 三个词库就位: system_dict.db / domains.db / common.db" -ForegroundColor Green

# ---- 3. CMake Release 编译 DLL ----
Write-Host ""
Write-Host "── 3/5 CMake Release 编译 ──" -ForegroundColor Yellow
$buildDir = Join-Path $repoRoot "platform\windows"
if (-not $SkipBuild) {
    Push-Location $buildDir
    try {
        & cmake --build build --config Release 2>&1
        if ($LASTEXITCODE -ne 0) { throw "CMake build failed" }
        Write-Host "[OK] DLL 编译完成"
    } finally { Pop-Location }
} else {
    Write-Host "[SKIP] -SkipBuild"
}

# ---- 4. 收集产物到 out/ ----
Write-Host ""
Write-Host "── 4/5 收集产物 ──" -ForegroundColor Yellow
$outDir = Join-Path $repoRoot "install\out"
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

# DLL
$dllSrc = Join-Path $buildDir "out\taishen_ime.dll"
if (-not (Test-Path $dllSrc)) {
    # 尝试 build/Release
    $dllSrc = Join-Path $buildDir "build\Release\taishen_ime.dll"
}
if (Test-Path $dllSrc) {
    Copy-Item $dllSrc (Join-Path $outDir "taishen_ime.dll") -Force
    Write-Host "[OK] taishen_ime.dll"
} else {
    Write-Host "[ERROR] taishen_ime.dll not found at $dllSrc" -ForegroundColor Red
    Write-Host "  Build first: cd platform\windows && cmake --build build --config Release"
    exit 1
}

# System dict DB
$sysDict = Join-Path $repoRoot "resources\system_dict.db"
if (Test-Path $sysDict) {
    Copy-Item $sysDict (Join-Path $outDir "system_dict.db") -Force
    Write-Host "[OK] system_dict.db ($([math]::Round((Get-Item $sysDict).Length/1MB,1)) MB)"
} else {
    Write-Host "[WARN] system_dict.db not found"
}

# System dict .bin (precompiled index)
$sysBin = Join-Path $repoRoot "resources\system_dict.db.bin"
if (Test-Path $sysBin) {
    Copy-Item $sysBin (Join-Path $outDir "system_dict.db.bin") -Force
    Write-Host "[OK] system_dict.db.bin ($([math]::Round((Get-Item $sysBin).Length/1MB,1)) MB)"
} else {
    Write-Host "[WARN] system_dict.db.bin not found — first activation will build it (~6s)"
}

# Domains DB
$domainsDb = Join-Path $domainsDir "domains.db"
if (Test-Path $domainsDb) {
    Copy-Item $domainsDb (Join-Path $outDir "domains.db") -Force
    Write-Host "[OK] domains.db ($([math]::Round((Get-Item $domainsDb).Length/1MB,1)) MB)"
} else {
    Write-Host "[WARN] domains.db not found — run tools/build_domains_db.py first"
}

# Common DB
if (Test-Path $commonDb) {
    Copy-Item $commonDb (Join-Path $outDir "common.db") -Force
    Write-Host "[OK] common.db ($([math]::Round((Get-Item $commonDb).Length/1KB,0)) KB)"
} else {
    Write-Host "[WARN] common.db not found — run tools/build_common_db.py first"
}

# ---- 5. NSIS 打包 ----
Write-Host ""
Write-Host "── 5/5 NSIS 打包 ──" -ForegroundColor Yellow
if (-not $SkipNSIS) {
    $nsisExe = (Get-Command makensis -ErrorAction SilentlyContinue)?.Source
    if (-not $nsisExe) {
        $nsisExe = "C:\Program Files (x86)\NSIS\makensis.exe"
    }
    $nsiScript = Join-Path $repoRoot "install\install.nsi"
    if (Test-Path $nsisExe -PathType Leaf) {
        Push-Location (Join-Path $repoRoot "install")
        try {
            & $nsisExe "/DVERSION=$Version" $nsiScript 2>&1
            if ($LASTEXITCODE -ne 0) { throw "NSIS failed" }
            $setupFile = Join-Path $repoRoot "install\TaishenIME-Setup-${Version}.exe"
            if (Test-Path $setupFile) {
                $size = [math]::Round((Get-Item $setupFile).Length/1MB, 1)
                Write-Host "[OK] $setupFile ($size MB)" -ForegroundColor Green
            }
        } finally { Pop-Location }
    } else {
        Write-Host "[WARN] makensis not found — skipping NSIS. Install manually with install.ps1"
    }
} else {
    Write-Host "[SKIP] -SkipNSIS"
}

# ---- 完成 ----
Write-Host ""
Write-Host "============================================" -ForegroundColor Green
Write-Host "  打包完成 v${Version}" -ForegroundColor Green
Write-Host "  产物: install\out\ (直接安装: .\install\install.ps1)" -ForegroundColor Green
if (-not $SkipNSIS -and (Test-Path (Join-Path $repoRoot "install\TaishenIME-Setup-${Version}.exe"))) {
    Write-Host "  安装包: install\TaishenIME-Setup-${Version}.exe" -ForegroundColor Green
}
Write-Host "============================================" -ForegroundColor Green
