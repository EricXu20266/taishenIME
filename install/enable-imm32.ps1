# ============================================================
#  泰深输入法 — 启用 IMM32 兼容层（老游戏/老应用适配）
#
#  默认安装不再做系统级注册。需要 LOL 等 IMM32 老应用时手动运行本脚本。
#
#  ⚠️ 注册后该 IME 会被写入
#     HKLM\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804，
#     登录界面（winlogon/LogonUI）也会加载它。0.1.70 起 DLL 已做登录进程隔离，
#     但如果你在重启后遇到无法登录/登录循环，请从另一个管理员账户运行
#     disable-imm32.ps1 或 fix-login-loop.ps1。
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

$ime = Join-Path $PSScriptRoot "taishen_ime_imm32.ime"
if (-not (Test-Path $ime)) {
    # 安装后脚本位于 $INSTDIR\tools\，.ime 在上一级
    $ime = Join-Path (Split-Path $PSScriptRoot -Parent) "taishen_ime_imm32.ime"
}
if (-not (Test-Path $ime)) {
    Write-Host "[ERROR] 找不到 taishen_ime_imm32.ime" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}

Write-Host "[..] 注册 Keyboard Layouts..."
$proc = Start-Process regsvr32 -ArgumentList '/s', ('"' + $ime + '"') -Wait -PassThru
Start-Sleep -Milliseconds 800

$layoutKey = "HKLM:\SYSTEM\CurrentControlSet\Control\Keyboard Layouts\E0C00804"
if (-not (Test-Path $layoutKey)) {
    Write-Host "[ERROR] regsvr32 未写入 Keyboard Layouts（退出码 $($proc.ExitCode)）" -ForegroundColor Red
    Read-Host "按回车退出"
    exit 1
}
Write-Host "[OK] Keyboard Layouts E0C00804 已写入" -ForegroundColor Green

Write-Host "[..] ImmInstallIME — 创建 HKL..."
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Imm32Helper {
    [DllImport("imm32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr ImmInstallIME(string lpszIMEFileName, string lpszLayoutText);
}
'@
$hkl = [Imm32Helper]::ImmInstallIME($ime, "泰深拼音")
if ($hkl -ne [IntPtr]::Zero) {
    Write-Host "[OK] ImmInstallIME 成功，HKL = 0x$($hkl.ToString('X'))" -ForegroundColor Green
} else {
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    Write-Host "[WARN] ImmInstallIME 返回 NULL（错误 $err），可能已安装过" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "完成。重启后老游戏可使用泰深输入法。" -ForegroundColor Cyan
Write-Host "如出现登录异常，请从另一管理员账户运行 disable-imm32.ps1 或 fix-login-loop.ps1。"
Read-Host "按回车退出"
