# 可选：为 taishen_server.exe 启用本地自签 uiAccess（V4 V3 高 z-band 置顶覆盖）
#
# 适用场景：若在 Win11 任务栏搜索框（SearchApp）中能正常出字、但候选窗被系统搜索面板遮挡，
# 可运行本脚本：生成本机自签代码签名证书 → 导入本机受信任根 → 为 taishen_server.exe 嵌入
# uiAccess="true" manifest 并签名 → 复制至 %ProgramFiles%\TaishenIME 并更新自启路径。
#
# 用法（需管理员权限）：
#   pwsh -NoProfile -ExecutionPolicy Bypass -File tools\enable-uiaccess.ps1

$ErrorActionPreference = "Stop"

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[ERROR] 请以管理员身份运行本脚本（需写入 LocalMachine\Root 与 Program Files）" -ForegroundColor Red
    exit 1
}

# 源：优先最新构建产物，其次打包产物，最后已安装位置
$repoRoot = Split-Path $PSScriptRoot -Parent
$srcServer = Join-Path $repoRoot "platform\windows\out\taishen_server.exe"
if (-not (Test-Path $srcServer)) {
    $srcServer = Join-Path $repoRoot "install\out\taishen_server.exe"
}
if (-not (Test-Path $srcServer)) {
    $srcServer = Join-Path $env:ProgramFiles "TaishenIME\taishen_server.exe"
}
if (-not (Test-Path $srcServer)) {
    Write-Host "[ERROR] 未找到 taishen_server.exe，请先安装或构建" -ForegroundColor Red
    exit 1
}

# 1. 查找签名证书（优先复用已导入的 CN=Taishen IME Dev，否则创建本地 uiAccess 证书）
$cert = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My -ErrorAction SilentlyContinue |
    Where-Object { $_.Subject -eq "CN=Taishen IME Dev" -and $_.HasPrivateKey } |
    Select-Object -First 1
if (-not $cert) {
    $subject = "CN=TaishenIME Local uiAccess"
    $cert = Get-ChildItem Cert:\LocalMachine\My | Where-Object { $_.Subject -eq $subject -and $_.HasPrivateKey } | Select-Object -First 1
    if (-not $cert) {
        Write-Host "[..] 创建本机自签代码签名证书..."
        $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject `
            -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddYears(5)
    }
}

# 2. 确保证书进入 LocalMachine\Root（Windows uiAccess 要求签名链受本机根信任）
$inRoot = Get-ChildItem Cert:\LocalMachine\Root -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $cert.Thumbprint } |
    Select-Object -First 1
if (-not $inRoot) {
    $cerPath = Join-Path $env:TEMP "taishen_uiaccess.cer"
    Export-Certificate -Cert $cert -FilePath $cerPath -Force | Out-Null
    Import-Certificate -FilePath $cerPath -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    Remove-Item $cerPath -Force -ErrorAction SilentlyContinue
}
Write-Host "[OK] uiAccess 签名证书就绪: $($cert.Subject) ($($cert.Thumbprint))"

# 3. 准备受保护目录（Windows uiAccess 要求位于 Program Files / System32）
$pfDir = Join-Path $env:ProgramFiles "TaishenIME"
New-Item -ItemType Directory -Force -Path $pfDir | Out-Null
Stop-Process -Name "taishen_server" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 200

$dstServer = Join-Path $pfDir "taishen_server.exe"
Copy-Item $srcServer $dstServer -Force
# 保留单一 %LOCALAPPDATA%\TaishenIME\config.ini，移除旧快照以免覆盖或漂移
Remove-Item (Join-Path $pfDir "config.ini") -Force -ErrorAction SilentlyContinue

# 4. 剥离旧签名后嵌入 uiAccess="true" manifest（否则已签名 PE 经 mt.exe 改资源后再签会报损坏）
$mt = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\mt.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $mt) {
    Write-Host "[ERROR] 未找到 Windows SDK mt.exe，无法写入 uiAccess manifest" -ForegroundColor Red
    exit 1
}
$signtool = Join-Path (Split-Path $mt.FullName -Parent) "signtool.exe"
if ((Get-AuthenticodeSignature $dstServer).Status -ne 'NotSigned') {
    if (Test-Path $signtool) {
        & $signtool remove /s $dstServer | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "signtool remove 失败 (exit=$LASTEXITCODE)" }
    } else {
        throw "目标 EXE 已签名但未找到 signtool.exe，无法在 mt.exe 前剥离旧签名"
    }
}

$manifestPath = Join-Path $env:TEMP "taishen_server_uiaccess.manifest"
@"
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="true" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"@ | Set-Content -Path $manifestPath -Encoding UTF8

& $mt.FullName -manifest $manifestPath -outputresource:"$dstServer;#1" | Out-Null
Remove-Item $manifestPath -Force -ErrorAction SilentlyContinue
if ($LASTEXITCODE -ne 0) { throw "mt.exe 嵌入 manifest 失败" }

# 5. 签名：uiAccess 要求「清单 uiAccess=true」+「证书链受本机根信任」两者同时成立
Write-Host "[..] 签名 $dstServer ..."
& $signtool sign /fd SHA256 /sha1 $($cert.Thumbprint) /s My $dstServer | Out-Null
if ($LASTEXITCODE -ne 0) { throw "signtool sign 失败 (exit=$LASTEXITCODE)" }
$sig = Get-AuthenticodeSignature $dstServer
Write-Host "[OK] 签名状态: $($sig.Status)"
if ($sig.Status -ne 'Valid') {
    Write-Host "[WARN] 签名未处于 Valid 状态（$($sig.Status)），uiAccess 可能不会生效" -ForegroundColor Yellow
}

# 6. 重启 server（以新清单与签名运行）
Stop-Process -Name "taishen_server" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300
Start-Process $dstServer | Out-Null
Write-Host "[OK] taishen_server 已重启（uiAccess 清单 + 签名就绪）"

# 5. 代码签名并校验 Valid
$sig = Set-AuthenticodeSignature -FilePath $dstServer -Certificate $cert -HashAlgorithm SHA256
if ($sig.Status -ne 'Valid') {
    throw "Set-AuthenticodeSignature 失败: $($sig.Status) $($sig.StatusMessage)"
}
Write-Host "[OK] 已签名并部署到 $dstServer (Status=$($sig.Status))"

# 6. 更新登录自启并拉起
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
    -Name "TaishenIMEServer" -Value ('"' + $dstServer + '"')
Start-Process explorer.exe -ArgumentList ('"' + $dstServer + '"')
Write-Host "[完成] taishen_server.exe (uiAccess=true) 已启动，可在任务栏搜索框中测试置顶效果。" -ForegroundColor Green
