# 四产物原子部署脚本（支持从 pending\ 或任意构建输出目录部署）
#
# 解决两个历史缺陷：
#   1. 仅替换单个 DLL 导致 4 个二进制（x64/x86 × TSF/IMM32）版本错位；
#   2. 多会话并发部署互相覆盖（通过 deploy.lock 互斥）。
#
# 用法：
#   pwsh -NoProfile -File install\tools\apply-pending.ps1
#   pwsh -NoProfile -File install\tools\apply-pending.ps1 -SourceDir install\out
#   pwsh -NoProfile -File install\tools\apply-pending.ps1 -Force
#   pwsh -NoProfile -File install\tools\apply-pending.ps1 -AllowPartial
#
# 退出码：0 成功；1 被占用 / 缺文件 / 锁冲突 / 替换失败。

param(
    [string]$SourceDir = "",
    [switch]$Force,
    [switch]$AllowPartial,
    [switch]$KeepSource
)

$ErrorActionPreference = "Stop"
$dst = Join-Path $env:LOCALAPPDATA "TaishenIME"
if (-not (Test-Path $dst)) {
    New-Item -ItemType Directory -Force -Path $dst | Out-Null
}

$isPendingMode = [string]::IsNullOrWhiteSpace($SourceDir)
$src = if ($isPendingMode) { Join-Path $dst "pending" } else { (Resolve-Path $SourceDir).Path }

if (-not (Test-Path $src)) {
    Write-Host "[OK] 来源目录不存在（$src），无需处理"
    exit 0
}

# ---- 1. 部署锁（防多会话并发覆盖）----
$lockFile = Join-Path $dst "deploy.lock"
if (Test-Path $lockFile) {
    $stale = $false
    try {
        $lockObj = Get-Content $lockFile -Raw | ConvertFrom-Json
        if ($lockObj.pid) {
            $proc = Get-Process -Id ([int]$lockObj.pid) -ErrorAction SilentlyContinue
            if (-not $proc) { $stale = $true }
        } else {
            $stale = $true
        }
    } catch {
        $stale = $true
    }
    if ($stale) {
        Remove-Item $lockFile -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "[BLOCKED] 另一进程正在部署（$lockFile）：" -ForegroundColor Red
        Get-Content $lockFile | Write-Host
        exit 1
    }
}

$lockPayload = [ordered]@{
    pid       = $PID
    startedAt = (Get-Date -Format "o")
    source    = $src
} | ConvertTo-Json
Set-Content -Path $lockFile -Value $lockPayload -Encoding UTF8

try {
    # ---- 2. 四产物完整性校验 ----
    $requiredBins = @(
        "taishen_ime.dll",
        "taishen_ime_x86.dll",
        "taishen_ime_imm32.ime",
        "taishen_ime_imm32_x86.ime"
    )

    $present = @()
    $missing = @()
    foreach ($name in $requiredBins) {
        $p = Join-Path $src $name
        if (Test-Path $p) {
            $present += $name
        } else {
            $missing += $name
        }
    }

    if ($present.Count -eq 0) {
        Write-Host "[OK] 来源目录中无待部署二进制（$src）"
        exit 0
    }

    if ($missing.Count -gt 0 -and -not $AllowPartial) {
        Write-Host "[BLOCKED] 四产物不完整，拒绝部分部署（避免 x64/x86 版本错位）：" -ForegroundColor Red
        Write-Host ("  缺失: " + ($missing -join ", "))
        Write-Host "  请先重新打包凑齐四产物（.\package.ps1），或显式传 -AllowPartial 覆盖。"
        exit 1
    }

    # ---- 3. 检查宿主进程占用 ----
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
        Write-Host "[BLOCKED] 以下进程仍加载着 taishen_ime*，替换会失败：" -ForegroundColor Yellow
        $busy | Sort-Object Name | Format-Table -AutoSize | Out-String | Write-Host
        Write-Host "请先关闭它们（泰深 / explorer / 游戏），或注销后重登，再执行本脚本。"
        Write-Host "提示：重启 explorer 会自动拉起：Stop-Process -Id <explorer 的 PID> -Force"
        exit 1
    }

    # ---- 4. 备份并逐一替换 + SHA256 校验 ----
    $backupDir = Join-Path $env:USERPROFILE (".taishen\backups\taishenIME\apply-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
    New-Item -ItemType Directory -Force -Path $backupDir | Out-Null
    Write-Host "backup -> $backupDir"

    $failed = 0
    $manifestFiles = [ordered]@{}

    foreach ($name in $present) {
        $srcFile = Join-Path $src $name
        $target = Join-Path $dst $name
        if (Test-Path $target) {
            Copy-Item $target (Join-Path $backupDir $name) -Force
        }
        try {
            $srcHash = (Get-FileHash $srcFile -Algorithm SHA256).Hash
            Copy-Item $srcFile $target -Force -ErrorAction Stop
            $dstHash = (Get-FileHash $target -Algorithm SHA256).Hash
            $ok = ($srcHash -eq $dstHash)
            if (-not $ok) {
                throw "SHA256 校验不一致"
            }
            $info = Get-Item $target
            $manifestFiles[$name] = [ordered]@{
                sha256 = $dstHash
                size   = $info.Length
                mtime  = $info.LastWriteTime.ToString("o")
            }
            Write-Host ("[OK] {0} -> {1} (sha256={2})" -f $name, $info.LastWriteTime.ToString("MM-dd HH:mm:ss"), $dstHash.Substring(0, 12))
            if ($isPendingMode -and -not $KeepSource) {
                Remove-Item $srcFile -Force
            }
        } catch {
            Write-Host ("[FAIL] {0}: {1}" -f $name, $_.Exception.Message) -ForegroundColor Red
            $failed++
        }
    }

    if ($failed -gt 0) {
        Write-Host "`n有 $failed 个文件未替换，请关闭占用进程后重试。" -ForegroundColor Red
        exit 1
    }

    # ---- 5. 写入 DEPLOY-VERSION.json 供验收核对 ----
    $repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
    $gitCommit = "unknown"
    try {
        $c = (& git -C $repoRoot rev-parse --short HEAD 2>$null)
        if ($LASTEXITCODE -eq 0 -and $c) { $gitCommit = $c.Trim() }
    } catch {}

    $deployManifest = [ordered]@{
        deployed_at = (Get-Date -Format "o")
        git_commit  = $gitCommit
        source_dir  = $src
        partial     = ($missing.Count -gt 0)
        binaries    = $manifestFiles
    } | ConvertTo-Json -Depth 4
    $manifestPath = Join-Path $dst "DEPLOY-VERSION.json"
    Set-Content -Path $manifestPath -Value $deployManifest -Encoding UTF8

    Write-Host "`n[完成] 已原子部署 $($present.Count) 个产物，清单写入 $manifestPath" -ForegroundColor Green
    Write-Host "请重启使用输入法的程序（或重启系统）后生效。"
    exit 0
} finally {
    Remove-Item $lockFile -Force -ErrorAction SilentlyContinue
}
