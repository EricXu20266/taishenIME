# Q3.2 安装与交付升级自动化自检脚本
# 验证项：
#   1. 静态检查：install.nsi 迁 $PROGRAMFILES64、SetRegView 64、VERSION_MAJOR/MINOR 拆解、下线自签 Root CA
#   2. 实编译 install.nsi（/DVERSION=0.1.90 与 1.2.3 通过，badver 阻断），输出到 TEMP 后清理
#   3. Windows SDK mt.exe 校验 taishen_server.exe 内嵌清单（-ExpectUiAccess false|true）
#   4. 可选 -CheckInstalled：在实机已安装正式签名版本时复核 %ProgramFiles%\TaishenIME 签名与 uiAccess

param(
    [ValidateSet("false", "true")]
    [string]$ExpectUiAccess = "false",
    [switch]$SkipNsisCompile,
    [switch]$CheckInstalled
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path $PSScriptRoot -Parent
$failed = 0

function Assert-True([bool]$cond, [string]$msg) {
    if ($cond) {
        Write-Host "[OK]   $msg" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] $msg" -ForegroundColor Red
        $script:failed++
    }
}

Write-Host "=== Q3.2 安装与交付升级自检 ===" -ForegroundColor Cyan

# ---- 1. 静态检查 ----
$nsiPath = Join-Path $repoRoot "install\install.nsi"
$nsiText = Get-Content $nsiPath -Raw -Encoding UTF8

Assert-True ($nsiText -match 'InstallDir\s+"\$PROGRAMFILES64\\TaishenIME"') "install.nsi 默认安装路径已迁至 `$PROGRAMFILES64\TaishenIME"
Assert-True ($nsiText -match 'SetRegView\s+64') "install.nsi 已显式使用 SetRegView 64"
Assert-True ($nsiText -match '"VersionMajor"\s+\$\{VERSION_MAJOR\}') "install.nsi VersionMajor 已改为 `${VERSION_MAJOR}"
Assert-True ($nsiText -match '"VersionMinor"\s+\$\{VERSION_MINOR\}') "install.nsi VersionMinor 已改为 `${VERSION_MINOR}"
Assert-True ($nsiText -notmatch '"VersionMajor"\s+0\b') "install.nsi 已移除 VersionMajor 硬编码 0"
Assert-True ($nsiText -notmatch '"VersionMinor"\s+2\b') "install.nsi 已移除 VersionMinor 硬编码 2"
Assert-True ($nsiText -notmatch '!define\s+SRC_CERT|CertPageCreate|CertPageLeave|certutil\s+-addstore') "install.nsi 已彻底移除自签 Root CA 捆绑与导入向导"

Assert-True (-not (Test-Path (Join-Path $repoRoot "certs\TaishenIME-Dev-Root.cer"))) "certs/TaishenIME-Dev-Root.cer 已物理删除"
Assert-True (-not (Test-Path (Join-Path $repoRoot "tools\enable-uiaccess.ps1"))) "tools/enable-uiaccess.ps1 已物理删除"

# 全仓代码与脚本不再含自签根生成/导入逻辑（排除本自检脚本与历史文档）
$scanTargets = @(
    (Join-Path $repoRoot "install"),
    (Join-Path $repoRoot "platform\windows\src"),
    (Join-Path $repoRoot "platform\windows\include"),
    (Join-Path $repoRoot "package.ps1"),
    (Join-Path $repoRoot ".github\workflows\windows-build.yml")
)
$selfSignHits = @()
foreach ($t in $scanTargets) {
    if (-not (Test-Path $t)) { continue }
    $hits = Get-ChildItem $t -Recurse -File -Include *.ps1,*.nsi,*.cpp,*.h,*.yml |
        ForEach-Object { Select-String -Path $_.FullName -Pattern 'New-SelfSignedCertificate|certutil\s+-addstore|CN=Taishen IME Dev' }
    if ($hits) { $selfSignHits += $hits }
}
$toolsOther = Get-ChildItem (Join-Path $repoRoot "tools") -File -Filter *.ps1 |
    Where-Object { $_.Name -ne "verify-q32-delivery.ps1" }
foreach ($f in $toolsOther) {
    $hits = Select-String -Path $f.FullName -Pattern 'New-SelfSignedCertificate|certutil\s+-addstore|CN=Taishen IME Dev'
    if ($hits) { $selfSignHits += $hits }
}
Assert-True ($selfSignHits.Count -eq 0) "代码/安装脚本/CI 已零自签证书生成或 CN=Taishen IME Dev 引用"

$wfText = Get-Content (Join-Path $repoRoot ".github\workflows\windows-build.yml") -Raw -Encoding UTF8
Assert-True ($wfText -match 'TAISHEN_ENABLE_UIACCESS') "windows-build.yml 已接入 TAISHEN_ENABLE_UIACCESS 开关"
Assert-True ($wfText -match 'Verify signed installer' -and $wfText -match 'Apply signed binaries') "windows-build.yml 已接入二进制与安装包签后校验"

# ---- 2. NSIS 实编译验证（输出到 TEMP 并关闭压缩，不污染工作树）----
if (-not $SkipNsisCompile) {
    $makensis = (Get-Command makensis.exe -ErrorAction SilentlyContinue)?.Source
    if (-not $makensis -and (Test-Path "C:\Program Files (x86)\NSIS\makensis.exe")) {
        $makensis = "C:\Program Files (x86)\NSIS\makensis.exe"
    }
    if (-not $makensis) {
        Write-Host "[FAIL] 未找到 makensis.exe，无法验证 install.nsi 编译" -ForegroundColor Red
        $failed++
    } else {
        $outExe1 = Join-Path $env:TEMP "taishen_q32_0190.exe"
        $outExe2 = Join-Path $env:TEMP "taishen_q32_123.exe"
        $outExeBad = Join-Path $env:TEMP "taishen_q32_bad.exe"
        Remove-Item $outExe1, $outExe2, $outExeBad -Force -ErrorAction SilentlyContinue
        Push-Location (Join-Path $repoRoot "install")
        try {
            $log1 = & $makensis /V4 "/DVERSION=0.1.90" "/DOUT_FILE=$outExe1" '/XSetCompress off' $nsiPath 2>&1
            $code1 = $LASTEXITCODE
            $txt1 = $log1 -join "`n"
            Assert-True ($code1 -eq 0 -and ($txt1 -match 'version=0\.1\.90 -> major=0 minor=1') -and ($txt1 -match 'WriteRegDWORD:.*\\VersionMajor=0\b') -and ($txt1 -match 'WriteRegDWORD:.*\\VersionMinor=1\b')) "makensis /DVERSION=0.1.90 解析 major=0 minor=1 且 WriteRegDWORD 正确"

            $log2 = & $makensis /V4 "/DVERSION=1.2.3" "/DOUT_FILE=$outExe2" '/XSetCompress off' $nsiPath 2>&1
            $code2 = $LASTEXITCODE
            $txt2 = $log2 -join "`n"
            Assert-True ($code2 -eq 0 -and ($txt2 -match 'version=1\.2\.3 -> major=1 minor=2') -and ($txt2 -match 'WriteRegDWORD:.*\\VersionMajor=1\b') -and ($txt2 -match 'WriteRegDWORD:.*\\VersionMinor=2\b')) "makensis /DVERSION=1.2.3 解析 major=1 minor=2 且 WriteRegDWORD 正确"

            $logBad = & $makensis /V4 "/DVERSION=badver" "/DOUT_FILE=$outExeBad" '/XSetCompress off' $nsiPath 2>&1
            $codeBad = $LASTEXITCODE
            Assert-True ($codeBad -ne 0) "makensis /DVERSION=badver 被正确拦截（exit=$codeBad）"
        } finally {
            Pop-Location
            Remove-Item $outExe1, $outExe2, $outExeBad -Force -ErrorAction SilentlyContinue
        }
    }
}

# ---- 3. Manifest 验证 ----
$mtExe = (Get-Command mt.exe -ErrorAction SilentlyContinue)?.Source
if (-not $mtExe) {
    $mtItem = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\mt.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1
    if ($mtItem) { $mtExe = $mtItem.FullName }
}
$serverExe = Join-Path $repoRoot "platform\windows\out\taishen_server.exe"
if (-not $mtExe) {
    Write-Host "[FAIL] 未找到 mt.exe" -ForegroundColor Red
    $failed++
} elseif (-not (Test-Path $serverExe)) {
    Write-Host "[FAIL] 未找到 $serverExe" -ForegroundColor Red
    $failed++
} else {
    $tmpXml = Join-Path $env:TEMP ("taishen_q32_manifest_" + $PID + ".xml")
    Remove-Item $tmpXml -Force -ErrorAction SilentlyContinue
    & $mtExe -nologo "-inputresource:$serverExe;#1" "-out:$tmpXml" 2>&1 | Out-Null
    $mtCode = $LASTEXITCODE
    $xml = if (Test-Path $tmpXml) { Get-Content $tmpXml -Raw -Encoding UTF8 } else { "" }
    Remove-Item $tmpXml -Force -ErrorAction SilentlyContinue

    Assert-True ($mtCode -eq 0 -and $xml.Length -gt 0) "taishen_server.exe 包含内嵌 RT_MANIFEST #1 资源"
    Assert-True ($xml -match ('<requestedExecutionLevel[^>]*level\s*=\s*"asInvoker"[^>]*uiAccess\s*=\s*"' + $ExpectUiAccess + '"')) "taishen_server.exe manifest 包含 level=`"asInvoker`" uiAccess=`"$ExpectUiAccess`""
    Assert-True ($xml -match '\{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\}') "taishen_server.exe manifest 包含 Windows 10/11 supportedOS"
}

# ---- 4. 可选：实机安装核验 ----
if ($CheckInstalled) {
    $pfDir = Join-Path $env:ProgramFiles "TaishenIME"
    $pfServer = Join-Path $pfDir "taishen_server.exe"
    Assert-True (Test-Path $pfServer) "已安装 %ProgramFiles%\TaishenIME\taishen_server.exe"
    if (Test-Path $pfServer) {
        $sig = Get-AuthenticodeSignature $pfServer
        Assert-True ($sig.Status -eq 'Valid') "已安装 taishen_server.exe 签名有效（当前: $($sig.Status)）"
    }
    $reg = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\TaishenIME" -ErrorAction SilentlyContinue
    Assert-True ($null -ne $reg) "64 位 Uninstall 注册表项存在"
}

if ($failed -gt 0) {
    Write-Host "`n[FAIL] 共 $failed 项 Q3.2 交付检查失败" -ForegroundColor Red
    exit 1
}
Write-Host "`n[PASS] Q3.2 交付自检全部通过 (ExpectUiAccess=$ExpectUiAccess)" -ForegroundColor Green
exit 0
