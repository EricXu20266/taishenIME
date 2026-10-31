# ============================================================
#  泰深输入法 — 登录循环救援脚本
#
#  适用：安装 0.1.51–0.1.69 后，微软账户输入 PIN 后「欢迎」转圈又弹回
#        登录界面，无限循环；安全模式同样进不去。
#
#  原理：旧版把 taishen_ime_imm32.ime 注册进 HKLM Keyboard Layouts，
#        登录界面也会加载它；模块卸载后残留线程/窗口类让 winlogon 崩溃。
#        删除该 HKLM 注册后，登录界面不再加载，循环即解除。
#
#  用法（必须在「能登录的另一个管理员账户」下运行）：
#      右键「以管理员身份运行」
#  若没有其他管理员账户：用 Windows 安装 U 盘启动进入修复环境，
#      打开命令提示符执行 reg delete 删除
#      HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804
# ============================================================

$ErrorActionPreference = "Stop"

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "[ERROR] 需要管理员权限。请右键选择「以管理员身份运行」。" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}

Write-Host "=== 泰深输入法 登录循环救援 ===" -ForegroundColor Cyan
Write-Host ""

$layoutKey = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
$removed = $false

if (Test-Path $layoutKey) {
    $layoutFile = (Get-ItemProperty -Path $layoutKey -Name "Layout File" -ErrorAction SilentlyContinue)."Layout File"
    if ($layoutFile -and $layoutFile -like "*taishen_ime_imm32*") {
        Remove-Item -Path $layoutKey -Recurse -Force
        $removed = $true
        Write-Host "[OK] 已删除系统级 IMM32 注册：E0C00804" -ForegroundColor Green
    } else {
        Write-Host "[SKIP] E0C00804 不指向泰深输入法，未改动：$layoutFile" -ForegroundColor Yellow
    }
} else {
    Write-Host "[OK] 系统级 IMM32 注册已不存在" -ForegroundColor Green
}

# 清理已加载用户 hive 的 Preload（不影响加载判定，只为语言列表干净）
$fixedUsers = 0
foreach ($root in @(Get-ChildItem Registry::HKEY_USERS -ErrorAction SilentlyContinue)) {
    $sid = $root.PSChildName
    $base = "Registry::HKEY_USERS\$sid"
    $preload = Join-Path $base "Keyboard Layout\Preload"
    if (-not (Test-Path $preload)) { continue }

    $props = Get-ItemProperty -Path $preload -ErrorAction SilentlyContinue
    if (-not $props) { continue }

    $entries = @($props.PSObject.Properties |
        Where-Object { $_.Name -match '^\d+$' } |
        Sort-Object { [int]$_.Name })
    if (-not (@($entries | Where-Object { $_.Value -eq 'e0c00804' }).Count -gt 0)) { continue }

    $kept = @($entries | Where-Object { $_.Value -ne 'e0c00804' } | ForEach-Object { $_.Value })
    foreach ($e in $entries) {
        Remove-ItemProperty -Path $preload -Name $e.Name -Force -ErrorAction SilentlyContinue
    }
    for ($i = 0; $i -lt $kept.Count; $i++) {
        Set-ItemProperty -Path $preload -Name ($i + 1) -Value $kept[$i] -Force
    }

    $substitutes = Join-Path $base "Keyboard Layout\Substitutes"
    if (Test-Path $substitutes) {
        Remove-ItemProperty -Path $substitutes -Name "e0c00804" -Force -ErrorAction SilentlyContinue
    }

    $fixedUsers++
    Write-Host "[OK] 已清理用户 $sid 的 Preload" -ForegroundColor Green
}

Write-Host ""
if ($removed -or $fixedUsers -gt 0) {
    Write-Host "处理完成。请重启电脑，然后尝试用原账户登录。" -ForegroundColor Cyan
} else {
    Write-Host "未发现泰深相关注册。登录循环可能是其他原因。" -ForegroundColor Yellow
    Write-Host "可查看事件查看器 → Windows 日志 → 应用程序，找 winlogon.exe 的 1000 事件。"
}
Write-Host ""
Write-Host "重启后建议安装 0.1.70 或更高版本（默认不再注册 IMM32）。"
Read-Host "按回车退出"
