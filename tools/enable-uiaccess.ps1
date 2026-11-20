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

$srcServer = Join-Path $env:LOCALAPPDATA "TaishenIME\taishen_server.exe"
if (-not (Test-Path $srcServer)) {
    $repoRoot = Split-Path $PSScriptRoot -Parent
    $srcServer = Join-Path $repoRoot "install\out\taishen_server.exe"
}
if (-not (Test-Path $srcServer)) {
    Write-Host "[ERROR] 未找到 taishen_server.exe，请先安装或构建" -ForegroundColor Red
    exit 1
}

# 1. 查找或创建本机自签代码签名证书
$subject = "CN=TaishenIME Local uiAccess"
$cert = Get-ChildItem Cert:\LocalMachine\My | Where-Object { $_.Subject -eq $subject } | Select-Object -First 1
if (-not $cert) {
    Write-Host "[..] 创建本机自签代码签名证书..."
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject `
        -CertStoreLocation Cert:\LocalMachine\My -NotAfter (Get-Date).AddYears(5)
}

# 2. 导入 LocalMachine\Root（Windows uiAccess 要求签名链受本机根信任）
$cerPath = Join-Path $env:TEMP "taishen_uiaccess.cer"
Export-Certificate -Cert $cert -FilePath $cerPath -Force | Out-Null
Import-Certificate -FilePath $cerPath -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
Remove-Item $cerPath -Force -ErrorAction SilentlyContinue
Write-Host "[OK] 证书已信任: $($cert.Thumbprint)"

# 3. 准备受保护目录（Windows uiAccess 要求位于 Program Files / System32）
$pfDir = Join-Path $env:ProgramFiles "TaishenIME"
New-Item -ItemType Directory -Force -Path $pfDir | Out-Null
Stop-Process -Name "taishen_server" -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 200

$dstServer = Join-Path $pfDir "taishen_server.exe"
Copy-Item $srcServer $dstServer -Force
$cfgSrc = Join-Path $env:LOCALAPPDATA "TaishenIME\config.ini"
if (Test-Path $cfgSrc) {
    Copy-Item $cfgSrc (Join-Path $pfDir "config.ini") -Force
}

# 4. 嵌入 uiAccess="true" manifest（需 Windows SDK 的 mt.exe）
$mt = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin\*\x64\mt.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $mt) {
    Write-Host "[ERROR] 未找到 Windows SDK mt.exe，无法写入 uiAccess manifest" -ForegroundColor Red
    exit 1
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

# 5. 代码签名
Set-AuthenticodeSignature -FilePath $dstServer -Certificate $cert -HashAlgorithm SHA256 | Out-Null
Write-Host "[OK] 已签名并部署到 $dstServer"

# 6. 更新登录自启并拉起
Set-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" `
    -Name "TaishenIMEServer" -Value ('"' + $dstServer + '"')
Start-Process explorer.exe -ArgumentList ('"' + $dstServer + '"')
Write-Host "[完成] taishen_server.exe (uiAccess=true) 已启动，可在任务栏搜索框中测试置顶效果。" -ForegroundColor Green
