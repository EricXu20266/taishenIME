# 清理安装目录堆积的历史 .old* 备份二进制
# 用法：pwsh -NoProfile -File tools\cleanup_old.ps1 [-DryRun]

param(
    [switch]$DryRun
)

$dest = Join-Path $env:LOCALAPPDATA "TaishenIME"
if (-not (Test-Path $dest)) {
    Write-Output "Directory not found: $dest"
    exit 0
}

$targets = @(Get-ChildItem $dest -File | Where-Object {
    $_.Name -match '\.old' -or $_.Name -eq 'taishen_ime_v015.dll'
})

if ($targets.Count -eq 0) {
    Write-Output "[OK] No stale .old* files to clean in $dest"
    exit 0
}

$deleted = 0
$locked = 0
$freedBytes = 0L

foreach ($f in $targets) {
    if ($DryRun) {
        Write-Output ("[DRY-RUN] would delete {0} ({1:N2} MB)" -f $f.Name, ($f.Length / 1MB))
        continue
    }
    try {
        $len = $f.Length
        Remove-Item $f.FullName -Force -ErrorAction Stop
        $deleted++
        $freedBytes += $len
    } catch {
        $locked++
        Write-Output ("[LOCKED] {0}: {1}" -f $f.Name, $_.Exception.Message.Split("`n")[0])
    }
}

if (-not $DryRun) {
    Write-Output ("[SUMMARY] deleted={0} freed={1:N2} MB locked={2}" -f $deleted, ($freedBytes / 1MB), $locked)
    if ($locked -gt 0) {
        Write-Output "Hint: locked files will be removable after reboot or closing host processes."
    }
}
