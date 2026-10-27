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
$domainsDir = Join-Path $repoRoot "resources\domains"
$commonDb = Join-Path $repoRoot "resources\common.db"

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
if (Test-Path $verFile) {
    & $python.Source (Join-Path $repoRoot "tools\verify_resources.py") $verFile
    if ($LASTEXITCODE -ne 0) { throw "词库与 VERSION.json 对账失败" }
}

# ---- 2. Rust 引擎 Release 编译 ----
# 必须步骤：CMake 只链接 engine/target/release 下现成的静态库，不会触发 cargo。
# 漏掉本步会让新 C++ 协议链接到旧引擎（FFI 契约不匹配 → 选词静默失效）。
Write-Host ""
Write-Host "── 2/5 Rust 引擎 Release 编译 ──" -ForegroundColor Yellow
$engineDir = Join-Path $repoRoot "engine"
$engineLib = Join-Path $engineDir "target\release\taishen_engine.lib"
if (-not $SkipBuild) {
    Push-Location $engineDir
    try {
        & cargo build --release 2>&1
        if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }
        Write-Host "[OK] 引擎静态库编译完成"
    } finally { Pop-Location }

    # 防呆：静态库必须不早于 engine/src 下任一 .rs，否则说明 cargo 未真正重建
    if (-not (Test-Path $engineLib)) {
        Write-Host "[ERROR] 引擎静态库缺失: $engineLib" -ForegroundColor Red
        exit 1
    }
    $libTime = (Get-Item $engineLib).LastWriteTimeUtc
    $staleSource = Get-ChildItem (Join-Path $engineDir "src") -Recurse -Filter *.rs |
        Where-Object { $_.FullName -notmatch '\\src\\bin\\' -and $_.LastWriteTimeUtc -gt $libTime } |
        Select-Object -First 1
    if ($staleSource) {
        Write-Host "[ERROR] 引擎静态库早于源码: $($staleSource.Name)" -ForegroundColor Red
        Write-Host "        cargo 未真正重建，拒绝打包（否则选词会静默失效）" -ForegroundColor Red
        exit 1
    }
    Write-Host "[OK] 静态库时间戳校验通过" -ForegroundColor Green
} else {
    Write-Host "[SKIP] -SkipBuild"
}

# ---- 3. CMake Release 编译 DLL ----
Write-Host ""
Write-Host "── 3/5 CMake Release 编译 ──" -ForegroundColor Yellow
$buildDir = Join-Path $repoRoot "platform\windows"
# PM: NMake generator 需要 MSVC 环境变量。普通 PowerShell 里 nmake 不在 PATH，
# 直接 cmake --build 会报 "Generator: execution of make failed"。
# 策略：PATH 里没有 nmake 时，定位 vcvars64.bat 并在同一 cmd 进程内完成构建。
if (-not $SkipBuild) {
    if (Get-Command nmake -ErrorAction SilentlyContinue) {
        Push-Location $buildDir
        try {
            & cmake --build build --config Release 2>&1
            if ($LASTEXITCODE -ne 0) { throw "CMake build failed" }
            Write-Host "[OK] DLL 编译完成"
        } finally { Pop-Location }
    } else {
        $vcvars = $null
        # 1) 从 CMakeCache 记录的 cl.exe 反推 VS 根目录（最准，即配置生成时的真实工具链）
        $cacheFile = Join-Path $buildDir "build\CMakeCache.txt"
        if (Test-Path $cacheFile) {
            $m = Select-String -Path $cacheFile -Pattern '^CMAKE_CXX_COMPILER:FILEPATH=(.+)$' | Select-Object -First 1
            if ($m) {
                $clPath = $m.Matches.Groups[1].Value -replace '/', '\'
                if ($clPath -match '^(.*?)\\VC\\Tools\\MSVC\\[^\\]+\\bin\\Host[^\\]+\\') {
                    $cand = Join-Path $Matches[1] "VC\Auxiliary\Build\vcvars64.bat"
                    if (Test-Path $cand) { $vcvars = $cand }
                }
            }
        }
        # 2) vswhere（部分 BuildTools 独立安装不登记，可能查不到）
        if (-not $vcvars) {
            $vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
            if (Test-Path $vswhere) {
                $vsRoot = @(& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null) | Select-Object -First 1
                if ($vsRoot) {
                    $cand = Join-Path $vsRoot.Trim() "VC\Auxiliary\Build\vcvars64.bat"
                    if (Test-Path $cand) { $vcvars = $cand }
                }
            }
        }
        # 3) 兜底扫描安装目录，取最近修改的一个
        if (-not $vcvars) {
            $scanned = Get-ChildItem "C:\Program Files (x86)\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat" -ErrorAction SilentlyContinue |
                Sort-Object LastWriteTime -Descending | Select-Object -First 1
            if ($scanned) { $vcvars = $scanned.FullName }
        }
        if (-not $vcvars) {
            Write-Host "[ERROR] 未找到 vcvars64.bat，无法加载 MSVC 环境" -ForegroundColor Red
            Write-Host "        请在 Developer PowerShell 中执行，或先安装 VS Build Tools" -ForegroundColor Red
            exit 1
        }
        Write-Host "[ENV] MSVC: $vcvars"
        $cmdFile = Join-Path $env:TEMP "taishen-cmake-build.cmd"
        $lines = @(
            "@echo off"
            "call ""$vcvars"" >nul || exit /b 1"
            "cd /d ""$buildDir"" || exit /b 1"
            "cmake --build build --config Release"
            "exit /b %ERRORLEVEL%"
        )
        Set-Content -Path $cmdFile -Value $lines -Encoding ascii
        & cmd /c $cmdFile 2>&1
        $buildCode = $LASTEXITCODE
        Remove-Item $cmdFile -Force -ErrorAction SilentlyContinue
        if ($buildCode -ne 0) { throw "CMake build failed (exit $buildCode)" }
        Write-Host "[OK] DLL 编译完成（MSVC 环境自动加载）" -ForegroundColor Green
    }
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
# V0.5.13 P1：缺失或早于 .db → 部署期预生成（build_index），
# 保证安装包自带 .bin，用户首启秒开（不再走 SQLite 全量重建 6-7s）
$sysBin = Join-Path $repoRoot "resources\system_dict.db.bin"
if (-not (Test-Path $sysBin) -or
    (Get-Item $sysBin).LastWriteTime -lt (Get-Item $sysDict).LastWriteTime) {
    Write-Host "[..] system_dict.db.bin 缺失/过期，部署期预生成（build_index）..." -ForegroundColor Yellow
    Push-Location (Join-Path $repoRoot "engine")
    try {
        & cargo run --release --bin build_index -- $sysDict $sysBin 2>&1
        if ($LASTEXITCODE -ne 0) { throw "build_index failed" }
        Write-Host "[OK] build_index 完成"
    } finally { Pop-Location }
}
if (Test-Path $sysBin) {
    Copy-Item $sysBin (Join-Path $outDir "system_dict.db.bin") -Force
    Write-Host "[OK] system_dict.db.bin ($([math]::Round((Get-Item $sysBin).Length/1MB,1)) MB)"
} else {
    Write-Host "[WARN] system_dict.db.bin not found — first activation will build it (~6s)"
}

# Domains DB
$domainsDb = Join-Path $domainsDir "domains.db"
if (Test-Path $domainsDb) {
    $outDomainsDir = Join-Path $outDir "domains"
    New-Item -ItemType Directory -Path $outDomainsDir -Force | Out-Null
    Copy-Item $domainsDb (Join-Path $outDomainsDir "domains.db") -Force
    Write-Host "[OK] domains\domains.db ($([math]::Round((Get-Item $domainsDb).Length/1MB,1)) MB)"
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
