# ============================================================
#  泰深输入法 — 关闭 IMM32 兼容层（老游戏适配）
#
#  BUG-2026-10-31-01：旧版把 taishan_ime_imm32.ime 注册进
#  HKLM\Keyboard Layouts\E0C00804，登录界面也会加载；模块卸载后残留
#  线程/窗口类导致 winlogon 崩溃、无限登录循环。
#  本脚本摘掉系统级注册并重建各用户 Preload，重启后登录界面不再加载它。
#
#  用法：右键「以管理员身份运行」
# ============================================================

$ErrorActionPreference = "Stop"

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[ERROR] 需要管理员权限。请右键选择「以管理员身份运行」。" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}

$layoutKey = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
$removedLayout = $false

if (Test-Path $layoutKey) {
    $layoutFile = (Get-ItemProperty -Path $layoutKey -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($layoutFile -and $layoutFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $layoutKey -Recurse -Force
        $removedLayout = $true
        Write-Host "[OK] 已删除 HKLM Keyboard Layouts\E0C00804" -ForegroundColor Green
    } else {
        Write-Host "[SKIP] E0C00804 不指向泰深输入法，未改动：$layoutFile" -ForegroundColor Yellow
    }
} else {
    Write-Host "[OK] 系统级 IMM32 注册不存在" -ForegroundColor Green
}

# 重建每个已加载用户 hive 的 Preload（过滤 e0c00804，序号连续）
$fixedUsers = 0
$userRoots = @(Get-ChildItem Registry::HKEY_USERS -ErrorAction SilentlyContinue)
foreach ($root in $userRoots) {
    $base = "Registry::HKEY_USERS\$($root.PSChildName)"
    $preload = Join-Path $base "Keyboard Layout\Preload"
    if (-not (Test-Path $preload)) { continue }

    $props = Get-ItemProperty -Path $preload -ErrorAction SilentlyContinue
    if (-not $props) { continue }

    $entries = @($props.PSObject.Properties |
        Where-Object { $_.Name -match '^\d+$' } |
        Sort-Object { [int]$_.Name })

    $hasTaishan = @($entries | Where-Object { $_.Value -eq 'e0c00804' }).Count -gt 0
    if (-not $hasTaishan) { continue }

    $kept = @($entries | Where-Object { $_.Value -ne 'e0c00804' } | ForEach-Object { $_.Value })
    try {
        # 先覆盖写回（失败时旧值仍在），再删尾部多余项，避免中途失败清空键盘列表
        for ($i = 0; $i -lt $kept.Count; $i++) {
            Set-ItemProperty -Path $preload -Name ($i + 1) -Value $kept[$i] -Force -ErrorAction Stop
        }
        for ($i = $kept.Count + 1; $i -le $entries.Count; $i++) {
            Remove-ItemProperty -Path $preload -Name $i -Force -ErrorAction SilentlyContinue
        }
    } catch {
        Write-Warning "Preload 重建失败：$($_.Exception.Message)。请在系统设置中重新添加键盘。"
        continue
    }

    $substitutes = Join-Path $base "Keyboard Layout\Substitutes"
    if (Test-Path $substitutes) {
        Remove-ItemProperty -Path $substitutes -Name "e0c00804" -Force -ErrorAction SilentlyContinue
    }

    $fixedUsers++
    Write-Host "[OK] 已清理用户 $($root.PSChildName) 的 Preload" -ForegroundColor Green
}

Write-Host ""
if ($removedLayout -or $fixedUsers -gt 0) {
    Write-Host "完成。请重启电脑，登录界面将不再加载泰深 IMM32 兼容层。" -ForegroundColor Cyan
} else {
    Write-Host "没有需要清理的项。" -ForegroundColor Cyan
}
Write-Host "TSF 通道不受影响，重启后输入法仍可正常使用。"
Read-Host "按回车退出"
