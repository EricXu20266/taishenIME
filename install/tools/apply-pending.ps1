# 应用暂存的 IME DLL（宿主占用时无法直接替换）
#
# 场景：%LOCALAPPDATA%\TaishenIME 下的 taishen_ime.dll 被正在运行的宿主
# （explorer / 泰深 electron / 游戏等）以不可删除方式映射，Copy-Item 会失败。
# 构建脚本会把新 DLL 先放到 pending\ 目录，等宿主退出后再执行本脚本替换。
#
# 用法：
#   pwsh -NoProfile -File install\tools\apply-pending.ps1
#   pwsh -NoProfile -File install\tools\apply-pending.ps1 -Force   # 占用时也尝试
#
# 退出码：0 成功（或有不需要替换的文件）；1 有文件被占用未替换。

param(
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$dst = Join-Path $env:LOCALAPPDATA "TaishenIME"
$pending = Join-Path $dst "pending"

if (-not (Test-Path $pending)) {
    Write-Host "[OK] 没有暂存目录，无需处理"
    exit 0
}

# 查找加载 taishen_ime*.dll 的进程（这些进程会阻止替换）
function Get-BusyProcesses {
    $found = @()
    Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            foreach ($m in $_.Modules) {
                if ($m.ModuleName -like "taishen_ime*") {
                    $found += [pscustomobject]@{ PID = $_.Id; Name = $_.ProcessName }
                    break
                }
            }
        } catch {}
    }
    $found
}

$busy = Get-BusyProcesses
if ($busy.Count -gt 0 -and -not $Force) {
    Write-Host "[BLOCKED] 以下进程仍加载着 taishen_ime*.dll，替换会失败："
    $busy | Sort-Object Name | Format-Table -AutoSize | Out-String | Write-Host
    Write-Host "请先关闭它们（泰深 / explorer / 游戏），或注销后重登，再执行本脚本。"
    Write-Host "提示：重启 explorer 会自动拉起：Stop-Process -Id <explorer 的 PID> -Force"
    exit 1
}

$backupDir = Join-Path $env:USERPROFILE (".taishen\backups\taishenIME\apply-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
Write-Host "backup -> $backupDir"

$failed = 0
Get-ChildItem $pending -Filter "taishen_ime*" -File | ForEach-Object {
    $name = $_.Name
    $target = Join-Path $dst $name
    if (Test-Path $target) {
        Copy-Item $target (Join-Path $backupDir $name) -Force
    }
    try {
        Copy-Item $_.FullName $target -Force -ErrorAction Stop
        $ok = (Get-FileHash $_.FullName -Algorithm SHA256).Hash -eq (Get-FileHash $target -Algorithm SHA256).Hash
        Write-Host ("[OK] {0} -> {1} (hashMatch={2})" -f $name, (Get-Item $target).LastWriteTime.ToString("MM-dd HH:mm:ss"), $ok)
        Remove-Item $_.FullName -Force
    } catch {
        Write-Host ("[FAIL] {0}: {1}" -f $name, $_.Exception.Message)
        $failed++
    }
}

if ($failed -gt 0) {
    Write-Host "`n有 $failed 个文件未替换，仍留在 $pending"
    exit 1
}
Write-Host "`n[完成] 所有暂存 DLL 已替换。重启使用输入法的程序后生效。"
exit 0
